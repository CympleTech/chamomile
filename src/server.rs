use smol::{
    channel::{Receiver, Sender},
    fs,
    io::Result,
    lock::RwLock,
};
use std::collections::HashMap;
use std::sync::Arc;

use chamomile_types::{
    message::{DeliveryType, ReceiveMessage, SendMessage, StateRequest, StateResponse},
    types::{new_io_error, Broadcast, PeerId, TransportType},
};

use crate::config::Config;
use crate::hole_punching::{nat, DHT};
use crate::keys::{KeyType, Keypair, SessionKey};
use crate::peer::Peer;
use crate::peer_list::PeerList;
use crate::primitives::{STORAGE_KEY_KEY, STORAGE_NAME, STORAGE_PEER_LIST_KEY};
use crate::session::{
    direct_stable, new_session_channel, relay_stable, session_spawn, ConnectType, Session,
    SessionMessage,
};
use crate::transports::{
    start as transport_start, EndpointMessage, RemotePublic, TransportRecvMessage,
    TransportSendMessage,
};

pub(crate) struct Global {
    pub peer_id: PeerId,
    pub peer: Peer,
    pub key: Keypair,
    pub transport_sender: Sender<TransportSendMessage>,
    pub out_sender: Sender<ReceiveMessage>,
}

impl Global {
    #[inline]
    pub fn generate_remote(&self) -> (SessionKey, RemotePublic) {
        // random gennerate, so must return. no keep-loop.
        loop {
            if let Ok(session_key) = self.key.generate_session_key() {
                let remote_pk = RemotePublic(
                    self.key.public(),
                    self.peer.clone(),
                    session_key.out_bytes(),
                );
                return (session_key, remote_pk);
            }
        }
    }

    #[inline]
    pub fn complete_remote(
        &self,
        remote_key: &Keypair,
        dh_bytes: Vec<u8>,
    ) -> Option<(SessionKey, RemotePublic)> {
        if let Some(session_key) = self.key.complete_session_key(remote_key, dh_bytes) {
            let remote_pk = RemotePublic(
                self.key.public(),
                self.peer.clone(),
                session_key.out_bytes(),
            );
            Some((session_key, remote_pk))
        } else {
            None
        }
    }

    #[inline]
    pub async fn trans_send(&self, msg: TransportSendMessage) -> Result<()> {
        self.transport_sender
            .send(msg)
            .await
            .map_err(|_e| new_io_error("Transport missing"))
    }

    #[inline]
    pub async fn out_send(&self, msg: ReceiveMessage) -> Result<()> {
        self.out_sender
            .send(msg)
            .await
            .map_err(|_e| new_io_error("Outside missing"))
    }
}

