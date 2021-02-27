use chamomile_types::{
    delivery_split,
    message::{DeliveryType, ReceiveMessage},
    types::{new_io_error, PeerId},
};
use smol::{
    channel::{self, Receiver, Sender},
    future,
    io::Result,
    prelude::*,
};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use crate::global::Global;
use crate::hole_punching::{nat, DHT};
use crate::keys::SessionKey;
use crate::peer::Peer;
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
    channel::unbounded()
}

pub(crate) async fn direct_stable(
    tid: u64,
    to: PeerId,
    data: Vec<u8>, // need as stable bytes to send.
    addr: SocketAddr,
    global: Arc<Global>,
    is_recv_data: bool,
) -> Result<()> {
    // 1. send stable connect.
    // 2. if stable connected, keep it.

    let (endpoint_sender, endpoint_receiver) = new_endpoint_channel(); // transpot's use.
    let (stream_sender, stream_receiver) = new_endpoint_channel(); // session's use.

    let (mut session_key, remote_pk) = global.generate_remote();

    global
        .trans_send(TransportSendMessage::StableConnect(
            stream_sender.clone(),
            endpoint_receiver,
            addr,
            remote_pk,
        ))
        .await?;

    if let Ok(EndpointMessage::Handshake(RemotePublic(remote_key, remote_peer, dh_key))) =
        stream_receiver.recv().await
    {
        // ok connected.
        let remote_peer_id = remote_key.peer_id();
        if &remote_peer_id == global.peer_id() {
            let _ = endpoint_sender.send(EndpointMessage::Close).await;
            if tid != 0 {
                global
                    .out_send(ReceiveMessage::Delivery(
                        DeliveryType::StableConnect,
                        tid,
                        false,
                        delivery_split!(data, global.delivery_length),
                    ))
                    .await?;
            }
            return Ok(());
        }

        if !session_key.complete(&remote_key.pk, dh_key) {
            return Ok(());
        }

        let remote_peer = nat(addr, remote_peer);
        let (session_sender, session_receiver) = new_session_channel(); // server's use.

        // save to tmp stable connection.
        global.peer_list.write().await.add_tmp_stable(
            remote_peer_id,
            session_sender.clone(),
            stream_sender.clone(),
        );

        let mut session = Session::new(
            remote_peer,
            session_sender,
            session_receiver,
            stream_sender,
            stream_receiver,
            ConnectType::Direct(endpoint_sender),
            session_key,
            global,
            is_recv_data,
            false,
        );

        session
            .send_core_data(CoreData::StableConnect(tid, data))
            .await?;

        session.listen().await
    } else {
        drop(stream_sender);
        drop(stream_receiver);
        drop(endpoint_sender);

        // try start relay stable.
        let ss = if let Some((s, _, _)) = global.peer_list.read().await.get(&to) {
            Some(s.clone())
        } else {
            None
        };

        if let Some(ss) = ss {
            relay_stable(tid, to, data, ss, global, is_recv_data).await
        } else {
            if tid != 0 {
                global
                    .out_send(ReceiveMessage::Delivery(
                        DeliveryType::StableConnect,
                        tid,
                        false,
                        delivery_split!(data, global.delivery_length),
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
    relay_sender: Sender<SessionMessage>,
    global: Arc<Global>,
    is_recv_data: bool,
) -> Result<()> {
    debug!("start stable connection.");

    // 1. try relay connect. (timeout).
    // 2. send stable connect.
    // 3. if stable connected, keep it.

    let (stream_sender, stream_receiver) = new_endpoint_channel(); // session's use.
    let (session_sender, session_receiver) = new_session_channel(); // server's use.
    let (mut session_key, remote_pk) = global.generate_remote();

    global.peer_list.write().await.add_tmp_stable(
        to,
        session_sender.clone(),
        stream_sender.clone(),
    );

    relay_sender
        .send(SessionMessage::RelayConnect(remote_pk, to))
        .await
        .map_err(|_e| new_io_error("Session missing"))?;
    drop(relay_sender);

    let msg = session_receiver
        .recv()
        .or(async {
            smol::Timer::after(std::time::Duration::from_secs(10)).await;
            Err(smol::channel::RecvError)
        })
        .await;

    if let Ok(SessionMessage::RelayResult(RemotePublic(remote_key, remote_peer, dh_key), recv_ss)) =
        msg
    {
        // ok connected.
        let remote_peer_id = remote_key.peer_id();
        if &remote_peer_id == global.peer_id() {
            global.peer_list.write().await.remove_tmp_stable(&to);
            if tid != 0 {
                global
                    .out_send(ReceiveMessage::Delivery(
                        DeliveryType::StableConnect,
                        tid,
                        false,
                        delivery_split!(data, global.delivery_length),
                    ))
                    .await?;
            }
            return Ok(());
        }

        if !session_key.complete(&remote_key.pk, dh_key) {
            return Ok(());
        }

        let mut session = Session::new(
            remote_peer,
            session_sender,
            session_receiver,
            stream_sender,
            stream_receiver,
            ConnectType::Relay(recv_ss),
            session_key,
            global,
            is_recv_data,
            false,
        );

        session
            .send_core_data(CoreData::StableConnect(tid, data))
            .await?;

        session.listen().await
    } else {
        // failure send outside.
        global.peer_list.write().await.remove_tmp_stable(&to);
        if tid != 0 {
            global
                .out_send(ReceiveMessage::Delivery(
                    DeliveryType::StableConnect,
                    tid,
                    false,
                    delivery_split!(data, global.delivery_length),
                ))
                .await?;
        }

        Ok(())
    }
}

pub(crate) fn session_spawn(mut session: Session) {
    smol::spawn(async move { session.listen().await }).detach();
}

pub(crate) enum ConnectType {
    Direct(Sender<EndpointMessage>),
    Relay(Sender<SessionMessage>),
}

pub(crate) struct Session {
    pub remote_peer: Peer,
    pub session_sender: Sender<SessionMessage>,
    pub session_receiver: Receiver<SessionMessage>,
    pub stream_sender: Sender<EndpointMessage>,
    pub stream_receiver: Receiver<EndpointMessage>,
    pub endpoint: ConnectType,
    pub session_key: SessionKey,
    pub global: Arc<Global>,
    pub is_recv_data: bool,
    pub is_stable: bool,
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
        session_receiver: Receiver<SessionMessage>,
        stream_sender: Sender<EndpointMessage>,
        stream_receiver: Receiver<EndpointMessage>,
        endpoint: ConnectType,
        session_key: SessionKey,
        global: Arc<Global>,
        is_recv_data: bool,
        is_stable: bool,
    ) -> Session {
        Session {
            remote_peer,
            session_sender,
            session_receiver,
            stream_sender,
            stream_receiver,
            endpoint,
            session_key,
            global,
            is_recv_data,
            is_stable,
            heartbeat: 0,
            relay_sessions: HashMap::new(),
        }
    }

    fn my_id(&self) -> &PeerId {
        self.global.peer_id()
    }

    fn remote_id(&self) -> &PeerId {
        self.remote_peer.id()
    }

    async fn close(&mut self, is_leave: bool) -> Result<()> {
        let peer_id = self.remote_id();

        if !self.is_direct() && self.is_stable {
            let _ = self.out_send(ReceiveMessage::StableLeave(*peer_id)).await;
            let _ = self
                .relay_send(SessionMessage::RelayClose(*self.my_id()))
                .await;

            if is_leave {
                self.global.peer_list.write().await.stable_leave(peer_id);
            } else {
                self.global.peer_list.write().await.stable_remove(peer_id);
            }
        } else if self.is_direct() && self.is_stable {
            let _ = self.out_send(ReceiveMessage::StableLeave(*peer_id)).await;

            if is_leave {
                self.global.peer_list.write().await.stable_leave(peer_id);
                let _ = self.direct_send(EndpointMessage::Close).await;
            } else {
                if self.global.peer_list.write().await.stable_remove(peer_id) {
                    self.send_core_data(CoreData::Unstable).await?;
                    self.is_stable = false;
                    return Ok(()); // keep no-stable
                } else {
                    let _ = self.direct_send(EndpointMessage::Close).await;
                }
            }
        } else {
            self.global.peer_list.write().await.peer_remove(peer_id);
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
                *self.my_id(),
                *self.remote_id(),
                e_data,
            ))
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
                            self.out_send(ReceiveMessage::Data(*self.remote_id(), p_data))
                                .await?;
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
                        self.out_send(ReceiveMessage::StableConnect(*self.remote_id(), data))
                            .await?;
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
                        self.out_send(ReceiveMessage::StableResult(*self.remote_id(), is_ok, data))
                            .await?;
                        if tid != 0 {
                            self.send_core_data(CoreData::Delivery(
                                DeliveryType::StableResult,
                                tid,
                                delivery_data,
                            ))
                            .await?;
                        }

                        if !self.is_stable {
                            // continue handle connections buffer.
                            let buffers = self
                                .global
                                .buffer
                                .write()
                                .await
                                .remove_stable(&self.remote_id());
                            if let Some(connections) = buffers {
                                for buffer in connections {
                                    self.send_core_data(CoreData::StableConnect(
                                        buffer.0, buffer.1,
                                    ))
                                    .await?;
                                }
                            }
                        }

                        if !self.is_stable && is_ok {
                            // upgrade to stable connection.
                            debug!("Upgrade to stable connection controller");
                            self.upgrade().await?;
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
        self.is_stable = true;
        self.is_recv_data = true;

        let mut peer_list_lock = self.global.peer_list.write().await;

        peer_list_lock.peer_remove(self.remote_id());
        peer_list_lock.remove_tmp_stable(self.remote_id());

        peer_list_lock.stable_add(
            *self.remote_id(),
            self.session_sender.clone(),
            self.stream_sender.clone(),
            self.remote_peer.clone(),
            self.is_direct(),
        );

        drop(peer_list_lock);

        Ok(())
    }

    async fn forever(&mut self) -> Result<()> {
        loop {
            match future::race(
                future::race(
                    async {
                        self.session_receiver
                            .recv()
                            .await
                            .map(|msg| FutureResult::Out(msg))
                    },
                    async {
                        self.stream_receiver
                            .recv()
                            .await
                            .map(|msg| FutureResult::Endpoint(msg))
                    },
                ),
                future::race(
                    async {
                        smol::Timer::after(std::time::Duration::from_secs(5)).await;
                        Ok(FutureResult::HeartBeat)
                    },
                    async {
                        // 60s to check all connection channels is ok.
                        smol::Timer::after(std::time::Duration::from_secs(60)).await;
                        Ok(FutureResult::Robust)
                    },
                ),
            )
            .await
            {
                Ok(FutureResult::Out(msg)) => {
                    self.handle_outside(msg).await?;
                }
                Ok(FutureResult::Endpoint(msg)) => {
                    self.handle_endpoint(msg).await?;
                }
                Ok(FutureResult::HeartBeat) => {
                    self.handle_heartbeat().await?;
                }
                Ok(FutureResult::Robust) => {
                    self.handle_robust().await?;
                }
                Err(_) => break,
            }
        }
        Ok(())
    }

    pub async fn listen(&mut self) -> Result<()> {
        let _ = self.forever().await;
        debug!("Session broke: {:?}", self.remote_id());
        self.close(true).await
    }

    async fn handle_outside(&mut self, msg: SessionMessage) -> Result<()> {
        match msg {
            SessionMessage::Data(tid, data) => {
                self.send_core_data(CoreData::Data(tid, data)).await?;
            }
            SessionMessage::StableConnect(tid, data) => {
                debug!("Session recv stable connect to: {:?}", self.remote_id());

                self.send_core_data(CoreData::StableConnect(tid, data))
                    .await?;
            }
            SessionMessage::StableResult(tid, is_ok, is_force, data) => {
                debug!(
                    "Session recv stable result to: {:?}, {}",
                    self.remote_id(),
                    is_ok
                );

                self.send_core_data(CoreData::StableResult(tid, is_ok, data))
                    .await?;

                if !self.is_stable {
                    if is_ok {
                        // upgrade session to stable.
                        debug!("Upgrade to stable connection controller");
                        self.upgrade().await?;
                    } else {
                        // TODO Better for waiting this.
                        //
                        // let _ = self
                        //     .relay_send(SessionMessage::RelayClose(*self.my_id()))
                        //     .await;
                        // self.global.peer_list
                        //     .write()
                        //     .await
                        //     .remove_tmp_stable(self.remote_id());

                        // return Err(new_io_error("force close"));
                    }
                }

                if is_force {
                    return Err(new_io_error("force close"));
                }
            }
            SessionMessage::RelayData(from, to, data) => {
                println!("Relay session need send data!");
                if &to == self.remote_id() && &from == self.my_id() {
                    // cannot send it.
                    self.failure_send(data).await?;
                    return Ok(());
                }

                if self.is_direct() {
                    println!("Relay session is_here!");
                    self.direct_send(EndpointMessage::RelayData(from, to, data))
                        .await?;
                } else {
                    println!("Relay session need a new!");
                    if let Some((_, stream_sender, _)) =
                        self.global.peer_list.read().await.dht_get(&to)
                    {
                        let _ = stream_sender
                            .send(EndpointMessage::RelayData(from, to, data))
                            .await;
                    }
                }
            }
            SessionMessage::RelayConnect(from_peer, to) => {
                if &to == self.remote_id() && from_peer.id() == self.my_id() {
                    // cannot send it.
                    //continue;
                    return Ok(());
                }

                if self.is_direct() {
                    debug!("Send to remote stream direct");
                    self.direct_send(EndpointMessage::RelayConnect(from_peer, to))
                        .await?;
                } else {
                    if let Some((_, stream_sender, _)) =
                        self.global.peer_list.read().await.dht_get(&to)
                    {
                        let _ = stream_sender
                            .send(EndpointMessage::RelayConnect(from_peer, to))
                            .await;
                    }
                }
            }
            SessionMessage::RelayResult(..) => {
                error!("Only happen once");
            }
            SessionMessage::RelayClose(peer_id) => {
                // if peer_id == self.endpoints[0] {
                //     TODO changed to other session.
                // }
                self.relay_sessions.remove(&peer_id);
            }
            SessionMessage::Close => {
                self.close(false).await?;
            }
            SessionMessage::DirectIncoming(
                remote_peer,
                stream_sender,
                stream_receiver,
                endpoint_sender,
            ) => {
                self.stream_sender = stream_sender;
                self.stream_receiver = stream_receiver;
                self.endpoint = ConnectType::Direct(endpoint_sender);
                self.remote_peer = remote_peer;
                self.upgrade().await?;
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
                    let sender = &self.global.transport_sender;

                    for p in peers {
                        if p.id() != self.my_id()
                            && !self.global.peer_list.read().await.contains(p.id())
                        {
                            let (session_key, remote_pk) = self.global.generate_remote();
                            //if let Some(sender) = self.global.transports.read().await.get(p.transport()) {}
                            let _ = sender
                                .send(TransportSendMessage::Connect(
                                    *p.addr(),
                                    remote_pk,
                                    session_key,
                                ))
                                .await;
                        }
                    }
                }
            }
            EndpointMessage::Hole(_hole) => {
                todo!();
            }
            EndpointMessage::HoleConnect => {
                todo!();
            }
            EndpointMessage::Data(e_data) => {
                self.handle_core_data(e_data).await?;
            }
            EndpointMessage::RelayData(from, to, data) => {
                if &to == self.my_id() {
                    if &from == self.remote_id() {
                        self.handle_core_data(data).await?;
                    } else {
                        if let Some(stream_sender) = self
                            .global
                            .peer_list
                            .read()
                            .await
                            .get_endpoint_with_tmp(&from)
                        {
                            let _ = stream_sender
                                .send(EndpointMessage::RelayData(from, to, data))
                                .await;
                        } else {
                            debug!("Relay data session not found");
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
                            .next_closest(&to, self.remote_id())
                        {
                            let _ = sender.send(SessionMessage::RelayData(from, to, data)).await;
                        }
                    }
                }
            }
            EndpointMessage::RelayConnect(from_peer, to) => {
                debug!("Relay Connect to: {:?}", to);
                if &to == self.my_id() {
                    let remote_peer_id = from_peer.id().clone();
                    if &remote_peer_id == self.my_id() {
                        return Ok(());
                    }

                    if let Some(sender) = self
                        .global
                        .peer_list
                        .read()
                        .await
                        .get_tmp_stable(&remote_peer_id)
                    {
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
                    let RemotePublic(remote_key, remote_peer, dh_key) = from_peer;

                    let result = self.global.complete_remote(&remote_key, dh_key);
                    if result.is_none() {
                        return Ok(());
                    }
                    let (new_session_key, new_remote_pk) = result.unwrap(); // safe checked.

                    let (new_stream_sender, new_stream_receiver) = new_endpoint_channel(); // session's use.
                    let (new_session_sender, new_session_receiver) = new_session_channel(); // server's use.

                    self.global.peer_list.write().await.add_tmp_stable(
                        remote_peer_id,
                        new_session_sender.clone(),
                        new_stream_sender.clone(),
                    );

                    let new_session = Session::new(
                        remote_peer,
                        new_session_sender,
                        new_session_receiver,
                        new_stream_sender,
                        new_stream_receiver,
                        ConnectType::Relay(self.session_sender.clone()),
                        new_session_key,
                        self.global.clone(),
                        false, // default is not recv data.
                        false,
                    );

                    // if use session_run directly, it will cycle error in rust check.
                    session_spawn(new_session);

                    self.direct_send(EndpointMessage::RelayConnect(new_remote_pk, remote_peer_id))
                        .await?;
                } else {
                    if self.global.is_relay_data {
                        if let Some(sender) = self
                            .global
                            .peer_list
                            .read()
                            .await
                            .next_closest(&to, self.remote_id())
                        {
                            let _ = sender
                                .send(SessionMessage::RelayConnect(from_peer, to))
                                .await;
                        }
                    }
                }
            }
        }

        Ok(())
    }

    async fn handle_heartbeat(&mut self) -> Result<()> {
        if self.heartbeat > 5 {
            return Err(new_io_error("timeout"));
        }

        self.heartbeat += 1;
        self.send_core_data(CoreData::Ping).await
    }

    async fn handle_robust(&mut self) -> Result<()> {
        // 60s timer out when lost connection, and cannot build a new one.
        debug!("10s timer to do robust check, check all connections is connected.");

        Ok(())
    }
}

/// core data transfer and encrypted.
pub(crate) enum CoreData {
    Ping,
    Pong,
    Data(u64, Vec<u8>),
    Delivery(DeliveryType, u64, Vec<u8>),
    StableConnect(u64, Vec<u8>),
    StableResult(u64, bool, Vec<u8>),
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
            CoreData::Unstable => {
                bytes[0] = 7u8;
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
            7u8 => Ok(CoreData::Unstable),
            _ => Err(()),
        }
    }
}
