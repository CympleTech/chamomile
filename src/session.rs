use std::collections::HashMap;
use std::sync::Arc;
use tokio::{
    io::Result,
    select,
    sync::mpsc::{self, Receiver, Sender},
};

use chamomile_types::{
    delivery_split,
    message::{DeliveryType, ReceiveMessage},
    types::new_io_error,
    Peer, PeerId,
};

use crate::buffer::BufferKey;
use crate::global::Global;
use crate::hole_punching::{nat, DHT};
use crate::kad::KadValue;
use crate::session_key::SessionKey;
use crate::transports::{
    new_endpoint_channel, EndpointMessage, RemotePublic, TransportSendMessage,
};

/// To solve the tokio async cycle.
fn own_spawn(p: Peer, global: Arc<Global>) {
    tokio::spawn(async move {
        // add to stable buffer.
        let mut buffer_lock = global.buffer.write().await;
        if buffer_lock.add_connect(BufferKey::Peer(p.assist), 0, vec![]) {
            debug!("Outside: StableConnect is processing, save to buffer.");
        }
        drop(buffer_lock);

        let _ = direct_stable(0, vec![], p, global, true, true).await;
    });
}

/// direct start stable connection, if had IP.
pub(crate) async fn direct_stable(
    tid: u64,
    delivery: Vec<u8>,
    to: Peer,
    global: Arc<Global>,
    is_recv_data: bool,
    is_own: bool,
) -> Result<()> {
    debug!("Session want to connect directly.");
    let (endpoint_sender, endpoint_receiver) = new_endpoint_channel(); // transpot's use.
    let (stream_sender, mut stream_receiver) = new_endpoint_channel(); // session's use.
    let (mut session_key, remote_pk) = global.generate_remote();

    let bufferkey = if to.effective_id() {
        BufferKey::Peer(to.id)
    } else {
        BufferKey::Addr(to.socket)
    };

    // 1. send stable connect.
    global
        .trans_send(
            &to.transport,
            TransportSendMessage::StableConnect(
                stream_sender.clone(),
                endpoint_receiver,
                to.socket,
                remote_pk,
            ),
        )
        .await?;

    // 2. waiting remote send remote info.
    if let Some(EndpointMessage::Handshake(RemotePublic(remote_peer, dh_key))) =
        stream_receiver.recv().await
    {
        // 3.1.1 if ok connected. keep it and update to stable.
        let remote_id = remote_peer.id;
        if to.effective_id() && remote_id != to.id {
            warn!("CHAMOMILE: STABLE CONNECT FAILURE UNKNOWN PEER.");
            let _ = endpoint_sender.send(EndpointMessage::Close).await;
            return Err(new_io_error("session stable unknown peer."));
        }

        let mut is_own = false;
        if &remote_id == global.peer_id() {
            if &remote_peer.assist == global.assist_id() {
                if tid != 0 {
                    global
                        .out_send(ReceiveMessage::Delivery(
                            DeliveryType::StableConnect,
                            tid,
                            false,
                            delivery,
                        ))
                        .await?;
                }
                warn!("CHAMOMILE: STABLE CONNECT NERVER TO SELF.");
                let _ = endpoint_sender.send(EndpointMessage::Close).await;
                return Err(new_io_error("session stable self failure."));
            }
            is_own = true;
        }
        let toid = if is_own {
            remote_peer.assist
        } else {
            remote_peer.id
        };

        // 3.1.2 check & update session key.
        if !session_key.complete(&remote_id, dh_key) {
            global.buffer.write().await.remove_connect(bufferkey);
            return Err(new_io_error("session stable key failure."));
        }

        let remote_peer = nat(to.socket, remote_peer);
        let (session_sender, session_receiver) = new_session_channel(); // server's use.

        // 3.1.3 save to tmp buffer.
        let buffers = global
            .add_tmp(
                toid,
                bufferkey,
                KadValue(session_sender.clone(), stream_sender, remote_peer),
                true,
            )
            .await;

        let mut session = Session::new(
            remote_peer,
            session_sender,
            stream_receiver,
            ConnectType::Direct(endpoint_sender),
            session_key,
            global,
            is_recv_data,
            is_own,
        );

        // 3.1.4 send all connect info to remote.
        for buffer in buffers {
            session
                .send_core_data(CoreData::StableConnect(buffer.0, buffer.1))
                .await?;
        }

        // 3.1.5 upgrade to stable.
        if !session.is_stable {
            session.upgrade().await?;
        }

        // 3.1.6 session listen.
        session.listen(session_receiver).await
    } else {
        drop(stream_sender);
        drop(stream_receiver);
        drop(endpoint_sender);

        // 3.2.1 try start relay stable.
        let toid = if is_own { &to.assist } else { &to.id };
        let ss = if let Some((s, _, _)) = global.peer_list.read().await.get(toid) {
            Some(s.clone())
        } else {
            None
        };

        if let Some(ss) = ss {
            relay_stable(tid, delivery, to, ss, global, is_recv_data, is_own).await
        } else {
            if tid != 0 {
                global
                    .out_send(ReceiveMessage::Delivery(
                        DeliveryType::StableConnect,
                        tid,
                        false,
                        delivery,
                    ))
                    .await?;
            }
            let key = if to.effective_id() {
                BufferKey::Peer(*toid)
            } else {
                BufferKey::Addr(to.socket)
            };
            global.buffer.write().await.remove_connect(key);
            Err(new_io_error("no closest peer."))
        }
    }
}

