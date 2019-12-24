use async_std::{
    io::Result,
    prelude::*,
    sync::{Arc, Mutex, Receiver, Sender},
    task,
};
use futures::{select, FutureExt};
use std::collections::HashMap;
use std::net::SocketAddr;

use crate::transports::TcpEndpoint;
use crate::transports::UdpEndpoint;
use crate::transports::{new_channel, Endpoint, EndpointMessage, StreamMessage};
use crate::{Config, Message};

use super::keys::{KeyType, PrivateKey, PublicKey};
use super::peer_id::PeerId;
use super::peer_list::{PeerList, PeerTable};
use super::session::session_start;

#[derive(Debug, Hash)]
enum Transport {
    UDP(SocketAddr), // 0u8
    TCP(SocketAddr), // 1u8
}

impl Transport {
    fn symbol(&self) -> u8 {
        match self {
            &Transport::UDP(_) => 0u8,
            &Transport::TCP(_) => 1u8,
        }
    }
}

pub struct Server {
    peer_id: PeerId,
    pk: PublicKey,
    psk: PrivateKey,
    peer_list: PeerTable,
    while_list: PeerList,
    black_list: PeerList,
    default_transport: Transport,
    transports: HashMap<u8, Sender<EndpointMessage>>,
}

impl Server {
    fn new(config: Config) -> Self {
        // load or generate keypair
        let (psk, pk) = PrivateKey::generate(KeyType::Ed25519);
        let peer_id = pk.peer_id();

        // TODO load peers & merge config

        Self {
            peer_id,
            pk,
            psk,
            peer_list: PeerTable::init(peer_id),
            while_list: PeerList::default(),
            black_list: PeerList::default(),
            default_transport: Transport::TCP(config.addr),
            transports: HashMap::new(),
        }
    }

    pub fn start(config: Config, out_send: Sender<Message>, mut self_recv: Receiver<Message>) {
        task::spawn(async move {
            let server = Self::new(config);
            println!("server start peer id: {:?}", server.peer_id);

            // mock
            let (send, mut recv) = new_channel();
            let transport_send = match server.default_transport {
                Transport::UDP(addr) => {
                    UdpEndpoint::start(addr, server.peer_id.clone(), send.clone())
                        .await
                        .expect("UDP Transport binding failure!")
                }
                Transport::TCP(addr) => {
                    TcpEndpoint::start(addr, server.peer_id.clone(), send.clone())
                        .await
                        .expect("TCP Transport binding failure!")
                }
            };

            let m1 = Arc::new(Mutex::new(server));
            let m2 = m1.clone();

            loop {
                select! {
                    msg = recv.next().fuse() => match msg {
                        Some(message) => {
                            println!("Server: recv from transport: {:?}", message);
                            let mut server = m1.lock().await;

                            match message {
                                EndpointMessage::PreConnected(addr, receiver, sender) => {
                                    // check and start session
                                    if server.black_list.contains_addr(&addr) {
                                        sender.send(StreamMessage::Close).await;
                                    } else {
                                        session_start(
                                            receiver,
                                            sender,
                                            send.clone(),
                                            out_send.clone(),
                                            server.psk.clone(),
                                            server.pk.clone(),
                                        )
                                    }
                                }
                                EndpointMessage::Connected(peer_id, sender) => {
                                    // check and save tmp and save outside
                                    if server.black_list.contains_peer(&peer_id) {
                                        sender.send(StreamMessage::Close).await;
                                    } else {
                                        server.peer_list.add_tmp_peer(peer_id, sender);
                                        out_send.send(Message::PeerJoin(peer_id)).await;
                                    }
                                }
                                EndpointMessage::Close(peer_id) => {
                                    server.peer_list.remove(&peer_id);
                                    out_send.send(Message::PeerLeave(peer_id)).await;
                                }
                                _ => {}
                            }
                        },
                        None => break,
                    },
                    msg = self_recv.next().fuse() => match msg {
                        Some(message) => {
                                println!("Server: recv from outside: {:?}", message);
                                let mut server = m2.lock().await;

                                match message {
                                    Message::Connect(addr) => {
                                        transport_send
                                            .send(EndpointMessage::Connect(addr, server.pk.to_bytes()))
                                            .await;
                                    }
                                    Message::DisConnect(addr) => {
                                        transport_send.send(EndpointMessage::Disconnect(addr)).await;
                                    }
                                    Message::PeerJoinResult(peer_id, is_ok) => {
                                        let sender = server.peer_list.get(&peer_id);
                                        if sender.is_some() {
                                            let sender = sender.unwrap();
                                            if is_ok {
                                                sender.send(StreamMessage::Ok).await;
                                                server.peer_list.stabilize_tmp_peer(peer_id);
                                            } else {
                                                sender.send(StreamMessage::Close).await;
                                                server.peer_list.remove_tmp_peer(&peer_id);
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                        },
                        None => break,
                    }
                }
            }
        });
    }
}
