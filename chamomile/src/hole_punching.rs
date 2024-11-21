use std::io::Result;
use std::net::SocketAddr;

use chamomile_types::{
    peer::{Peer, PEER_LENGTH},
    types::{new_io_error, PeerId, TransportType},
};

use super::peer_list::PeerList;

pub enum Hole {
    StunOne,
    StunTwo,
    Help,
}

pub struct DHT(pub Vec<Peer>);

impl Hole {
    pub fn from_byte(byte: u8) -> Result<Self> {
        match byte {
            0u8 => Ok(Hole::Help),
            1u8 => Ok(Hole::StunOne),
            2u8 => Ok(Hole::StunTwo),
            _ => Err(new_io_error("Hole bytes failure.")),
        }
    }

    pub fn to_byte(&self) -> u8 {
        match self {
            Hole::Help => 0u8,
            Hole::StunOne => 1u8,
            Hole::StunTwo => 2u8,
        }
    }
}

impl DHT {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < 4 {
            return Err(new_io_error("DHT bytes failure."));
        }
        let mut len_bytes = [0u8; 4];
        len_bytes.copy_from_slice(&bytes[0..4]);
        let len = u32::from_le_bytes(len_bytes) as usize;
        let raw_bytes = &bytes[4..];
        if raw_bytes.len() < len * PEER_LENGTH {
            return Err(new_io_error("DHT bytes failure."));
        }
        let mut peers = vec![];
        for i in 0..len {
            peers.push(Peer::from_bytes(
                &raw_bytes[i * PEER_LENGTH..(i + 1) * PEER_LENGTH],
            )?);
        }
        Ok(Self(peers))
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = vec![];
        bytes.extend(&(self.0.len() as u32).to_le_bytes());
        for peer in &self.0 {
            bytes.append(&mut peer.to_bytes());
        }
        bytes
    }
}

pub fn nat(mut remote_addr: SocketAddr, mut local: Peer) -> Peer {
    local.is_pub = remote_addr.port() == local.socket.port();
    match local.transport {
        TransportType::TCP => {
            remote_addr.set_port(local.socket.port()); // TODO TCP hole punching
        }
        _ => {}
    }

    local.socket = remote_addr;
    local
}

pub(crate) async fn _handle(_remote_peer: &PeerId, hole: Hole, _peers: &PeerList) -> Result<()> {
    match hole {
        Hole::StunOne => {
            // first test
        }
        Hole::StunTwo => {
            // secound test
        }
        Hole::Help => {
            // help hole
        }
    }

    Ok(())
}