pub(crate) async fn relay_stable(
    tid: u64,
    delivery: Vec<u8>,
    to: Peer,
    relay_sender: Sender<SessionMessage>,
    global: Arc<Global>,
    is_recv_data: bool,
    is_own: bool,
) -> Result<()> {
    debug!("Session want to connect relay.");

    // 1. try relay connect. (timeout).
    // 2. send stable connect.
    // 3. if stable connected, keep it.

    let (stream_sender, stream_receiver) = new_endpoint_channel(); // session's use.
    let (session_sender, mut session_receiver) = new_session_channel(); // server's use.
    let (mut session_key, remote_pk) = global.generate_remote();
    let toid = if is_own { to.assist } else { to.id };

    let (connects, results) = global
        .add_all_tmp(
            toid,
            KadValue(session_sender.clone(), stream_sender, Peer::default()),
            false,
        )
        .await;

    relay_sender
        .send(SessionMessage::RelayConnect(remote_pk, toid))
        .await
        .map_err(|_e| new_io_error("Session missing"))?;
    drop(relay_sender);

    let msg = select! {
        v = session_receiver
            .recv() => v,
        v = async {
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            None
        } => v
    };

    if let Some(SessionMessage::RelayResult(remote, recv_ss)) = msg {
        let RemotePublic(remote_peer, dh_key) = remote;

        let remote_id = remote_peer.id;
        if remote_id != to.id {
            warn!("CHAMOMILE: STABLE CONNECT FAILURE UNKNOWN PEER.");
            global.buffer.write().await.remove_tmp(&to.id);
            return Err(new_io_error("session stable unknown peer."));
        }

        let mut is_own = false;
        if &remote_id == global.peer_id() {
            if &remote_peer.assist == global.assist_id() {
                warn!("CHAMOMILE: STABLE CONNECT NERVER TO SELF.");
                global.buffer.write().await.remove_tmp(&to.id);
                if tid != 0 {
                    global
                        .out_send(ReceiveMessage::Delivery(
                            DeliveryType::StableConnect,
                            tid,
                            false,
                            delivery,
                        ))
                        .await?;
                }
                return Err(new_io_error("session stable self failure."));
            }
            is_own = true;
        }
        let toid = if is_own { to.assist } else { to.id };

        if !session_key.complete(&remote_id, dh_key) {
            global.buffer.write().await.remove_tmp(&toid);
            return Err(new_io_error("session stable key failure."));
        }

        global.buffer.write().await.update_peer(&toid, remote_peer);
        let mut session = Session::new(
            remote_peer,
            session_sender,
            stream_receiver,
            ConnectType::Relay(recv_ss),
            session_key,
            global,
            is_recv_data,
            is_own,
        );

        for buffer in connects {
            session
                .send_core_data(CoreData::StableConnect(buffer.0, buffer.1))
                .await?;
        }

        for buffer in results {
            session
                .send_core_data(CoreData::ResultConnect(buffer.0, buffer.1))
                .await?;
        }

        if !session.is_stable {
            session.upgrade().await?;
        }

        session.listen(session_receiver).await
    } else {
        debug!("Session cannot connect relay.");
        if tid != 0 {
            global
                .out_send(ReceiveMessage::Delivery(
                    DeliveryType::StableConnect,
                    tid,
                    false,
                    delivery,
                ))
                .await?;
        }
        global.buffer.write().await.remove_tmp(&toid);
        debug!("Session clear stable buffer.");
        Err(new_io_error("session relay reach faiure."))
    }
}

