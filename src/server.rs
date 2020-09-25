use postcard::{from_bytes, to_allocvec};
use smol::{
    channel::{Receiver, Sender},
    fs,
    io::Result,
    lock::{Mutex, RwLock},
    prelude::*,
};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use chamomile_types::{
    message::{ReceiveMessage, SendMessage},
    types::{PeerId, TransportType},
};

use crate::config::Config;
use crate::hole_punching::DHT;
use crate::keys::{KeyType, Keypair};
use crate::peer::Peer;
use crate::peer_list::PeerList;
use crate::primitives::{STORAGE_KEY_KEY, STORAGE_NAME, STORAGE_PEER_LIST_KEY};
use crate::session::{
    new_session_receive_channel, start as session_start, RemotePublic, SessionReceiveMessage,
    SessionSendMessage,
};
use crate::transports::{
    start as endpoint_start, EndpointIncomingMessage, EndpointSendMessage, EndpointStreamMessage,
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
        permission,
    } = config;
    db_dir.push(STORAGE_NAME);
    if !db_dir.exists() {
        fs::create_dir_all(&db_dir).await?;
    }
    let mut key_path = db_dir.clone();
    key_path.push(STORAGE_KEY_KEY);
    let key_bytes = fs::read(&key_path).await.unwrap_or(vec![]);

    let key = match from_bytes::<Keypair>(&key_bytes) {
        Ok(keypair) => keypair,
        Err(_) => {
            let key = KeyType::Ed25519.generate_kepair();
            let key_bytes = to_allocvec(&key).unwrap_or(vec![]);
            fs::write(key_path, key_bytes).await?;
            key
        }
    };

    let peer_id = key.peer_id();

    let mut peer_list_path = db_dir;
    peer_list_path.push(STORAGE_PEER_LIST_KEY);
    let peer_list_bytes = fs::read(&peer_list_path).await.unwrap_or(vec![]);

    let peer_list = match from_bytes::<PeerList>(&peer_list_bytes) {
        Ok(mut peer_list) => {
            peer_list.merge(
                peer_id,
                (white_peer_list, white_list),
                (black_peer_list, black_list),
            );
            Arc::new(RwLock::new(peer_list))
        }
        Err(_) => {
            let peer_list = PeerList::init(
                peer_id,
                (white_peer_list, white_list),
                (black_peer_list, black_list),
            );
            fs::write(&peer_list_path, peer_list.to_bytes()).await?;
            Arc::new(RwLock::new(peer_list))
        }
    };

    let peer = Peer::new(
        key.peer_id(),
        addr,
        TransportType::from_str(&transport),
        true,
    );

    // TODO features.
    let _transports: HashMap<u8, Sender<EndpointSendMessage>> = HashMap::new();

    let (endpoint_send, endpoint_recv) = endpoint_start(peer.transport(), *peer.addr())
        .await
        .expect("Transport binding failure!");

    let (session_send, session_recv) = new_session_receive_channel();

    let remote_bytes =
        RemotePublic(key.public().clone(), peer.clone(), join_data.clone()).to_bytes();

    // bootstrap white list.
    for a in peer_list.read().await.bootstrap() {
        endpoint_send
            .send(EndpointSendMessage::Connect(
                *a,
                false,
                remote_bytes.clone(),
            ))
            .await
            .expect("Server to Endpoint (Connect)");
    }

    let peer = Arc::new(peer);
    let key = Arc::new(key);

    let out_send_1 = out_send.clone();
    let peer_1 = peer.clone();
    let key_1 = key.clone();
    let peer_list_1 = peer_list.clone();

    smol::spawn(async move {
        loop {
            match endpoint_recv.recv().await {
                Ok(EndpointIncomingMessage(addr, receiver, sender, is_stable)) => {
                    // check and start session
                    if peer_list_1.read().await.is_black_addr(&addr) {
                        sender
                            .send(EndpointStreamMessage::Close)
                            .await
                            .expect("Server to Endpoint (Close)");
                    } else {
                        session_start(
                            addr,
                            receiver,
                            sender,
                            session_send.clone(),
                            out_send_1.clone(),
                            key_1.clone(),
                            peer_1.clone(),
                            peer_list_1.clone(),
                            is_stable,
                            permission,
                        )
                    }
                }
                Err(_) => break,
            }
        }
    })
    .detach();

    let peer_list_2 = peer_list.clone();
    let endpoint_send_2 = endpoint_send.clone();
    let peer_list_path_2 = peer_list_path.clone();

    smol::spawn(async move {
        loop {
            match session_recv.recv().await {
                Ok(SessionReceiveMessage::Connected(
                    r_peer_id,
                    sender,
                    remote_peer,
                    data,
                    is_stable,
                )) => {
                    // check and save tmp and save outside
                    if r_peer_id == peer_id || peer_list_2.read().await.is_black_peer(&peer_id) {
                        sender
                            .send(SessionSendMessage::Close)
                            .await
                            .expect("Server to Senssion (Close)");
                    } else {
                        if is_stable {
                            let mut peer_list_lock = peer_list_2.write().await;
                            let is_save =
                                peer_list_lock.stable_stabilize(peer_id, sender, remote_peer);
                            drop(peer_list_lock);

                            if is_save {
                                let _ = fs::write(
                                    &peer_list_path_2,
                                    peer_list_2.read().await.to_bytes(),
                                )
                                .await;
                            }
                        } else {
                            if permission {
                                let addr = remote_peer.addr().clone();
                                peer_list_2.write().await.stable_tmp_add(
                                    peer_id,
                                    sender,
                                    remote_peer,
                                );
                                out_send
                                    .send(ReceiveMessage::PeerJoin(peer_id, addr, data))
                                    .await
                                    .expect("Server to Outside (PeerJoin)");
                            } else {
                                let peer_list_lock = peer_list_2.write().await;
                                let is_save = peer_list_2.write().await.peer_add(
                                    peer_id,
                                    sender.clone(),
                                    remote_peer,
                                );
                                drop(peer_list_lock);

                                if is_save {
                                    sender
                                        .send(SessionSendMessage::Ok(vec![], false))
                                        .await
                                        .expect("Server to Senssion (Ok)");
                                    let _ = fs::write(
                                        &peer_list_path_2,
                                        peer_list_2.read().await.to_bytes(),
                                    )
                                    .await;
                                } else {
                                    sender
                                        .send(SessionSendMessage::Close)
                                        .await
                                        .expect("Server to Senssion (Old Close)");
                                }
                            }
                        }
                    }
                }
                Ok(SessionReceiveMessage::Close(peer_id)) => {
                    peer_list_2.write().await.peer_remove(&peer_id);
                    if permission {
                        peer_list_2.write().await.stable_leave(&peer_id);
                        out_send
                            .send(ReceiveMessage::PeerLeave(peer_id))
                            .await
                            .expect("Server to Outside (PeerLeave)");
                    }
                }
                Ok(SessionReceiveMessage::Connect(addr)) => {
                    // DHT Helper's peers
                    endpoint_send_2
                        .send(EndpointSendMessage::Connect(
                            addr,
                            false,
                            remote_bytes.clone(),
                        ))
                        .await
                        .expect("Server to Endpoint (Connect)");
                }
                Err(_) => break,
            }
        }
    })
    .detach();

    smol::spawn(async move {
        loop {
            match self_recv.recv().await {
                Ok(SendMessage::PeerConnect(peer_id, socket, data)) => {
                    if peer_list.write().await.pre_stable_add(peer_id) {
                        continue;
                    }
                    if let Some(addr) = socket {
                        endpoint_send
                            .send(EndpointSendMessage::Connect(
                                addr,
                                true,
                                RemotePublic(key.public().clone(), *peer.clone(), data).to_bytes(),
                            ))
                            .await
                            .expect("Server to Endpoint (Connect)");
                    } else {
                        // TODO search the peer's socket in kad.
                        todo!()
                    }
                }
                Ok(SendMessage::PeerDisconnect(peer_id)) => {
                    if let Some((session, _p)) = peer_list.write().await.stable_remove(&peer_id) {
                        session
                            .send(SessionSendMessage::Close)
                            .await
                            .expect("Server to Session (PeerDisconnect)");
                    }
                }
                Ok(SendMessage::Connect(addr, data)) => {
                    let join = if data.is_none() {
                        join_data.clone()
                    } else {
                        data.unwrap()
                    };

                    endpoint_send
                        .send(EndpointSendMessage::Connect(
                            addr,
                            false,
                            RemotePublic(key.public().clone(), *peer.clone(), join).to_bytes(),
                        ))
                        .await
                        .expect("Server to Endpoint (Connect)");
                }
                Ok(SendMessage::DisConnect(addr)) => {
                    peer_list.write().await.peer_disconnect(&addr);
                    // send to endpoint, beacuse not konw session peer_id.
                    endpoint_send
                        .send(EndpointSendMessage::Close(addr))
                        .await
                        .expect("Server to Endpoint (DisConnect)");
                }
                Ok(SendMessage::PeerJoinResult(peer_id, is_ok, is_force, data)) => {
                    let mut peer_list_lock = peer_list.write().await;
                    if let Some(sender) = peer_list_lock.get(&peer_id) {
                        if is_ok || !is_force {
                            sender
                                .send(SessionSendMessage::Ok(data, is_ok))
                                .await
                                .expect("Server to Session (Join Ok)");
                            // stable or kad

                            if peer_list_lock.stable_tmp_stabilize(peer_id, is_ok) {
                                let _ = fs::write(
                                    &peer_list_path,
                                    to_allocvec(&peer_list_lock.to_bytes()).unwrap_or(vec![]),
                                )
                                .await;
                            }
                        } else {
                            // close
                            sender
                                .send(SessionSendMessage::Close)
                                .await
                                .expect("Server to Session (Join Close)");
                            peer_list_lock.stable_tmp_remove(&peer_id);
                        }
                    }
                }
                Ok(SendMessage::Data(to, data)) => {
                    debug!(
                        "DEBUG: data is send to: {}, {}",
                        to.short_show(),
                        data.len()
                    );
                    let peer_list_lock = peer_list.read().await;
                    if let Some(sender) = peer_list_lock.get(&to) {
                        sender
                            .send(SessionSendMessage::Bytes(peer_id, to, data))
                            .await
                            .expect("Server to Session (Data)");
                    }
                }
                Ok(SendMessage::Broadcast(_broadcast, _data)) => {
                    todo!();
                }
                Ok(SendMessage::Stream(_symbol, _stream_type)) => {
                    todo!();
                }
                Err(_) => break,
            }
        }
    })
    .detach();

    Ok(peer_id)
}
