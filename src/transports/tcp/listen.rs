use std::net::SocketAddr;
use std::str::FromStr;

use actix::io::FramedWrite;
use actix::prelude::*;
use futures::Stream;
use multiaddr::Multiaddr;
use socket2::{Domain, Socket, Type};
use tokio::codec::BytesCodec;
use tokio::codec::FramedRead;
use tokio::io::AsyncRead;
use tokio::net::{TcpListener, TcpStream};
use tokio::reactor::Handle;

use crate::core::peer_id::PeerID;
use crate::core::server::ServerActor;
use crate::core::session::SessionCreate;
use crate::protocol::keys::{PrivateKey, PublicKey};

use super::super::TransportType;
use super::session::TcpSessionActor;

struct TcpListenActor {
    self_peer_id: PeerID,
    self_pk: PublicKey,
    self_psk: PrivateKey,
    self_socket: SocketAddr,
    self_multiaddr: Multiaddr,
    server_addr: Addr<ServerActor>,
}

impl Actor for TcpListenActor {
    type Context = Context<Self>;
}

struct TcpConnect(pub TcpStream, pub SocketAddr);

impl Message for TcpConnect {
    type Result = ();
}

impl Handler<TcpConnect> for TcpListenActor {
    type Result = ();

    fn handle(&mut self, msg: TcpConnect, _: &mut Context<Self>) {
        println!("DEBUG: ListenActor receive connect TCP: {}", msg.1);

        let server_addr = self.server_addr.clone();
        let self_peer_id = self.self_peer_id.clone();
        let self_pk = self.self_pk.clone();
        let self_psk = self.self_psk.clone();
        let self_multiaddr = self.self_multiaddr.clone();

        TcpSessionActor::create(move |ctx| {
            let (r, w) = msg.0.split();
            TcpSessionActor::add_stream(FramedRead::new(r, BytesCodec::new()), ctx);
            TcpSessionActor::new(
                self_peer_id,
                self_pk,
                self_psk,
                self_multiaddr,
                server_addr,
                FramedWrite::new(w, BytesCodec::new(), ctx),
                msg.1,
            )
        });
    }
}

impl Handler<SessionCreate> for TcpListenActor {
    type Result = ();

    fn handle(&mut self, msg: SessionCreate, _ctx: &mut Context<Self>) -> Self::Result {
        let remote_socket = TransportType::extract_socket(&msg.0);

        println!(
            "DEBUG: ListenActor create session to: {}, real: {}",
            msg.0, remote_socket
        );

        let socket = Socket::new(Domain::ipv4(), Type::stream(), None).unwrap();
        socket.set_reuse_address(true).unwrap();

        let reuse_result = socket.bind(&self.self_socket.into());
        if reuse_result.is_ok() {
            println!("DEBUG: ListenActor reuse ok! Hole Punching will on TCP");
        }

        let connect_result = socket.connect(&remote_socket.into());
        if connect_result.is_err() {
            println!("DEBUG: ListenActor Connect to remote failure");
            return;
        }

        let std_stream = socket.into_tcp_stream();
        let stream = TcpStream::from_std(std_stream, &Handle::default()).unwrap();

        let server_addr = self.server_addr.clone();
        let self_peer_id = self.self_peer_id.clone();
        let self_pk = self.self_pk.clone();
        let self_psk = self.self_psk.clone();
        let self_multiaddr = self.self_multiaddr.clone();

        TcpSessionActor::create(move |ctx| {
            let (r, w) = stream.split();
            TcpSessionActor::add_stream(FramedRead::new(r, BytesCodec::new()), ctx);
            TcpSessionActor::new(
                self_peer_id,
                self_pk,
                self_psk,
                self_multiaddr,
                server_addr,
                FramedWrite::new(w, BytesCodec::new(), ctx),
                remote_socket,
            )
        });
    }
}

pub(crate) fn start_tcp(
    self_peer_id: PeerID,
    self_pk: PublicKey,
    self_psk: PrivateKey,
    server_addr: Addr<ServerActor>,
    addr: SocketAddr,
) -> Recipient<SessionCreate> {
    let socket = Socket::new(Domain::ipv4(), Type::stream(), None).unwrap();
    socket.set_reuse_address(true).unwrap();
    socket.bind(&addr.into()).unwrap();
    socket.listen(128).unwrap();
    let std_listener = socket.into_tcp_listener();

    let listener = TcpListener::from_std(std_listener, &Handle::default()).unwrap();

    TcpListenActor::create(move |ctx| {
        ctx.add_message_stream(listener.incoming().map_err(|_| ()).map(|st| {
            let addr = st.peer_addr().unwrap();
            TcpConnect(st, addr)
        }));

        let self_multiaddr = TransportType::TCP.to_multiaddr(&addr);

        TcpListenActor {
            self_peer_id: self_peer_id,
            self_socket: addr,
            self_pk: self_pk,
            self_psk: self_psk,
            server_addr: server_addr,
            self_multiaddr: self_multiaddr,
        }
    })
    .recipient::<SessionCreate>()
}
