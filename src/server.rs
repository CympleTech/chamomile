use postcard::{from_bytes, to_allocvec};
use smol::{
    channel::{Receiver, Sender},
    fs,
    io::Result,
    lock::{Mutex, RwLock},
    prelude::*,
};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use chamomile_types::{
    message::{ReceiveMessage, SendMessage},
    types::{Broadcast, PeerId, TransportType},
};

use crate::config::Config;
use crate::hole_punching::DHT;
use crate::keys::{KeyType, Keypair};
use crate::peer::Peer;
use crate::peer_list::PeerList;
use crate::primitives::{STORAGE_KEY_KEY, STORAGE_NAME, STORAGE_PEER_LIST_KEY};
use crate::session::{start as session_start, RemotePublic, SessionSendMessage};
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
        transport,
        white_list,
        black_list,
        white_peer_list,
        black_peer_list,
        permission,
        only_stable_data,
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
            peer_list
                .merge(
                    peer_id,
                    peer_list_path.clone(),
                    (white_peer_list, white_list),
                    (black_peer_list, black_list),
                )
                .await;
            Arc::new(RwLock::new(peer_list))
        }
        Err(_) => {
            let peer_list = PeerList::init(
                peer_id,
                peer_list_path,
                (white_peer_list, white_list),
                (black_peer_list, black_list),
            )
            .await;
            Arc::new(RwLock::new(peer_list))
        }
    };

    let default_transport = TransportType::from_str(&transport);

    let peer = Peer::new(key.peer_id(), addr, default_transport, true);

    let mut transports: HashMap<TransportType, Sender<EndpointSendMessage>> = HashMap::new();

    let (endpoint_send, endpoint_recv) = endpoint_start(peer.transport(), *peer.addr())
        .await
        .expect("Transport binding failure!");

    transports.insert(default_transport, endpoint_send.clone());
    let transports = Arc::new(RwLock::new(transports));

    let remote_bytes = RemotePublic(key.public().clone(), peer.clone()).to_bytes();

    // bootstrap white list.
    for a in peer_list.read().await.bootstrap() {
        endpoint_send
            .send(EndpointSendMessage::Connect(*a, remote_bytes.clone(), None))
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
                        let _ = sender.send(EndpointStreamMessage::Close).await;
                    } else {
                        smol::spawn(session_start(
                            addr,
                            receiver,
                            sender,
                            out_send_1.clone(),
                            key_1.clone(),
                            peer_1.clone(),
                            peer_list_1.clone(),
                            transports.clone(),
                            permission,
                            is_stable,
                        ))
                        .detach();
                    }
                }
                Err(_) => break,
            }
        }
    })
    .detach();

    let out_send_2 = out_send.clone();
    let peer_list_2 = peer_list.clone();
    let endpoint_send_2 = endpoint_send.clone();

    smol::spawn(async move {
        loop {
            match self_recv.recv().await {
                Ok(SendMessage::StableConnect(to, socket, data)) => {
                    if peer_list.read().await.contains_stable(&to) {
                        out_send_2
                            .send(ReceiveMessage::StableResult(to, true, data))
                            .await
                            .expect("Server to Outside (Stable Result)");
                        continue;
                    }
                    if let Some(addr) = socket {
                        endpoint_send
                            .send(EndpointSendMessage::Connect(
                                addr,
                                remote_bytes.clone(),
                                Some(data),
                            ))
                            .await
                            .expect("Server to Endpoint (Connect)");
                    } else {
                        if let Some(sender) = peer_list.read().await.get(&peer_id) {
                            let _ = sender
                                .send(SessionSendMessage::StableConnect(to, data))
                                .await;
                        }
                    }
                }
                Ok(SendMessage::StableDisconnect(peer_id)) => {
                    if let Some((session, _p)) = peer_list.write().await.stable_remove(&peer_id) {
                        let _ = session.send(SessionSendMessage::Close).await;
                    }
                }
                Ok(SendMessage::Connect(addr)) => {
                    endpoint_send
                        .send(EndpointSendMessage::Connect(
                            addr,
                            remote_bytes.clone(),
                            None,
                        ))
                        .await
                        .expect("Server to Endpoint (Connect)");
                }
                Ok(SendMessage::DisConnect(addr)) => {
                    peer_list.write().await.peer_disconnect(&addr).await;
                    // send to endpoint, beacuse not konw session peer_id.
                    endpoint_send
                        .send(EndpointSendMessage::Close(addr))
                        .await
                        .expect("Server to Endpoint (DisConnect)");
                }
                Ok(SendMessage::StableResult(peer_id, is_ok, is_force, data)) => {
                    let peer_list_lock = peer_list.read().await;
                    if let Some(sender) = peer_list_lock.get(&peer_id) {
                        if is_ok || !is_force {
                            let _ = sender
                                .send(SessionSendMessage::StableResult(peer_id, is_ok, data))
                                .await;
                        } else {
                            let _ = sender.send(SessionSendMessage::Close).await;
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
                        let _ = sender
                            .send(SessionSendMessage::Bytes(peer_id, to, data))
                            .await;
                    }
                }
                Ok(SendMessage::Broadcast(broadcast, data)) => match broadcast {
                    Broadcast::StableAll => {
                        let peer_list_lock = peer_list.read().await;
                        for (to, sender) in peer_list_lock.stable_all() {
                            let _ = sender
                                .send(SessionSendMessage::Bytes(peer_id, to, data.clone()))
                                .await;
                        }
                        drop(peer_list_lock);
                    }
                    Broadcast::Gossip => {
                        // TODO more Gossip base on Kad.
                        let peer_list_lock = peer_list.read().await;
                        for (to, sender) in peer_list_lock.all() {
                            let _ = sender
                                .send(SessionSendMessage::Bytes(peer_id, to, data.clone()))
                                .await;
                        }
                        drop(peer_list_lock);
                    }
                },
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
