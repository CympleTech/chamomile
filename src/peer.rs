use serde::{Deserialize, Serialize};
use std::fmt::{Debug, Formatter, Result as FmtResult};
use std::net::SocketAddr;

use chamomile_types::types::{PeerId, TransportType};

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

impl Debug for Peer {
    fn fmt(&self, f: &mut Formatter) -> FmtResult {
        write!(f, "Peer: {:?}", self.id)
    }
}
