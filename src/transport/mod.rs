use actix::prelude::{Addr, Message};
use bytes::Bytes;
use futures::stream::Stream;
use futures::Async;
use std::net::SocketAddr;
use tokio::codec::BytesCodec;
use tokio::io::{AsyncRead, AsyncWrite, Error, ErrorKind};
use tokio::net::{TcpListener, TcpStream};
use tokio::net::{UdpFramed, UdpSocket};

use crate::core::peer_id::PeerID;
use crate::core::server_actor::server::ActorServer;
use crate::protocol::keys::{PrivateKey, PublicKey};

mod quic;
mod tcp;
mod udp;

// pub use ... as TransportReadFrame;
// pub use ... as TransportWriteFrame;

pub(crate) fn listen_tcp(
    self_peer_id: PeerID,
    self_pk: PublicKey,
    self_psk: PrivateKey,
    server_addr: Addr<ActorServer>,
    addr: &SocketAddr,
) -> Result<TcpStream, Error> {
    let mut listener = TcpListener::bind(&addr).unwrap();
    match listener.poll_accept() {
        Ok(Async::Ready((socket, _addr))) => Ok(socket),
        Ok(Async::NotReady) => Err(Error::new(ErrorKind::Other, "oh no!")),
        Err(_e) => Err(Error::new(ErrorKind::Other, "oh no!")),
    }
}

pub(crate) fn listen_udp(
    self_peer_id: PeerID,
    self_pk: PublicKey,
    self_psk: PrivateKey,
    server_addr: Addr<ActorServer>,
    addr: &SocketAddr,
) -> Result<UdpFramed<BytesCodec>, Error> {
    let sock = UdpSocket::bind(&addr).unwrap();
    Ok(UdpFramed::new(sock, BytesCodec::new()))
}

pub(crate) struct BytesMessage(pub Bytes);

impl Message for BytesMessage {
    type Result = ();
}