pub(crate) fn session_spawn(mut session: Session, session_receiver: Receiver<SessionMessage>) {
    tokio::spawn(async move { session.listen(session_receiver).await });
}

pub(crate) enum ConnectType {
    Direct(Sender<EndpointMessage>),
    Relay(Sender<SessionMessage>),
}

pub(crate) struct Session {
    pub remote_peer: Peer,
    pub session_sender: Sender<SessionMessage>,
    pub stream_receiver: Receiver<EndpointMessage>,
    pub endpoint: ConnectType,
    pub session_key: SessionKey,
    pub global: Arc<Global>,
    pub is_recv_data: bool,
    pub is_stable: bool,
    pub is_own: bool,
    pub heartbeat: u32,
    pub relay_sessions: HashMap<PeerId, Sender<SessionMessage>>,
}

enum FutureResult {
    Out(SessionMessage),
    Endpoint(EndpointMessage),
    HeartBeat,
    Robust,
}

impl Session {
    pub fn new(
        remote_peer: Peer,
        session_sender: Sender<SessionMessage>,
        stream_receiver: Receiver<EndpointMessage>,
        endpoint: ConnectType,
        session_key: SessionKey,
        global: Arc<Global>,
        is_recv_data: bool,
        is_own: bool,
    ) -> Session {
        Session {
            remote_peer,
            session_sender,
            stream_receiver,
            endpoint,
            session_key,
            global,
            is_recv_data,
            is_own,
            is_stable: false,
            heartbeat: 0,
            relay_sessions: HashMap::new(),
        }
    }

    fn is_to_me(&self, to: &PeerId) -> bool {
        self.global.peer_id() == to || self.global.assist_id() == to
    }

    fn is_from_remote(&self, from: &PeerId) -> bool {
        &self.remote_peer.id == from || &self.remote_peer.assist == from
    }

    fn is_own_remote(&self, peer: &Peer) -> bool {
        self.global.peer_id() == &peer.id && self.global.assist_id() != &peer.assist
    }

    async fn is_new_remote(&self, p: &Peer) -> bool {
        self.global.peer_id() != &p.id && !self.global.peer_list.read().await.contains(&p.id)
    }

    fn remote_assist(&self) -> Peer {
        let mut new_p = self.remote_peer.clone();
        new_p.id = new_p.assist;
        new_p
    }

