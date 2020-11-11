use chamomile_types::{
    message::ReceiveMessage,
    types::{PeerId, TransportType, PEER_ID_LENGTH},
};
use smol::{
    channel::{self, Receiver, Sender},
    future,
    io::Result,
    lock::RwLock,
};
use std::collections::HashMap;
use std::sync::Arc;

use crate::hole_punching::{Hole, DHT};
use crate::keys::{Keypair, SessionKey};
use crate::peer::{Peer, PEER_LENGTH};
use crate::peer_list::PeerList;
use crate::transports::{EndpointSendMessage, EndpointStreamMessage};

/// server send to session message in channel.
pub(crate) enum SessionSendMessage {
    /// send bytes to session what want to send to peer..
    Data(Vec<u8>),
    /// when need build a stable connection.
    StableConnect(Vec<u8>),
    /// when receive a stable result.
    StableResult(bool, Vec<u8>),
    /// Receive Relay data.
    RelayData(PeerId, PeerId, Vec<u8>),
    /// Relay stable connect.
    RelayStableConnect(RemotePublic, PeerId, Vec<u8>),
    /// Relay stable connect result.
    RelayStableResult(RemotePublic, PeerId, Vec<u8>),
    /// close the session.
    Close,
}

/// new a channel for send message to session.
pub(crate) fn new_session_send_channel(
) -> (Sender<SessionSendMessage>, Receiver<SessionSendMessage>) {
    channel::unbounded()
}

/// Session Endpoint Message.
/// bytes[0] is type, bytes[1..] is data.
pub(crate) enum EndpointMessage {
    /// type is 1u8.
    Ping,
    /// type is 2u8.
    Pong,
    /// type is 3u8.
    DHT(DHT),
    /// type is 4u8.
    Hole(Hole),
    /// type is 5u8.
    HoleConnect,
    /// type is 6u8.
    Key(Vec<u8>),
    /// type is 7u8.
    Data(Vec<u8>),
    /// type is 8u8.
    StableConnect(Vec<u8>),
    /// type is 9u8.
    StableResult(bool, Vec<u8>),
    /// type is 10u8
    RelayData(PeerId, PeerId, Vec<u8>),
    /// type is 11u8.
    RelayStableConnect(RemotePublic, PeerId, Vec<u8>),
    /// type is 12u8.
    RelayStableResult(RemotePublic, PeerId, Vec<u8>),
}

impl EndpointMessage {
    pub fn to_bytes(self) -> Vec<u8> {
        let mut bytes = vec![0u8];
        match self {
            EndpointMessage::Ping => {
                bytes[0] = 1u8;
            }
            EndpointMessage::Pong => {
                bytes[0] = 2u8;
            }
            EndpointMessage::DHT(dht) => {
                bytes[0] = 3u8;
                bytes.append(&mut dht.to_bytes());
            }
            EndpointMessage::Hole(hole) => {
                bytes[0] = 4u8;
                bytes.push(hole.to_byte());
            }
            EndpointMessage::HoleConnect => {
                bytes[0] = 5u8;
            }
            EndpointMessage::Key(mut data) => {
                bytes[0] = 6u8;
                bytes.append(&mut data);
            }
            EndpointMessage::Data(mut data) => {
                bytes[0] = 7u8;
                bytes.append(&mut data);
            }
            EndpointMessage::StableConnect(mut data) => {
                bytes[0] = 8u8;
                bytes.append(&mut data);
            }
            EndpointMessage::StableResult(ok, mut data) => {
                bytes[0] = 9u8;
                bytes.push(if ok { 1u8 } else { 0u8 });
                bytes.append(&mut data);
            }
            EndpointMessage::RelayData(p1_id, p2_id, mut data) => {
                bytes[0] = 10u8;
                bytes.append(&mut p1_id.to_bytes());
                bytes.append(&mut p2_id.to_bytes());
                bytes.append(&mut data);
            }
            EndpointMessage::RelayStableConnect(p1_peer, p2_id, mut data) => {
                bytes[0] = 11u8;
                let mut peer_bytes = p1_peer.to_bytes();
                bytes.extend(&(peer_bytes.len() as u32).to_le_bytes()[..]);
                bytes.append(&mut peer_bytes);
                bytes.append(&mut p2_id.to_bytes());
                bytes.append(&mut data);
            }
            EndpointMessage::RelayStableResult(p1_peer, p2_id, mut data) => {
                bytes[0] = 12u8;
                let mut peer_bytes = p1_peer.to_bytes();
                bytes.extend(&(peer_bytes.len() as u32).to_le_bytes()[..]);
                bytes.append(&mut peer_bytes);
                bytes.append(&mut p2_id.to_bytes());
                bytes.append(&mut data);
            }
        }

        if bytes.len() < 20 {
            debug!("DEBUG: SEND REMOTE BYTES: {} {}", bytes[0], bytes.len());
        } else {
            debug!(
                "DEBUG: SEND REMOTE BYTES: {} {} {:?}",
                bytes[0],
                bytes.len(),
                &bytes[0..20]
            );
        }

        bytes
    }

