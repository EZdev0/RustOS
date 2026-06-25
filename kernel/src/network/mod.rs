use crate::hardware::e1000::E1000;
use smoltcp::phy::{Device, DeviceCapabilities, Medium};
use smoltcp::time::Instant;
use smoltcp::iface::{Interface, Config, SocketSet, SocketHandle};
use smoltcp::wire::{EthernetAddress, HardwareAddress, IpAddress, IpCidr, Icmpv4Packet, Icmpv4Repr};
use smoltcp::socket::{icmp, tcp};
use alloc::vec::Vec;
use alloc::string::String;
use spin::Mutex;
use alloc::vec;
use lazy_static::lazy_static;

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum HttpState {
    Init,
    Connecting,
    SendingRequest,
    ReceivingResponse,
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
    type RxToken<'b> = RustOsRxToken where Self: 'b;
    type TxToken<'b> = RustOsTxToken<'b> where Self: 'b;

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
    pub http_response: String,
    pub http_request_ip: Option<[u8; 4]>,
}

impl NetworkManager {
    pub fn request_http(&mut self, ip: [u8; 4]) {
        self.http_state = HttpState::Init;
        self.http_response.clear();
        self.http_request_ip = Some(ip);
        
        // Abort old connection if active
        let socket = self.sockets.get_mut::<tcp::Socket>(self.tcp_handle);
        socket.abort();
    }

    pub fn poll(&mut self, timestamp: u64) {
        let instant = Instant::from_millis(timestamp as i64);
        let mut device = RustOsNetDevice::new(&mut self.e1000);
        self.iface.poll(instant, &mut device, &mut self.sockets);

        let socket = self.sockets.get_mut::<icmp::Socket>(self.icmp_handle);
        if socket.can_recv() {
            if let Ok((payload, _)) = socket.recv() {
                if let Ok(Icmpv4Repr::EchoReply { seq_no, .. }) = Icmpv4Repr::parse(&Icmpv4Packet::new_unchecked(payload), &smoltcp::phy::ChecksumCapabilities::default()) {
                    // Reply received
                    self.ping_reply = Some(([0,0,0,0], seq_no));
                }
            }
        }
        
        if let Some(ip) = self.http_request_ip {
            let cx = self.iface.context();
            let socket = self.sockets.get_mut::<tcp::Socket>(self.tcp_handle);
            match self.http_state {
                HttpState::Init => {
                    let remote_addr = IpAddress::v4(ip[0], ip[1], ip[2], ip[3]);
                    let local_port = 49152 + (timestamp % 10000) as u16;
                    if socket.connect(cx, (remote_addr, 80), local_port).is_ok() {
                        self.http_state = HttpState::Connecting;
                    }
                }
                HttpState::Connecting => {
                    if socket.is_active() {
                        self.http_state = HttpState::SendingRequest;
                    } else if !socket.is_open() {
                        self.http_state = HttpState::Done;
                    }
                }
                HttpState::SendingRequest => {
                    if socket.can_send() {
                        let request = b"GET / HTTP/1.1\r\nHost: example.com\r\nConnection: close\r\n\r\n";
                        if socket.send_slice(request).is_ok() {
                            self.http_state = HttpState::ReceivingResponse;
                        }
                    }
                }
                HttpState::ReceivingResponse => {
                    if socket.can_recv() {
                        if let Ok(_data) = socket.recv(|data| {
                            let len = data.len();
                            if let Ok(s) = core::str::from_utf8(data) {
                                self.http_response.push_str(s);
                            }
                            (len, data)
                        }) {
                        }
                    } else if !socket.may_recv() {
                        self.http_state = HttpState::Done;
                        self.http_request_ip = None;
                        socket.abort();
                    }
                }
                HttpState::Done => {}
            }
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
        
        socket.send_slice(&packet_buffer, IpAddress::v4(ip[0], ip[1], ip[2], ip[3])).unwrap();
        
        socket.bind(icmp::Endpoint::Ident(0x1234)).unwrap();
        Ok(())
    }
}

lazy_static! {
    pub static ref NETWORK_MANAGER: Mutex<Option<NetworkManager>> = Mutex::new(None);
}

pub fn get_network_status() -> bool {
    NETWORK_MANAGER.lock().is_some()
}

pub fn start_http_request(ip: [u8; 4]) {
    if let Some(ref mut nm) = *NETWORK_MANAGER.lock() {
        nm.request_http(ip);
    }
}

pub fn get_http_response() -> Option<String> {
    if let Some(ref mut nm) = *NETWORK_MANAGER.lock() {
        if nm.http_state == HttpState::Done && !nm.http_response.is_empty() {
            let res = nm.http_response.clone();
            nm.http_response.clear();
            return Some(res);
        }
    }
    None
}

pub fn init(mut e1000: E1000) {
    let mac = e1000.mac_address();
    let hw_addr = HardwareAddress::Ethernet(EthernetAddress([mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]]));
    
    let config = Config::new(hw_addr);
    
    let mut device = RustOsNetDevice::new(&mut e1000);
    let mut iface = Interface::new(config, &mut device, Instant::from_millis(0));
    
    // We update IP to something static for now until DHCP is built, or just leave it loopback
    iface.update_ip_addrs(|addrs| {
        let _ = addrs.push(IpCidr::new(IpAddress::v4(10, 0, 2, 15), 24));
    });

    let icmp_rx_buf = icmp::PacketBuffer::new(alloc::vec![icmp::PacketMetadata::EMPTY], alloc::vec![0; 256]);
    let icmp_tx_buf = icmp::PacketBuffer::new(alloc::vec![icmp::PacketMetadata::EMPTY], alloc::vec![0; 256]);
    let icmp_socket = icmp::Socket::new(icmp_rx_buf, icmp_tx_buf);

    let mut sockets = SocketSet::new(alloc::vec![]);
    let icmp_handle = sockets.add(icmp_socket);

    let tcp_rx_buf = tcp::SocketBuffer::new(alloc::vec![0; 4096]);
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
        http_response: alloc::string::String::new(),
        http_request_ip: None,
    });
}

struct YieldNow {
    yielded: bool,
}

impl core::future::Future for YieldNow {
    type Output = ();
    fn poll(mut self: core::pin::Pin<&mut Self>, cx: &mut core::task::Context<'_>) -> core::task::Poll<()> {
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
        if let Some(ref mut nm) = *NETWORK_MANAGER.lock() {
            let ticks = crate::interrupts::TIMER_TICKS.load(core::sync::atomic::Ordering::Relaxed);
            nm.poll(ticks as u64);
        }
        
        YieldNow { yielded: false }.await;
    }
}
