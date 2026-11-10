//! TCP/IP stack glue (smoltcp) plus a small HTTP/1.1 client.
//!
//! Hardening notes (see `docs/http_network_indicator.md`):
//!
//! * the request that goes on the wire is built by `rustos-web` (unit tested on
//!   the host) and written in as many `send_slice` calls as the TX buffer
//!   allows, so long paths/cookies cannot be silently truncated,
//! * every transfer has an explicit byte budget; exceeding it aborts the
//!   connection instead of eating the 16 MiB kernel heap,
//! * a full chunk queue is a *failure*, never a silent data drop.

use crate::hardware::net::Nic;
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use lazy_static::lazy_static;
use smoltcp::iface::{Config, Interface, SocketHandle, SocketSet};
use smoltcp::phy::{Device, DeviceCapabilities, Medium};
use smoltcp::socket::{dhcpv4, dns, icmp, tcp};
use smoltcp::time::Instant;
use smoltcp::wire::{
    DnsQueryType, EthernetAddress, HardwareAddress, Icmpv4Packet, Icmpv4Repr, IpAddress, IpCidr,
    Ipv4Address,
};
use spin::Mutex;

/// Default upper bound for a streamed response body (4 MiB, as used by the
/// updater). The browser passes a smaller value.
pub const DEFAULT_MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
/// Number of decoded chunks that may wait for the UI to drain them.
const CHUNK_QUEUE_CAPACITY: usize = 256;

/// Frames handed up from the NIC since boot (only the first few are logged).
static FRAMES_SEEN: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

/// HTTP client state machine.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum HttpState {
    /// Waiting for a free socket / about to connect.
    Init,
    /// `SYN` sent, waiting for the connection to come up.
    Connecting,
    /// Connection established, request bytes are being written.
    SendingRequest,
    /// Reading the response.
    ReceivingResponse,
    /// Transfer finished (either cleanly or with `failed`).
    Done,
}

/// The IP configuration the interface is running with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IpConfig {
    /// IPv4 address in network byte order.
    pub address: [u8; 4],
    /// Prefix length (`/24` → 24).
    pub prefix: u8,
    /// Default gateway, when one is known.
    pub gateway: Option<[u8; 4]>,
    /// True when the lease came from DHCP, false for the built-in fallback.
    pub dhcp: bool,
}

/// Result of a host lookup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lookup {
    /// The name resolved (or was cached earlier).
    Resolved([u8; 4]),
    /// The query is queued or in flight: retry later.
    Pending,
    /// The resolver answered with an error, or there is no resolver at all.
    Failed,
}

pub struct RustOsNetDevice<'a> {
    nic: &'a mut Nic,
}

impl<'a> RustOsNetDevice<'a> {
    pub fn new(nic: &'a mut Nic) -> Self {
        Self { nic }
    }
}

pub struct RustOsRxToken {
    buffer: Vec<u8>,
}

impl smoltcp::phy::RxToken for RustOsRxToken {
    fn consume<R, F>(mut self, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        f(&mut self.buffer)
    }
}

pub struct RustOsTxToken<'a> {
    nic: &'a mut Nic,
}

impl<'a> smoltcp::phy::TxToken for RustOsTxToken<'a> {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let mut buffer = vec![0; len];
        let result = f(&mut buffer);
        self.nic.transmit(&buffer);
        result
    }
}

impl<'a> Device for RustOsNetDevice<'a> {
    type RxToken<'b>
        = RustOsRxToken
    where
        Self: 'b;
    type TxToken<'b>
        = RustOsTxToken<'b>
    where
        Self: 'b;

