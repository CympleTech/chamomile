use std::fmt::{Debug, Formatter, Result as FmtResult};
use std::net::{IpAddr, Ipv6Addr, SocketAddr};

use chamomile_types::types::{PeerId, TransportType};

// [u8; 18]
fn socket_addr_to_bytes(socket: &SocketAddr) -> Vec<u8> {
    let ip_bytes: [u8; 16] = match socket {
        SocketAddr::V4(ipv4) => ipv4.ip().to_ipv6_mapped().octets(),
        SocketAddr::V6(ipv6) => ipv6.ip().octets(),
    };
    let port_bytes: [u8; 2] = socket.port().to_le_bytes();

    let mut bytes = vec![];
    bytes.extend(&ip_bytes);
    bytes.extend(&port_bytes);
    bytes
}

fn socket_addr_from_bytes(bytes: &[u8]) -> Result<SocketAddr, ()> {
    if bytes.len() != 18 {
        return Err(());
    }
    let mut port_bytes = [0u8; 2];
    port_bytes.copy_from_slice(&bytes[16..18]);
    let port = u16::from_le_bytes(port_bytes);

    let mut ip_bytes = [0u8; 16];
    ip_bytes.copy_from_slice(&bytes[0..16]);
    let ipv6 = Ipv6Addr::from(ip_bytes);
    if let Some(ipv4) = ipv6.to_ipv4() {
        Ok(SocketAddr::new(IpAddr::V4(ipv4), port))
    } else {
        Ok(SocketAddr::new(IpAddr::V6(ipv6), port))
    }
}

#[derive(Copy, Clone)]
pub struct Peer {
    id: PeerId,
    addr: SocketAddr,
    transport: TransportType,
    is_pub: bool,
}

pub const PEER_LENGTH: usize = 52;

impl Peer {
    pub fn new(id: PeerId, addr: SocketAddr, transport: TransportType, is_pub: bool) -> Self {
        Self {
            id,
            addr,
            transport,
            is_pub,
        }
    }

    pub fn id(&self) -> &PeerId {
        &self.id
    }

    pub fn addr(&self) -> &SocketAddr {
        &self.addr
    }

    pub fn set_addr(&mut self, addr: SocketAddr) {
        self.addr = addr;
    }

    pub fn transport(&self) -> &TransportType {
        &self.transport
    }

    pub fn set_transport(&mut self, transport: TransportType) {
        self.transport = transport;
    }

    pub fn is_pub(&self) -> bool {
        self.is_pub
    }

    pub fn set_is_pub(&mut self, is_pub: bool) {
        self.is_pub = is_pub
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ()> {
        if bytes.len() != PEER_LENGTH {
            return Err(());
        }

        let id = PeerId::from_bytes(&bytes[0..32])?;
        let addr = socket_addr_from_bytes(&bytes[32..50])?;
        let transport = TransportType::from_byte(bytes[50])?;
        let is_pub = bytes[51] == 1u8;
        Ok(Self {
            id,
            addr,
            transport,
            is_pub,
        })
    }
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = vec![];
        bytes.append(&mut self.id.to_bytes()); // 32-bytes
        bytes.append(&mut socket_addr_to_bytes(&self.addr)); // 18-bytes
        bytes.push(self.transport.to_byte()); // 1-bytes
        bytes.push(if self.is_pub { 1u8 } else { 0u8 }); // 1-bytes
        bytes
    }
}

impl Debug for Peer {
    fn fmt(&self, f: &mut Formatter) -> FmtResult {
        write!(f, "Peer: {:?}", self.id)
    }
}
