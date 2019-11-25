use async_std::io::Result;
use async_std::sync::{channel, Sender, Receiver};
use async_trait::async_trait;
use std::net::SocketAddr;

mod udp;
mod tcp;
mod rtp;
mod udt;

/// max task capacity for udp to handle.
pub const MAX_MESSAGE_CAPACITY: usize = 1024;

/// Message Type for transport and outside.
/// a tuple struct.
pub type EndpointMessage = (Vec<u8>, SocketAddr);

pub fn new_channel() -> (Sender<EndpointMessage>, Receiver<EndpointMessage>) {
    channel(MAX_MESSAGE_CAPACITY)
}

/// Transports trait, all transport protocol will implement this.
#[async_trait]
pub trait Endpoint {
    /// Init and run a Endpoint object.
    /// You need send a bind-socketaddr and received message's addr,
    /// and return the endpoint's sender addr.
    async fn start(
        bind_addr: SocketAddr,
        send_channel: Sender<EndpointMessage>,
    ) -> Result<Sender<EndpointMessage>>;
}

pub use udp::UdpEndpoint;
pub use tcp::TcpEndpoint;

//TODO pub use rtp::RtpEndpoint;
//TODO pub use udt::UdtEndpoint;

// use actix::prelude::{Addr, Message, Recipient};
// use bytes::Bytes;
// use multiaddr::{AddrComponent, Multiaddr};
// use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

// use crate::core::peer_id::PeerID;
// use crate::core::server::ServerActor;
// use crate::core::session::SessionCreate;
// use crate::protocol::keys::{PrivateKey, PublicKey};

// use quic::start_quic;
// use tcp::start_tcp;

// #[derive(Hash, Eq, PartialEq, Clone, Debug)]
// pub enum TransportType {
//     TCP,
//     UDP,
//     QUIC,
//     KCP,
// }

// impl Default for TransportType {
//     fn default() -> Self {
//         TransportType::TCP
//     }
// }

// impl TransportType {
//     pub fn from_multiaddr(multiaddr: &Multiaddr) -> Self {
//         // TODO need improve
//         for v in multiaddr.iter() {
//             match v {
//                 AddrComponent::TCP(p) => return TransportType::TCP,
//                 AddrComponent::UDP(p) => return TransportType::UDP,
//                 _ => {}
//             }
//         }

//         Default::default()
//     }

//     pub fn to_multiaddr(&self, socket: &SocketAddr) -> Multiaddr {
//         let port = socket.port();

//         let ip_component = match socket {
//             SocketAddr::V4(v4_sock) => AddrComponent::IP4(v4_sock.ip().clone()),
//             SocketAddr::V6(v6_sock) => AddrComponent::IP6(v6_sock.ip().clone()),
//         };

//         let mut multi_addr: Multiaddr = ip_component.into();

//         let proto_component = match self {
//             TransportType::TCP => AddrComponent::TCP(port),
//             TransportType::UDP => AddrComponent::UDP(port),
//             TransportType::QUIC => AddrComponent::UDP(port),
//             TransportType::KCP => AddrComponent::UDP(port),
//         };

//         multi_addr.append(proto_component);

//         match self {
//             TransportType::QUIC => multi_addr.append(AddrComponent::QUIC),
//             _ => {}
//         }

//         multi_addr
//     }

//     pub fn extract_socket(addr: &Multiaddr) -> SocketAddr {
//         let mut ip_string: String = "0.0.0.0".to_owned();
//         let mut port: u16 = 0;

//         for v in addr.iter() {
//             match v {
//                 AddrComponent::IP4(ip) => ip_string = format!("{}", ip),
//                 AddrComponent::IP6(ip) => ip_string = format!("{}", ip),
//                 AddrComponent::TCP(p) => port = p,
//                 AddrComponent::UDP(p) => port = p,
//                 _ => {}
//             }
//         }

//         SocketAddr::new(ip_string.parse().unwrap(), port)
//     }

//     pub(crate) fn start_listener(
//         &self,
//         self_peer_id: PeerID,
//         self_pk: PublicKey,
//         self_psk: PrivateKey,
//         server_addr: Addr<ServerActor>,
//         multiaddr: &Multiaddr,
//     ) -> Recipient<SessionCreate> {
//         let addr = Self::extract_socket(multiaddr);

//         match self {
//             TransportType::TCP => listen_tcp(self_peer_id, self_pk, self_psk, server_addr, addr),
//             TransportType::UDP => listen_udp(self_peer_id, self_pk, self_psk, server_addr, addr),
//             TransportType::QUIC => listen_quic(self_peer_id, self_pk, self_psk, server_addr, addr),
//             TransportType::KCP => listen_kcp(self_peer_id, self_pk, self_psk, server_addr, addr),
//         }
//     }
// }

// pub(crate) fn listen_quic(
//     self_peer_id: PeerID,
//     _self_pk: PublicKey,
//     _self_psk: PrivateKey,
//     server_addr: Addr<ServerActor>,
//     addr: SocketAddr,
// ) -> Recipient<SessionCreate> {
//     start_quic(self_peer_id, server_addr, addr)
// }

// pub(crate) fn listen_tcp(
//     self_peer_id: PeerID,
//     self_pk: PublicKey,
//     self_psk: PrivateKey,
//     server_addr: Addr<ServerActor>,
//     addr: SocketAddr,
// ) -> Recipient<SessionCreate> {
//     start_tcp(self_peer_id, self_pk, self_psk, server_addr, addr)
// }

// pub(crate) fn listen_udp(
//     self_peer_id: PeerID,
//     _self_pk: PublicKey,
//     _self_psk: PrivateKey,
//     server_addr: Addr<ServerActor>,
//     addr: SocketAddr,
// ) -> Recipient<SessionCreate> {
//     start_quic(self_peer_id, server_addr, addr)
// }

// pub(crate) fn listen_kcp(
//     self_peer_id: PeerID,
//     _self_pk: PublicKey,
//     _self_psk: PrivateKey,
//     server_addr: Addr<ServerActor>,
//     addr: SocketAddr,
// ) -> Recipient<SessionCreate> {
//     start_quic(self_peer_id, server_addr, addr)
// }
