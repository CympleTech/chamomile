use async_std::{
    io::Result,
    prelude::*,
    sync::{Arc, Mutex, Receiver, RwLock, Sender},
    task,
};
use futures::{select, FutureExt};
use std::collections::HashMap;
use std::net::SocketAddr;

use crate::config::Config;
use crate::hole_punching::DHT;
use crate::keys::{KeyType, Keypair};
use crate::message::{ReceiveMessage, SendMessage};
use crate::peer::{Peer, PeerId};
use crate::peer_list::PeerList;
use crate::primitives::{STORAGE_KEY_KEY, STORAGE_NAME};
use crate::session::{
    new_session_receive_channel, start as session_start, RemotePublic, SessionReceiveMessage,
    SessionSendMessage,
};
use crate::storage::LocalDB;
use crate::transports::{
    start as endpoint_start, EndpointIncomingMessage, EndpointSendMessage, EndpointStreamMessage,
    TransportType,
};

/// start server
pub async fn start(
    config: Config,
    out_send: Sender<ReceiveMessage>,
    self_recv: Receiver<SendMessage>,
) -> Result<PeerId> {
    let Config {
        mut db_dir,
        addr,
        join_data,
        transport,
        white_list,
        black_list,
        white_peer_list,
        black_peer_list,
    } = config;
    db_dir.push(STORAGE_NAME);
    let mut db = LocalDB::open_absolute(&db_dir)?;
    let db_key_key = STORAGE_KEY_KEY.as_bytes().to_vec();
    let key_result = db.read::<Keypair>(&db_key_key); // TODO KeyStore
    let key = if key_result.is_none() {
        let key = KeyType::Ed25519.generate_kepair();
        db.write(db_key_key, &key)?;
        key
    } else {
        key_result.unwrap()
    };

    let peer_id = key.peer_id();
    let db_peer_list_key = peer_id.0.to_vec();
    let peer_list_result = db.read::<PeerList>(&db_peer_list_key);
    let peer_list = if peer_list_result.is_none() {
        Arc::new(RwLock::new(PeerList::init(
            peer_id,
            (white_peer_list, white_list),
            (black_peer_list, black_list),
        )))
    } else {
        let mut peer_list = peer_list_result.unwrap();
        peer_list.merge(
            peer_id,
            (white_peer_list, white_list),
            (black_peer_list, black_list),
        );
        Arc::new(RwLock::new(peer_list))
    };

    let peer = Peer::new(
        key.peer_id(),
        addr,
        TransportType::from_str(&transport),
        true,
    );

    let _transports: HashMap<u8, Sender<EndpointSendMessage>> = HashMap::new();

    let (endpoint_send, endpoint_recv) = endpoint_start(peer.transport(), *peer.addr())
        .await
        .expect("Transport binding failure!");

    let (session_send, session_recv) = new_session_receive_channel();

    task::spawn(async move {
        // bootstrap white list.
        for a in peer_list.read().await.bootstrap() {
            endpoint_send
                .send(EndpointSendMessage::Connect(
                    *a,
                    RemotePublic(key.public().clone(), peer.clone(), join_data.clone()).to_bytes(),
                ))
                .await;
        }

        let peer = Arc::new(peer);
        let key = Arc::new(key);

        loop {
            select! {
                msg = endpoint_recv.recv().fuse() => match msg {
                    Some(message) => {
                        let EndpointIncomingMessage(addr, receiver, sender, is_by_self) = message;

                        // check and start session
                        if peer_list.read().await.is_black_addr(&addr) {
                            sender.send(EndpointStreamMessage::Close).await;
                        } else {
                            session_start(
                                addr,
                                receiver,
                                sender,
                                session_send.clone(),
                                out_send.clone(),
                                key.clone(),
                                peer.clone(),
                                peer_list.clone(),
                                is_by_self,
                            )
                        }
                    },
                    None => break,
                },
                msg = session_recv.recv().fuse() => match msg {
                    Some(message) =>{
                        match message {
                            SessionReceiveMessage::Connected(peer_id, sender, remote_peer, data) => {
                                // check and save tmp and save outside
                                if &peer_id == peer.id()
                                    || peer_list.read().await.is_black_peer(&peer_id) {
                                    sender.send(SessionSendMessage::Close).await;
                                } else {
                                    let addr = remote_peer.addr().clone();
                                    peer_list
                                        .write()
                                        .await
                                        .add_tmp_peer(peer_id, sender, remote_peer);
                                    out_send
                                        .send(ReceiveMessage::PeerJoin(peer_id, addr, data))
                                        .await;
                                }
                            }
                            SessionReceiveMessage::Close(peer_id) => {
                                peer_list.write().await.remove(&peer_id);
                                out_send.send(ReceiveMessage::PeerLeave(peer_id)).await;
                            }
                            SessionReceiveMessage::Connect(addr) => {
                                // DHT Helper's peers
                                endpoint_send
                                    .send(EndpointSendMessage::Connect(
                                        addr,
                                        RemotePublic(
                                            key.public().clone(),
                                            *peer.clone(),
                                            join_data.clone()
                                        ).to_bytes(),
                                    ))
                                    .await;
                            }
                        }
                    },
                    None => break,
                },
                msg = self_recv.recv().fuse() => match msg {
                    Some(message) => {
                        match message {
                            SendMessage::PeerJoin(peer_id, socket, data) => {
                                // TODO Add peer to directly connect.
                            }
                            SendMessage::Connect(addr, data) => {
                                let join = if data.is_none() {
                                    join_data.clone()
                                }  else {
                                    data.unwrap()
                                };

                                endpoint_send
                                    .send(EndpointSendMessage::Connect(
                                        addr,
                                        RemotePublic(
                                            key.public().clone(),
                                            *peer.clone(),
                                            join
                                        ).to_bytes()
                                    ))
                                    .await;
                            }
                            SendMessage::DisConnect(addr) => {
                                endpoint_send.send(EndpointSendMessage::Close(addr)).await;
                            }
                            SendMessage::PeerJoinResult(peer_id, is_ok, is_force, data) => {
                                let mut peer_list_lock = peer_list.write().await;
                                let sender = peer_list_lock.get(&peer_id);
                                if sender.is_some() {
                                    let sender = sender.unwrap();
                                    // TODO is_ok, build two-channel to that peer.
                                    if is_ok || !is_force {
                                        sender.send(SessionSendMessage::Ok(data)).await;
                                        peer_list_lock.stabilize_tmp_peer(
                                            peer_id,
                                            &db_peer_list_key,
                                            &mut db
                                        );
                                    } else {
                                        sender.send(SessionSendMessage::Close).await;
                                        peer_list_lock.remove_tmp_peer(&peer_id);
                                    }
                                }
                            }
                            SendMessage::PeerLeave(peer_id) => {
                                let mut peer_list_lock = peer_list.write().await;
                                let sender = peer_list_lock.get(&peer_id);
                                if sender.is_some() {
                                    let sender = sender.unwrap();
                                    sender.send(SessionSendMessage::Close).await;
                                    peer_list_lock.remove_tmp_peer(&peer_id);
                                }
                            },
                            SendMessage::Data(to, data) => {
                                println!("DEBUG: data is send to: {}, {}", to.short_show(), data.len());
                                let peer_list_lock = peer_list.read().await;
                                let sender = peer_list_lock.get(&to);
                                if sender.is_some() {
                                    let sender = sender.unwrap();
                                    sender.send(SessionSendMessage::Bytes(peer_id, to, data)).await;
                                }
                            },
                            SendMessage::Broadcast(_broadcast, _data) => {
                                // TODO
                            }
                        }
                    },
                    None => break,
                }
            }
        }
    });

    Ok(peer_id)
}
