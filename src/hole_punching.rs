use serde::{Deserialize, Serialize};
use smol::io::Result;
use std::net::SocketAddr;

use chamomile_types::types::{PeerId, TransportType};

use super::peer::Peer;
use super::peer_list::PeerList;

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
