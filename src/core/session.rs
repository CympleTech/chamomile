use async_std::{
    io,
    io::BufReader,
    prelude::*,
    sync::{Arc, Receiver, RwLock, Sender},
    task,
};
use futures::{select, FutureExt};
use serde_derive::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::time::Duration;

use crate::transports::{new_stream_channel, EndpointMessage, StreamMessage};
use crate::Message;

use super::hole_punching::{nat, Hole, DHT};
use super::keys::{KeyType, Keypair, SessionKey};
use super::peer::{Peer, PeerId};
use super::peer_list::PeerList;

pub fn start(
    remote_addr: SocketAddr,
    mut transport_receiver: Receiver<StreamMessage>,
    transport_sender: Sender<StreamMessage>,
    server_sender: Sender<EndpointMessage>,
    out_sender: Sender<Message>,
    key: Arc<Keypair>,
    peer: Arc<Peer>,
    peer_list: Arc<RwLock<PeerList>>,
    mut is_ok: bool,
) {
    task::spawn(async move {
        // timeout 10s to read peer_id & public_key
        let result: io::Result<Option<RemotePublic>> = io::timeout(Duration::from_secs(5), async {
            while let Some(msg) = transport_receiver.recv().await {
                let remote_peer_key = match msg {
                    StreamMessage::Bytes(bytes) => RemotePublic::from_bytes(key.key, bytes).ok(),
                    _ => None,
                };
                return Ok(remote_peer_key);
            }

            Ok(None)
        })
        .await;

        if result.is_err() {
            println!("Debug: Session timeout");
            transport_sender.send(StreamMessage::Close).await;
            drop(transport_receiver);
            drop(transport_sender);
            return;
        }
        let result = result.unwrap();
        if result.is_none() {
            println!("Debug: Session invalid pk");
            transport_sender.send(StreamMessage::Close).await;
            drop(transport_receiver);
            drop(transport_sender);
            return;
        }
        let RemotePublic(remote_peer_key, remote_local_addr, remote_join_data) = result.unwrap();
        let remote_peer_id = remote_peer_key.peer_id();
        let mut session_key: SessionKey = key.key.session_key(&key, &remote_peer_key);

        println!("Debug: Session connected: {}", remote_peer_id.short_show());
        let (sender, mut receiver) = new_stream_channel();
        let remote_transport = nat(remote_addr, remote_local_addr);
        println!("Debug: NAT addr: {}", remote_transport.addr());
        server_sender
            .send(EndpointMessage::Connected(
                remote_peer_id,
                sender,
                remote_transport,
                remote_join_data,
            ))
            .await;

        let mut buffers: Vec<Vec<u8>> = vec![];
        let mut receiver_buffers: Vec<Vec<u8>> = vec![];

        loop {
            select! {
                msg = transport_receiver.next().fuse() => match msg {
                    Some(msg) => {
                        if !is_ok {
                            continue;
                        }

                        match msg {
                            StreamMessage::Bytes(bytes) => {
                                match SessionType::from_bytes(bytes) {
                                    Ok(t) => match t {
                                        SessionType::Key(bytes) => {
                                            if !session_key.is_ok() {
                                                if !session_key.in_bytes(bytes) {
                                                    server_sender
                                                        .send(EndpointMessage::Close(remote_peer_id))
                                                        .await;
                                                    transport_sender.send(StreamMessage::Close).await;
                                                    break;
                                                }

                                                transport_sender
                                                    .send(StreamMessage::Bytes(
                                                        SessionType::Key(session_key.out_bytes())
                                                            .to_bytes()
                                                    ))
                                                    .await;
                                            }

                                            while !buffers.is_empty() {
                                                let bytes = buffers.pop().unwrap();
                                                let e_data = session_key.encrypt(bytes);
                                                let data = SessionType::Data(e_data).to_bytes();
                                                transport_sender
                                                    .send(StreamMessage::Bytes(data))
                                                    .await;
                                            }

                                            while !receiver_buffers.is_empty() {
                                                let e_data = buffers.pop().unwrap();
                                                let d_data = session_key.decrypt(e_data);
                                                if d_data.is_ok() {
                                                    out_sender
                                                        .send(Message::Data(
                                                            remote_peer_id,
                                                            d_data.unwrap()
                                                        )).await;
                                                }
                                            }

                                        }
                                        SessionType::Data(e_data) => {
                                            if !session_key.is_ok() {
                                                receiver_buffers.push(e_data);
                                                continue;
                                            }

                                            let d_data = session_key.decrypt(e_data);
                                            if d_data.is_ok() {
                                                out_sender
                                                    .send(Message::Data(
                                                        remote_peer_id,
                                                        d_data.unwrap()
                                                    )).await;
                                            }
                                        }
                                        SessionType::DHT(peers, _sign) => {
                                            let DHT(peers) = peers;
                                            // TODO DHT Helper
                                            // remote_peer_key.verify()

                                            for p in peers {
                                                if p.id() != peer.id() && peer_list.read().await.get_it(p.id()).is_none() {
                                                    server_sender.send(EndpointMessage::Connect(*p.addr(), vec![])).await;
                                                }
                                            }

                                        }
                                        SessionType::Relay(_peer_id, _data) => {
                                            // TODO Relay send
                                        }
                                        SessionType::Ping => {
                                            transport_sender
                                                .send(StreamMessage::Bytes(
                                                    SessionType::Pong.to_bytes()
                                                ))
                                                .await;
                                        }
                                        SessionType::Pong => {
                                            // TODO Heartbeat Ping/Pong
                                        },
                                        _ => {
                                            //TODO Hole
                                        }
                                    }
                                    Err(e) => {
                                        println!("Debug: Error Serialize SessionType {:?}", e)
                                    },
                                }
                            },
                            StreamMessage::Close => {
                                server_sender
                                    .send(EndpointMessage::Close(remote_peer_id))
                                    .await;
                                break;
                            }
                            _ => break,
                        }
                    },
                    None => break,
                },
                out_msg = receiver.next().fuse() => match out_msg {
                    Some(msg) => {
                        match msg {
                            StreamMessage::Bytes(bytes) => {
                                if session_key.is_ok() {
                                    let e_data = session_key.encrypt(bytes);
                                    let data = SessionType::Data(e_data).to_bytes();
                                    transport_sender
                                        .send(StreamMessage::Bytes(data))
                                        .await;
                                } else {
                                    buffers.push(bytes);
                                }
                            },
                            StreamMessage::Ok(data) => {
                                is_ok = true;

                                println!("data: {:?}", data);
                                transport_sender
                                    .send(StreamMessage::Bytes(RemotePublic(key.public(), *peer.clone(), data).to_bytes()))
                                    .await;

                                transport_sender
                                    .send(StreamMessage::Bytes(
                                        SessionType::Key(session_key.out_bytes()).to_bytes()
                                    ))
                                    .await;

                                let sign = vec![]; // TODO
                                let peers = peer_list.read().await.get_dht_help(&remote_peer_id);
                                transport_sender
                                    .send(StreamMessage::Bytes(
                                        SessionType::DHT(DHT(peers), sign).to_bytes()
                                    ))
                                    .await;
                            },
                            StreamMessage::Close => {
                                transport_sender.send(StreamMessage::Close).await;
                                break;
                            }
                        }
                    },
                    None => break,
                }
            }
        }
    });
}

#[derive(Deserialize, Serialize)]
enum SessionType {
    Key(Vec<u8>),
    Data(Vec<u8>),
    Relay(PeerId, Vec<u8>),
    DHT(DHT, Vec<u8>),
    Hole(Hole),
    HoleConnect,
    Ping,
    Pong,
}

impl SessionType {
    fn to_bytes(&self) -> Vec<u8> {
        bincode::serialize(self).unwrap()
    }

    fn from_bytes(bytes: Vec<u8>) -> Result<Self, ()> {
        bincode::deserialize(&bytes[..]).map_err(|_e| ())
    }
}

/// Rtemote Public Info, include local transport and public key bytes.
#[derive(Deserialize, Serialize)]
pub struct RemotePublic(pub Keypair, pub Peer, pub Vec<u8>);

impl RemotePublic {
    pub fn from_bytes(key: KeyType, bytes: Vec<u8>) -> Result<Self, ()> {
        bincode::deserialize(&bytes).map_err(|_e| ())
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        bincode::serialize(self).unwrap()
    }
}
