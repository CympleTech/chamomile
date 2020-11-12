use smol::{
    channel::{Receiver, Sender},
    fs,
    io::Result,
    lock::{Mutex, RwLock},
};
use std::collections::HashMap;
use std::sync::Arc;

use chamomile_types::{
    message::{ReceiveMessage, SendMessage, StateRequest, StateResponse},
    types::{Broadcast, PeerId, TransportType},
};

use crate::config::Config;
use crate::hole_punching::{nat, DHT};
use crate::keys::{KeyType, Keypair, SessionKey};
use crate::peer::Peer;
use crate::peer_list::PeerList;
use crate::primitives::{STORAGE_KEY_KEY, STORAGE_NAME, STORAGE_PEER_LIST_KEY};
use crate::session::{
    direct_dht, new_session_send_channel, relay_stable, ConnectType, EndpointMessage, RemotePublic,
    Session, SessionSendMessage,
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

    // use in below.
    let peer_2 = peer.clone();

    let peer = Arc::new(peer);
    let key = Arc::new(key);

    let out_send_1 = out_send.clone();
    let endpoint_send_1 = endpoint_send.clone();
    let peer_1 = peer.clone();
    let key_1 = key.clone();
    let peer_list_1 = peer_list.clone();
    let transports_1 = transports.clone();
    let remote_bytes_1 = remote_bytes.clone();

    smol::spawn(async move {
        loop {
            match endpoint_recv.recv().await {
                Ok(EndpointIncomingMessage(
                    addr,
                    remote_public_bytes,
                    is_stable,
                    is_self,
                    stream_receiver,
                    stream_sender,
                )) => {
                    debug!("receiver incoming connect: {:?}", addr);
                    // check and start session
                    if peer_list_1.read().await.is_black_addr(&addr) {
                        debug!("receiver incoming connect is blocked");
                        let _ = stream_sender.send(EndpointStreamMessage::Close).await;
                        continue;
                    }
                    // check pk info.
                    let rpr = RemotePublic::from_bytes(remote_public_bytes);
                    if rpr.is_err() {
                        debug!("Debug: Session invalid pk, close it.");
                        let _ = stream_sender.send(EndpointStreamMessage::Close).await;
                        continue;
                    }
                    let RemotePublic(remote_peer_key, remote_peer) = rpr.unwrap();

                    let remote_peer_id = remote_peer_key.peer_id();
                    debug!("Debug: Session connected: {}", remote_peer_id.short_show());

                    let remote_peer = nat(addr, remote_peer);
                    debug!("Debug: NAT addr: {}", remote_peer.addr());

                    // check and save tmp and save outside
                    if remote_peer_id == peer_id
                        || peer_list_1.read().await.is_black_peer(&remote_peer_id)
                    {
                        debug!("session remote peer is blocked, close it.");
                        let _ = stream_sender.send(EndpointStreamMessage::Close).await;
                        continue;
                    }

                    let (session_sender, session_receiver) = new_session_send_channel();

                    // save to peer_list.
                    let mut peer_list_lock = peer_list_1.write().await;
                    let is_new = peer_list_lock
                        .peer_add(remote_peer_id, session_sender.clone(), remote_peer)
                        .await;
                    drop(peer_list_lock);

                    // check if connection had.
                    if !is_new {
                        debug!("Session is had connected, close it.");
                        let _ = stream_sender.send(EndpointStreamMessage::Close).await;
                        continue;
                    }

                    // if not self, send self publics info.
                    if !is_self {
                        stream_sender
                            .send(EndpointStreamMessage::Handshake(remote_bytes_1.clone()))
                            .await
                            .expect("Session to Endpoint (Data Key)");
                    }

                    // TODO exchange first session_key.
                    let session_key: SessionKey = key_1.key.session_key(&key_1, &remote_peer_key);
                    stream_sender
                        .send(EndpointStreamMessage::Bytes(
                            EndpointMessage::Key(session_key.out_bytes()).to_bytes(),
                        ))
                        .await
                        .expect("Session to Endpoint (Data SessionKey)");

                    // DHT help.
                    let peers = peer_list_1.read().await.get_dht_help(&remote_peer_id);
                    stream_sender
                        .send(EndpointStreamMessage::Bytes(
                            EndpointMessage::DHT(DHT(peers)).to_bytes(),
                        ))
                        .await
                        .expect("Sesssion to Endpoint (Data)");

                    // check if it is stable connection.
                    if !is_self {
                        if let Some(stable_bytes) = is_stable {
                            out_send_1
                                .send(ReceiveMessage::StableConnect(remote_peer_id, stable_bytes))
                                .await
                                .expect("Server to Outside");
                        }
                    }

                    smol::spawn(direct_dht(
                        Session {
                            my_sender: session_sender,
                            my_peer_id: peer_id.clone(),
                            remote_peer_id: remote_peer_id.clone(),
                            remote_peer: remote_peer,
                            endpoint_sender: endpoint_send_1.clone(),
                            out_sender: out_send_1.clone(),
                            connect: ConnectType::Direct(stream_sender),
                            session_receiver: session_receiver,
                            session_key: session_key,
                            key: key_1.clone(),
                            peer: peer_1.clone(),
                            peer_list: peer_list_1.clone(),
                            transports: transports_1.clone(),
                            is_recv_data: !only_stable_data,
                            is_relay_data: !permission,
                            is_stable: false,
                            heartbeat: Arc::new(Mutex::new(0)),
                        },
                        stream_receiver,
                    ))
                    .detach();
                }
                Err(_) => break,
            }
        }
    })
    .detach();

    smol::spawn(async move {
        loop {
            match self_recv.recv().await {
                Ok(SendMessage::StableConnect(to, socket, data)) => {
                    debug!("Send stable connect to: {:?}", to);
                    if to == peer_id {
                        info!("Nerver here, stable connect to self.");
                        continue;
                    }

                    if peer_list.read().await.stable_contains(&to) {
                        debug!("Aready stable connected");
                        out_send
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
                        let is_tmp = if let Some((sender, is_it)) = peer_list.read().await.get(&to)
                        {
                            if is_it {
                                let _ = sender.send(SessionSendMessage::StableConnect(data)).await;
                                false
                            } else {
                                let _ = sender
                                    .send(SessionSendMessage::RelayStableConnect(
                                        RemotePublic(key.public(), peer_2.clone()),
                                        to,
                                        data,
                                    ))
                                    .await;
                                true
                            }
                        } else {
                            false
                        };

                        if is_tmp {
                            // save to tmp_stable.
                            peer_list.write().await.add_tmp_stable(to, None);
                        }
                    }
                }
                Ok(SendMessage::StableResult(to, is_ok, is_force, data)) => {
                    debug!("Send stable result {} to: {:?}", is_ok, to);
                    if to == peer_id {
                        info!("Nerver here, stable result to self.");
                        continue;
                    }

                    let is_sender = if let Some((sender, is_it)) = peer_list.read().await.get(&to) {
                        debug!("Got peer to send stable result.");
                        if is_ok || !is_force {
                            debug!("ok to send.");
                            if is_it {
                                debug!("ok to send directly.");
                                let _ = sender
                                    .send(SessionSendMessage::StableResult(is_ok, data))
                                    .await;
                                None
                            } else {
                                debug!("ok to send relay.");
                                Some((sender.clone(), data))
                            }
                        } else if is_force && is_it {
                            let _ = sender.send(SessionSendMessage::Close).await;
                            None
                        } else {
                            Some((sender.clone(), data))
                        }
                    } else {
                        None
                    };

                    if let Some((sender, mut data)) = is_sender {
                        debug!("start send by relay");
                        let mut peer_list_lock = peer_list.write().await;
                        let remote_pk_result = peer_list_lock.tmp_stable(&to);
                        if remote_pk_result.is_none() {
                            continue;
                        }
                        let remote_pk_value = remote_pk_result.unwrap(); // unwrap is safe, checked.
                        if remote_pk_value.is_none() {
                            continue;
                        }
                        let RemotePublic(remote_key, remote_peer) = remote_pk_value.unwrap(); // unwrap is safe, checked.
                        let session_key: SessionKey = key.key.session_key(&key, &remote_key);
                        let mut c_data = if is_ok { vec![1u8] } else { vec![0u8] };
                        c_data.append(&mut data);
                        let e_data = session_key.encrypt(c_data);
                        let _ = sender
                            .send(SessionSendMessage::RelayStableResult(
                                RemotePublic(key.public(), peer_2.clone()),
                                to,
                                e_data,
                            ))
                            .await;

                        if is_ok {
                            // create a new stable session.
                            // only happen when the connection by relay.
                            let (session_sender, session_receiver) = new_session_send_channel();
                            peer_list_lock.stable_add(
                                to,
                                session_sender.clone(),
                                remote_peer.clone(),
                                false,
                            );

                            smol::spawn(relay_stable(Session {
                                my_sender: session_sender,
                                my_peer_id: peer_id.clone(),
                                remote_peer_id: to,
                                remote_peer: remote_peer,
                                endpoint_sender: endpoint_send.clone(),
                                out_sender: out_send.clone(),
                                connect: ConnectType::Relay(vec![sender]),
                                session_receiver: session_receiver,
                                session_key: session_key,
                                key: key.clone(),
                                peer: peer.clone(),
                                peer_list: peer_list.clone(),
                                transports: transports.clone(),
                                is_recv_data: true, // this is stable connection.
                                is_relay_data: !permission,
                                is_stable: true,
                                heartbeat: Arc::new(Mutex::new(0)),
                            }))
                            .detach();
                        }
                        drop(peer_list_lock);
                    }
                }
                Ok(SendMessage::StableDisconnect(peer_id)) => {
                    debug!("Send stable disconnect to: {:?}", peer_id);
                    if let Some(session) = peer_list.write().await.stable_remove(&peer_id) {
                        let _ = session.send(SessionSendMessage::Close).await;
                    }
                }
                Ok(SendMessage::Connect(addr)) => {
                    debug!("Send connect to: {:?}", addr);
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
                    debug!("Send disconnect to: {:?}", addr);
                    peer_list.write().await.peer_disconnect(&addr).await;
                    // send to endpoint, beacuse not konw session peer_id.
                    endpoint_send
                        .send(EndpointSendMessage::Close(addr))
                        .await
                        .expect("Server to Endpoint (DisConnect)");
                }
                Ok(SendMessage::Data(to, data)) => {
                    debug!(
                        "DEBUG: data is send to: {}, {}",
                        to.short_show(),
                        data.len()
                    );
                    let peer_list_lock = peer_list.read().await;
                    if let Some((sender, is_it)) = peer_list_lock.get(&to) {
                        if is_it {
                            let _ = sender.send(SessionSendMessage::Data(data)).await;
                        } else {
                            // only happen when the stable connection lost, or permissionless.
                            let _ = sender
                                .send(SessionSendMessage::RelayData(peer_id, to, data))
                                .await;
                        }
                    }
                }
                Ok(SendMessage::Broadcast(broadcast, data)) => match broadcast {
                    Broadcast::StableAll => {
                        let peer_list_lock = peer_list.read().await;
                        for (_to, (sender, _)) in peer_list_lock.stable_all() {
                            let _ = sender.send(SessionSendMessage::Data(data.clone())).await;
                        }
                        drop(peer_list_lock);
                    }
                    Broadcast::Gossip => {
                        // TODO more Gossip base on Kad.
                        let peer_list_lock = peer_list.read().await;
                        for (_to, sender) in peer_list_lock.all() {
                            let _ = sender.send(SessionSendMessage::Data(data.clone())).await;
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
