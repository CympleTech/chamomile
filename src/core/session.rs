use async_std::{
    io,
    io::BufReader,
    prelude::*,
    sync::{Receiver, Sender},
    task,
};
use futures::{select, FutureExt};
use std::time::Duration;

use crate::transports::{new_stream_channel, EndpointMessage, StreamMessage};
use crate::Message;

use super::keys::{PrivateKey, PublicKey, Signature};
use super::peer_id::PeerId;

pub fn session_start(
    mut transport_receiver: Receiver<StreamMessage>,
    transport_sender: Sender<StreamMessage>,
    server_sender: Sender<EndpointMessage>,
    out_sender: Sender<Message>,
    _self_peer_psk: PrivateKey,
    self_peer_pk: PublicKey,
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
        let mut is_ok = false;
        println!("Session connected: {:?}", remote_peer_id);
        let (sender, mut receiver) = new_stream_channel();
        server_sender
            .send(EndpointMessage::Connected(remote_peer_id, sender))
            .await;

        loop {
            select! {
                msg = transport_receiver.next().fuse() => match msg {
                    Some(msg) => {
                        if !is_ok {
                            continue;
                        }

                        match msg {
                            StreamMessage::Bytes(bytes) => {
                                // TODO Decrypt
                                out_sender.send(Message::Data(remote_peer_id, bytes)).await;
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
                                // TODO encrypt
                                transport_sender
                                    .send(StreamMessage::Bytes(bytes))
                                    .await;
                            },
                            StreamMessage::Ok => {
                                is_ok = true;
                                transport_sender
                                    .send(StreamMessage::Bytes(self_peer_pk.to_bytes()))
                                    .await;

                                // TODO start DH
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
