use smol::{
    channel::{Receiver, Sender},
    fs, future,
    io::Result,
    lock::RwLock,
};
use std::collections::HashMap;
use std::sync::Arc;

use chamomile_types::{
    delivery_split,
    message::{DeliveryType, ReceiveMessage, SendMessage, StateRequest, StateResponse},
    types::{Broadcast, PeerId, TransportType},
};

use crate::buffer::Buffer;
use crate::config::Config;
use crate::global::Global;
use crate::hole_punching::{nat, DHT};
use crate::kad::KadValue;
use crate::keys::{KeyType, Keypair};
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
        allowlist,
        blocklist,
        allow_peer_list,
        block_peer_list,
        permission,
        only_stable_data,
        delivery_length,
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
        (allow_peer_list, allowlist),
        (block_peer_list, blocklist),
    )));

    let default_transport = TransportType::from_str(&transport);
    let peer = Peer::new(key.peer_id(), addr, default_transport, true);

    let mut transports: HashMap<TransportType, Sender<TransportSendMessage>> = HashMap::new();
    let (trans_send, trans_recv) = transport_start(peer.transport(), *peer.addr())
        .await
        .expect("Transport binding failure!");
    transports.insert(default_transport, trans_send.clone());
    let _transports = Arc::new(RwLock::new(transports)); // TODO more about multiple transports.

    let global = Arc::new(Global {
        peer,
        key,
        out_sender,
        delivery_length,
        transport_sender: trans_send,
        buffer: Arc::new(RwLock::new(Buffer::init())),
        peer_list: peer_list.clone(),
        is_relay_data: !permission,
    });

    // bootstrap allow list.
    for a in peer_list.read().await.bootstrap() {
        let (session_key, remote_pk) = global.generate_remote();
        global
            .transport_sender
            .send(TransportSendMessage::Connect(*a, remote_pk, session_key))
            .await
            .expect("Server to Endpoint (Connect)");
    }

    drop(peer_list);

    let recv_data = !only_stable_data;
    let inner_global = global.clone();
    smol::spawn(async move {
        enum FutureResult {
            Trans(TransportRecvMessage),
            Clear,
            Check,
        }
        loop {
            match future::race(
                async { trans_recv.recv().await.map(|msg| FutureResult::Trans(msg)) },
                future::race(
                    async {
                        // Check Timer: every 10s to check network. (read only).
                        smol::Timer::after(std::time::Duration::from_secs(10)).await;
                        Ok(FutureResult::Check)
                    },
                    async {
                        // Clear Timer: every 60s to check buffer.
                        smol::Timer::after(std::time::Duration::from_secs(60)).await;
                        Ok(FutureResult::Clear)
                    },
                ),
            )
            .await
            {
                Ok(FutureResult::Trans(TransportRecvMessage(
                    addr,
                    RemotePublic(remote_key, remote_peer, dh_key),
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

                    let remote_id = remote_key.peer_id();
                    let remote_peer = nat(addr, remote_peer);
                    debug!("Incoming remote NAT addr: {}", remote_peer.addr());

                    // 2. check is self or is block peer.
                    if &remote_id == inner_global.peer_id()
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
                        if session_key.complete(&remote_key.pk, dh_key) {
                            session_key
                        } else {
                            debug!("Incoming remote session key is invalid, close it.");
                            let _ = endpoint_sender.send(EndpointMessage::Close).await;
                            continue;
                        }
                    } else {
                        if let Some((session_key, remote_pk)) =
                            inner_global.complete_remote(&remote_key, dh_key)
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

                    // 5. save to DHTs.
                    let (session_sender, session_receiver) = new_session_channel();
                    let kv = KadValue(session_sender.clone(), stream_sender.clone(), remote_peer);
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

                    session_spawn(Session::new(
                        remote_peer,
                        session_sender,
                        session_receiver,
                        stream_sender,
                        stream_receiver,
                        ConnectType::Direct(endpoint_sender),
                        session_key,
                        inner_global.clone(),
                        recv_data,
                    ));
                    debug!("Incoming remote sessioned: {}.", remote_id.short_show());
                }
                Ok(FutureResult::Check) => {
                    if inner_global.peer_list.read().await.is_empty() {
                        let _ = inner_global.out_send(ReceiveMessage::NetworkLost).await;
                    }
                }
                Ok(FutureResult::Clear) => {
                    inner_global.buffer.write().await.timer_clear().await;
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
                    debug!("Outside: StableConnect to {}.", to.short_show());
                    if &to == global.peer_id() {
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

                    // 1. get it or closest peer.
                    let peer_list_lock = global.peer_list.read().await;
                    let results = peer_list_lock.get(&to);
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
                        if let Some(sender) = global.buffer.read().await.get_tmp_session(&to) {
                            debug!("Outside: StableConnect is in tmp, send to it.");
                            let _ = sender.send(SessionMessage::StableConnect(tid, data)).await;
                            continue;
                        }

                        // 4. add to stable buffer.
                        let mut buffer_lock = global.buffer.write().await;
                        let delivery = delivery_split!(data, global.delivery_length);
                        if buffer_lock.add_connect(to, tid, data) {
                            debug!("Outside: StableConnect is processing, save to buffer.");
                            drop(buffer_lock);
                            continue;
                        }
                        drop(buffer_lock);

                        let g = global.clone();
                        if let Some(addr) = socket {
                            debug!("Outside: StableConnect start new connection with IP.");
                            smol::spawn(async move {
                                let _ = direct_stable(tid, delivery, to, addr, g, recv_data).await;
                            })
                            .detach();
                        } else {
                            debug!("Outside: StableConnect start new connection with ID.");
                            smol::spawn(async move {
                                let _ = relay_stable(tid, delivery, to, ss, g, recv_data).await;
                            })
                            .detach();
                        }
                    }
                }
                Ok(SendMessage::StableResult(tid, to, is_ok, is_force, data)) => {
                    debug!("Outside: StableResult to {}.", to.short_show());
                    if &to == global.peer_id() {
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
                    if let Some(sender) = global.buffer.read().await.get_tmp_session(&to) {
                        debug!("Outside: StableResult get the tmp session.");
                        let _ = sender
                            .send(SessionMessage::StableResult(tid, is_ok, is_force, data))
                            .await;
                        continue;
                    }

                    // 2. check if in DHT or stable.
                    let peer_list_lock = global.peer_list.read().await;
                    let results = peer_list_lock.get(&to);
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
                        if let Some(sender) = global.buffer.read().await.get_tmp_session(&to) {
                            debug!("Outside: StableResult had tmp session.");
                            let _ = sender
                                .send(SessionMessage::StableResult(tid, is_ok, is_force, data))
                                .await;
                            continue;
                        }

                        // 5. add to stable buffer.
                        let mut buffer_lock = global.buffer.write().await;
                        let delivery = delivery_split!(data, global.delivery_length);
                        if buffer_lock.add_result(to, tid, data) {
                            debug!("Outside: StableResult is processing, save to buffer.");
                            drop(buffer_lock);
                            continue;
                        }
                        drop(buffer_lock);

                        let g = global.clone();
                        debug!("Outside: StableResult start new connection with ID.");
                        smol::spawn(async move {
                            let _ = relay_stable(tid, delivery, to, ss, g, recv_data).await;
                        })
                        .detach();
                    }
                }
                Ok(SendMessage::StableDisconnect(pid)) => {
                    debug!("Outside: StableDisconnect to {}.", pid.short_show());
                    if let Some((sender, _, is_it)) = global.peer_list.read().await.get(&pid) {
                        if is_it {
                            let _ = sender.send(SessionMessage::Close).await;
                        }
                    }
                }
                Ok(SendMessage::Connect(addr)) => {
                    debug!("Outside: DHT Connect to {}.", addr);
                    let (session_key, remote_pk) = global.generate_remote();
                    let _ = global
                        .trans_send(TransportSendMessage::Connect(addr, remote_pk, session_key))
                        .await;
                }
                Ok(SendMessage::DisConnect(addr)) => {
                    debug!("Outside: DHT Disconnect to {}.", addr);
                    global.peer_list.write().await.peer_disconnect(&addr).await;
                }
                Ok(SendMessage::Data(tid, to, data)) => {
                    if let Some((sender, stream_sender, is_it)) =
                        global.peer_list.read().await.get(&to)
                    {
                        if is_it {
                            let _ = sender.send(SessionMessage::Data(tid, data)).await;
                        } else {
                            // only happen on permissionless.
                            let _ = stream_sender
                                .send(EndpointMessage::RelayData(*global.peer_id(), to, data))
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
                Ok(SendMessage::Broadcast(broadcast, data)) => match broadcast {
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
                Ok(SendMessage::Stream(_symbol, _stream_type, _data)) => {
                    // TODO WIP
                }
                Ok(SendMessage::NetworkState(req, res_sender)) => match req {
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
                        let seeds = global.peer_list.read().await.bootstrap().clone();
                        let _ = res_sender.send(StateResponse::Seed(seeds)).await;
                    }
                },
                Ok(SendMessage::NetworkReboot) => {
                    // rebootstrap allow list.
                    for a in global.peer_list.read().await.bootstrap() {
                        let (session_key, remote_pk) = global.generate_remote();
                        global
                            .transport_sender
                            .send(TransportSendMessage::Connect(*a, remote_pk, session_key))
                            .await
                            .expect("Server to Endpoint (Connect)");
                    }
                }
                Err(_) => break,
            }
        }
    })
    .detach();

    Ok(peer_id)
}