    fn receive(&mut self, _timestamp: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        if let Some(data) = self.nic.receive() {
            // The first frames of a boot are the only cheap window into "does
            // the RX path deliver anything at all"; without this a dead link
            // and a filtered link look identical in the serial log.
            let seen = FRAMES_SEEN.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            if seen < 12 {
                let ethertype = if data.len() >= 14 {
                    u16::from_be_bytes([data[12], data[13]])
                } else {
                    0
                };
                crate::serial_println!(
                    "[net] rx frame #{} len={} ethertype={:#06x}",
                    seen,
                    data.len(),
                    ethertype
                );
            }
            let rx = RustOsRxToken { buffer: data };
            let tx = RustOsTxToken { nic: self.nic };
            Some((rx, tx))
        } else {
            None
        }
    }

    fn transmit(&mut self, _timestamp: Instant) -> Option<Self::TxToken<'_>> {
        if self.nic.can_transmit() {
            Some(RustOsTxToken { nic: self.nic })
        } else {
            None
        }
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.max_transmission_unit = 1500;
        caps.max_burst_size = Some(1);
        caps.medium = Medium::Ethernet;
        caps
    }
}

pub struct NetworkManager {
    nic: Nic,
    iface: Interface,
    sockets: SocketSet<'static>,
    icmp_handle: SocketHandle,
    tcp_handle: SocketHandle,
    pub ping_reply: Option<([u8; 4], u16)>, // IP, Seq
    pub http_state: HttpState,
    pub http_chunks: crossbeam_queue::ArrayQueue<Vec<u8>>,
    pub total_bytes_received: usize,
    pub http_request_ip: Option<[u8; 4]>,
    pub http_request: Vec<u8>,
    pub http_sent: usize,
    pub http_max_bytes: usize,
    pub http_failed: bool,
    dhcp_handle: SocketHandle,
    dns_handle: SocketHandle,
    /// Active IP configuration (`None` until DHCP answered or the fallback ran).
    ip_config: Option<IpConfig>,
    /// Uptime after which the static fallback replaces a silent DHCP server.
    dhcp_deadline: u64,
    dns_servers: Vec<[u8; 4]>,
    dns_queue: Vec<String>,
    dns_inflight: Vec<(String, dns::QueryHandle)>,
    dns_cache: Vec<(String, [u8; 4])>,
    dns_failed: Vec<String>,
}

impl NetworkManager {
    /// Reset the transfer state and prepare a new request.
    pub fn request_raw(&mut self, ip: [u8; 4], request: Vec<u8>, max_bytes: usize) {
        self.update_http_request_state(ip);
        self.http_request = request;
        self.http_sent = 0;
        self.http_max_bytes = max_bytes.max(1024);
        self.http_failed = false;

        // Abort the previous connection so the socket is reusable.
        let socket = self.sockets.get_mut::<tcp::Socket>(self.tcp_handle);
        socket.abort();
    }

    pub fn update_http_request_state(&mut self, ip: [u8; 4]) {
        self.http_state = HttpState::Init;
        while self.http_chunks.pop().is_some() {}
        self.total_bytes_received = 0;
        self.http_request_ip = Some(ip);
        self.http_sent = 0;
        self.http_failed = false;
    }

    /// Legacy helper: request `/` from `example.com`.
    pub fn request_http(&mut self, ip: [u8; 4]) {
        let request = default_request("example.com", "/");
        self.request_raw(ip, request.into_bytes(), DEFAULT_MAX_RESPONSE_BYTES);
    }

