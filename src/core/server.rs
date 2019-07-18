use actix::prelude::*;
use std::collections::HashMap;
use std::path::PathBuf;

use super::config::Configure;
use super::message::*;
use super::peer_list::{NodeAddr, PeerList};
use super::session::{SessionClose, SessionCreate, SessionOpen, SessionReceive, SessionSend};
use crate::core::peer_id::PeerID;
use crate::protocol::keys::{PrivateKey, PublicKey};
use crate::transports::TransportType;

pub struct ServerActor {
    peer_id: PeerID,
    peer_pk: PublicKey,
    peer_psk: PrivateKey,
    recipient_p2p: Recipient<DirectP2PMessage>,
    recipient_peer_join: Recipient<PeerJoin>,
    recipient_peer_leave: Recipient<PeerLeave>,
    peer_list: PeerList,
    transport_config: Configure,
    running_transports: HashMap<TransportType, Recipient<SessionCreate>>,
}

impl ServerActor {
    pub fn load(
        path: PathBuf,
        recipient_p2p: Recipient<DirectP2PMessage>,
        recipient_peer_join: Recipient<PeerJoin>,
        recipient_peer_leave: Recipient<PeerLeave>,
    ) -> Self {
        let transport_config = Configure::load();
        let peer_id = PeerID::default();
        let peer_pk = Default::default();
        let peer_psk = Default::default();

        let peer_list = PeerList::init(peer_id.clone());
        let running_transports = HashMap::new();

        ServerActor {
            peer_id,
            peer_pk,
            peer_psk,
            recipient_p2p,
            recipient_peer_join,
            recipient_peer_leave,
            peer_list,
            transport_config,
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
        let main_multiaddr = self.transport_config.main_multiaddr();
        println!("DEBUG: server listening: {}", main_multiaddr);

        let session_create = self.transport_config.main_transport.start_listener(
            self.peer_id.clone(),
            self.peer_pk.clone(),
            self.peer_psk.clone(),
            ctx.address(),
            main_multiaddr,
        );

        for maddr in self.transport_config.bootstraps.iter() {
            session_create.do_send(SessionCreate(maddr.clone()));
        }

        self.running_transports
            .insert(self.transport_config.main_transport.clone(), session_create);
    }
}

impl Handler<DirectP2PMessage> for ServerActor {
    type Result = ();

    fn handle(&mut self, msg: DirectP2PMessage, _ctx: &mut Context<Self>) {
        let (peer_id, data) = (msg.0, msg.1);
        self.peer_list.get(&peer_id).and_then(|node| {
            node.session()
                .do_send(SessionSend(peer_id, data, false))
                .ok()
        });
    }
}

impl Handler<BroadcastP2PMessage> for ServerActor {
    type Result = ();

    fn handle(&mut self, msg: BroadcastP2PMessage, _ctx: &mut Context<Self>) {
        let data = msg.0;
        self.peer_list.all().into_iter().map(|(peer_id, session)| {
            session
                .do_send(SessionSend(peer_id, data.clone(), false))
                .ok()
        });
    }
}

impl Handler<PeerJoinResult> for ServerActor {
    type Result = ();

    fn handle(&mut self, msg: PeerJoinResult, _ctx: &mut Context<Self>) {
        let (peer_id, is_joined) = (msg.0, msg.1);
        if is_joined {
            self.peer_list.stabilize_tmp_peer(peer_id);
        }
    }
}

impl Handler<SessionOpen> for ServerActor {
    type Result = ();

    fn handle(&mut self, msg: SessionOpen, _ctx: &mut Context<Self>) {
        let (peer_id, multiaddr, writer, join_data) = (msg.0, msg.1, msg.2, msg.3);
        println!("DEBUG: ServerActor connect open: {}", multiaddr);

        self.peer_list
            .add_tmp_peer(peer_id.clone(), NodeAddr::new(multiaddr, writer));

        self.recipient_peer_join
            .do_send(PeerJoin(peer_id, join_data));
    }
}

impl Handler<SessionClose> for ServerActor {
    type Result = ();

    fn handle(&mut self, msg: SessionClose, _ctx: &mut Context<Self>) {
        println!("DEBUG: ServerActor connect close");
        self.peer_list.remove(&msg.0);
        self.recipient_peer_leave.do_send(PeerLeave(msg.0));
    }
}

impl Handler<SessionReceive> for ServerActor {
    type Result = ();

    fn handle(&mut self, msg: SessionReceive, _ctx: &mut Context<Self>) {
        let (from, to, data) = (msg.0, msg.1, msg.2);
        println!(
            "DEBUG: ServerActor receive peer: {:?}, to: {:?}, data: {:?}",
            from, to, data
        );

        if to == self.peer_id {
            self.recipient_p2p.do_send(DirectP2PMessage(from, data));
        } else {
            self.peer_list
                .get(&to)
                .and_then(|node| node.session().do_send(SessionSend(to, data, false)).ok());
        }
    }
}
