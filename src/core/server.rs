use actix::prelude::*;
use multiaddr::Multiaddr;
use rckad::KadTree;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;

use super::message::*;
use super::peer_list::PeerList;
use super::session::{SessionClose, SessionCreate, SessionOpen, SessionReceive, SessionSend};
use crate::core::peer_id::PeerID;
use crate::protocol::keys::{PrivateKey, PublicKey};
use crate::transports::TransportType;

pub struct ServerActor {
    peer_id: PeerID,
    peer_pk: PublicKey,
    peer_psk: PrivateKey,
    recipient_p2p: Recipient<P2PMessage>,
    recipient_peer_join: Recipient<PeerJoin>,
    recipient_peer_leave: Recipient<PeerLeave>,
    sessions: HashMap<PeerID, Recipient<SessionSend>>,
    peer_list: PeerList,
    main_transport_type: TransportType,
    running_transports: HashMap<TransportType, Recipient<SessionCreate>>,
}

impl ServerActor {
    pub fn load(
        main_transport_type: TransportType,
        path: PathBuf,
        recipient_p2p: Recipient<P2PMessage>,
        recipient_peer_join: Recipient<PeerJoin>,
        recipient_peer_leave: Recipient<PeerLeave>,
    ) -> Self {
        let socket: SocketAddr = "127.0.0.1:8000".parse().unwrap();
        let peer_id = PeerID::default();
        let sessions = HashMap::new();
        let peer_pk = Default::default();
        let peer_psk = Default::default();
        let peer_list = PeerList::init(peer_id.clone(), main_transport_type.to_multiaddr(&socket));
        let running_transports = HashMap::new();

        ServerActor {
            peer_id,
            peer_pk,
            peer_psk,
            recipient_p2p,
            recipient_peer_join,
            recipient_peer_leave,
            sessions,
            peer_list,
            main_transport_type,
            running_transports,
        }
    }

    pub fn peer_id(&self) -> &PeerID {
        &self.peer_id
    }
}

impl Actor for ServerActor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Context<Self>) {
        let socket: SocketAddr = "127.0.0.1:8001".parse().unwrap();
        let test_sock: SocketAddr = "127.0.0.1:8000".parse().unwrap();

        println!("DEBUG: server listening: {}", socket);
        let session_create = self.main_transport_type.start_listener(
            self.peer_id.clone(),
            self.peer_pk.clone(),
            self.peer_psk.clone(),
            ctx.address(),
            socket,
        );

        let tt = TransportType::TCP;

        let _ = session_create.do_send(SessionCreate(tt.to_multiaddr(&test_sock)));

        self.running_transports
            .insert(self.main_transport_type.clone(), session_create);
    }
}

impl Handler<P2PMessage> for ServerActor {
    type Result = ();

    fn handle(&mut self, msg: P2PMessage, _ctx: &mut Context<Self>) {
        let (_peer_id, _data) = (msg.0, msg.1);
    }
}

impl Handler<PeerJoinResult> for ServerActor {
    type Result = ();

    fn handle(&mut self, msg: PeerJoinResult, _ctx: &mut Context<Self>) {
        let (_peer_id, _is_joined) = (msg.0, msg.1);
    }
}

impl Handler<SessionOpen> for ServerActor {
    type Result = ();

    fn handle(&mut self, msg: SessionOpen, _ctx: &mut Context<Self>) {
        let (_peer_id, multiaddr, writer) = (msg.0, msg.1, msg.2);
        println!("DEBUG: ServerActor connect open: {}", multiaddr);
        let _ = writer.do_send(SessionSend(vec![2, 4, 6, 8], false));
    }
}

impl Handler<SessionClose> for ServerActor {
    type Result = ();

    fn handle(&mut self, _msg: SessionClose, _ctx: &mut Context<Self>) {
        println!("DEBUG: ServerActor connect close");
    }
}

impl Handler<SessionReceive> for ServerActor {
    type Result = ();

    fn handle(&mut self, msg: SessionReceive, _ctx: &mut Context<Self>) {
        let (_peer_id, _data) = (msg.0, msg.1);
    }
}
