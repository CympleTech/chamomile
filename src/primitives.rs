//use multiaddr::Multiaddr;
//use serde_derive::{Deserialize, Serialize};
//use std::collections::HashMap;
//use std::net::{IpAddr, Ipv4Addr, SocketAddr};

//use crate::core::peer_id::PeerID;
//use crate::protocol::keys::PublicKey;
//use crate::transports::TransportType;

pub const MAX_MESSAGE_CAPACITY: usize = 1024;

pub const PEER_ID_LENGTH: usize = 42;

// lazy_static! {
//     pub static ref DEFAULT_TRANSPORT_SOCKET: HashMap<TransportType, Multiaddr> = {
//         let mut m = HashMap::new();
//         m.insert(
//             TransportType::TCP,
//             TransportType::TCP.to_multiaddr(&SocketAddr::new(
//                 IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
//                 7364,
//             )),
//         );
//         m.insert(
//             TransportType::UDP,
//             TransportType::UDP.to_multiaddr(&SocketAddr::new(
//                 IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
//                 7365,
//             )),
//         );
//         m.insert(
//             TransportType::KCP,
//             TransportType::KCP.to_multiaddr(&SocketAddr::new(
//                 IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
//                 7366,
//             )),
//         );
//         m.insert(
//             TransportType::QUIC,
//             TransportType::QUIC.to_multiaddr(&SocketAddr::new(
//                 IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
//                 7367,
//             )),
//         );
//         m
//     };
// }

// #[derive(Serialize, Deserialize)]
// pub enum DataType {
//     Identity(String, PublicKey), //String -> Multiaddr
//     DHT(Vec<(PeerID, String)>),  // String -> Multiaddr
//     DH(Vec<u8>),
//     RawData(PeerID, Vec<u8>), // to_peer_id, data
//     Hole(String),             // String -> Multiaddr
//     Ping,
//     Pong,
// }