    async fn close(&mut self, is_leave: bool) -> Result<()> {
        let peer_id = &self.remote_peer.id;
        let assist_id = &self.remote_peer.assist;

        if self.is_stable {
            if self.is_own {
                let _ = self
                    .out_send(ReceiveMessage::OwnLeave(self.remote_assist()))
                    .await;
            } else {
                let _ = self
                    .out_send(ReceiveMessage::StableLeave(self.remote_peer))
                    .await;
            }

            if !self.is_direct() {
                let _ = self
                    .relay_send(SessionMessage::RelayClose(*self.global.peer_id()))
                    .await;
            }

            if is_leave {
                self.global.peer_list.write().await.stable_leave(peer_id);
                let _ = self.direct_send(EndpointMessage::Close).await;
            } else if self.is_direct() {
                self.global.stable_to_dht(peer_id).await?;
            }
        } else if self.is_direct() {
            if is_leave {
                let mut buffer_lock = self.global.buffer.write().await;
                buffer_lock.remove_tmp(peer_id);
                buffer_lock.remove_tmp(assist_id);
                drop(buffer_lock);
                let mut peers_lock = self.global.peer_list.write().await;
                peers_lock.remove_peer(peer_id, assist_id);
                drop(peers_lock);
            } else {
                self.global.tmp_to_dht(peer_id).await?;
            }
        } else {
            self.global.buffer.write().await.remove_tmp(peer_id);
        }

        Err(new_io_error("close session"))
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
                    CoreData::Unstable => {}
                    CoreData::Delivery(..) => {}
                    CoreData::Data(tid, data) => {
                        if tid != 0 {
                            self.out_send(ReceiveMessage::Delivery(
                                DeliveryType::Data,
                                tid,
                                false,
                                delivery_split!(data, self.global.delivery_length),
                            ))
                            .await?;
                        }
                    }
                    CoreData::StableConnect(tid, data) => {
                        if tid != 0 {
                            self.out_send(ReceiveMessage::Delivery(
                                DeliveryType::StableConnect,
                                tid,
                                false,
                                delivery_split!(data, self.global.delivery_length),
                            ))
                            .await?;
                        }
                    }
                    CoreData::StableResult(tid, _, data) => {
                        if tid != 0 {
                            self.out_send(ReceiveMessage::Delivery(
                                DeliveryType::StableResult,
                                tid,
                                false,
                                delivery_split!(data, self.global.delivery_length),
                            ))
                            .await?;
                        }
                    }
                    CoreData::ResultConnect(tid, data) => {
                        if tid != 0 {
                            self.out_send(ReceiveMessage::Delivery(
                                DeliveryType::StableResult,
                                tid,
                                false,
                                delivery_split!(data, self.global.delivery_length),
                            ))
                            .await?;
                        }
                    }
                }
            }
        }
        Ok(())
    }

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
            let (from, to) = if self.is_own {
                (*self.global.assist_id(), self.remote_peer.assist)
            } else {
                (*self.global.peer_id(), self.remote_peer.id)
            };
            self.relay_send(SessionMessage::RelayData(from, to, e_data))
                .await
        }
    }

    async fn handle_core_data(&mut self, e_data: Vec<u8>) -> Result<()> {
        if let Ok(bytes) = self.session_key.decrypt(e_data) {
            if let Ok(msg) = CoreData::from_bytes(bytes) {
                match msg {
                    CoreData::Ping => {
                        self.send_core_data(CoreData::Pong).await?;
                    }
                    CoreData::Pong => {
                        self.heartbeat = 0;
                    }
                    CoreData::Data(tid, p_data) => {
                        if self.is_recv_data {
                            let delivery_data =
                                delivery_split!(p_data, self.global.delivery_length);
                            if self.is_own {
                                self.out_send(ReceiveMessage::OwnEvent(
                                    self.remote_peer.assist,
                                    p_data,
                                ))
                                .await?;
                            } else {
                                self.out_send(ReceiveMessage::Data(self.remote_peer.id, p_data))
                                    .await?;
                            }

                            if tid != 0 {
                                self.send_core_data(CoreData::Delivery(
                                    DeliveryType::Data,
                                    tid,
                                    delivery_data,
                                ))
                                .await?;
                            }
                        }
                    }
                    CoreData::Delivery(t, tid, data) => {
                        if tid != 0 {
                            match t {
                                DeliveryType::Data => {
                                    if self.is_recv_data {
                                        self.out_send(ReceiveMessage::Delivery(t, tid, true, data))
                                            .await?;
                                    }
                                }
                                _ => {
                                    self.out_send(ReceiveMessage::Delivery(t, tid, true, data))
                                        .await?;
                                }
                            }
                        }
                    }
                    CoreData::StableConnect(tid, data) => {
                        let delivery_data = delivery_split!(data, self.global.delivery_length);
                        if self.is_own {
                            self.out_send(ReceiveMessage::OwnConnect(self.remote_assist()))
                                .await?;
                            self.send_core_data(CoreData::StableResult(0, true, data))
                                .await?;
                            self.upgrade().await?;
                        } else {
                            self.out_send(ReceiveMessage::StableConnect(self.remote_peer, data))
                                .await?;
                        }

                        if tid != 0 {
                            self.send_core_data(CoreData::Delivery(
                                DeliveryType::StableConnect,
                                tid,
                                delivery_data,
                            ))
                            .await?;
                        }
                    }
                    CoreData::StableResult(tid, is_ok, data) => {
                        let delivery_data = delivery_split!(data, self.global.delivery_length);
                        if self.is_own {
                            self.out_send(ReceiveMessage::OwnConnect(self.remote_assist()))
                                .await?;
                        } else {
                            self.out_send(ReceiveMessage::StableResult(
                                self.remote_peer,
                                is_ok,
                                data,
                            ))
                            .await?;
                        }
                        if tid != 0 {
                            self.send_core_data(CoreData::Delivery(
                                DeliveryType::StableResult,
                                tid,
                                delivery_data,
                            ))
                            .await?;
                        }
                    }
                    CoreData::ResultConnect(tid, data) => {
                        let delivery_data = delivery_split!(data, self.global.delivery_length);
                        if self.is_own {
                            self.out_send(ReceiveMessage::OwnConnect(self.remote_assist()))
                                .await?;
                            self.send_core_data(CoreData::StableResult(0, true, data))
                                .await?;
                            self.upgrade().await?;
                        } else {
                            self.out_send(ReceiveMessage::ResultConnect(self.remote_peer, data))
                                .await?;
                        }

                        if tid != 0 {
                            self.send_core_data(CoreData::Delivery(
                                DeliveryType::StableResult,
                                tid,
                                delivery_data,
                            ))
                            .await?;
                        }
                    }
                    CoreData::Unstable => self.close(false).await?,
                }
            }
        } else {
            warn!("Session Key decrypt failure!");
        }

        Ok(())
    }

    async fn upgrade(&mut self) -> Result<()> {
        debug!("UPGRADE TO STABLE CONNECTION");
        self.is_stable = true;
        self.is_recv_data = true;
        if self.is_own {
            self.global.upgrade_own(&self.remote_peer.assist).await;
            Ok(())
        } else {
            self.global
                .upgrade(&self.remote_peer.id, &self.remote_peer.assist)
                .await
        }
    }

    async fn forever(&mut self, mut session_receiver: Receiver<SessionMessage>) -> Result<()> {
        loop {
            let res = select! {
                v = async {
                    session_receiver
                        .recv()
                        .await
                        .map(|msg| FutureResult::Out(msg))
                } => v,
                v = async {
                    self.stream_receiver
                        .recv()
                        .await
                        .map(|msg| FutureResult::Endpoint(msg))
                } => v,

                v = async {
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    Some(FutureResult::HeartBeat)
                } => v,
                v = async {
                    // 60s to check all connection channels is ok.
                    tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                    Some(FutureResult::Robust)
                } => v,
            };
            match res {
                Some(FutureResult::Out(msg)) => {
                    self.handle_outside(msg).await?;
                }
                Some(FutureResult::Endpoint(msg)) => {
                    self.handle_endpoint(msg).await?;
                }
                Some(FutureResult::HeartBeat) => {
                    self.handle_heartbeat().await?;
                }
                Some(FutureResult::Robust) => {
                    self.handle_robust().await?;
                }
                None => break,
            }
        }
        Ok(())
    }

    pub async fn listen(&mut self, session_receiver: Receiver<SessionMessage>) -> Result<()> {
        debug!("Session running: {}.", self.remote_peer.id.short_show());
        let _ = self.forever(session_receiver).await;
        debug!("Session broke: {}.", self.remote_peer.id.short_show());
        self.close(true).await
    }

    async fn handle_outside(&mut self, msg: SessionMessage) -> Result<()> {
        match msg {
            SessionMessage::Data(tid, data) => {
                self.send_core_data(CoreData::Data(tid, data)).await?;
            }
            SessionMessage::StableConnect(tid, data) => {
                debug!(
                    "SessionMessage StableConnect to: {:?}",
                    self.remote_peer.id.short_show()
                );

                self.send_core_data(CoreData::StableConnect(tid, data))
                    .await?;

                if !self.is_stable {
                    self.upgrade().await?;
                }
            }
            SessionMessage::StableResult(tid, is_ok, is_force, data) => {
                debug!(
                    "SessionMessage StableResult to: {:?}",
                    self.remote_peer.id.short_show()
                );

                self.send_core_data(CoreData::StableResult(tid, is_ok, data))
                    .await?;

                if !self.is_stable && is_ok {
                    self.upgrade().await?;
                }

                if !self.is_stable && !is_ok {
                    self.close(false).await?;
                }

                if is_force {
                    return Err(new_io_error("force close"));
                }
            }
            SessionMessage::RelayData(from, to, data) => {
                debug!("SessionMessage RelayData to: {:?}", to.short_show());
                if !self.is_own && to == self.remote_peer.id && &from == self.global.peer_id() {
                    warn!("CHAMOMILE: RELAY TO SELF, MUST DIRECTLY.");
                    self.failure_send(data).await?;
                    return Ok(());
                }

                if self.is_direct() {
                    debug!("SessionMessage RelayData directly send");
                    self.direct_send(EndpointMessage::RelayData(from, to, data))
                        .await?;
                } else {
                    debug!("SessionMessage RelayData need relay again");
                    if let Some((ss, _, _)) = self.global.peer_list.read().await.dht_get(&to) {
                        let _ = ss.send(SessionMessage::RelayData(from, to, data)).await;
                    } else {
                        warn!("CHAMOMILE: CANNOT REACH NETWORK.");
                    }
                }
            }
            SessionMessage::RelayConnect(from_peer, to) => {
                debug!("SessionMessage RelayConnect to: {:?}", to.short_show());
                if !self.is_own
                    && to == self.remote_peer.id
                    && from_peer.id() == self.global.peer_id()
                {
                    warn!("CHAMOMILE: RELAY TO SELF, MUST DIRECTLY.");
                    return Ok(());
                }

                if self.is_direct() {
                    debug!(
                        "SessionMessage RelayConnect to directly {}",
                        self.remote_peer.assist.to_hex()
                    );
                    self.direct_send(EndpointMessage::RelayHandshake(from_peer, to))
                        .await?;
                } else {
                    debug!("SessionMessage RelayData need relay again");
                    if let Some((ss, _, _)) = self.global.peer_list.read().await.dht_get(&to) {
                        let _ = ss.send(SessionMessage::RelayConnect(from_peer, to)).await;
                    } else {
                        warn!("CHAMOMILE: CANNOT REACH NETWORK.");
                    }
                }
            }
            SessionMessage::RelayResult(..) => {
                warn!("CHAMOMILE SESSION DONOT HANDSHAKE TWICE.");
            }
            SessionMessage::RelayClose(peer_id) => {
                self.relay_sessions.remove(&peer_id);
            }
            SessionMessage::Close => {
                self.close(false).await?;
            }
            SessionMessage::DirectIncoming(
                remote_peer,
                _stream_sender,
                stream_receiver,
                endpoint_sender,
            ) => {
                // 1. close relay.
                let _ = self
                    .relay_send(SessionMessage::RelayClose(*self.global.peer_id()))
                    .await;
                // 2. update stream and info.
                self.stream_receiver = stream_receiver;
                self.endpoint = ConnectType::Direct(endpoint_sender);
                self.remote_peer = remote_peer;
                // 3. need use new session_key? no !.
            }
        }

        Ok(())
    }

    async fn handle_endpoint(&mut self, msg: EndpointMessage) -> Result<()> {
        match msg {
            EndpointMessage::Close => {
                return Err(new_io_error("close"));
            }
            EndpointMessage::Handshake(_) => {
                error!("endpoint handshake only happen once.");
            }
            EndpointMessage::DHT(DHT(peers)) => {
                if peers.len() > 0 {
                    for p in peers {
                        if self.is_own_remote(&p) {
                            let new_g = self.global.clone();
                            own_spawn(p, new_g);
                        } else if self.is_new_remote(&p).await {
                            let (session_key, remote_pk) = self.global.generate_remote();
                            let _ = self
                                .global
                                .trans_send(
                                    &p.transport,
                                    TransportSendMessage::Connect(p.socket, remote_pk, session_key),
                                )
                                .await;
                        }
                    }
                }
            }
            EndpointMessage::Hole(_hole) => {
                // TODO
            }
            EndpointMessage::HoleConnect => {
                // TODO
            }
            EndpointMessage::Data(e_data) => {
                self.handle_core_data(e_data).await?;
            }
            EndpointMessage::RelayData(from, to, data) => {
                debug!(
                    "Endpoint RelayData from: {}, to: {}",
                    from.to_hex(),
                    to.to_hex()
                );
                if self.is_to_me(&to) {
                    debug!("Got self RelayData");
                    if self.is_from_remote(&from) {
                        self.handle_core_data(data).await?;
                    } else {
                        if let Some(stream_sender) =
                            self.global.peer_list.read().await.get_stable_stream(&from)
                        {
                            debug!("RelayData is in STABLE.");
                            let _ = stream_sender.send(EndpointMessage::Data(data)).await;
                        } else if let Some(stream_sender) =
                            self.global.buffer.read().await.get_tmp_stream(&from)
                        {
                            debug!("RelayData is in TMP.");
                            let _ = stream_sender.send(EndpointMessage::Data(data)).await;
                        } else {
                            debug!("RelayData is MISSING.");
                            if self.is_recv_data {
                                // only happen permissionless
                                self.out_send(ReceiveMessage::Data(from, data)).await?;
                            }
                        }
                    }
                } else {
                    if self.global.is_relay_data {
                        if let Some(sender) = self
                            .global
                            .peer_list
                            .read()
                            .await
                            .next_closest(&to, &[self.remote_peer.id, self.remote_peer.assist])
                        {
                            let _ = sender.send(SessionMessage::RelayData(from, to, data)).await;
                        } else {
                            debug!("RelayData not found next closest!");
                        }
                    }
                }
            }
            EndpointMessage::RelayHandshake(from_peer, to) => {
                debug!(
                    "Relay Handshake to: {:?}, is me: {}",
                    to.short_show(),
                    self.is_to_me(&to),
                );
                if self.is_to_me(&to) {
                    let mut remote_peer_id = from_peer.id().clone();
                    let mut is_own = false;
                    if &remote_peer_id == self.global.peer_id() {
                        if from_peer.assist() == self.global.assist_id() {
                            warn!("CHAMOMILE: RELAY NERVER TO SELF.");
                            return Ok(());
                        }
                        remote_peer_id = from_peer.assist().clone();
                        is_own = true;
                    }

                    if let Some(sender) = self
                        .global
                        .buffer
                        .read()
                        .await
                        .get_tmp_session(&remote_peer_id)
                    {
                        debug!("Relay Result have got. send to session.");
                        // this is relay connect sender.
                        let _ = sender
                            .send(SessionMessage::RelayResult(
                                from_peer,
                                self.session_sender.clone(),
                            ))
                            .await;
                        return Ok(());
                    }

                    // this is relay connect receiver.
                    let RemotePublic(remote_peer, dh_key) = from_peer;

                    let result = self.global.complete_remote(&remote_peer.id, dh_key);
                    if result.is_none() {
                        return Ok(());
                    }
                    let (new_session_key, new_remote_pk) = result.unwrap(); // safe checked.

                    let (new_stream_sender, new_stream_receiver) = new_endpoint_channel(); // session's use.
                    let (new_session_sender, new_session_receiver) = new_session_channel(); // server's use.

                    self.global.buffer.write().await.add_tmp(
                        remote_peer_id,
                        KadValue(new_session_sender.clone(), new_stream_sender, remote_peer),
                        false,
                    );

                    let new_session = Session::new(
                        remote_peer,
                        new_session_sender,
                        new_stream_receiver,
                        ConnectType::Relay(self.session_sender.clone()),
                        new_session_key,
                        self.global.clone(),
                        is_own || false, // default is not recv data.
                        is_own,
                    );

                    // if use session_run directly, it will cycle error in rust check.
                    session_spawn(new_session, new_session_receiver);

                    self.direct_send(EndpointMessage::RelayHandshake(
                        new_remote_pk,
                        remote_peer_id,
                    ))
                    .await?;
                } else {
                    if self.global.is_relay_data {
                        if let Some(sender) = self
                            .global
                            .peer_list
                            .read()
                            .await
                            .next_closest(&to, &[self.remote_peer.id, self.remote_peer.assist])
                        {
                            let _ = sender
                                .send(SessionMessage::RelayConnect(from_peer, to))
                                .await;
                        } else {
                            debug!("RelayHandshake not found next closest!");
                        }
                    }
                }
            }
        }

        Ok(())
    }

    async fn handle_heartbeat(&mut self) -> Result<()> {
        if self.heartbeat > 3 {
            return Err(new_io_error("timeout"));
        }

        self.heartbeat += 1;
        self.send_core_data(CoreData::Ping).await
    }

    async fn handle_robust(&mut self) -> Result<()> {
        // 60s timer out when lost connection, and cannot build a new one.
        debug!("60s timer to do robust check, check all connections is connected.");

        Ok(())
    }
}

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
    /// Directly incoming.
    DirectIncoming(
        Peer,
        Sender<EndpointMessage>, // stream sender (endpoint -> session sender).
        Receiver<EndpointMessage>, // stream receiver (endpoint -> session receiver).
        Sender<EndpointMessage>, // endpoint sender (session -> endpointsender).
    ),
}