    pub fn poll(&mut self, timestamp: u64) {
        let instant = Instant::from_millis(timestamp as i64);
        let mut device = RustOsNetDevice::new(&mut self.nic);
        self.iface.poll(instant, &mut device, &mut self.sockets);

        // Address configuration and name resolution: DHCP first, then the
        // queued DNS queries. Both are plain polling sockets in smoltcp, so
        // this is where they make progress.
        self.poll_dhcp(timestamp);
        self.poll_dns();

        let socket = self.sockets.get_mut::<icmp::Socket>(self.icmp_handle);
        if socket.can_recv() {
            if let Ok((payload, _)) = socket.recv() {
                if let Ok(Icmpv4Repr::EchoReply { seq_no, .. }) = Icmpv4Repr::parse(
                    &Icmpv4Packet::new_unchecked(payload),
                    &smoltcp::phy::ChecksumCapabilities::default(),
                ) {
                    self.ping_reply = Some(([0, 0, 0, 0], seq_no));
                }
            }
        }

        if self.http_request_ip.is_none() {
            return;
        }

        // Destructure so the borrow checker can see that the socket borrow is
        // disjoint from the transfer bookkeeping.
        let NetworkManager {
            sockets,
            tcp_handle,
            http_chunks,
            total_bytes_received,
            http_state,
            http_request_ip,
            http_request,
            http_sent,
            http_max_bytes,
            http_failed,
            iface,
            ..
        } = self;

        let cx = iface.context();
        let socket = sockets.get_mut::<tcp::Socket>(*tcp_handle);
        // Copy the state out: the match must not keep a borrow of `http_state`
        // alive, because every arm writes it back.
        let state = *http_state;

        match state {
            HttpState::Init => {
                if let Some(ip) = *http_request_ip {
                    let remote_addr = IpAddress::v4(ip[0], ip[1], ip[2], ip[3]);
                    let local_port = 49152 + (timestamp % 10000) as u16;
                    if socket.connect(cx, (remote_addr, 80), local_port).is_ok() {
                        *http_state = HttpState::Connecting;
                    } else {
                        // Port in use or socket still shutting down: retry next
                        // poll instead of spinning on a dead socket.
                        socket.abort();
                    }
                }
            }
            HttpState::Connecting => {
                if socket.is_active() {
                    *http_state = HttpState::SendingRequest;
                } else if !socket.is_open() {
                    *http_state = HttpState::Done;
                    *http_request_ip = None;
                    *http_failed = true;
                }
            }
            HttpState::SendingRequest => {
                if socket.can_send() {
                    let remaining = http_request.get(*http_sent..).unwrap_or(&[]);
                    if remaining.is_empty() {
                        *http_state = HttpState::ReceivingResponse;
                    } else {
                        match socket.send_slice(remaining) {
                            Ok(written) => {
                                *http_sent = http_sent.saturating_add(written);
                                if *http_sent >= http_request.len() {
                                    *http_state = HttpState::ReceivingResponse;
                                }
                            }
                            Err(_) => {
                                *http_failed = true;
                                *http_state = HttpState::Done;
                                *http_request_ip = None;
                                socket.abort();
                            }
                        }
                    }
                }
            }
            HttpState::ReceivingResponse => {
                if socket.can_recv() {
                    let budget = *http_max_bytes;
                    let result = socket.recv(|data| {
                        let len = data.len();
                        if total_bytes_received.saturating_add(len) > budget {
                            // Hard stop: the caller asked for a bounded body.
                            *http_failed = true;
                        } else {
                            *total_bytes_received = total_bytes_received.saturating_add(len);
                            if http_chunks.push(data.to_vec()).is_err() {
                                // Queue full: dropping bytes would corrupt the
                                // document, so fail loudly instead.
                                *http_failed = true;
                            }
                        }
                        (len, ())
                    });
                    if result.is_err() {
                        *http_failed = true;
                    }
                    if *http_failed {
                        socket.abort();
                        *http_state = HttpState::Done;
                        *http_request_ip = None;
                    }
                } else if !socket.may_recv() {
                    *http_state = HttpState::Done;
                    *http_request_ip = None;
                }
            }
            HttpState::Done => {}
        }
    }

