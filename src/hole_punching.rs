use async_std::{
    io::Result,
    sync::{Receiver, Sender},
};
use serde_derive::{Deserialize, Serialize};
use std::net::SocketAddr;

use super::peer::{Peer, PeerId};
use super::peer_list::PeerList;
use crate::transports::TransportType;

#[derive(Deserialize, Serialize)]
pub(crate) enum Hole {
    StunOne,
    StunTwo,
    Help,
}

#[derive(Deserialize, Serialize)]
pub(crate) struct DHT(pub Vec<Peer>);

pub fn nat(mut remote_addr: SocketAddr, mut local: Peer) -> Peer {
    match local.transport() {
        TransportType::TCP => {
            remote_addr.set_port(local.addr().port()); // TODO TCP hole punching
        }
        _ => {}
    }

    local.set_addr(remote_addr);
    local.set_is_pub(remote_addr.port() == local.addr().port());
    local
}

pub(crate) async fn handle(remote_peer: &PeerId, hole: Hole, peers: &PeerList) -> Result<()> {
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