/// new a channel for send message to session.
pub(crate) fn new_session_channel() -> (Sender<SessionMessage>, Receiver<SessionMessage>) {
    mpsc::channel(1024)
}

/// core data transfer and encrypted.
pub(crate) enum CoreData {
    Ping,
    Pong,
    Data(u64, Vec<u8>),
    Delivery(DeliveryType, u64, Vec<u8>),
    StableConnect(u64, Vec<u8>),
    StableResult(u64, bool, Vec<u8>),
    ResultConnect(u64, Vec<u8>),
    Unstable,
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
            CoreData::Data(tid, mut data) => {
                bytes[0] = 3u8;
                bytes.extend(&tid.to_le_bytes()[..]);
                bytes.append(&mut data);
            }
            CoreData::Delivery(t, tid, data) => {
                bytes[0] = 4u8;
                let b = match t {
                    DeliveryType::Data => 0u8,
                    DeliveryType::StableConnect => 1u8,
                    DeliveryType::StableResult => 2u8,
                };
                bytes.push(b);
                bytes.extend(&tid.to_le_bytes()[..]);
                bytes.extend(data);
            }
            CoreData::StableConnect(tid, mut data) => {
                bytes[0] = 5u8;
                bytes.extend(&tid.to_le_bytes()[..]);
                bytes.append(&mut data);
            }
            CoreData::StableResult(tid, is_ok, mut data) => {
                bytes[0] = 6u8;
                bytes.extend(&tid.to_le_bytes()[..]);
                bytes.push(if is_ok { 1u8 } else { 0u8 });
                bytes.append(&mut data);
            }
            CoreData::ResultConnect(tid, mut data) => {
                bytes[0] = 7u8;
                bytes.extend(&tid.to_le_bytes()[..]);
                bytes.append(&mut data);
            }
            CoreData::Unstable => {
                bytes[0] = 8u8;
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
            3u8 => {
                if bytes.len() < 8 {
                    return Err(());
                }
                let mut tid_bytes = [0u8; 8];
                tid_bytes.copy_from_slice(bytes.drain(0..8).as_slice());
                let tid = u64::from_le_bytes(tid_bytes);
                Ok(CoreData::Data(tid, bytes))
            }
            4u8 => {
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
                Ok(CoreData::Delivery(t, tid, bytes))
            }
            5u8 => {
                if bytes.len() < 8 {
                    return Err(());
                }
                let mut tid_bytes = [0u8; 8];
                tid_bytes.copy_from_slice(bytes.drain(0..8).as_slice());
                let tid = u64::from_le_bytes(tid_bytes);
                Ok(CoreData::StableConnect(tid, bytes))
            }
            6u8 => {
                if bytes.len() < 9 {
                    return Err(());
                }
                let mut tid_bytes = [0u8; 8];
                tid_bytes.copy_from_slice(bytes.drain(0..8).as_slice());
                let tid = u64::from_le_bytes(tid_bytes);
                let is_ok = bytes.drain(0..1).as_slice()[0] == 1u8;
                Ok(CoreData::StableResult(tid, is_ok, bytes))
            }
            7u8 => {
                if bytes.len() < 8 {
                    return Err(());
                }
                let mut tid_bytes = [0u8; 8];
                tid_bytes.copy_from_slice(bytes.drain(0..8).as_slice());
                let tid = u64::from_le_bytes(tid_bytes);
                Ok(CoreData::ResultConnect(tid, bytes))
            }
            8u8 => Ok(CoreData::Unstable),
            _ => Err(()),
        }
    }
}
