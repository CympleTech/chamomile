use rand_core::{CryptoRng, RngCore};
use std::{
    fmt::{Debug, Formatter, Result as FmtResult},
    io::Result,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
};

use crate::types::{new_io_error, PeerId, TransportType};

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

fn socket_addr_from_bytes(bytes: &[u8]) -> Result<SocketAddr> {
    if bytes.len() != 18 {
        return Err(new_io_error("peer bytes failure."));
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

#[derive(Copy, Clone, Eq, PartialEq)]
pub struct Peer {
    pub id: PeerId,
    pub assist: PeerId,
    pub socket: SocketAddr,
    pub transport: TransportType,
    pub is_pub: bool,
}

// PEER_ID_LENGTH + ASSIST + SOCKET_ADDR_LENGTH + 2 = 20 + 20 + 18 + 2 = 40
pub const PEER_LENGTH: usize = 60;

impl Peer {
    /// generate assist peer id for DHT.
    pub fn gen_assist<R: CryptoRng + RngCore>(&mut self, rng: &mut R) {
        let mut bytes = [0u8; 20];
        rng.fill_bytes(&mut bytes);
        self.assist = PeerId(bytes);
    }

    /// create peer.
    pub fn new(id: PeerId, socket: SocketAddr, transport: TransportType, is_pub: bool) -> Self {
        Self {
            id,
            socket,
            transport,
            is_pub,
            assist: PeerId::default(),
        }
    }

    /// create peer by only socket address.
    pub fn socket(socket: SocketAddr) -> Self {
        Self {
            socket,
            id: Default::default(),
            transport: TransportType::QUIC,
            is_pub: true,
            assist: PeerId::default(),
        }
    }

    /// create peer by only peer id.
    pub fn peer(id: PeerId) -> Self {
        Self {
            id,
            socket: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0),
            transport: TransportType::QUIC,
            is_pub: true,
            assist: PeerId::default(),
        }
    }

    pub fn effective(&self) -> bool {
        self.effective_socket() || self.effective_id()
    }

    /// check if this peer contains effective peer id.
    pub fn effective_id(&self) -> bool {
        self.id != PeerId::default()
    }

    /// check if this peer contains effective socket address.
    pub fn effective_socket(&self) -> bool {
        self.socket != SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0)
    }

    /// change socket port to 0, and bind generate by system.
    pub fn zero_port(&mut self) {
        self.socket.set_port(0)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != PEER_LENGTH {
            return Err(new_io_error("peer bytes failure."));
        }

        let id = PeerId::from_bytes(&bytes[0..20])?;
        let assist = PeerId::from_bytes(&bytes[20..40])?;
        let socket = socket_addr_from_bytes(&bytes[40..58])?;
        let transport = TransportType::from_byte(bytes[58])?;
        let is_pub = bytes[59] == 1u8;
        Ok(Self {
            id,
            assist,
            socket,
            transport,
            is_pub,
        })
    }
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = vec![];
        bytes.append(&mut self.id.to_bytes()); // 20-bytes
        bytes.append(&mut self.assist.to_bytes()); // 20-bytes
        bytes.append(&mut socket_addr_to_bytes(&self.socket)); // 18-bytes
        bytes.push(self.transport.to_byte()); // 1-bytes
        bytes.push(if self.is_pub { 1u8 } else { 0u8 }); // 1-bytes
        bytes
    }

    /// Enhanced multiaddr, you can import/export it.
    /// 1 is ip version,
    /// 2 is bind ip address,
    /// 3 is transport type,
    /// 4 is bind port,
    /// 5 is open or not,
    /// 6 is peer id hex encode.
    /// example: "/ip4/127.0.0.1/tcp/1234/false/xxxxxx"
    pub fn to_string<'a>(&self) -> String {
        let v = if self.socket.is_ipv4() { "4" } else { "6" };

        format!(
            "/ip{}/{}/{}/{}/{}/{}",
            v,
            self.socket.ip(),
            self.transport.to_str(),
            self.socket.port(),
            self.is_pub,
            self.id.to_hex()
        )
    }

    /// from string exported to peer.
    pub fn from_string(s: &str) -> Result<Self> {
        let mut ss = s.split("/");
        let _ = ss.next(); // ipv4 / ipv6
        let ipaddr = ss
            .next()
            .ok_or(new_io_error("peer string is invalid."))?
            .parse()
            .or(Err(new_io_error("peer string is invalid.")))?; // safe
        let transport = TransportType::from_str(ss.next().unwrap()); // safe
        let port = ss
            .next()
            .ok_or(new_io_error("peer string is invalid."))?
            .parse()
            .or(Err(new_io_error("peer string is invalid.")))?; // safe
        let socket = SocketAddr::new(ipaddr, port);
        let is_pub: bool = ss
            .next()
            .ok_or(new_io_error("peer string is invalid."))?
            .parse()
            .or(Err(new_io_error("peer string is invalid.")))?;
        let id = PeerId::from_hex(ss.next().ok_or(new_io_error("peer string is invalid."))?)?;

        Ok(Self {
            id,
            is_pub,
            socket,
            transport,
            assist: PeerId::default(),
        })
    }

    /// only load this peer by socket and transport.
    /// example: "/ip4/127.0.0.1/tcp/1234"
    pub fn from_multiaddr_string(s: &str) -> Result<Self> {
        let mut ss = s.split("/");
        let _ = ss.next(); // ipv4 / ipv6
        let ipaddr = ss
            .next()
            .ok_or(new_io_error("peer string is invalid."))?
            .parse()
            .or(Err(new_io_error("peer string is invalid.")))?; // safe
        let transport = TransportType::from_str(ss.next().unwrap()); // safe
        let port = ss
            .next()
            .ok_or(new_io_error("peer string is invalid."))?
            .parse()
            .or(Err(new_io_error("peer string is invalid.")))?; // safe
        let socket = SocketAddr::new(ipaddr, port);

        Ok(Self {
            socket,
            transport,
            id: Default::default(),
            is_pub: true,
            assist: PeerId::default(),
        })
    }

    /// Multiaddr, you can import/export it.
    /// 1 is ip version,
    /// 2 is bind ip address,
    /// 3 is transport type,
    /// 4 is bind port
    /// example: "/ip4/127.0.0.1/tcp/1234"
    pub fn to_multiaddr_string(&self) -> String {
        let v = if self.socket.is_ipv4() { "4" } else { "6" };

        format!(
            "/ip{}/{}/{}/{}",
            v,
            self.socket.ip(),
            self.transport.to_str(),
            self.socket.port(),
        )
    }
}

impl Default for Peer {
    fn default() -> Self {
        Self {
            id: PeerId::default(),
            socket: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0),
            transport: TransportType::TCP,
            is_pub: true,
            assist: PeerId::default(),
        }
    }
}

impl Debug for Peer {
    fn fmt(&self, f: &mut Formatter) -> FmtResult {
        write!(f, "Peer: {:?} {}", self.id, self.to_multiaddr_string())
    }
}