    /// Ask the DHCP client for a lease and apply whatever it produced.
    fn poll_dhcp(&mut self, timestamp: u64) {
        enum Event {
            Configured {
                address: [u8; 4],
                prefix: u8,
                gateway: Option<[u8; 4]>,
                dns: Vec<[u8; 4]>,
            },
            Deconfigured,
        }

        let mut event: Option<Event> = None;
        {
            let socket = self.sockets.get_mut::<dhcpv4::Socket>(self.dhcp_handle);
            match socket.poll() {
                Some(dhcpv4::Event::Configured(config)) => {
                    let mut dns: Vec<[u8; 4]> = Vec::new();
                    for server in &config.dns_servers {
                        dns.push(server.0);
                    }
                    event = Some(Event::Configured {
                        address: config.address.address().0,
                        prefix: config.address.prefix_len(),
                        gateway: config.router.map(|router| router.0),
                        dns,
                    });
                }
                Some(dhcpv4::Event::Deconfigured) => event = Some(Event::Deconfigured),
                None => {}
            }
        }

        match event {
            Some(Event::Configured {
                address,
                prefix,
                gateway,
                dns,
            }) => {
                // A lease always wins over the built-in static setup.
                crate::serial_println!(
                    "[net] dhcp: ip={}.{}.{}.{}/{} gw={} dns={}",
                    address[0],
                    address[1],
                    address[2],
                    address[3],
                    prefix,
                    format_ipv4(gateway),
                    format_dns(&dns)
                );
                self.apply_ip_config(address, prefix, gateway, &dns, true);
            }
            Some(Event::Deconfigured) => {
                // Only a *lease* can be lost. The static setup from `init` is
                // not affected, and smoltcp also reports `Deconfigured` while a
                // new discovery round starts - logging that as "lease lost"
                // would be misleading (and used to disable a working link).
                if self.ip_config.map(|config| config.dhcp).unwrap_or(false) {
                    crate::serial_println!("[net] dhcp: lease lost");
                    self.ip_config = None;
                    // Give the client a moment, then fall back to the static
                    // setup instead of leaving the machine unconfigured.
                    self.dhcp_deadline = timestamp.saturating_add(2_000);
                }
            }
            None => {}
        }

        // A DHCP server that never answers (bridged setups, TAP without a
        // server, QEMU without `-net user`) must not leave the machine without
        // an address forever: fall back to QEMU's documented user-mode network.
        if self.ip_config.is_none() && timestamp >= self.dhcp_deadline {
            crate::serial_println!(
                "[net] dhcp: no lease after {}s, falling back to static 10.0.2.15/24 gw=10.0.2.2",
                self.dhcp_deadline / 1000
            );
            self.apply_ip_config(
                [10, 0, 2, 15],
                24,
                Some([10, 0, 2, 2]),
                &[[10, 0, 2, 3]],
                false,
            );
            // Do not retry the fallback on every poll.
            self.dhcp_deadline = u64::MAX;
        }
    }

    fn apply_ip_config(
        &mut self,
        address: [u8; 4],
        prefix: u8,
        gateway: Option<[u8; 4]>,
        dns_servers: &[[u8; 4]],
        dhcp: bool,
    ) {
        let cidr = IpCidr::new(
            IpAddress::v4(address[0], address[1], address[2], address[3]),
            prefix,
        );
        self.iface.update_ip_addrs(|addrs| {
            addrs.clear();
            let _ = addrs.push(cidr);
        });
        if let Some(gateway) = gateway {
            let installed = self
                .iface
                .routes_mut()
                .add_default_ipv4_route(Ipv4Address(gateway))
                .is_ok();
            if installed {
                crate::serial_println!("[net] default route via {}", format_ipv4(Some(gateway)));
            }
        }

        self.dns_servers.clear();
        for server in dns_servers {
            self.dns_servers.push(*server);
        }
        let mut servers: Vec<IpAddress> = Vec::new();
        for server in &self.dns_servers {
            servers.push(IpAddress::v4(server[0], server[1], server[2], server[3]));
        }
        if servers.is_empty() {
            // QEMU's user-mode DNS, also the value Limbo documents.
            servers.push(IpAddress::v4(10, 0, 2, 3));
        }
        self.sockets
            .get_mut::<dns::Socket>(self.dns_handle)
            .update_servers(&servers);

        self.ip_config = Some(IpConfig {
            address,
            prefix,
            gateway,
            dhcp,
        });
    }

