use chamomile_types::{
    message::{DeliveryType, ReceiveMessage},
    types::{new_io_error, PeerId},
};
use smol::{
    channel::{self, Receiver, Sender},
    future,
    io::Result,
    lock::{Mutex, RwLock},
    prelude::*,
};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use crate::hole_punching::{nat, DHT};
use crate::keys::SessionKey;
use crate::peer::Peer;
use crate::peer_list::PeerList;
use crate::server::Global;
use crate::transports::{
    new_endpoint_channel, EndpointMessage, RemotePublic, TransportSendMessage,
};

/// server send to session message in channel.
pub(crate) enum SessionMessage {
    /// send bytes to session what want to send to peer..
    Data(u64, Vec<u8>),
    /// when need build a stable connection.
    StableConnect(u64, Vec<u8>),
    /// when receive a stable result.
    StableResult(u64, bool, bool, Vec<u8>),
    /// relay data help.
    RelayData(PeerId, PeerId, Vec<u8>),
    /// relay connect help.
    RelayConnect(RemotePublic, PeerId),
    /// relay connect result from other sessions.
    RelayResult(RemotePublic, Sender<SessionMessage>),
    /// relay closed.
    RelayClose(PeerId),
    /// close the session.
    Close,
}

/// new a channel for send message to session.
pub(crate) fn new_session_channel() -> (Sender<SessionMessage>, Receiver<SessionMessage>) {
    channel::unbounded()
}

pub(crate) async fn direct_stable(
    tid: u64,
    to: PeerId,
    data: Vec<u8>, // need as stable bytes to send.
    addr: SocketAddr,
    my_peer_id: PeerId,
    global: Arc<Global>,
    peer_list: Arc<RwLock<PeerList>>,
    is_recv_data: bool,
    is_relay_data: bool,
) -> Result<()> {
    // 1. send stable connect.
    // 2. if stable connected, keep it.

    let (endpoint_sender, endpoint_receiver) = new_endpoint_channel(); // transpot's use.
    let (stream_sender, stream_receiver) = new_endpoint_channel(); // session's use.

    global
        .trans_send(TransportSendMessage::StableConnect(
            stream_sender.clone(),
            endpoint_receiver,
            addr,
            global.remote_pk(),
        ))
        .await?;

    if let Ok(EndpointMessage::Handshake(RemotePublic(remote_key, remote_peer))) =
        stream_receiver.recv().await
    {
        // ok connected.
        let remote_peer_id = remote_key.peer_id();
        if remote_peer_id == my_peer_id {
            let _ = endpoint_sender.send(EndpointMessage::Close).await;
            if tid != 0 {
                global
                    .out_send(ReceiveMessage::Delivery(
                        DeliveryType::StableConnect,
                        tid,
                        false,
                    ))
                    .await?;
            }
            return Ok(());
        }

        let remote_peer = nat(addr, remote_peer);
        let (session_sender, session_receiver) = new_session_channel(); // server's use.
        let session_key: SessionKey = global.key.session_key(&remote_key);

        // save to tmp stable connection.
        peer_list.write().await.add_tmp_stable(
            remote_peer_id,
            session_sender.clone(),
            stream_sender.clone(),
        );

        let session = Session::new(
            my_peer_id,
            remote_peer,
            stream_sender,
            session_sender,
            session_receiver,
            ConnectType::Direct(endpoint_sender),
            session_key,
            global,
            peer_list,
            is_recv_data,
            is_relay_data,
            false,
        );

        session
            .send_core_data(CoreData::StableConnect(tid, data))
            .await?;

        session_run(session, stream_receiver).await
    } else {
        drop(stream_sender);
        drop(stream_receiver);
        drop(endpoint_sender);

        // try start relay stable.
        let ss = if let Some((_, stream_sender, _)) = peer_list.read().await.get(&to) {
            Some(stream_sender.clone())
        } else {
            None
        };

        if ss.is_some() {
            relay_stable(
                tid,
                to,
                data,
                ss.unwrap(),
                my_peer_id,
                global,
                peer_list,
                is_recv_data,
                is_relay_data,
            )
            .await
        } else {
            if tid != 0 {
                global
                    .out_send(ReceiveMessage::Delivery(
                        DeliveryType::StableConnect,
                        tid,
                        false,
                    ))
                    .await?;
            }

            Ok(())
        }
    }
}

