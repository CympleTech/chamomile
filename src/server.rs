use rand_chacha::{
    rand_core::{RngCore, SeedableRng},
    ChaChaRng,
};
use std::{collections::HashMap, path::PathBuf, sync::Arc};
use tokio::{
    fs,
    io::Result,
    select,
    sync::mpsc::{Receiver, Sender},
    sync::RwLock,
};

use chamomile_types::{
    delivery_split,
    key::Key,
    message::{DeliveryType, ReceiveMessage, SendMessage, StateRequest, StateResponse},
    types::{Broadcast, PeerId, TransportType, PEER_ID_LENGTH},
    Peer,
};

use crate::buffer::{Buffer, BufferKey};
use crate::config::Config;
use crate::global::Global;
use crate::hole_punching::{nat, DHT};
use crate::kad::KadValue;
use crate::peer_list::PeerList;
use crate::primitives::{STORAGE_ASSIST, STORAGE_KEY_KEY, STORAGE_PEER_LIST_KEY};
use crate::session::{
    direct_stable, new_session_channel, relay_stable, session_spawn, ConnectType, Session,
    SessionMessage,
};
use crate::transports::{
    start as transport_start, EndpointMessage, RemotePublic, TransportRecvMessage,
    TransportSendMessage,
};

async fn get_keypair(mut key_path: PathBuf) -> Key {
    key_path.push(STORAGE_KEY_KEY);

    let key_bytes = fs::read(&key_path).await.unwrap_or(vec![]); // safe.
    let key = match Key::from_db_bytes(&key_bytes) {
        Ok(keypair) => keypair,
        Err(_) => {
            let key = Key::generate(&mut ChaChaRng::from_entropy());
            let key_bytes = key.to_db_bytes();
            let _ = fs::write(key_path, key_bytes).await;
            key
        }
    };

    key
}

async fn get_assist(mut path: PathBuf) -> PeerId {
    path.push(STORAGE_ASSIST);
    let bytes = fs::read(&path).await.unwrap_or(vec![]); // safe.
    let mut id_bytes = [0u8; PEER_ID_LENGTH];

    if bytes.len() == PEER_ID_LENGTH {
        id_bytes.copy_from_slice(&bytes);
        PeerId(id_bytes)
    } else {
        let rng = &mut ChaChaRng::from_entropy();
        rng.fill_bytes(&mut id_bytes);
        let _ = fs::write(path, id_bytes).await;
        PeerId(id_bytes)
    }
}

pub async fn start(
    config: Config,
    out_sender: Sender<ReceiveMessage>,
    self_receiver: Receiver<SendMessage>,
) -> Result<PeerId> {
    let key = get_keypair(config.db_dir.clone()).await;
    start_with_key(config, out_sender, self_receiver, key).await
}

async fn start_bootstrap_peers(
    config: Config,
    out_sender: Sender<ReceiveMessage>,
    key: Key,
) -> (Arc<Global>, Receiver<TransportRecvMessage>) {
    let peer_id = key.peer_id();

    let Config {
        db_dir,
        mut peer,
        mut allowlist,
        blocklist,
        allow_peer_list,
        block_peer_list,
        permission,
        only_stable_data: _,
        delivery_length,
    } = config;

    allowlist.extend(allow_peer_list.iter().map(|pid| Peer::peer(*pid)));

    peer.id = peer_id;
    peer.assist = get_assist(db_dir.clone()).await;
    debug!("P2P ID: {} - {}", peer.id.to_hex(), peer.assist.to_hex());

    let mut peer_list_path = db_dir;
    peer_list_path.push(STORAGE_PEER_LIST_KEY);
    let peer_list = Arc::new(RwLock::new(PeerList::load(
        peer_id,
        peer.assist,
        peer_list_path,
        allowlist,
        (block_peer_list, blocklist),
    )));

    let mut transports: HashMap<TransportType, Sender<TransportSendMessage>> = HashMap::new();

    let (local_addr, trans_send, trans_option, main_option) = transport_start(&peer, None)
        .await
        .expect("Transport binding failure!");
    let trans_recv = trans_option.unwrap(); // safe
    let main_trans = main_option.unwrap(); // safe

    peer.socket = local_addr;
    transports.insert(peer.transport, trans_send.clone());

    let global = Arc::new(Global {
        peer,
        key,
        out_sender,
        delivery_length,
        trans: main_trans,
        transports: Arc::new(RwLock::new(transports)),
        buffer: Arc::new(RwLock::new(Buffer::init())),
        peer_list: peer_list.clone(),
        is_relay_data: !permission,
    });

    // bootstrap allow list.
    for a in peer_list.read().await.bootstrap() {
        let (session_key, remote_pk) = global.generate_remote();
        let _ = global
            .trans_send(
                &a.transport,
                TransportSendMessage::Connect(a.socket, remote_pk, session_key),
            )
            .await;
    }

    (global, trans_recv)
}