    /// Start queued DNS queries and collect the finished ones.
    fn poll_dns(&mut self) {
        while self.dns_inflight.len() < 4 {
            let Some(host) = self.dns_queue.first().cloned() else {
                break;
            };
            self.dns_queue.remove(0);
            let started = if self.dns_servers.is_empty() {
                Err(())
            } else {
                let socket = self.sockets.get_mut::<dns::Socket>(self.dns_handle);
                socket
                    .start_query(self.iface.context(), &host, DnsQueryType::A)
                    .map_err(|_| ())
            };
            match started {
                Ok(handle) => self.dns_inflight.push((host, handle)),
                Err(()) => self.dns_record_failure(host),
            }
        }

        let inflight = core::mem::take(&mut self.dns_inflight);
        for (host, handle) in inflight {
            let result = self
                .sockets
                .get_mut::<dns::Socket>(self.dns_handle)
                .get_query_result(handle);
            match result {
                Ok(addresses) => {
                    let mut found: Option<[u8; 4]> = None;
                    for address in addresses.iter() {
                        if let IpAddress::Ipv4(v4) = address {
                            found = Some(v4.0);
                            break;
                        }
                    }
                    match found {
                        Some(ip) => {
                            crate::serial_println!(
                                "[net] dns: {} -> {}",
                                host,
                                format_ipv4(Some(ip))
                            );
                            if self.dns_cache.len() >= 16 {
                                self.dns_cache.remove(0);
                            }
                            self.dns_cache.push((host, ip));
                        }
                        None => self.dns_record_failure(host),
                    }
                }
                Err(dns::GetQueryResultError::Pending) => self.dns_inflight.push((host, handle)),
                Err(_) => self.dns_record_failure(host),
            }
        }
    }

    fn dns_record_failure(&mut self, host: String) {
        crate::serial_println!("[net] dns: {} could not be resolved", host);
        if self.dns_failed.len() >= 16 {
            self.dns_failed.remove(0);
        }
        self.dns_failed.push(host);
    }

    /// Resolve `host` from the cache, queueing a query when it is unknown.
    pub fn lookup_host(&mut self, host: &str) -> Lookup {
        if let Some((_, ip)) = self.dns_cache.iter().find(|(name, _)| name == host) {
            return Lookup::Resolved(*ip);
        }
        if self.dns_failed.iter().any(|name| name == host) {
            return Lookup::Failed;
        }
        let known = self.dns_queue.iter().any(|name| name == host)
            || self.dns_inflight.iter().any(|(name, _)| name == host);
        if !known && self.dns_queue.len() < 8 {
            self.dns_queue.push(String::from(host));
        }
        Lookup::Pending
    }

    /// The active IP configuration, if the interface has one.
    pub fn ip_config(&self) -> Option<IpConfig> {
        self.ip_config
    }

    pub fn send_ping(&mut self, ip: [u8; 4], seq_no: u16) -> Result<(), &'static str> {
        let socket = self.sockets.get_mut::<icmp::Socket>(self.icmp_handle);
        // Bind first: an unbound socket can neither send nor match replies, and
        // `can_send()` reflects that. Re-binding an already bound socket fails,
        // which is fine - the ident stays the same.
        let _ = socket.bind(icmp::Endpoint::Ident(0x1234));
        if !socket.can_send() {
            return Err("Socket cannot send");
        }

        let payload = alloc::vec![0x42; 32];
        let repr = Icmpv4Repr::EchoRequest {
            ident: 0x1234,
            seq_no,
            data: &payload,
        };

        let mut packet_buffer = alloc::vec![0; repr.buffer_len()];
        let mut packet = Icmpv4Packet::new_unchecked(&mut packet_buffer);
        repr.emit(&mut packet, &smoltcp::phy::ChecksumCapabilities::default());

        socket
            .send_slice(&packet_buffer, IpAddress::v4(ip[0], ip[1], ip[2], ip[3]))
            .map_err(|_| "Failed to send packet")?;
        Ok(())
    }
}

/// How long the DHCP client may stay silent before the static fallback runs.
const DHCP_TIMEOUT_MS: u64 = 5_000;

