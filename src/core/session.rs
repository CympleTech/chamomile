use async_std::{
    io,
    io::BufReader,
    prelude::*,
    sync::{Arc, Receiver, Sender},
    task,
};
use futures::{select, FutureExt};
use serde_derive::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::time::Duration;

use crate::transports::{new_stream_channel, EndpointMessage, StreamMessage};
use crate::Message;

use super::hole_punching::nat;
use super::keys::{KeyType, Keypair, SessionKey};
use super::peer_id::PeerId;
use super::transport::Transport;

/// Rtemote Public Info, include local transport and public key bytes.
#[derive(Deserialize, Serialize)]
pub struct RemotePublic(pub Keypair, pub Transport);

impl RemotePublic {
    pub fn from_bytes(key: KeyType, bytes: Vec<u8>) -> Result<Self, ()> {
        bincode::deserialize(&bytes).map_err(|_e| ())
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        bincode::serialize(self).unwrap()
    }
}

pub fn start(
    remote_addr: SocketAddr,
    mut transport_receiver: Receiver<StreamMessage>,
    transport_sender: Sender<StreamMessage>,
    server_sender: Sender<EndpointMessage>,
    out_sender: Sender<Message>,
    key: Arc<Keypair>,
    self_transport: Transport,
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
        let RemotePublic(remote_peer_key, remote_local_addr) = result.unwrap();
        let remote_peer_id = remote_peer_key.peer_id();
        let mut session_key: SessionKey = key.key.session_key(&key, &remote_peer_key);

        println!("Debug: Session connected: {:?}", remote_peer_id);
        let (sender, mut receiver) = new_stream_channel();
        let transport = nat(remote_addr, remote_local_addr);
        server_sender
            .send(EndpointMessage::Connected(
                remote_peer_id,
                sender,
                transport,
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
                                        SessionType::DHT(_peers, _sign) => {
                                            // TODO DHT Helper
                                            // remote_peer_key.verify()
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
                            StreamMessage::Ok => {
                                is_ok = true;
                                transport_sender
                                    .send(StreamMessage::Bytes(RemotePublic(key.public(), self_transport.clone()).to_bytes()))
                                    .await;

                                transport_sender
                                    .send(StreamMessage::Bytes(
                                        SessionType::Key(session_key.out_bytes()).to_bytes()
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
    DHT(Vec<(PeerId, SocketAddr)>, Vec<u8>),
    Relay(PeerId, Vec<u8>),
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