    fn from_bytes(mut bytes: Vec<u8>) -> std::result::Result<Self, ()> {
        if bytes.len() < 1 {
            return Err(());
        }

        let t: Vec<u8> = bytes.drain(0..1).collect();
        match t[0] {
            1u8 => Ok(EndpointMessage::Ping),
            2u8 => Ok(EndpointMessage::Pong),
            3u8 => {
                let dht = DHT::from_bytes(&bytes)?;
                Ok(EndpointMessage::DHT(dht))
            }
            4u8 => {
                if bytes.len() != 1 {
                    return Err(());
                }
                let hole = Hole::from_byte(bytes[0])?;
                Ok(EndpointMessage::Hole(hole))
            }
            5u8 => Ok(EndpointMessage::HoleConnect),
            6u8 => Ok(EndpointMessage::Key(bytes)),
            7u8 => Ok(EndpointMessage::Data(bytes)),
            8u8 => Ok(EndpointMessage::StableConnect(bytes)),
            9u8 => {
                if bytes.len() < 1 {
                    return Err(());
                }
                let ok = bytes.drain(0..1).as_slice()[0] == 1u8;
                Ok(EndpointMessage::StableResult(ok, bytes))
            }
            10u8 => {
                if bytes.len() < PEER_ID_LENGTH * 2 {
                    return Err(());
                }
                let p1 = PeerId::from_bytes(&bytes.drain(0..PEER_ID_LENGTH).as_slice())?;
                let p2 = PeerId::from_bytes(&bytes.drain(0..PEER_ID_LENGTH).as_slice())?;
                Ok(EndpointMessage::RelayData(p1, p2, bytes))
            }
            11u8 => {
                if bytes.len() < 4 {
                    return Err(());
                }
                let mut peer_len_bytes = [0u8; 4];
                peer_len_bytes.copy_from_slice(bytes.drain(0..4).as_slice());
                let peer_len = u32::from_le_bytes(peer_len_bytes) as usize;
                if bytes.len() < peer_len + PEER_ID_LENGTH {
                    return Err(());
                }
                let peer =
                    RemotePublic::from_bytes(bytes.drain(0..peer_len).collect()).map_err(|_| ())?;
                let p2 = PeerId::from_bytes(&bytes.drain(0..PEER_ID_LENGTH).as_slice())?;
                Ok(EndpointMessage::RelayStableConnect(peer, p2, bytes))
            }
            12u8 => {
                if bytes.len() < 4 {
                    return Err(());
                }
                let mut peer_len_bytes = [0u8; 4];
                peer_len_bytes.copy_from_slice(bytes.drain(0..4).as_slice());
                let peer_len = u32::from_le_bytes(peer_len_bytes) as usize;
                if bytes.len() < peer_len + PEER_ID_LENGTH {
                    return Err(());
                }
                let peer =
                    RemotePublic::from_bytes(bytes.drain(0..peer_len).collect()).map_err(|_| ())?;
                let p2 = PeerId::from_bytes(&bytes.drain(0..PEER_ID_LENGTH).as_slice())?;
                Ok(EndpointMessage::RelayStableResult(peer, p2, bytes))
            }
            _ => Err(()),
        }
    }
}