/// `1.2.3.4` for logging, `none` when the address is unknown.
fn format_ipv4(address: Option<[u8; 4]>) -> String {
    match address {
        Some(ip) => format!("{}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3]),
        None => String::from("none"),
    }
}

fn format_dns(servers: &[[u8; 4]]) -> String {
    if servers.is_empty() {
        return String::from("none");
    }
    let mut text = String::new();
    for (index, server) in servers.iter().enumerate() {
        if index > 0 {
            text.push(',');
        }
        text.push_str(&format!(
            "{}.{}.{}.{}",
            server[0], server[1], server[2], server[3]
        ));
    }
    text
}

/// Milliseconds since boot, from the 1 kHz PIT.
fn ticks() -> u64 {
    crate::interrupts::TIMER_TICKS.load(core::sync::atomic::Ordering::Relaxed) as u64
}

/// Wait until `deadline_ms` of uptime has elapsed.
async fn wait_until(deadline_ms: u64) {
    while ticks() < deadline_ms {
        YieldNow { yielded: false }.await;
    }
}

/// Boot-time network self test: ping the QEMU gateway and report the result.
///
/// This turns "networking works" into something CI can assert on: the boot test
/// that runs with a NIC requires the `[net] online` line. Without it a silently
/// disabled driver would look exactly like a healthy boot.
pub async fn net_selftest_task() {
    // Let the link settle, DHCP run and ARP resolve.
    wait_until(1_500).await;

    let driver = {
        NETWORK_MANAGER
            .lock()
            .as_ref()
            .map(|manager| manager.nic.name())
    };
    let Some(driver) = driver else {
        crate::serial_println!("[net] offline: no network device");
        return;
    };

    // Wait for an address: DHCP usually answers in well under a second, the
    // static fallback kicks in after `DHCP_TIMEOUT_MS`.
    let deadline = ticks() + DHCP_TIMEOUT_MS + 2_000;
    let mut config = None;
    while ticks() < deadline {
        config = NETWORK_MANAGER
            .lock()
            .as_ref()
            .and_then(|manager| manager.ip_config());
        if config.is_some() {
            break;
        }
        YieldNow { yielded: false }.await;
    }

    let Some(config) = config else {
        crate::serial_println!("[net] offline: no IP address on {}", driver);
        return;
    };
    crate::serial_println!(
        "[net] ip: {}/{} via {}",
        format_ipv4(Some(config.address)),
        config.prefix,
        if config.dhcp { "dhcp" } else { "static" }
    );

    // Ping the gateway: that proves ARP, the TX ring and the RX ring all work
    // without depending on any external host being reachable.
    let Some(gateway) = config.gateway else {
        crate::serial_println!("[net] offline: no default gateway on {}", driver);
        return;
    };

    for seq in 0..5u16 {
        {
            let mut manager = NETWORK_MANAGER.lock();
            if let Some(manager) = manager.as_mut() {
                // Never swallow this: a rejected send ("socket cannot send")
                // looks exactly like a dead link from the outside.
                if let Err(error) = manager.send_ping(gateway, seq) {
                    crate::serial_println!("[net] ping send failed: {}", error);
                }
            }
        }
        let deadline = ticks() + 700;
        while ticks() < deadline {
            YieldNow { yielded: false }.await;
            let reply = {
                let mut manager = NETWORK_MANAGER.lock();
                manager
                    .as_mut()
                    .and_then(|manager| manager.ping_reply.take())
            };
            if reply.is_some() {
                crate::serial_println!(
                    "[net] online: {} answered ping seq={} ({})",
                    format_ipv4(Some(gateway)),
                    seq,
                    driver
                );
                return;
            }
        }
    }

    crate::serial_println!(
        "[net] offline: no ping reply from {} ({})",
        format_ipv4(Some(gateway)),
        driver
    );
}

/// Resolve `host` through the DNS cache/queue (see [`Lookup`]).
pub fn lookup_host(host: &str) -> Lookup {
    if let Some(ref mut manager) = *NETWORK_MANAGER.lock() {
        return manager.lookup_host(host);
    }
    Lookup::Failed
}

/// The active IP configuration, for the status bar and the boot log.
pub fn ip_config() -> Option<IpConfig> {
    NETWORK_MANAGER
        .lock()
        .as_ref()
        .and_then(|manager| manager.ip_config())
}

/// Driver name of the NIC in use (`e1000`, `rtl8139`, ...).
pub fn driver_name() -> Option<&'static str> {
    NETWORK_MANAGER
        .lock()
        .as_ref()
        .map(|manager| manager.nic.name())
}

