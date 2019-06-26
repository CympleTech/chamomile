use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::path::PathBuf;

use chamomile::actor::actix::*;
use chamomile::actor::{
    ActorServer, P2PMessage, PeerID, PeerJoin, PeerJoinResult, PeerLeave, SpecialP2PMessage,
};

/// Define tcp server that will accept incoming tcp connection and create
/// chat actors.
struct ChatServer {
    name: String,
    peers: HashMap<String, PeerID>,
    p2p_server: Addr<ActorServer>,
}

impl Actor for ChatServer {
    type Context = Context<Self>;
}

impl Handler<P2PMessage> for ChatServer {
    type Result = ();

    fn handle(&mut self, msg: P2PMessage, _ctx: &mut Context<Self>) {
        let (peer_id, data) = (msg.0, msg.1);
    }
}

impl Handler<PeerJoin> for ChatServer {
    type Result = ();

    fn handle(&mut self, msg: PeerJoin, _ctx: &mut Context<Self>) {
        let (peer_id, join_data) = (msg.0, msg.1);
    }
}

impl Handler<PeerLeave> for ChatServer {
    type Result = ();

    fn handle(&mut self, msg: PeerLeave, _ctx: &mut Context<Self>) {
        let peer_id = msg.0;
    }
}

fn main() -> std::io::Result<()> {
    System::run(|| {
        let socket: SocketAddr = "0.0.0.0:12345".parse().unwrap();
        let path: PathBuf = PathBuf::from("./tea");

        ChatServer::create(move |ctx| {
            let addr = ctx.address();

            let server = ActorServer::load(
                socket,
                path,
                addr.clone().recipient::<P2PMessage>(),
                addr.clone().recipient::<PeerJoin>(),
                addr.recipient::<PeerLeave>(),
            );

            let peer_id = server.peer_id();
            println!("DEBUG PeerID: {:?}", peer_id);

            let server_addr = server.start();

            ChatServer {
                name: "test".to_owned(),
                peers: HashMap::new(),
                p2p_server: server_addr,
            }
        });

        println!("Running chat server on 127.0.0.1:12345");
    })
}