pub(crate) async fn relay_stable(
    tid: u64,
    to: PeerId,
    data: Vec<u8>,
    relay_sender: Sender<EndpointMessage>,
    my_peer_id: PeerId,
    global: Arc<Global>,
    peer_list: Arc<RwLock<PeerList>>,
    is_recv_data: bool,
    is_relay_data: bool,
) -> Result<()> {
    info!("start stable connection.");

    // 1. try relay connect. (timeout).
    // 2. send stable connect.
    // 3. if stable connected, keep it.

    let (stream_sender, stream_receiver) = new_endpoint_channel(); // session's use.
    let (session_sender, session_receiver) = new_session_channel(); // server's use.

    peer_list
        .write()
        .await
        .add_tmp_stable(to, session_sender.clone(), stream_sender.clone());

    relay_sender
        .send(EndpointMessage::RelayConnect(global.remote_pk(), to))
        .await
        .map_err(|_e| new_io_error("Session missing"))?;

    let msg = session_receiver
        .recv()
        .or(async {
            smol::Timer::after(std::time::Duration::from_secs(10)).await;
            Err(smol::channel::RecvError)
        })
        .await;

    if let Ok(SessionMessage::RelayResult(RemotePublic(remote_key, remote_peer), recv_ss)) = msg {
        // ok connected.
        let remote_peer_id = remote_key.peer_id();
        if remote_peer_id == my_peer_id {
            peer_list.write().await.remove_tmp_stable(&to);
            if tid != 0 {
                global
                    .out_send(ReceiveMessage::Delivery(
                        DeliveryType::StableConnect,
                        tid,
                        false,
                    ))
                    .await?;
            }
            return Ok(());
        }

        let session_key: SessionKey = global.key.session_key(&remote_key);

        let session = Session::new(
            my_peer_id,
            remote_peer,
            stream_sender,
            session_sender,
            session_receiver,
            ConnectType::Relay(recv_ss),
            session_key,
            global,
            peer_list,
            is_recv_data,
            is_relay_data,
            false,
        );

        session
            .send_core_data(CoreData::StableConnect(tid, data))
            .await?;

        session_run(session, stream_receiver).await
    } else {
        // failure send outside.
        peer_list.write().await.remove_tmp_stable(&to);
        if tid != 0 {
            global
                .out_send(ReceiveMessage::Delivery(
                    DeliveryType::StableConnect,
                    tid,
                    false,
                ))
                .await?;
        }

        Ok(())
    }
}

pub(crate) fn session_tmp(session: Session, stream_receiver: Receiver<EndpointMessage>) {
    smol::spawn(session_run(session, stream_receiver)).detach();
}

pub(crate) async fn session_run(
    mut session: Session,
    stream_receiver: Receiver<EndpointMessage>,
) -> Result<()> {
    info!("start Session listening.");
    let upgrade = future::race(
        future::race(session.heartbeat(), session.listen_outside()),
        session.listen_endpoint(&stream_receiver),
    )
    .await;

    info!(
        "over DHT connection. will start stable: {}",
        upgrade.is_ok()
    );

    let mut peer_list_lock = session.peer_list.write().await;
    peer_list_lock.peer_remove(session.remote_peer.id());
    peer_list_lock.remove_tmp_stable(session.remote_peer.id());
    drop(peer_list_lock);

    if upgrade.is_ok() {
        // stable connection.
        session.is_recv_data = true;
        session.is_stable = true;

        session.peer_list.write().await.stable_add(
            session.remote_peer_id(),
            session.session_sender.clone(),
            session.stream_sender.clone(),
            session.remote_peer.clone(),
            true,
        );

        info!("start stable connection.");
        let _ = future::race(
            future::race(session.heartbeat(), session.listen_outside()),
            future::race(session.robust(), session.listen_endpoint(&stream_receiver)),
        )
        .await;

        session
            .peer_list
            .write()
            .await
            .stable_leave(session.remote_peer.id());

        session
            .out_send(ReceiveMessage::StableLeave(session.remote_peer_id()))
            .await?;
    }

    session.close().await?;

    debug!("Session broke: {:?}", session.remote_peer.id());

    Ok(())
}