/// A minimal `GET` request, used when the caller has no prebuilt request.
pub fn default_request(host: &str, path: &str) -> String {
    format!(
        "GET {path} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: RustOS/0.3\r\nAccept: text/html,*/*;q=0.8\r\nAccept-Encoding: identity\r\nConnection: close\r\n\r\n"
    )
}

lazy_static! {
    pub static ref NETWORK_MANAGER: Mutex<Option<NetworkManager>> = Mutex::new(None);
}

pub fn get_network_status() -> bool {
    NETWORK_MANAGER.lock().is_some()
}

/// Send exactly `request` to `ip:80`, accepting at most `max_bytes` of body.
pub fn start_http_request_raw(ip: [u8; 4], request: Vec<u8>, max_bytes: usize) {
    if let Some(ref mut manager) = *NETWORK_MANAGER.lock() {
        manager.request_raw(ip, request, max_bytes);
    }
}

/// Build and send a `GET` for `host`/`path`.
pub fn start_http_request_with(ip: [u8; 4], host: &str, path: &str, max_bytes: usize) {
    let request = default_request(host, path);
    start_http_request_raw(ip, request.into_bytes(), max_bytes);
}

/// Legacy entry point kept for the updater: `GET /` from `example.com`.
pub fn start_http_request(ip: [u8; 4]) {
    if let Some(ref mut manager) = *NETWORK_MANAGER.lock() {
        manager.request_http(ip);
    }
}

pub fn get_http_response() -> Option<String> {
    None // Stubbed out as chunks are preferred now
}

pub fn poll_http_chunk() -> Option<alloc::vec::Vec<u8>> {
    if let Some(ref mut manager) = *NETWORK_MANAGER.lock() {
        return manager.http_chunks.pop();
    }
    None
}

/// True when the last transfer was aborted (budget, queue or socket error).
pub fn http_failed() -> bool {
    if let Some(ref manager) = *NETWORK_MANAGER.lock() {
        return manager.http_failed;
    }
    false
}

/// Current client state, for the browser status line.
pub fn http_state() -> Option<HttpState> {
    if let Some(ref manager) = *NETWORK_MANAGER.lock() {
        return Some(manager.http_state);
    }
    None
}

/// Abort the transfer and drop any queued chunks.
pub fn cancel_http_request() {
    if let Some(ref mut manager) = *NETWORK_MANAGER.lock() {
        while manager.http_chunks.pop().is_some() {}
        manager.http_request_ip = None;
        manager.http_state = HttpState::Done;
        manager.total_bytes_received = 0;
    }
}

