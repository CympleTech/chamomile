use actix::prelude::*;
use rckad::KadTree;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;

use super::message::*;
use super::session::{SessionActor, SessionClose, SessionOpen, SessionReceive, SessionSend};
use crate::core::peer_id::PeerID;

pub struct ActorServer {
    peer_id: PeerID,
    recipient_p2p: Recipient<P2PMessage>,
    recipient_peer_join: Recipient<PeerJoin>,
    recipient_peer_leave: Recipient<PeerLeave>,
    sessions: HashMap<PeerID, Addr<SessionActor>>,
    dht: KadTree<PeerID, SocketAddr>,
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
        let sessions = HashMap::new();
        let dht = KadTree::new(peer_id.clone(), socket.clone());

        ActorServer {
            peer_id,
            recipient_p2p,
            recipient_peer_join,
            recipient_peer_leave,
            sessions,
            dht,
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
        let (_peer_id, _data) = (msg.0, msg.1);
    }
}

impl Handler<PeerJoinResult> for ActorServer {
    type Result = ();

    fn handle(&mut self, msg: PeerJoinResult, _ctx: &mut Context<Self>) {
        let (_peer_id, _is_joined) = (msg.0, msg.1);
    }
}

impl Handler<SessionOpen> for ActorServer {
    type Result = ();

    fn handle(&mut self, msg: SessionOpen, _ctx: &mut Context<Self>) {
        let (_peer_id, _socket) = (msg.0, msg.1);
    }
}

impl Handler<SessionClose> for ActorServer {
    type Result = ();

    fn handle(&mut self, msg: SessionClose, _ctx: &mut Context<Self>) {
        let _peer_id = msg.0;
    }
}

impl Handler<SessionReceive> for ActorServer {
    type Result = ();

    fn handle(&mut self, msg: SessionReceive, _ctx: &mut Context<Self>) {
        let (_peer_id, _data) = (msg.0, msg.1);
    }
}
