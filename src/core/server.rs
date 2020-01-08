use async_std::{
    io::Result,
    prelude::*,
    sync::{Arc, Mutex, Receiver, RwLock, Sender},
    task,
};
use futures::{select, FutureExt};
use std::collections::HashMap;
use std::net::SocketAddr;

use crate::transports::{
    new_channel, start as transport_start, EndpointMessage, StreamMessage, TransportType,
};
use crate::{Config, Message};

use super::hole_punching::DHT;
use super::keys::KeyType;
use super::peer::Peer;
use super::peer_list::PeerList;
use super::session::{start as session_start, RemotePublic};

/// start server
pub async fn start(
    config: Config,
    out_send: Sender<Message>,
    mut self_recv: Receiver<Message>,
) -> Result<()> {
    // TODO load or init config.
    let Config {
        addr,
        join_data,
        transport,
        white_list,
        black_list,
        white_peer_list,
        black_peer_list,
    } = config;

    // load or generate keypair
    let key = KeyType::Ed25519.generate_kepair();

    let peer_id = key.peer_id();
    let peer_list = Arc::new(RwLock::new(PeerList::init(
        peer_id,
        (white_peer_list, white_list),
        (black_peer_list, black_list),
    )));

    let peer = Peer::new(
        key.peer_id(),
        addr,
        TransportType::from_str(&transport),
        true,
    );

    let _transports: HashMap<u8, Sender<EndpointMessage>> = HashMap::new();

    let (send, mut recv) = new_channel();
    let transport_send = transport_start(peer.transport(), peer.addr(), send.clone())
        .await
        .expect("Transport binding failure!");

    println!("Debug: peer id: {}", peer_id.short_show());

    task::spawn(async move {
        // bootstrap white list.
        for a in peer_list.read().await.bootstrap() {
            transport_send
                .send(EndpointMessage::Connect(
                    *a,
                    RemotePublic(key.public().clone(), peer.clone(), join_data.clone()).to_bytes(),
                ))
                .await;
        }

        let peer = Arc::new(peer);
        let key = Arc::new(key);

        loop {
            select! {
                msg = recv.next().fuse() => match msg {
                    Some(message) => {
                        match message {
                            EndpointMessage::PreConnected(addr, receiver, sender, is_by_self) => {
                                // check and start session
                                if peer_list.read().await.is_black_addr(&addr) {
                                    sender.send(StreamMessage::Close).await;
                                } else {
                                    session_start(
                                        addr,
                                        receiver,
                                        sender,
                                        send.clone(),
                                        out_send.clone(),
                                        key.clone(),
                                        peer.clone(),
                                        peer_list.clone(),
                                        is_by_self,
                                    )
                                }
                            }
                            EndpointMessage::Connected(peer_id, sender, remote_peer, data) => {
                                // check and save tmp and save outside
                                if &peer_id == peer.id() || peer_list.read().await.is_black_peer(&peer_id) {
                                    sender.send(StreamMessage::Close).await;
                                } else {
                                    let addr = remote_peer.addr().clone();
                                    peer_list.write().await.add_tmp_peer(peer_id, sender, remote_peer);
                                    out_send.send(Message::PeerJoin(peer_id, addr, data)).await;
                                }
                            }
                            EndpointMessage::Close(peer_id) => {
                                peer_list.write().await.remove(&peer_id);
                                out_send.send(Message::PeerLeave(peer_id)).await;
                            }
                            EndpointMessage::Connect(addr, _empty) => {
                                // DHT Helper's peers
                                transport_send
                                    .send(EndpointMessage::Connect(
                                        addr,
                                        RemotePublic(key.public().clone(), *peer.clone(), join_data.clone()).to_bytes()
                                    ))
                                    .await;
                            }
                            _ => {}
                        }
                    },
                    None => break,
                },
                msg = self_recv.next().fuse() => match msg {
                    Some(message) => {
                        match message {
                            Message::Connect(addr, data) => {
                                let join = if data.is_none() {
                                    join_data.clone()
                                }  else {
                                    data.unwrap()
                                };

                                transport_send
                                    .send(EndpointMessage::Connect(
                                        addr,
                                        RemotePublic(key.public().clone(), *peer.clone(), join).to_bytes()
                                    ))
                                    .await;
                            }
                            Message::DisConnect(addr) => {
                                transport_send.send(EndpointMessage::Disconnect(addr)).await;
                            }
                            Message::PeerJoinResult(peer_id, is_ok, data) => {
                                let mut peer_list_lock = peer_list.write().await;
                                let sender = peer_list_lock.get(&peer_id);
                                if sender.is_some() {
                                    let sender = sender.unwrap();
                                    if is_ok {
                                        sender.send(StreamMessage::Ok(data)).await;
                                        peer_list_lock.stabilize_tmp_peer(peer_id);
                                    } else {
                                        sender.send(StreamMessage::Close).await;
                                        peer_list_lock.remove_tmp_peer(&peer_id);
                                    }
                                }
                            }
                            Message::Data(peer_id, data) => {
                                let peer_list_lock = peer_list.read().await;
                                let sender = peer_list_lock.get(&peer_id);
                                if sender.is_some() {
                                    let sender = sender.unwrap();
                                    sender.send(StreamMessage::Bytes(data)).await;
                                }
                            },
                            Message::PeerLeave(peer_id) => {
                                let mut peer_list_lock = peer_list.write().await;
                                let sender = peer_list_lock.get(&peer_id);
                                if sender.is_some() {
                                    let sender = sender.unwrap();
                                    sender.send(StreamMessage::Close).await;
                                    peer_list_lock.remove_tmp_peer(&peer_id);
                                }
                            },
                            Message::PeerJoin(_peer_id, _addr,_data) => {},  // TODO search peer and join
                        }
                    },
                    None => break,
                }
            }
        }
        drop(send);
    });

    Ok(())
}
