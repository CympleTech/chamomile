use tokio::sync::mpsc::Sender;

use crate::peer::Peer;
use crate::types::{Broadcast, PeerId, TransportStream};

/// Custom apply for build a stream between nodes.
#[derive(Debug)]
pub enum StreamType {
    /// request for build a stream, params is peer id, transport type and request custom info.
    Req(Peer),
    /// response for build a stream, params is is_ok, and response custom info.
    Res(bool),
    /// if response is ok, will build a stream, and return the stream to ouside.
    Ok(TransportStream),
}

/// delivery message type.
#[derive(Debug, Clone)]
pub enum DeliveryType {
    Data,
    StableConnect,
    StableResult,
}

/// main received message for outside channel, send from chamomile to outside.
#[derive(Debug)]
pub enum ReceiveMessage {
    /// when peer what to stable connect, send from chamomile to outside.
    /// params is `peer` and `connect_info`.
    StableConnect(Peer, Vec<u8>),
    /// when peer get stable connect result.
    /// params is `peer`, `is_ok` and `result_data`.
    StableResult(Peer, bool, Vec<u8>),
    /// when peer want to response a stable result, but the session is closed,
    /// if stable result is ok, then need create a result connect to sender.
    /// the data type is stable result data type.
    ResultConnect(Peer, Vec<u8>),
    /// when a stable connection's peer leave,
    /// send from chamomile to outside.
    /// params is `peer`.
    StableLeave(Peer),
    /// when received a data from a trusted peer,
    /// send to outside.
    /// params is `peer_id` and `data_bytes`.
    Data(PeerId, Vec<u8>),
    /// (Only stable connected) Apply for build a stream between nodes.
    /// params is `u32` stream symbol, and `StreamType`.
    Stream(u32, StreamType, Vec<u8>),
    /// (Only stable connected) Delivery feedback. include StableConnect, StableResult, Data. `id(u32) != 0`.
    Delivery(DeliveryType, u64, bool, Vec<u8>),
    /// when network lost all DHT network and direct stables. will tell outside.
    NetworkLost,
    /// when same PeerId peer is connected.
    /// this peer.id is assist_id.
    OwnConnect(Peer),
    /// when same PeerId is leaved.
    /// this peer.id is assist_id.
    OwnLeave(Peer),
    /// when receive same PeerId message.
    /// params is `assist_id` and `data_bytes`.
    OwnEvent(PeerId, Vec<u8>),
}

/// main send message for outside channel, send from outside to chamomile.
#[derive(Debug)]
pub enum SendMessage {
    /// when peer request for join, outside decide connect or not.
    /// params is `delivery_feedback_id`, `peer`, `is_connect`, `is_force_close`, `result info`.
    /// if `delivery_feedback_id = 0` will not feedback.
    /// if `is_connect` is true, it will add to allow directly list.
    /// we want to build a better network, add a `is_force_close`.
    /// if `is_connect` is false, but `is_force_close` if true, we
    /// will use this peer to build our DHT for better connection.
    /// if false, we will force close it.
    /// In general, you can example as `StableResult(0, Peer, is_ok, false, vec![])`.
    StableResult(u64, Peer, bool, bool, Vec<u8>),
    /// when need add a peer to stable connect, send to chamomile from outside.
    /// if success connect, will start a stable connection, and add peer to kad, stables,
    /// bootstraps and allowlists. if failure, will send `PeerLeave` to outside.
    /// params is `delivery_feedback_id`, `peer` and custom `join_info`.
    /// if `delivery_feedback_id = 0` will not feedback.
    StableConnect(u64, Peer, Vec<u8>),
    /// when outside want to close a stable connectioned peer. use it force close.
    /// params is `peer_id`.
    StableDisconnect(PeerId),
    /// (DHT connected) when outside want to connect a peer. will try connect directly.
    /// if connected, chamomile will add to kad and bootstrap.
    /// params is `Peer`.
    Connect(Peer),
    /// (DHT connected) when outside donnot want to connect peer. use it to force close.
    /// it will remove from kad and bootstrap list.
    /// params is `Peer`.
    DisConnect(Peer),
    /// when need send a data to a peer, only need know the peer_id,
    /// the chamomile will help you send data to there.
    /// params is `delivery_feedback_id`, `peer_id` and `data_bytes`.
    /// if `delivery_feedback_id = 0` will not feedback.
    Data(u64, PeerId, Vec<u8>),
    /// when need broadcast a data to all network,
    /// chamomile support some common algorithm, use it, donnot worry.
    /// params is `broadcast_type` and `data_bytes`
    Broadcast(Broadcast, Vec<u8>),
    /// (Only Stable connected) Apply for build a stream between nodes.
    /// params is `u32` stream symbol, and `StreamType`.
    Stream(u32, StreamType, Vec<u8>),
    /// Request for return the network current state info.
    /// params is request type, and return channel's sender (async).
    NetworkState(StateRequest, Sender<StateResponse>),
    /// When receive `ReceiveMessage::NetworkLost`, want to reboot network, it can use.
    NetworkReboot,
    /// When want to close p2p network.
    NetworkStop,
    /// when want to broadcast message with same PeerId.
    OwnEvent(Vec<u8>),
}

/// Network state info response.
#[derive(Debug, Clone)]
pub enum StateRequest {
    Stable,
    DHT,
    Seed,
}

/// Network state info response.
#[derive(Debug)]
pub enum StateResponse {
    /// response is peer list and peer is relay or directly.
    Stable(Vec<(PeerId, bool)>),
    /// response is peer list.
    DHT(Vec<PeerId>),
    /// response is socket list.
    Seed(Vec<Peer>),
}