pub fn init(mut nic: Nic) {
    let mac = nic.mac_address();
    let hw_addr = HardwareAddress::Ethernet(EthernetAddress([
        mac[0], mac[1], mac[2], mac[3], mac[4], mac[5],
    ]));

    let config = Config::new(hw_addr);

    let mut device = RustOsNetDevice::new(&mut nic);
    let iface = Interface::new(config, &mut device, Instant::from_millis(0));

    // No address yet: a DHCP client gets one, and `poll` falls back to the
    // static 10.0.2.15/24 setup when no server answers in time.
    let icmp_rx_buf = icmp::PacketBuffer::new(
        alloc::vec![icmp::PacketMetadata::EMPTY],
        alloc::vec![0; 256],
    );
    let icmp_tx_buf = icmp::PacketBuffer::new(
        alloc::vec![icmp::PacketMetadata::EMPTY],
        alloc::vec![0; 256],
    );
    let icmp_socket = icmp::Socket::new(icmp_rx_buf, icmp_tx_buf);

    let mut sockets = SocketSet::new(alloc::vec![]);
    let icmp_handle = sockets.add(icmp_socket);

    // 16 KiB RX/TX keeps a full window in flight for 1500-byte segments.
    let tcp_rx_buf = tcp::SocketBuffer::new(alloc::vec![0; 16384]);
    let tcp_tx_buf = tcp::SocketBuffer::new(alloc::vec![0; 4096]);
    let tcp_socket = tcp::Socket::new(tcp_rx_buf, tcp_tx_buf);
    let tcp_handle = sockets.add(tcp_socket);

    let dhcp_handle = sockets.add(dhcpv4::Socket::new());
    // Servers are filled in once DHCP (or the fallback) configured the link.
    let dns_handle = sockets.add(dns::Socket::new(&[], alloc::vec![]));

    let mut manager = NetworkManager {
        nic,
        iface,
        sockets,
        icmp_handle,
        tcp_handle,
        ping_reply: None,
        http_state: HttpState::Init,
        http_chunks: crossbeam_queue::ArrayQueue::new(CHUNK_QUEUE_CAPACITY),
        total_bytes_received: 0,
        http_request_ip: None,
        http_request: Vec::new(),
        http_sent: 0,
        http_max_bytes: DEFAULT_MAX_RESPONSE_BYTES,
        http_failed: false,
        dhcp_handle,
        dns_handle,
        ip_config: None,
        dhcp_deadline: ticks() + DHCP_TIMEOUT_MS,
        dns_servers: Vec::new(),
        dns_queue: Vec::new(),
        dns_inflight: Vec::new(),
        dns_cache: Vec::new(),
        dns_failed: Vec::new(),
    };

    // Configure the link immediately. QEMU's and Limbo's user-mode NAT live on
    // 10.0.2.0/24, so this makes the machine usable within milliseconds; a
    // DHCP lease (when a server answers) replaces it a moment later. Doing it
    // the other way round - "wait for DHCP, fall back after seconds" - left the
    // interface without a source address while every socket was already
    // trying to talk, which starved the very first ARP/DHCP exchange.
    manager.apply_ip_config(
        [10, 0, 2, 15],
        24,
        Some([10, 0, 2, 2]),
        &[[10, 0, 2, 3]],
        false,
    );
    crate::serial_println!("[net] static ip 10.0.2.15/24 gw=10.0.2.2 (dhcp will override)");

    *NETWORK_MANAGER.lock() = Some(manager);
}

struct YieldNow {
    yielded: bool,
}

impl core::future::Future for YieldNow {
    type Output = ();
    fn poll(
        mut self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<()> {
        if self.yielded {
            core::task::Poll::Ready(())
        } else {
            self.yielded = true;
            cx.waker().wake_by_ref();
            core::task::Poll::Pending
        }
    }
}

#[allow(clippy::unused_async)]
pub async fn network_task() {
    loop {
        let received = {
            if let Some(ref mut manager) = *NETWORK_MANAGER.lock() {
                let before = manager.total_bytes_received;
                let ticks =
                    crate::interrupts::TIMER_TICKS.load(core::sync::atomic::Ordering::Relaxed);
                manager.poll(ticks as u64);
                manager.total_bytes_received != before
            } else {
                false
            }
        };

        if received {
            // New page bytes: the browser must repaint soon, otherwise the UI
            // would only refresh on the next user input.
            crate::desktop::request_repaint();
        }

        YieldNow { yielded: false }.await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_request_has_required_headers() {
        let request = default_request("example.com", "/index.html?x=1");
        assert!(request.starts_with("GET /index.html?x=1 HTTP/1.1\r\n"));
        assert!(request.contains("Host: example.com\r\n"));
        assert!(request.ends_with("\r\n\r\n"));
    }
}
