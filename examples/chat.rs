// use std::collections::{HashMap, HashSet};
// use std::net::SocketAddr;
// use std::path::PathBuf;

// use chamomile::actor::{
//     actix::*, DirectP2PMessage, PeerJoin, PeerJoinResult, PeerLeave, ServerActor, SpecialP2PMessage,
// };
// use chamomile::prelude::{PeerID, TransportType};

// /// Define tcp server that will accept incoming tcp connection and create
// /// chat actors.
// struct ChatServer {
//     name: String,
//     peers: HashMap<String, PeerID>,
//     p2p_server: Addr<ServerActor>,
// }

// impl Actor for ChatServer {
//     type Context = Context<Self>;
// }

// impl Handler<DirectP2PMessage> for ChatServer {
//     type Result = ();

//     fn handle(&mut self, msg: DirectP2PMessage, _ctx: &mut Context<Self>) {
//         let (peer_id, data) = (msg.0, msg.1);
//     }
// }

// impl Handler<PeerJoin> for ChatServer {
//     type Result = ();

//     fn handle(&mut self, msg: PeerJoin, _ctx: &mut Context<Self>) {
//         let (peer_id, join_data) = (msg.0, msg.1);
//     }
// }

// impl Handler<PeerLeave> for ChatServer {
//     type Result = ();

//     fn handle(&mut self, msg: PeerLeave, _ctx: &mut Context<Self>) {
//         let peer_id = msg.0;
//     }
// }

// fn main() -> std::io::Result<()> {
//     System::run(|| {
//         let path: PathBuf = PathBuf::from("./tea");

//         ChatServer::create(move |ctx| {
//             let addr = ctx.address();

//             let server = ServerActor::load(
//                 path,
//                 addr.clone().recipient::<DirectP2PMessage>(),
//                 addr.clone().recipient::<PeerJoin>(),
//                 addr.recipient::<PeerLeave>(),
//             );

//             let peer_id = server.peer_id();
//             println!("DEBUG PeerID: {:?}", peer_id);

//             let server_addr = server.start();

//             ChatServer {
//                 name: "test".to_owned(),
//                 peers: HashMap::new(),
//                 p2p_server: server_addr,
//             }
//         });
//     })
// }
