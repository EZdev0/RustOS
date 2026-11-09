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

use crate::hardware::e1000::E1000;
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use lazy_static::lazy_static;
use smoltcp::iface::{Config, Interface, SocketHandle, SocketSet};
use smoltcp::phy::{Device, DeviceCapabilities, Medium};
use smoltcp::socket::{icmp, tcp};
use smoltcp::time::Instant;
use smoltcp::wire::{
    EthernetAddress, HardwareAddress, Icmpv4Packet, Icmpv4Repr, IpAddress, IpCidr,
};
use spin::Mutex;

/// Default upper bound for a streamed response body (4 MiB, as used by the
/// updater). The browser passes a smaller value.
pub const DEFAULT_MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
/// Number of decoded chunks that may wait for the UI to drain them.
const CHUNK_QUEUE_CAPACITY: usize = 256;

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

pub struct RustOsNetDevice<'a> {
    e1000: &'a mut E1000,
}

impl<'a> RustOsNetDevice<'a> {
    pub fn new(e1000: &'a mut E1000) -> Self {
        Self { e1000 }
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
    e1000: &'a mut E1000,
}

impl<'a> smoltcp::phy::TxToken for RustOsTxToken<'a> {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let mut buffer = vec![0; len];
        let result = f(&mut buffer);
        self.e1000.transmit(&buffer);
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
        if let Some(data) = self.e1000.receive() {
            let rx = RustOsRxToken { buffer: data };
            let tx = RustOsTxToken { e1000: self.e1000 };
            Some((rx, tx))
        } else {
            None
        }
    }

    fn transmit(&mut self, _timestamp: Instant) -> Option<Self::TxToken<'_>> {
        if self.e1000.can_transmit() {
            Some(RustOsTxToken { e1000: self.e1000 })
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
    e1000: E1000,
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
        let mut device = RustOsNetDevice::new(&mut self.e1000);
        self.iface.poll(instant, &mut device, &mut self.sockets);

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

    pub fn send_ping(&mut self, ip: [u8; 4], seq_no: u16) -> Result<(), &'static str> {
        let socket = self.sockets.get_mut::<icmp::Socket>(self.icmp_handle);
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

        socket
            .bind(icmp::Endpoint::Ident(0x1234))
            .map_err(|_| "Failed to bind socket")?;
        Ok(())
    }
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

pub fn init(mut e1000: E1000) {
    let mac = e1000.mac_address();
    let hw_addr = HardwareAddress::Ethernet(EthernetAddress([
        mac[0], mac[1], mac[2], mac[3], mac[4], mac[5],
    ]));

    let config = Config::new(hw_addr);

    let mut device = RustOsNetDevice::new(&mut e1000);
    let mut iface = Interface::new(config, &mut device, Instant::from_millis(0));

    // Static address until a DHCP client is wired up.
    iface.update_ip_addrs(|addrs| {
        let _ = addrs.push(IpCidr::new(IpAddress::v4(10, 0, 2, 15), 24));
    });

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

    *NETWORK_MANAGER.lock() = Some(NetworkManager {
        e1000,
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
    });
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