/// Rtemote Public Info, include local transport and public key bytes.
#[derive(Debug)]
pub struct RemotePublic(pub Keypair, pub Peer);

impl RemotePublic {
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self> {
        if bytes.len() < PEER_LENGTH {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "serialize remote public error",
            ));
        }
        let peer = Peer::from_bytes(&bytes[..PEER_LENGTH]).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::Other, "serialize remote public error")
        })?;
        let keypair = Keypair::from_bytes(&bytes[PEER_LENGTH..]).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::Other, "serialize remote public error")
        })?;

        Ok(Self(keypair, peer))
    }

    pub fn ref_to_bytes(key: &Keypair, peer: &Peer) -> Vec<u8> {
        let mut bytes = vec![];
        bytes.append(&mut peer.to_bytes());
        bytes.append(&mut key.to_bytes());
        bytes
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = vec![];
        bytes.append(&mut self.1.to_bytes());
        bytes.append(&mut self.0.to_bytes());
        bytes
    }
}

pub(crate) enum ConnectType {
    Direct(Sender<EndpointStreamMessage>),
    Relay(Vec<Sender<SessionSendMessage>>),
}

pub(crate) struct Session {
    pub my_sender: Sender<SessionSendMessage>,
    pub my_peer_id: PeerId,
    pub remote_peer_id: PeerId,
    pub remote_peer: Peer,
    pub endpoint_sender: Sender<EndpointSendMessage>,
    pub out_sender: Sender<ReceiveMessage>,
    pub connect: ConnectType,
    pub session_receiver: Receiver<SessionSendMessage>,
    pub session_key: SessionKey,
    pub key: Arc<Keypair>,
    pub peer: Arc<Peer>,
    pub peer_list: Arc<RwLock<PeerList>>,
    pub transports: Arc<RwLock<HashMap<TransportType, Sender<EndpointSendMessage>>>>,
    pub is_recv_data: bool,
    pub is_relay_data: bool,
    pub is_stable: bool,
}

pub(crate) async fn relay_stable(session: Session) -> Result<()> {
    info!("start stable connection.");
    let _ = future::race(session.listen_outside(), session.robust()).await;
    session
        .peer_list
        .write()
        .await
        .stable_leave(&session.remote_peer_id);

    let _ = session
        .out_sender
        .send(ReceiveMessage::StableLeave(session.remote_peer_id))
        .await;

    // Better tell listener, it is not need.

    debug!("Relay stable session broke: {:?}", session.remote_peer_id);

    Ok(())
}

pub(crate) async fn direct_dht(
    mut session: Session,
    stream_receiver: Receiver<EndpointStreamMessage>,
) -> Result<()> {
    info!("start DHT connection.");
    let upgrade = if session.is_direct() {
        future::race(
            session.listen_outside(),
            session.listen_endpoint(&stream_receiver),
        )
        .await
    } else {
        Err(std::io::Error::new(std::io::ErrorKind::Other, "PANIC"))
    };

    info!("over DHT connection.");

    let info = session
        .peer_list
        .write()
        .await
        .peer_remove(&session.remote_peer_id);

    if upgrade.is_ok() && info.is_some() {
        // stable connection.
        session.is_recv_data = true;
        session.is_stable = true;
        let (my_sender, peer_info) = info.unwrap();
        session
            .peer_list
            .write()
            .await
            .stable_add(session.remote_peer_id, my_sender, peer_info);

        info!("start stable connection.");
        let _ = future::race(
            session.listen_outside(),
            session.stable_listen_endpoint(&stream_receiver),
        )
        .await;

        session
            .peer_list
            .write()
            .await
            .stable_leave(&session.remote_peer_id);

        let _ = session
            .out_sender
            .send(ReceiveMessage::StableLeave(session.remote_peer_id))
            .await;
    }

    match session.connect {
        ConnectType::Direct(sender) => {
            let _ = sender.send(EndpointStreamMessage::Close).await;
        }
        _ => {}
    }

    debug!("Session broke: {:?}", session.remote_peer_id);

    Ok(())
}

