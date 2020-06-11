use serde::{Deserialize, Serialize};
use std::fmt::{Debug, Formatter, Result as FmtResult};
use std::net::SocketAddr;

use crate::transports::TransportType;

#[derive(Copy, Clone, Default, Eq, PartialEq, Ord, PartialOrd, Hash, Deserialize, Serialize)]
pub struct PeerId(pub [u8; 32]);

#[derive(Copy, Clone, Deserialize, Serialize)]
pub struct Peer {
    id: PeerId,
    addr: SocketAddr,
    transport: TransportType,
    is_pub: bool,
}

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
}

impl PeerId {
    pub fn short_show(&self) -> String {
        let mut hex = String::new();
        hex.extend(self.0.iter().map(|byte| format!("{:02x?}", byte)));
        let mut new_hex = String::new();
        new_hex.push_str("0x");
        new_hex.push_str(&hex[0..4]);
        new_hex.push_str("...");
        new_hex.push_str(&hex[hex.len() - 5..]);
        new_hex
    }

    pub fn from_hex(s: impl ToString) -> Result<PeerId, ()> {
        let s = s.to_string();
        if s.len() != 64 {
            return Err(());
        }

        let mut value = [0u8; 32];

        for i in 0..(s.len() / 2) {
            let res = u8::from_str_radix(&s[2 * i..2 * i + 2], 16).map_err(|_e| ())?;
            value[i] = res;
        }

        Ok(PeerId(value))
    }

    pub fn to_hex(&self) -> String {
        let mut hex = String::new();
        hex.extend(self.0.iter().map(|byte| format!("{:02x?}", byte)));
        hex
    }
}

impl Debug for PeerId {
    fn fmt(&self, f: &mut Formatter) -> FmtResult {
        let mut hex = String::new();
        hex.extend(self.0.iter().map(|byte| format!("{:02x?}", byte)));
        write!(f, "0x{}", hex)
    }
}

impl Debug for Peer {
    fn fmt(&self, f: &mut Formatter) -> FmtResult {
        write!(f, "Peer: {:?}", self.id)
    }
}