/// start server
pub async fn start_with_key(
    config: Config,
    out_sender: Sender<ReceiveMessage>,
    mut self_receiver: Receiver<SendMessage>,
    key: Key,
) -> Result<PeerId> {
    let peer_id = key.peer_id();

    let (global, mut trans_recv) = start_bootstrap_peers(config.clone(), out_sender, key).await;

    let only_stable_data = config.only_stable_data;
    let delivery_length = config.delivery_length;

    let recv_data = !only_stable_data;
    let inner_global = global.clone();
    let listen_task = tokio::spawn(async move {
        enum FutureResult {
            Trans(TransportRecvMessage),
            Clear,
            Check,
        }
        loop {
            let futres = select! {
                v = async {
                    trans_recv.recv().await.map(|msg| FutureResult::Trans(msg))
                } => v,
                v = async {
                    // Check Timer: every 10s to check network. (read only).
                    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                    Some(FutureResult::Check)
                } => v,
                v = async {
                    // Clear Timer: every 60s to check buffer.
                    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                    Some(FutureResult::Clear)
                } => v,
            };

            match futres {
                Some(FutureResult::Trans(TransportRecvMessage(
                    addr,
                    RemotePublic(remote_peer, dh_key),
                    is_self,
                    stream_sender,
                    stream_receiver,
                    endpoint_sender,
                ))) => {
                    debug!("Incoming remote peer...");
                    // 1. check is block ip.
                    if inner_global.peer_list.read().await.is_block_addr(&addr) {
                        debug!("Incoming remote ip is blocked, close it.");
                        let _ = endpoint_sender.send(EndpointMessage::Close).await;
                        continue;
                    }

                    let remote_id = remote_peer.id;
                    let remote_peer = nat(addr, remote_peer);
                    debug!("Incoming remote NAT addr: {}", remote_peer.socket);

                    // 2. check is self or is block peer.
                    if (&remote_id == inner_global.peer_id()
                        && &remote_peer.assist == inner_global.assist_id())
                        || inner_global
                            .peer_list
                            .read()
                            .await
                            .is_block_peer(&remote_id)
                    {
                        debug!("Incoming remote peer is blocked, close it.");
                        let _ = endpoint_sender.send(EndpointMessage::Close).await;
                        continue;
                    }

                    // 3. check session key and send self info to remote.
                    let session_key = if let Some(mut session_key) = is_self {
                        if session_key.complete(&remote_id, dh_key) {
                            session_key
                        } else {
                            debug!("Incoming remote session key is invalid, close it.");
                            let _ = endpoint_sender.send(EndpointMessage::Close).await;
                            continue;
                        }
                    } else {
                        if let Some((session_key, remote_pk)) =
                            inner_global.complete_remote(&remote_id, dh_key)
                        {
                            let _ = endpoint_sender
                                .send(EndpointMessage::Handshake(remote_pk))
                                .await;
                            session_key
                        } else {
                            debug!("Incoming remote session key is invalid, close it.");
                            let _ = endpoint_sender.send(EndpointMessage::Close).await;
                            continue;
                        }
                    };

                    // 4. check is stable relay connections.
                    if let Some(ss) = inner_global.peer_list.read().await.is_relay(&remote_id) {
                        debug!("Incoming remote upgrade to direct.");
                        let _ = ss
                            .send(SessionMessage::DirectIncoming(
                                remote_peer,
                                stream_sender,
                                stream_receiver,
                                endpoint_sender,
                            ))
                            .await;
                        continue;
                    }

                    // 5. save to DHTs or Owns.
                    let (session_sender, session_receiver) = new_session_channel();
                    let kv = KadValue(session_sender.clone(), stream_sender, remote_peer);

                    let is_own = &remote_id == inner_global.peer_id();
                    if is_own {
                        inner_global
                            .peer_list
                            .write()
                            .await
                            .add_own(remote_peer.assist, kv, true);
                    } else {
                        let is_new = inner_global.peer_list.write().await.add_dht(kv).await;
                        // 6. check if had connected.
                        if !is_new {
                            debug!("Incoming remote add dht failure, close it.");
                            let _ = endpoint_sender.send(EndpointMessage::Close).await;
                            continue;
                        }

                        // 7. DHT help.
                        let peers = inner_global.peer_list.read().await.help_dht(&remote_id);
                        let _ = endpoint_sender.send(EndpointMessage::DHT(DHT(peers))).await;
                    }

                    session_spawn(
                        Session::new(
                            remote_peer,
                            session_sender,
                            stream_receiver,
                            ConnectType::Direct(endpoint_sender),
                            session_key,
                            inner_global.clone(),
                            is_own || recv_data,
                            is_own,
                        ),
                        session_receiver,
                    );
                    debug!("Incoming remote sessioned: {}.", remote_id.short_show());
                }
                Some(FutureResult::Check) => {
                    if inner_global.peer_list.read().await.is_empty() {
                        let _ = inner_global.out_send(ReceiveMessage::NetworkLost).await;
                    }
                }
                Some(FutureResult::Clear) => {
                    inner_global.buffer.write().await.timer_clear().await;
                }
                None => break,
            }
        }
    });

    tokio::spawn(async move {
        loop {
            match self_receiver.recv().await {
                Some(SendMessage::StableConnect(tid, to, data)) => {
                    debug!("Outside: StableConnect to {}.", to.id.short_show());
                    if &to.id == global.peer_id() {
                        warn!("CHAMOMILE: STABLE CONNECT NERVER TO SELF.");
                        if tid != 0 {
                            let _ = global
                                .out_send(ReceiveMessage::Delivery(
                                    DeliveryType::StableConnect,
                                    tid,
                                    false,
                                    delivery_split!(data, delivery_length),
                                ))
                                .await;
                        }
                        continue;
                    }

                    // check peer is not effective and addr is effective
                    if !to.effective_id() && to.effective_socket() {
                        debug!("Outside: StableConnect start new connection with IP.");
                        let delivery = delivery_split!(data, global.delivery_length);

                        // add to stable buffer.
                        let mut buffer_lock = global.buffer.write().await;
                        if buffer_lock.add_connect(BufferKey::Addr(to.socket), tid, data) {
                            debug!("Outside: StableConnect is processing, save to buffer.");
                            drop(buffer_lock);
                            continue;
                        }
                        drop(buffer_lock);

                        let g = global.clone();
                        tokio::spawn(async move {
                            let _ = direct_stable(tid, delivery, to, g, recv_data, false).await;
                        });
                        continue;
                    }

                    // 1. get it or closest peer.
                    let peer_list_lock = global.peer_list.read().await;
                    let results = peer_list_lock.get(&to.id);
                    if results.is_none() {
                        drop(peer_list_lock);
                        warn!("CHAMOMILE: CANNOT REACH NETWORK.");
                        if tid != 0 {
                            let _ = global
                                .out_send(ReceiveMessage::Delivery(
                                    DeliveryType::StableConnect,
                                    tid,
                                    false,
                                    delivery_split!(data, delivery_length),
                                ))
                                .await;
                        }
                        continue;
                    }

                    // 2. if connected, send to remote.
                    let (s, _, is_it) = results.unwrap(); // safe checked.
                    if is_it {
                        debug!("Outside: StableConnect multiple stable connected.");
                        let _ = s.send(SessionMessage::StableConnect(tid, data)).await;
                        drop(peer_list_lock);
                    } else {
                        let ss = s.clone();
                        drop(peer_list_lock);

                        // 3. check if had in buffer tmp.
                        if let Some(sender) = global.buffer.read().await.get_tmp_session(&to.id) {
                            debug!("Outside: StableConnect is in tmp, send to it.");
                            let _ = sender.send(SessionMessage::StableConnect(tid, data)).await;
                            continue;
                        }

                        // 4. add to stable buffer.
                        let mut buffer_lock = global.buffer.write().await;
                        let delivery = delivery_split!(data, global.delivery_length);
                        if buffer_lock.add_connect(BufferKey::Peer(to.id), tid, data) {
                            debug!("Outside: StableConnect is processing, save to buffer.");
                            drop(buffer_lock);
                            continue;
                        }
                        drop(buffer_lock);

                        let g = global.clone();
                        if to.effective_socket() {
                            debug!("Outside: StableConnect start new connection with IP.");
                            tokio::spawn(async move {
                                let _ = direct_stable(tid, delivery, to, g, recv_data, false).await;
                            });
                        } else {
                            debug!("Outside: StableConnect start new connection with ID.");
                            tokio::spawn(async move {
                                let _ =
                                    relay_stable(tid, delivery, to, ss, g, recv_data, false).await;
                            });
                        }
                    }
                }
                Some(SendMessage::StableResult(tid, to, is_ok, is_force, data)) => {
                    debug!("Outside: StableResult to {}.", to.id.short_show());
                    if &to.id == global.peer_id() {
                        warn!("CHAMOMILE: STABLE CONNECT NERVER TO SELF.");
                        if tid != 0 {
                            let _ = global
                                .out_send(ReceiveMessage::Delivery(
                                    DeliveryType::StableResult,
                                    tid,
                                    false,
                                    delivery_split!(data, delivery_length),
                                ))
                                .await;
                        }
                        continue;
                    }

                    // 1. check if in tmp.
                    if let Some(sender) = global.buffer.read().await.get_tmp_session(&to.id) {
                        debug!("Outside: StableResult get the tmp session.");
                        let _ = sender
                            .send(SessionMessage::StableResult(tid, is_ok, is_force, data))
                            .await;
                        continue;
                    }

                    // 2. check if in DHT or stable.
                    let peer_list_lock = global.peer_list.read().await;
                    let results = peer_list_lock.get(&to.id);
                    if results.is_none() {
                        drop(peer_list_lock);
                        warn!("CHAMOMILE: CANNOT REACH NETWORK.");
                        if tid != 0 {
                            let _ = global
                                .out_send(ReceiveMessage::Delivery(
                                    DeliveryType::StableResult,
                                    tid,
                                    false,
                                    delivery_split!(data, delivery_length),
                                ))
                                .await;
                        }
                        continue;
                    }

                    let (s, _, is_it) = results.unwrap(); // safe checked.
                    if is_it {
                        debug!("Outside: StableResult get the is_it session.");
                        let _ = s
                            .send(SessionMessage::StableResult(tid, is_ok, is_force, data))
                            .await;
                        drop(peer_list_lock);
                    } else {
                        // 3. check if is_ok, if ok, start stable connected.
                        if !is_ok {
                            drop(peer_list_lock);
                            continue;
                        }

                        let ss = s.clone();
                        drop(peer_list_lock);

                        // 4. check if had in buffer tmp.
                        if let Some(sender) = global.buffer.read().await.get_tmp_session(&to.id) {
                            debug!("Outside: StableResult had tmp session.");
                            let _ = sender
                                .send(SessionMessage::StableResult(tid, is_ok, is_force, data))
                                .await;
                            continue;
                        }

                        // 5. add to stable buffer.
                        let delivery = delivery_split!(data, global.delivery_length);
                        let mut buffer_lock = global.buffer.write().await;
                        if buffer_lock.add_result(to.id, tid, data) {
                            debug!("Outside: StableResult is processing, save to buffer.");
                            drop(buffer_lock);
                            continue;
                        }
                        drop(buffer_lock);

                        let g = global.clone();
                        debug!("Outside: StableResult start new connection with ID.");
                        if to.effective_socket() {
                            tokio::spawn(async move {
                                let _ = direct_stable(tid, delivery, to, g, recv_data, false).await;
                            });
                        } else {
                            tokio::spawn(async move {
                                let _ =
                                    relay_stable(tid, delivery, to, ss, g, recv_data, false).await;
                            });
                        }
                    }
                }
                Some(SendMessage::StableDisconnect(pid)) => {
                    debug!("Outside: StableDisconnect to {}.", pid.short_show());
                    if let Some((sender, _, is_it)) = global.peer_list.read().await.get(&pid) {
                        if is_it {
                            let _ = sender.send(SessionMessage::Close).await;
                        }
                    }
                }
                Some(SendMessage::Connect(peer)) => {
                    debug!("Outside: DHT Connect to {}.", peer.socket);
                    let (session_key, remote_pk) = global.generate_remote();
                    let _ = global
                        .trans_send(
                            &peer.transport,
                            TransportSendMessage::Connect(peer.socket, remote_pk, session_key),
                        )
                        .await;
                }
                Some(SendMessage::DisConnect(peer)) => {
                    debug!("Outside: DHT Disconnect to {}.", peer.socket);
                    global
                        .peer_list
                        .write()
                        .await
                        .peer_disconnect(&peer.socket)
                        .await;
                }
                Some(SendMessage::Data(tid, to, data)) => {
                    // check if send to self. better circle for application.
                    if &to == global.peer_id() {
                        warn!("CHAMOMILE: Data to self. PLEASE use SendMessage::OwnEvent.");
                        if tid != 0 {
                            let _ = global
                                .out_send(ReceiveMessage::Delivery(
                                    DeliveryType::Data,
                                    tid,
                                    true,
                                    delivery_split!(data, delivery_length),
                                ))
                                .await;
                        }
                        let _ = global.out_send(ReceiveMessage::Data(to, data)).await;
                        continue;
                    }

                    if let Some((sender, _, is_it)) = global.peer_list.read().await.get(&to) {
                        if is_it {
                            let _ = sender.send(SessionMessage::Data(tid, data)).await;
                        } else {
                            // only happen on permissionless.
                            let _ = sender
                                .send(SessionMessage::RelayData(*global.peer_id(), to, data))
                                .await;
                        }
                    } else {
                        warn!("CHAMOMILE: CANNOT REACH NETWORK.");
                        if tid != 0 {
                            let _ = global
                                .out_send(ReceiveMessage::Delivery(
                                    DeliveryType::Data,
                                    tid,
                                    false,
                                    delivery_split!(data, delivery_length),
                                ))
                                .await;
                        }
                    }
                }
                Some(SendMessage::Broadcast(broadcast, data)) => match broadcast {
                    Broadcast::StableAll => {
                        for (_to, (sender, _)) in global.peer_list.read().await.stable_all() {
                            let _ = sender.send(SessionMessage::Data(0, data.clone())).await;
                        }
                    }
                    Broadcast::Gossip => {
                        // TODO more Gossip base on Kad.
                        for (_to, sender) in global.peer_list.read().await.all() {
                            let _ = sender.send(SessionMessage::Data(0, data.clone())).await;
                        }
                    }
                },
                Some(SendMessage::OwnEvent(data)) => {
                    let peer_list = global.peer_list.read().await;
                    for pid in peer_list.own() {
                        if let Some((sender, _, is_it)) = peer_list.get(&pid) {
                            if is_it {
                                let _ = sender.send(SessionMessage::Data(0, data.clone())).await;
                            }
                        }
                    }
                }
                Some(SendMessage::Stream(_symbol, _stream_type, _data)) => {
                    // TODO WIP
                }
                Some(SendMessage::NetworkState(req, res_sender)) => match req {
                    StateRequest::Stable => {
                        let peers = global
                            .peer_list
                            .read()
                            .await
                            .stable_all()
                            .iter()
                            .map(|(id, (_, is_direct))| (*id, *is_direct))
                            .collect();
                        let _ = res_sender.send(StateResponse::Stable(peers)).await;
                    }
                    StateRequest::DHT => {
                        let peers = global.peer_list.read().await.dht_keys();
                        let _ = res_sender.send(StateResponse::DHT(peers)).await;
                    }
                    StateRequest::Seed => {
                        let seeds = global
                            .peer_list
                            .read()
                            .await
                            .bootstrap()
                            .iter()
                            .map(|p| **p)
                            .collect();
                        let _ = res_sender.send(StateResponse::Seed(seeds)).await;
                    }
                },
                Some(SendMessage::NetworkReboot) => {
                    // rebootstrap allow list.
                    for a in global.peer_list.read().await.bootstrap() {
                        let (session_key, remote_pk) = global.generate_remote();
                        let _ = global
                            .trans_send(
                                &a.transport,
                                TransportSendMessage::Connect(a.socket, remote_pk, session_key),
                            )
                            .await;
                    }
                }
                Some(SendMessage::NetworkStop) => {
                    // clear all sessions
                    for (_, sender) in global.peer_list.read().await.all() {
                        let _ = sender.send(SessionMessage::Close).await;
                    }

                    // clear all transports.
                    for (_, sender) in global.transports.read().await.iter() {
                        let _ = sender.send(TransportSendMessage::Stop).await;
                    }

                    listen_task.abort();
                    break;
                }
                None => break,
            }
        }
    });

    Ok(peer_id)
}
