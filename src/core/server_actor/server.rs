use actix::prelude::*;
use std::net::SocketAddr;
use std::path::PathBuf;

use super::message::*;
use crate::core::peer_id::PeerID;

pub struct ActorServer {
    peer_id: PeerID,
    recipient_p2p: Recipient<P2PMessage>,
    recipient_peer_join: Recipient<PeerJoin>,
    recipient_peer_leave: Recipient<PeerLeave>,
}

impl ActorServer {
    pub fn load(
        socket: SocketAddr,
        path: PathBuf,
        recipient_p2p: Recipient<P2PMessage>,
        recipient_peer_join: Recipient<PeerJoin>,
        recipient_peer_leave: Recipient<PeerLeave>,
    ) -> Self {
        let peer_id = PeerID::default();

        ActorServer {
            peer_id,
            recipient_p2p,
            recipient_peer_join,
            recipient_peer_leave,
        }
    }

    pub fn peer_id(&self) -> &PeerID {
        &self.peer_id
    }
}

impl Actor for ActorServer {
    type Context = Context<Self>;
}

impl Handler<P2PMessage> for ActorServer {
    type Result = ();

    fn handle(&mut self, msg: P2PMessage, _ctx: &mut Context<Self>) {
        let (peer_id, data) = (msg.0, msg.1);
    }
}

impl Handler<PeerJoinResult> for ActorServer {
    type Result = ();

    fn handle(&mut self, msg: PeerJoinResult, _ctx: &mut Context<Self>) {
        let (peer_id, is_joined) = (msg.0, msg.1);
    }
}
