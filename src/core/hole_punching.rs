use async_std::{
    io::Result,
    sync::{Receiver, Sender},
};
use serde_derive::{Deserialize, Serialize};
use std::net::SocketAddr;

use super::peer_id::PeerId;
use super::peer_list::PeerTable;
use super::transport::Transport;
use crate::transports::StreamMessage;

#[derive(Deserialize, Serialize)]
pub enum HOLE {
    STUN_ONE,
    STUN_TWO,
    HELP,
}

pub fn nat(mut remote_addr: SocketAddr, local_addr: Transport) -> Transport {
    let is_pub = remote_addr.port() == local_addr.addr().port();
    match local_addr {
        Transport::UDP(_addr, pre) | Transport::UDT(_addr, pre) | Transport::RTP(_addr, pre) => {
            Transport::UDP(remote_addr, pre && is_pub)
        }
        Transport::TCP(addr, pre) => {
            remote_addr.set_port(addr.port());
            Transport::TCP(remote_addr, pre && is_pub)
        } // TODO TCP hole punching
    }
}

pub async fn handle(remote_peer: &PeerId, hole: HOLE, peers: &PeerTable) -> Result<()> {
    match hole {
        HOLE::STUN_ONE => {
            // first test
        }
        HOLE::STUN_TWO => {
            // secound test
        }
        HOLE::HELP => {
            // help hole
        }
    }

    Ok(())
}