/// start server
pub async fn start(
    config: Config,
    out_sender: Sender<ReceiveMessage>,
    self_receiver: Receiver<SendMessage>,
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
    let key_bytes = fs::read(&key_path).await.unwrap_or(vec![]); // safe.

    let key = match Keypair::from_db_bytes(&key_bytes) {
        Ok(keypair) => keypair,
        Err(_) => {
            let key = KeyType::Ed25519.generate_kepair();
            let key_bytes = key.to_db_bytes();
            fs::write(key_path, key_bytes).await?;
            key
        }
    };

    let peer_id = key.peer_id();

    let mut peer_list_path = db_dir;
    peer_list_path.push(STORAGE_PEER_LIST_KEY);
    let peer_list = Arc::new(RwLock::new(PeerList::load(
        peer_id,
        peer_list_path,
        (white_peer_list, white_list),
        (black_peer_list, black_list),
    )));

    let default_transport = TransportType::from_str(&transport);

    let peer = Peer::new(key.peer_id(), addr, default_transport, true);

    let mut transports: HashMap<TransportType, Sender<TransportSendMessage>> = HashMap::new();

    let (transport_sender, transport_receiver) = transport_start(peer.transport(), *peer.addr())
        .await
        .expect("Transport binding failure!");

    transports.insert(default_transport, transport_sender.clone());
    let _transports = Arc::new(RwLock::new(transports)); // TODO more about multiple transports.

    let global = Arc::new(Global {
        peer_id,
        peer,
        key,
        transport_sender,
        out_sender,
    });

    // bootstrap white list.
    for a in peer_list.read().await.bootstrap() {
        let (session_key, remote_pk) = global.generate_remote();
        global
            .transport_sender
            .send(TransportSendMessage::Connect(*a, remote_pk, session_key))
            .await
            .expect("Server to Endpoint (Connect)");
    }

    let peer_list_1 = peer_list.clone();
    let global_1 = global.clone();

    smol::spawn(async move {
        loop {
            match transport_receiver.recv().await {
                Ok(TransportRecvMessage(
                    addr,
                    remote_pk,
                    is_self,
                    stream_sender,
                    stream_receiver,
                    endpoint_sender,
                )) => {
                    debug!("receiver incoming connect: {:?}", addr);
                    // check and start session
                    if peer_list_1.read().await.is_black_addr(&addr) {
                        debug!("receiver incoming connect is blocked");
                        let _ = endpoint_sender.send(EndpointMessage::Close).await;
                        continue;
                    }
                    let RemotePublic(remote_key, remote_peer, dh_key) = remote_pk;

                    let remote_peer_id = remote_key.peer_id();
                    debug!("Debug: Session connected: {}", remote_peer_id.short_show());

                    let remote_peer = nat(addr, remote_peer);
                    debug!("Debug: NAT addr: {}", remote_peer.addr());

                    // check and save tmp and save outside
                    if remote_peer_id == peer_id
                        || peer_list_1.read().await.is_black_peer(&remote_peer_id)
                    {
                        debug!("session remote peer is blocked, close it.");
                        let _ = endpoint_sender.send(EndpointMessage::Close).await;
                        continue;
                    }

                    // if not self, send self publics info.
                    let session_key = if let Some(mut session_key) = is_self {
                        if session_key.complete(&remote_key.pk, dh_key) {
                            session_key
                        } else {
                            debug!("Session key is error!");
                            let _ = endpoint_sender.send(EndpointMessage::Close).await;
                            continue;
                        }
                    } else {
                        if let Some((session_key, remote_pk)) =
                            global_1.complete_remote(&remote_key, dh_key)
                        {
                            let _ = endpoint_sender
                                .send(EndpointMessage::Handshake(remote_pk))
                                .await;
                            session_key
                        } else {
                            debug!("Session key is error!");
                            let _ = endpoint_sender.send(EndpointMessage::Close).await;
                            continue;
                        }
                    };

                    // check is stable relay connections.
                    if let Some(sender) =
                        peer_list_1.read().await.stable_check_relay(&remote_peer_id)
                    {
                        let _ = sender
                            .send(SessionMessage::DirectIncoming(
                                remote_peer,
                                stream_sender,
                                stream_receiver,
                                endpoint_sender,
                            ))
                            .await;
                        continue;
                    }

                    // save to peer_list.
                    let (session_sender, session_receiver) = new_session_channel();
                    let mut peer_list_lock = peer_list_1.write().await;
                    let is_new = peer_list_lock
                        .peer_add(
                            &remote_peer_id,
                            session_sender.clone(),
                            stream_sender.clone(),
                            remote_peer,
                        )
                        .await;
                    drop(peer_list_lock);

                    // check if connection had.
                    if !is_new {
                        debug!("Session is had connected, close it.");
                        let _ = endpoint_sender.send(EndpointMessage::Close).await;
                        continue;
                    }

                    // DHT help.
                    let peers = peer_list_1.read().await.get_dht_help(&remote_peer_id);
                    endpoint_sender
                        .send(EndpointMessage::DHT(DHT(peers)))
                        .await
                        .expect("Sesssion to Endpoint (Data)");

                    session_spawn(Session::new(
                        peer_id.clone(),
                        remote_peer,
                        session_sender,
                        session_receiver,
                        stream_sender,
                        stream_receiver,
                        ConnectType::Direct(endpoint_sender),
                        session_key,
                        global_1.clone(),
                        peer_list_1.clone(),
                        !only_stable_data,
                        !permission,
                        false,
                    ));
                }
                Err(_) => break,
            }
        }
    })
    .detach();

    smol::spawn(async move {
        loop {
            match self_receiver.recv().await {
                Ok(SendMessage::StableConnect(tid, to, socket, data)) => {
                    debug!("Send stable connect to: {:?}", to);
                    if to == peer_id {
                        info!("Nerver here, stable connect to self.");
                        if tid != 0 {
                            let _ = global
                                .out_send(ReceiveMessage::Delivery(
                                    DeliveryType::StableConnect,
                                    tid,
                                    false,
                                ))
                                .await;
                        }
                        continue;
                    }

                    if peer_list.read().await.stable_contains(&to) {
                        debug!("Aready stable connected");
                        let _ = global
                            .out_send(ReceiveMessage::StableResult(to, true, data))
                            .await;
                        if tid != 0 {
                            let _ = global
                                .out_send(ReceiveMessage::Delivery(
                                    DeliveryType::StableConnect,
                                    tid,
                                    false,
                                ))
                                .await;
                        }
                        continue;
                    }

                    if let Some((s, _, is_it)) = peer_list.read().await.get(&to) {
                        if is_it {
                            let _ = s.send(SessionMessage::StableConnect(tid, data)).await;
                        } else {
                            info!("Will use stable direct ? {}", socket.is_some());
                            if let Some(addr) = socket {
                                smol::spawn(direct_stable(
                                    tid,
                                    to,
                                    data,
                                    addr,
                                    peer_id.clone(),
                                    global.clone(),
                                    peer_list.clone(),
                                    !only_stable_data,
                                    !permission,
                                ))
                                .detach();
                            } else {
                                smol::spawn(relay_stable(
                                    tid,
                                    to,
                                    data,
                                    s.clone(),
                                    peer_id.clone(),
                                    global.clone(),
                                    peer_list.clone(),
                                    !only_stable_data,
                                    !permission,
                                ))
                                .detach();
                            }
                        }
                    } else {
                        if tid != 0 {
                            let _ = global
                                .out_send(ReceiveMessage::Delivery(
                                    DeliveryType::StableConnect,
                                    tid,
                                    false,
                                ))
                                .await;
                        }
                    };
                }
                Ok(SendMessage::StableResult(tid, to, is_ok, is_force, data)) => {
                    debug!("Send stable result {} to: {:?}", is_ok, to);
                    if to == peer_id {
                        info!("Nerver here, stable result to self.");
                        if tid != 0 {
                            let _ = global
                                .out_send(ReceiveMessage::Delivery(
                                    DeliveryType::StableResult,
                                    tid,
                                    false,
                                ))
                                .await;
                        }
                        continue;
                    }

                    if let Some(sender) = peer_list.read().await.get_tmp_stable(&to) {
                        debug!("Got peer to send stable result.");
                        let _ = sender
                            .send(SessionMessage::StableResult(tid, is_ok, is_force, data))
                            .await;
                    } else {
                        if tid != 0 {
                            let _ = global
                                .out_send(ReceiveMessage::Delivery(
                                    DeliveryType::StableResult,
                                    tid,
                                    false,
                                ))
                                .await;
                        }
                    }
                }
                Ok(SendMessage::StableDisconnect(peer_id)) => {
                    debug!("Send stable disconnect to: {:?}", peer_id);
                    if let Some(session) = peer_list.write().await.stable_remove(&peer_id) {
                        let _ = session.send(SessionMessage::Close).await;
                    }
                }
                Ok(SendMessage::Connect(addr)) => {
                    debug!("Send connect to: {:?}", addr);
                    let (session_key, remote_pk) = global.generate_remote();
                    let _ = global
                        .trans_send(TransportSendMessage::Connect(addr, remote_pk, session_key))
                        .await;
                }
                Ok(SendMessage::DisConnect(addr)) => {
                    debug!("Send disconnect to: {:?}", addr);
                    peer_list.write().await.peer_disconnect(&addr).await;
                }
                Ok(SendMessage::Data(tid, to, data)) => {
                    debug!(
                        "DEBUG: data is send to: {}, {}",
                        to.short_show(),
                        data.len()
                    );
                    let peer_list_lock = peer_list.read().await;
                    if let Some((sender, stream_sender, is_it)) = peer_list_lock.get(&to) {
                        if is_it {
                            let _ = sender.send(SessionMessage::Data(tid, data)).await;
                        } else {
                            // only happen on permissionless. link to session's Line439
                            let _ = stream_sender
                                .send(EndpointMessage::RelayData(peer_id, to, data))
                                .await;
                        }
                    } else {
                        if tid != 0 {
                            let _ = global
                                .out_send(ReceiveMessage::Delivery(DeliveryType::Data, tid, false))
                                .await;
                        }
                    }
                }
                Ok(SendMessage::Broadcast(broadcast, data)) => match broadcast {
                    Broadcast::StableAll => {
                        let peer_list_lock = peer_list.read().await;
                        for (_to, (sender, _)) in peer_list_lock.stable_all() {
                            let _ = sender.send(SessionMessage::Data(0, data.clone())).await;
                        }
                        drop(peer_list_lock);
                    }
                    Broadcast::Gossip => {
                        // TODO more Gossip base on Kad.
                        let peer_list_lock = peer_list.read().await;
                        for (_to, sender) in peer_list_lock.all() {
                            let _ = sender.send(SessionMessage::Data(0, data.clone())).await;
                        }
                        drop(peer_list_lock);
                    }
                },
                Ok(SendMessage::Stream(_symbol, _stream_type)) => {
                    todo!();
                }
                Ok(SendMessage::NetworkState(req, res_sender)) => match req {
                    StateRequest::Stable => {
                        let peers = peer_list
                            .read()
                            .await
                            .stable_all()
                            .iter()
                            .map(|(id, (_, is_direct))| (*id, *is_direct))
                            .collect();
                        let _ = res_sender.send(StateResponse::Stable(peers)).await;
                    }
                    StateRequest::DHT => {
                        let peers = peer_list.read().await.dht_keys();
                        let _ = res_sender.send(StateResponse::DHT(peers)).await;
                    }
                    StateRequest::Seed => {
                        let seeds = peer_list.read().await.bootstrap().clone();
                        let _ = res_sender.send(StateResponse::Seed(seeds)).await;
                    }
                },
                Err(_) => break,
            }
        }
    })
    .detach();

    Ok(peer_id)
}
