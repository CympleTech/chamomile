mod quic;
mod tcp;
mod udp;

use actix::prelude::Addr;
use multiaddr::{AddrComponent, Multiaddr};
use std::net::SocketAddr;

use crate::core::peer_id::PeerID;
use crate::core::server::ServerActor;
use crate::protocol::keys::{PrivateKey, PublicKey};

pub enum TransportType {
    TCP,
    UDP,
    QUIC,
}

impl TransportType {
    pub fn to_multiaddr(&self, socket: &SocketAddr) -> Multiaddr {
        let port = socket.port();

        let ip_component = match socket {
            SocketAddr::V4(v4_sock) => AddrComponent::IP4(v4_sock.ip().clone()),
            SocketAddr::V6(v6_sock) => AddrComponent::IP6(v6_sock.ip().clone()),
        };

        let mut multi_addr: Multiaddr = ip_component.into();

        let proto_component = match self {
            TCP => AddrComponent::TCP(port),
            UDP => AddrComponent::UDP(port),
            QUIC => AddrComponent::UDP(port),
        };

        multi_addr.append(proto_component);

        match self {
            QUIC => multi_addr.append(AddrComponent::QUIC),
            _ => {}
        }

        multi_addr
    }
}

pub(crate) fn listen_tcp(
    self_peer_id: PeerID,
    self_pk: PublicKey,
    self_psk: PrivateKey,
    server_addr: Addr<ServerActor>,
    addr: &SocketAddr,
) {

}

pub(crate) fn listen_udp(
    self_peer_id: PeerID,
    self_pk: PublicKey,
    self_psk: PrivateKey,
    server_addr: Addr<ServerActor>,
    addr: &SocketAddr,
) {

}

pub(crate) fn listen_quic(
    self_peer_id: PeerID,
    self_pk: PublicKey,
    self_psk: PrivateKey,
    server_addr: Addr<ServerActor>,
    addr: &SocketAddr,
) {

}