impl Session {
    fn is_direct(&self) -> bool {
        match self.connect {
            ConnectType::Direct(..) => true,
            _ => false,
        }
    }

    async fn direct(&self, msg: EndpointMessage) -> Result<()> {
        match &self.connect {
            ConnectType::Direct(sender) => {
                let _ = sender
                    .send(EndpointStreamMessage::Bytes(msg.to_bytes()))
                    .await;
            }
            _ => {}
        }
        Ok(())
    }

    async fn relay(&self, msg: SessionSendMessage) -> Result<()> {
        match &self.connect {
            ConnectType::Relay(senders) => {
                let _ = senders[0].send(msg).await;
            }
            _ => {}
        }
        Ok(())
    }

    async fn listen_outside(&self) -> Result<()> {
        loop {
            match self.session_receiver.recv().await {
                Ok(SessionSendMessage::Data(data)) => {
                    debug!(
                        "Got outside data {:?} to remote: {:?}",
                        data.len(),
                        self.remote_peer_id
                    );
                    let e_data = self.session_key.encrypt(data);

                    if self.is_direct() {
                        self.direct(EndpointMessage::Data(e_data)).await?;
                    } else {
                        self.relay(SessionSendMessage::RelayData(
                            self.my_peer_id,
                            self.remote_peer_id,
                            e_data,
                        ))
                        .await?;
                    }
                }
                Ok(SessionSendMessage::StableConnect(data)) => {
                    debug!("Session recv stable connect to: {:?}", self.remote_peer_id);
                    // only dht connection, not relay has this stable connect.
                    self.direct(EndpointMessage::StableConnect(data)).await?;
                }
                Ok(SessionSendMessage::StableResult(is_ok, mut data)) => {
                    debug!(
                        "Session recv stable result to: {:?}, {}",
                        self.remote_peer_id, is_ok
                    );

                    if self.is_direct() {
                        self.direct(EndpointMessage::StableResult(is_ok, data))
                            .await?;
                    } else {
                        // when remote lost this stable connection.
                        let mut c_data = if is_ok { vec![1u8] } else { vec![0u8] };
                        c_data.append(&mut data);
                        let e_data = self.session_key.encrypt(c_data);
                        self.relay(SessionSendMessage::RelayStableResult(
                            RemotePublic(self.key.public(), *self.peer.clone()),
                            self.remote_peer_id,
                            e_data,
                        ))
                        .await?;
                    }

                    if !self.is_stable && is_ok {
                        // upgrade session to stable.
                        info!("Upgrade to stable connection controller");
                        return Ok(());
                    }
                }
                Ok(SessionSendMessage::RelayData(from, to, e_data)) => {
                    debug!("Got relay data to: {:?}", to);
                    if from == self.remote_peer_id && to == self.my_peer_id {
                        debug!("Got relay data self, send outside");
                        if let Ok(data) = self.session_key.decrypt(e_data) {
                            if self.is_recv_data {
                                self.out_sender
                                    .send(ReceiveMessage::Data(from, data))
                                    .await
                                    .expect("Session to Outside (Data)");
                            } else {
                                debug!("Permissioned: not accept data.")
                            }
                        }
                    } else {
                        if self.is_direct() {
                            self.direct(EndpointMessage::RelayData(from, to, e_data))
                                .await?;
                        } else {
                            self.relay(SessionSendMessage::RelayData(from, to, e_data))
                                .await?;
                        }
                    }
                }
                Ok(SessionSendMessage::RelayStableConnect(from_peer, to, data)) => {
                    debug!("Got relay stable connect to: {:?}", to);
                    if self.is_direct() {
                        self.direct(EndpointMessage::RelayStableConnect(from_peer, to, data))
                            .await?;
                    } else {
                        self.relay(SessionSendMessage::RelayStableConnect(from_peer, to, data))
                            .await?;
                    }
                }
                Ok(SessionSendMessage::RelayStableResult(from, to, data)) => {
                    debug!("Got relay stable result to: {:?}", to);
                    if self.is_direct() {
                        self.direct(EndpointMessage::RelayStableResult(from, to, data))
                            .await?;
                    } else {
                        self.relay(SessionSendMessage::RelayStableResult(from, to, data))
                            .await?;
                    }
                }
                Ok(SessionSendMessage::Close) => {
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

    async fn listen_endpoint(
        &self,
        stream_receiver: &Receiver<EndpointStreamMessage>,
    ) -> Result<()> {
        loop {
            match stream_receiver.recv().await {
                Ok(EndpointStreamMessage::Handshake(_)) => {
                    panic!("only send to endpoint.");
                }
                Ok(EndpointStreamMessage::Bytes(bytes)) => {
                    if bytes.len() < 1 {
                        continue;
                    }
                    if bytes.len() < 20 {
                        debug!("DEBUG: RECV REMOTE BYTES: {} {}", bytes[0], bytes.len());
                    } else {
                        debug!(
                            "DEBUG: RECV REMOTE BYTES: {} {} {:?}",
                            bytes[0],
                            bytes.len(),
                            &bytes[0..20]
                        );
                    }
                    let t_type_byte = bytes[0].clone();
                    let t = EndpointMessage::from_bytes(bytes);
                    if t.is_err() {
                        debug!(
                            "Debug: Error Serialize EndpointMessage type is {}",
                            t_type_byte
                        );
                        continue;
                    }

                    match t.unwrap() {
                        EndpointMessage::Ping => {
                            self.direct(EndpointMessage::Pong).await?;
                        }
                        EndpointMessage::Pong => {
                            debug!("Receive Poing");
                            // TODO Heartbeat Ping/Pong
                        }
                        EndpointMessage::DHT(DHT(peers)) => {
                            if peers.len() > 0 {
                                let remote_bytes =
                                    RemotePublic::ref_to_bytes(&self.key.public(), &self.peer);

                                for p in peers {
                                    if p.id() != &self.my_peer_id
                                        && !self.peer_list.read().await.contains(p.id())
                                    {
                                        if let Some(sender) =
                                            self.transports.read().await.get(p.transport())
                                        {
                                            let _ = sender
                                                .send(EndpointSendMessage::Connect(
                                                    *p.addr(),
                                                    remote_bytes.clone(),
                                                    None,
                                                ))
                                                .await;
                                        }
                                    }
                                }
                            }
                        }
                        EndpointMessage::Hole(..) => {
                            todo!();
                        }
                        EndpointMessage::HoleConnect => {
                            todo!();
                        }
                        EndpointMessage::Key(_bytes) => {
                            debug!("TODO session rebuild new encrypt key.");
                            // if !session_key.is_ok() {
                            //     if !session_key.in_bytes(bytes) {
                            //         server_send
                            //             .send(SessionReceiveEndpointMessage::Close(remote_peer_id))
                            //             .await
                            //             .expect("Session to Server (Key Close)");
                            //         endpoint_sender
                            //             .send(EndpointStreamMessage::Close)
                            //             .await
                            //             .expect("Session to Endpoint (Key Close)");
                            //         break;
                            //     }

                            //     endpoint_sender
                            //         .send(EndpointStreamMessage::Bytes(
                            //             EndpointMessage::Key(session_key.out_bytes()).to_bytes(),
                            //         ))
                            //         .await
                            //         .expect("Session to Endpoint (Bytes)");
                            // }

                            // while !buffers.is_empty() {
                            //     let (peer_id, bytes) = buffers.pop().unwrap();
                            //     let e_data = session_key.encrypt(bytes);
                            //     let data =
                            //         EndpointMessage::Data(my_peer_id, peer_id, e_data).to_bytes();
                            //     endpoint_sender
                            //         .send(EndpointStreamMessage::Bytes(data))
                            //         .await
                            //         .expect("Session to Endpoint (Bytes)");
                            // }

                            // while !receiver_buffers.is_empty() {
                            //     let e_data = receiver_buffers.pop().unwrap();
                            //     let d_data = session_key.decrypt(e_data);
                            //     if d_data.is_ok() {
                            //         out_sender
                            //             .send(ReceiveMessage::Data(remote_peer_id, d_data.unwrap()))
                            //             .await
                            //             .expect("Session to Outside (Data)");
                            //     }
                            // }
                        }
                        EndpointMessage::Data(e_data) => {
                            if let Ok(data) = self.session_key.decrypt(e_data) {
                                if self.is_recv_data {
                                    self.out_sender
                                        .send(ReceiveMessage::Data(self.remote_peer_id, data))
                                        .await
                                        .expect("Session to Outside (Data)");
                                } else {
                                    debug!("Permissioned: not accept data.")
                                }
                            }
                        }
                        EndpointMessage::StableConnect(data) => {
                            debug!("Recv stable connect from: {:?}", self.remote_peer_id);
                            self.out_sender
                                .send(ReceiveMessage::StableConnect(self.remote_peer_id, data))
                                .await
                                .expect("Session to Outside (Data)");
                        }
                        EndpointMessage::StableResult(is_ok, data) => {
                            debug!(
                                "Recv stable connect result {} from: {:?}",
                                is_ok, self.remote_peer_id
                            );
                            self.out_sender
                                .send(ReceiveMessage::StableResult(
                                    self.remote_peer_id,
                                    is_ok,
                                    data,
                                ))
                                .await
                                .expect("Session to Outside (Data)");

                            if !self.is_stable && is_ok {
                                // upgrade to stable connection.
                                info!("Upgrade to stable connection controller");
                                return Ok(());
                            }
                        }
                        EndpointMessage::RelayData(from, to, data) => {
                            debug!("Endpoint recv relay data to: {:?}", to);
                            if to == self.my_peer_id {
                                // send to from's session.
                                if let Some((sender, is_it)) =
                                    self.peer_list.read().await.get(&from)
                                {
                                    if is_it {
                                        let _ = sender
                                            .send(SessionSendMessage::RelayData(from, to, data))
                                            .await;
                                    }
                                }
                            } else {
                                // send to next relay
                                if let Some((sender, _)) = self.peer_list.read().await.get(&to) {
                                    let _ = sender
                                        .send(SessionSendMessage::RelayData(from, to, data))
                                        .await;
                                }
                            }
                        }
                        EndpointMessage::RelayStableConnect(from, to, data) => {
                            debug!("Endpoint recv relay stable connect to: {:?}", to);
                            if to == self.my_peer_id {
                                info!("Got stable connect from relay");
                                let remote_peer_id = from.1.id().clone();
                                if remote_peer_id == self.my_peer_id {
                                    warn!("Nerver here, relay stable connect from self to self.");
                                    continue;
                                }
                                // send to outside.
                                self.out_sender
                                    .send(ReceiveMessage::StableConnect(remote_peer_id, data))
                                    .await
                                    .expect("Session to Outside (Data)");
                                // save to tmp_stable.
                                self.peer_list
                                    .write()
                                    .await
                                    .add_tmp_stable(remote_peer_id, Some(from));
                            } else {
                                // send to next relay
                                if let Some((sender, _)) = self.peer_list.read().await.get(&to) {
                                    let _ = sender
                                        .send(SessionSendMessage::RelayStableConnect(
                                            from, to, data,
                                        ))
                                        .await;
                                }
                            }
                        }
                        EndpointMessage::RelayStableResult(from, to, e_data) => {
                            debug!("Endpoint recv relay stable result to: {:?}", to);
                            if to == self.my_peer_id {
                                info!("Got stable result from relay");
                                let RemotePublic(remote_key, remote_peer) = from;
                                let remote_peer_id = remote_peer.id().clone();

                                if remote_peer_id == self.my_peer_id {
                                    warn!("Nerver here, relay stable result from self to self.");
                                    continue;
                                }

                                // only happen when the connection by self check it.
                                debug!("aaa");
                                let mut peer_list_lock = self.peer_list.write().await;
                                debug!("aaa");
                                if peer_list_lock.tmp_stable(&remote_peer_id).is_none() {
                                    drop(peer_list_lock);
                                    warn!("Donnot request for stable connect to this peer");
                                    continue;
                                }

                                // create from's session.
                                let session_key: SessionKey =
                                    self.key.key.session_key(&self.key, &remote_key);

                                debug!("start decrypt stable result");
                                if let Ok(mut data) = self.session_key.decrypt(e_data) {
                                    debug!("decrypt stable result ok");
                                    let is_ok = data.drain(0..1).as_slice()[0] == 1u8;
                                    let _ = self
                                        .out_sender
                                        .send(ReceiveMessage::StableResult(
                                            remote_peer_id,
                                            is_ok,
                                            data,
                                        ))
                                        .await;

                                    if is_ok {
                                        let (session_sender, session_receiver) =
                                            new_session_send_channel();

                                        peer_list_lock.stable_add(
                                            remote_peer_id,
                                            session_sender.clone(),
                                            remote_peer.clone(),
                                        );

                                        smol::spawn(relay_stable(Session {
                                            my_sender: session_sender,
                                            my_peer_id: self.my_peer_id.clone(),
                                            remote_peer_id: remote_peer_id,
                                            remote_peer: remote_peer,
                                            endpoint_sender: self.endpoint_sender.clone(),
                                            out_sender: self.out_sender.clone(),
                                            connect: ConnectType::Relay(vec![self
                                                .my_sender
                                                .clone()]),
                                            session_receiver: session_receiver,
                                            session_key: session_key,
                                            key: self.key.clone(),
                                            peer: self.peer.clone(),
                                            peer_list: self.peer_list.clone(),
                                            transports: self.transports.clone(),
                                            is_recv_data: true, // this is stable connection.
                                            is_relay_data: self.is_relay_data,
                                            is_stable: true,
                                        }))
                                        .detach();
                                    }
                                }
                                drop(peer_list_lock);
                            } else {
                                // send to next relay
                                if let Some((sender, _)) = self.peer_list.read().await.get(&to) {
                                    let _ = sender
                                        .send(SessionSendMessage::RelayStableResult(
                                            from, to, e_data,
                                        ))
                                        .await;
                                }
                            }
                        }
                    }
                }
                Ok(EndpointStreamMessage::Close) => break,
                Err(_) => break,
            }
        }

        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "transport recever over",
        ))
    }

    async fn stable_listen_endpoint(
        &self,
        stream_receiver: &Receiver<EndpointStreamMessage>,
    ) -> Result<()> {
        if self.is_direct() {
            let _ = future::race(self.robust(), self.listen_endpoint(stream_receiver)).await;
        } else {
            let _ = self.robust().await;
        }

        Ok(())
    }

    async fn robust(&self) -> Result<()> {
        loop {
            let _ = smol::Timer::after(std::time::Duration::from_secs(10)).await;
            // 10s to do something.
            debug!("10s timer to do something, like: Ping/Pong");
        }
        //Ok(())
    }
}