pub(crate) enum ConnectType {
    Direct(Sender<EndpointMessage>),
    Relay(Sender<SessionMessage>),
}

pub(crate) struct Session {
    pub my_peer_id: PeerId,
    pub remote_peer: Peer,
    pub stream_sender: Sender<EndpointMessage>,
    pub session_sender: Sender<SessionMessage>,
    pub session_receiver: Receiver<SessionMessage>,
    pub endpoint: ConnectType,
    pub session_key: SessionKey,
    pub global: Arc<Global>,
    pub peer_list: Arc<RwLock<PeerList>>,
    pub is_recv_data: bool,
    pub is_relay_data: bool,
    pub is_stable: bool,
    pub heartbeat: Arc<Mutex<u32>>,
    pub relay_sessions: Arc<Mutex<HashMap<PeerId, Sender<SessionMessage>>>>,
}

impl Session {
    pub fn new(
        my_peer_id: PeerId,
        remote_peer: Peer,
        stream_sender: Sender<EndpointMessage>,
        session_sender: Sender<SessionMessage>,
        session_receiver: Receiver<SessionMessage>,
        endpoint: ConnectType,
        session_key: SessionKey,
        global: Arc<Global>,
        peer_list: Arc<RwLock<PeerList>>,
        is_recv_data: bool,
        is_relay_data: bool,
        is_stable: bool,
    ) -> Session {
        Session {
            my_peer_id,
            remote_peer,
            stream_sender,
            session_sender,
            session_receiver,
            endpoint,
            session_key,
            global,
            peer_list,
            is_recv_data,
            is_relay_data,
            is_stable,
            heartbeat: Arc::new(Mutex::new(0)),
            relay_sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    #[inline]
    fn remote_peer_id(&self) -> PeerId {
        self.remote_peer.id().clone()
    }

    async fn close(&self) -> Result<()> {
        if self.is_direct() {
            self.direct_send(EndpointMessage::Close).await
        } else {
            self.relay_send(SessionMessage::RelayClose(self.my_peer_id))
                .await
        }
    }

    fn is_direct(&self) -> bool {
        match self.endpoint {
            ConnectType::Direct(..) => true,
            _ => false,
        }
    }

    async fn failure_send(&self, e_data: Vec<u8>) -> Result<()> {
        if let Ok(bytes) = self.session_key.decrypt(e_data) {
            if let Ok(msg) = CoreData::from_bytes(bytes) {
                match msg {
                    CoreData::Ping => {}
                    CoreData::Pong => {}
                    CoreData::Key(_) => {}
                    CoreData::Delivery(..) => {}
                    CoreData::Data(tid, _) => {
                        if tid != 0 {
                            self.out_send(ReceiveMessage::Delivery(DeliveryType::Data, tid, false))
                                .await?;
                        }
                    }
                    CoreData::StableConnect(tid, _) => {
                        if tid != 0 {
                            self.out_send(ReceiveMessage::Delivery(
                                DeliveryType::StableConnect,
                                tid,
                                false,
                            ))
                            .await?;
                        }
                    }
                    CoreData::StableResult(tid, ..) => {
                        if tid != 0 {
                            self.out_send(ReceiveMessage::Delivery(
                                DeliveryType::StableResult,
                                tid,
                                false,
                            ))
                            .await?;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    #[inline]
    async fn out_send(&self, msg: ReceiveMessage) -> Result<()> {
        self.global.out_send(msg).await
    }

    async fn direct_send(&self, msg: EndpointMessage) -> Result<()> {
        match &self.endpoint {
            ConnectType::Direct(sender) => sender
                .send(msg)
                .await
                .map_err(|_e| new_io_error("Endpoint missing")),
            _ => Ok(()),
        }
    }

    async fn relay_send(&self, msg: SessionMessage) -> Result<()> {
        match &self.endpoint {
            ConnectType::Relay(sender) => sender
                .send(msg)
                .await
                .map_err(|_e| new_io_error("Endpoint missing")),
            _ => Ok(()),
        }
    }

    async fn send_core_data(&self, data: CoreData) -> Result<()> {
        let e_data = self.session_key.encrypt(data.to_bytes());
        if self.is_direct() {
            self.direct_send(EndpointMessage::Data(e_data)).await
        } else {
            self.relay_send(SessionMessage::RelayData(
                self.my_peer_id,
                self.remote_peer_id(),
                e_data,
            ))
            .await
        }
    }

    async fn handle_core_data(&self, e_data: Vec<u8>) -> Result<bool> {
        if let Ok(bytes) = self.session_key.decrypt(e_data) {
            if let Ok(msg) = CoreData::from_bytes(bytes) {
                match msg {
                    CoreData::Ping => {
                        self.send_core_data(CoreData::Pong).await?;
                    }
                    CoreData::Pong => {
                        let mut guard = self.heartbeat.lock().await;
                        *guard = 0;
                        drop(guard);
                    }
                    CoreData::Key(_key) => {
                        debug!("TODO session rebuild new encrypt key.");
                        // if !session_key.is_ok() {
                        //     if !session_key.in_bytes(bytes) {
                        //         server_send
                        //             .send(SessionReceiveEndpointMessage::Close(remote_peer_id))
                        //             .await
                        //             .expect("Session to Server (Key Close)");
                        //         endpoint_sender
                        //             .send(EndpointMessage::Close)
                        //             .await
                        //             .expect("Session to Endpoint (Key Close)");
                        //         break;
                        //     }

                        //     endpoint_sender
                        //         .send(EndpointMessage::Bytes(
                        //             EndpointMessage::Key(session_key.out_bytes()).to_bytes(),
                        //         ))
                        //         .await
                        //         .expect("Session to Endpoint (Bytes)");
                        // }
                    }
                    CoreData::Data(tid, p_data) => {
                        if self.is_recv_data {
                            self.out_send(ReceiveMessage::Data(self.remote_peer_id(), p_data))
                                .await?;
                            if tid != 0 {
                                self.send_core_data(CoreData::Delivery(DeliveryType::Data, tid))
                                    .await?;
                            }
                        }
                    }
                    CoreData::Delivery(t, tid) => {
                        if tid != 0 {
                            match t {
                                DeliveryType::Data => {
                                    if self.is_recv_data {
                                        self.out_send(ReceiveMessage::Delivery(t, tid, true))
                                            .await?;
                                    }
                                }
                                _ => {
                                    self.out_send(ReceiveMessage::Delivery(t, tid, true))
                                        .await?;
                                }
                            }
                        }
                    }
                    CoreData::StableConnect(tid, data) => {
                        self.out_send(ReceiveMessage::StableConnect(self.remote_peer_id(), data))
                            .await?;
                        if tid != 0 {
                            self.send_core_data(CoreData::Delivery(
                                DeliveryType::StableConnect,
                                tid,
                            ))
                            .await?;
                        }
                    }
                    CoreData::StableResult(tid, is_ok, data) => {
                        self.out_send(ReceiveMessage::StableResult(
                            self.remote_peer_id(),
                            is_ok,
                            data,
                        ))
                        .await?;
                        if tid != 0 {
                            self.send_core_data(CoreData::Delivery(
                                DeliveryType::StableResult,
                                tid,
                            ))
                            .await?;
                        }

                        if !self.is_stable && is_ok {
                            // upgrade to stable connection.
                            info!("Upgrade to stable connection controller");
                            return Ok(true);
                        }
                    }
                }
            }
        }

        Ok(false)
    }

    async fn listen_outside(&self) -> Result<()> {
        loop {
            match self.session_receiver.recv().await {
                Ok(SessionMessage::Data(tid, data)) => {
                    debug!(
                        "Got outside data {:?} to remote: {:?}",
                        data.len(),
                        self.remote_peer.id()
                    );

                    self.send_core_data(CoreData::Data(tid, data)).await?;
                }
                Ok(SessionMessage::StableConnect(tid, data)) => {
                    debug!(
                        "Session recv stable connect to: {:?}",
                        self.remote_peer.id()
                    );

                    self.send_core_data(CoreData::StableConnect(tid, data))
                        .await?;
                }
                Ok(SessionMessage::StableResult(tid, is_ok, is_force, data)) => {
                    debug!(
                        "Session recv stable result to: {:?}, {}",
                        self.remote_peer.id(),
                        is_ok
                    );

                    self.send_core_data(CoreData::StableResult(tid, is_ok, data))
                        .await?;

                    if !self.is_stable && is_ok {
                        // upgrade session to stable.
                        info!("Upgrade to stable connection controller");
                        return Ok(());
                    }

                    if is_force {
                        break;
                    }
                }
                Ok(SessionMessage::RelayData(from, to, data)) => {
                    info!(
                        "Got relay data need send to: {:?}, my: {:?}",
                        to,
                        self.remote_peer.id()
                    );
                    if &to == self.remote_peer.id() {
                        // cannot send it.
                        self.failure_send(data).await?;
                        continue;
                    }

                    if self.is_direct() {
                        self.direct_send(EndpointMessage::RelayData(from, to, data))
                            .await?;
                    } else {
                        if let Some((_, stream_sender, _)) =
                            self.peer_list.read().await.dht_get(&to)
                        {
                            let _ = stream_sender
                                .send(EndpointMessage::RelayData(from, to, data))
                                .await;
                        }
                    }
                }
                Ok(SessionMessage::RelayConnect(from_peer, to)) => {
                    if &to == self.remote_peer.id() {
                        // cannot send it.
                        continue;
                    }

                    if self.is_direct() {
                        self.direct_send(EndpointMessage::RelayConnect(from_peer, to))
                            .await?;
                    } else {
                        if let Some((_, stream_sender, _)) =
                            self.peer_list.read().await.dht_get(&to)
                        {
                            let _ = stream_sender
                                .send(EndpointMessage::RelayConnect(from_peer, to))
                                .await;
                        }
                    }
                }
                Ok(SessionMessage::RelayResult(..)) => {
                    error!("Only happen once");
                }
                Ok(SessionMessage::RelayClose(peer_id)) => {
                    // if peer_id == self.endpoints[0] {
                    //     TODO changed to other session.
                    // }

                    let mut guard = self.relay_sessions.lock().await;
                    let _ = guard.remove(&peer_id);
                    drop(guard);
                }
                Ok(SessionMessage::Close) => {
                    debug!("Got outside close it");
                    break;
                }
                Err(_e) => break,
            }
        }

        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "outside recever over",
        ))
    }

    async fn listen_endpoint(&self, stream_receiver: &Receiver<EndpointMessage>) -> Result<()> {
        loop {
            match stream_receiver.recv().await {
                Ok(EndpointMessage::Close) => {
                    break;
                }
                Ok(EndpointMessage::Handshake(_)) => {
                    error!("endpoint handshake only happen once.");
                }
                Ok(EndpointMessage::DHT(DHT(peers))) => {
                    if peers.len() > 0 {
                        let remote_pk = self.global.remote_pk();
                        let sender = &self.global.transport_sender;

                        for p in peers {
                            if p.id() != &self.my_peer_id
                                && !self.peer_list.read().await.contains(p.id())
                            {
                                //if let Some(sender) = self.global.transports.read().await.get(p.transport()) {}
                                let _ = sender
                                    .send(TransportSendMessage::Connect(
                                        *p.addr(),
                                        remote_pk.clone(),
                                    ))
                                    .await;
                            }
                        }
                    }
                }
                Ok(EndpointMessage::Hole(_hole)) => {
                    todo!();
                }
                Ok(EndpointMessage::HoleConnect) => {
                    todo!();
                }
                Ok(EndpointMessage::Data(e_data)) => {
                    if self.handle_core_data(e_data).await? {
                        return Ok(());
                    }
                }
                Ok(EndpointMessage::RelayData(from, to, data)) => {
                    debug!("Endpoint recv relay data to: {:?}", to);
                    if to == self.my_peer_id {
                        if &from == self.remote_peer.id() {
                            info!("Relay Data is send here.");
                            if self.handle_core_data(data).await? {
                                return Ok(());
                            }
                        } else {
                            info!("Relay data is send to my peer's session.");
                            if let Some(stream_sender) =
                                self.peer_list.read().await.get_endpoint_with_tmp(&from)
                            {
                                let _ = stream_sender
                                    .send(EndpointMessage::RelayData(from, to, data))
                                    .await;
                            } else {
                                info!("Relay data session not found");
                                if self.is_recv_data {
                                    // only happen permissionless
                                    self.out_send(ReceiveMessage::Data(from, data)).await?;
                                }
                            }
                        }
                    } else {
                        if self.is_relay_data {
                            if let Some((sender, _, _)) = self.peer_list.read().await.get(&to) {
                                let _ =
                                    sender.send(SessionMessage::RelayData(from, to, data)).await;
                            }
                        }
                    }
                }
                Ok(EndpointMessage::RelayConnect(from_peer, to)) => {
                    if to == self.my_peer_id {
                        let remote_peer_id = from_peer.id().clone();
                        if remote_peer_id == self.my_peer_id {
                            continue;
                        }

                        if let Some(sender) =
                            self.peer_list.read().await.get_tmp_stable(&remote_peer_id)
                        {
                            // this is relay connect sender.
                            let _ = sender
                                .send(SessionMessage::RelayResult(
                                    from_peer,
                                    self.session_sender.clone(),
                                ))
                                .await;
                            continue;
                        }

                        // this is relay connect receiver.
                        let RemotePublic(remote_key, remote_peer) = from_peer;

                        let (new_stream_sender, new_stream_receiver) = new_endpoint_channel(); // session's use.
                        let (new_session_sender, new_session_receiver) = new_session_channel(); // server's use.

                        self.peer_list.write().await.add_tmp_stable(
                            remote_peer_id,
                            new_session_sender.clone(),
                            new_stream_sender.clone(),
                        );

                        let new_session_key: SessionKey = self.global.key.session_key(&remote_key);
                        let new_session = Session::new(
                            self.my_peer_id.clone(),
                            remote_peer,
                            new_stream_sender,
                            new_session_sender,
                            new_session_receiver,
                            ConnectType::Relay(self.session_sender.clone()),
                            new_session_key,
                            self.global.clone(),
                            self.peer_list.clone(),
                            false, // default is not recv data.
                            self.is_relay_data,
                            false,
                        );

                        // if use session_run directly, it will cycle error in rust check.
                        session_tmp(new_session, new_stream_receiver);

                        self.direct_send(EndpointMessage::RelayConnect(
                            self.global.remote_pk(),
                            remote_peer_id,
                        ))
                        .await?;
                    } else {
                        if self.is_relay_data {
                            if let Some((sender, _, _)) = self.peer_list.read().await.get(&to) {
                                let _ = sender
                                    .send(SessionMessage::RelayConnect(from_peer, to))
                                    .await;
                            }
                        }
                    }
                }
                Err(_) => break,
            }
        }

        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "transport recever over",
        ))
    }

    async fn heartbeat(&self) -> Result<()> {
        //let mut hearts = vec![];
        loop {
            // 5s to ping/pong check.
            let _ = smol::Timer::after(std::time::Duration::from_secs(5)).await;
            let mut guard = self.heartbeat.lock().await;

            if *guard > 5 {
                // heartbeat lost 30s. closed it.
                drop(guard);
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "transport recever over",
                ));
            }

            *guard += 1;
            drop(guard);

            self.send_core_data(CoreData::Ping).await?;
        }
    }

    async fn robust(&self) -> Result<()> {
        loop {
            let _ = smol::Timer::after(std::time::Duration::from_secs(10)).await;
            // 10s to do something.
            // 30s timer out when lost connection, and cannot build a new one.
            debug!("10s timer to do robust check, check all connections is connected.");
        }
        //Ok(())
    }
}

/// core data transfer and encrypted.
pub(crate) enum CoreData {
    Ping,
    Pong,
    Key(Vec<u8>),
    Data(u64, Vec<u8>),
    Delivery(DeliveryType, u64),
    StableConnect(u64, Vec<u8>),
    StableResult(u64, bool, Vec<u8>),
}

impl CoreData {
    pub fn to_bytes(self) -> Vec<u8> {
        let mut bytes = vec![0u8];
        match self {
            CoreData::Ping => {
                bytes[0] = 1u8;
            }
            CoreData::Pong => {
                bytes[0] = 2u8;
            }
            CoreData::Key(mut data) => {
                bytes[0] = 3u8;
                bytes.append(&mut data);
            }
            CoreData::Data(tid, mut data) => {
                bytes[0] = 4u8;
                bytes.extend(&tid.to_le_bytes()[..]);
                bytes.append(&mut data);
            }
            CoreData::Delivery(t, tid) => {
                bytes[0] = 5u8;
                let b = match t {
                    DeliveryType::Data => 0u8,
                    DeliveryType::StableConnect => 1u8,
                    DeliveryType::StableResult => 2u8,
                };
                bytes.push(b);
                bytes.extend(&tid.to_le_bytes()[..]);
            }
            CoreData::StableConnect(tid, mut data) => {
                bytes[0] = 6u8;
                bytes.extend(&tid.to_le_bytes()[..]);
                bytes.append(&mut data);
            }
            CoreData::StableResult(tid, is_ok, mut data) => {
                bytes[0] = 7u8;
                bytes.extend(&tid.to_le_bytes()[..]);
                bytes.push(if is_ok { 1u8 } else { 0u8 });
                bytes.append(&mut data);
            }
        }

        bytes
    }

    fn from_bytes(mut bytes: Vec<u8>) -> std::result::Result<Self, ()> {
        if bytes.len() < 1 {
            return Err(());
        }

        let t: Vec<u8> = bytes.drain(0..1).collect();
        match t[0] {
            1u8 => Ok(CoreData::Ping),
            2u8 => Ok(CoreData::Pong),
            3u8 => Ok(CoreData::Key(bytes)),
            4u8 => {
                if bytes.len() < 8 {
                    return Err(());
                }
                let mut tid_bytes = [0u8; 8];
                tid_bytes.copy_from_slice(bytes.drain(0..8).as_slice());
                let tid = u64::from_le_bytes(tid_bytes);
                Ok(CoreData::Data(tid, bytes))
            }
            5u8 => {
                if bytes.len() < 9 {
                    return Err(());
                }
                let t = match bytes.drain(0..1).as_slice()[0] {
                    0u8 => DeliveryType::Data,
                    1u8 => DeliveryType::StableConnect,
                    2u8 => DeliveryType::StableResult,
                    _ => return Err(()),
                };
                let mut tid_bytes = [0u8; 8];
                tid_bytes.copy_from_slice(bytes.drain(0..8).as_slice());
                let tid = u64::from_le_bytes(tid_bytes);
                Ok(CoreData::Delivery(t, tid))
            }
            6u8 => {
                if bytes.len() < 8 {
                    return Err(());
                }
                let mut tid_bytes = [0u8; 8];
                tid_bytes.copy_from_slice(bytes.drain(0..8).as_slice());
                let tid = u64::from_le_bytes(tid_bytes);
                Ok(CoreData::StableConnect(tid, bytes))
            }
            7u8 => {
                if bytes.len() < 9 {
                    return Err(());
                }
                let mut tid_bytes = [0u8; 8];
                tid_bytes.copy_from_slice(bytes.drain(0..8).as_slice());
                let tid = u64::from_le_bytes(tid_bytes);
                let is_ok = bytes.drain(0..1).as_slice()[0] == 1u8;
                Ok(CoreData::StableResult(tid, is_ok, bytes))
            }
            _ => Err(()),
        }
    }
}
