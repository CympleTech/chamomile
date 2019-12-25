use async_std::{
    io,
    io::BufReader,
    prelude::*,
    sync::{Receiver, Sender},
    task,
};
use futures::{select, FutureExt};
use serde_derive::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::time::Duration;

use crate::transports::{new_stream_channel, EndpointMessage, StreamMessage};
use crate::Message;

use super::keys::{PrivateKey, PublicKey, SessionKey, Signature};
use super::peer_id::PeerId;

pub fn session_start(
    mut transport_receiver: Receiver<StreamMessage>,
    transport_sender: Sender<StreamMessage>,
    server_sender: Sender<EndpointMessage>,
    out_sender: Sender<Message>,
    self_peer_psk: PrivateKey,
    self_peer_pk: PublicKey,
    mut is_ok: bool,
) {
    task::spawn(async move {
        // timeout 10s to read peer_id & public_key
        let result: io::Result<Option<PublicKey>> = io::timeout(Duration::from_secs(5), async {
            while let Some(msg) = transport_receiver.recv().await {
                let remote_peer_pk = match msg {
                    StreamMessage::Bytes(bytes) => PublicKey::from_bytes(bytes).ok(),
                    _ => None,
                };
                return Ok(remote_peer_pk);
            }

            Ok(None)
        })
        .await;

        if result.is_err() {
            println!("Session timeout");
            transport_sender.send(StreamMessage::Close).await;
            drop(transport_receiver);
            drop(transport_sender);
            return;
        }
        let result = result.unwrap();
        if result.is_none() {
            println!("Session invalid pk");
            transport_sender.send(StreamMessage::Close).await;
            drop(transport_receiver);
            drop(transport_sender);
            return;
        }
        let remote_peer_pk = result.unwrap();
        let remote_peer_id = remote_peer_pk.peer_id();
        let mut session_key: SessionKey =
            SessionKey::generate(&remote_peer_pk, &self_peer_pk, &self_peer_psk);

        println!("Session connected: {:?}", remote_peer_id);
        let (sender, mut receiver) = new_stream_channel();
        server_sender
            .send(EndpointMessage::Connected(remote_peer_id, sender))
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
                                        println!("Error Serialize SessionType {:?}", e)
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
                                    .send(StreamMessage::Bytes(self_peer_pk.to_bytes()))
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
    DHT(Vec<(PeerId, SocketAddr)>, Signature),
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
