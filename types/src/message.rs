use async_channel::Sender;
use std::net::SocketAddr;

use crate::types::{Broadcast, PeerId, TransportStream, TransportType};

/// Custom apply for build a stream between nodes.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum StreamType {
    /// request for build a stream, params is peer id, transport type and request custom info.
    Req(PeerId, TransportType, Vec<u8>),
    /// response for build a stream, params is is_ok, and response custom info.
    Res(bool, Vec<u8>),
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
#[derive(Debug, Clone)]
pub enum ReceiveMessage {
    /// when peer what to stable connect, send from chamomile to outside.
    /// params is `peer_id`, `socket_addr` and peer `connect_info`.
    StableConnect(PeerId, Vec<u8>),
    /// when peer get stable connect result.
    /// params is `peer_id`, `is_ok` and `result_data`.
    StableResult(PeerId, bool, Vec<u8>),
    /// when a stable connection's peer leave,
    /// send from chamomile to outside.
    /// params is `peer_id`.
    StableLeave(PeerId),
    /// when received a data from a trusted peer,
    /// send to outside.
    /// params is `peer_id` and `data_bytes`.
    Data(PeerId, Vec<u8>),
    /// (Only stable connected) Apply for build a stream between nodes.
    /// params is `u32` stream symbol, and `StreamType`.
    Stream(u32, StreamType),
    /// (Only stable connected) Delivery feedback. include StableConnect, StableResult, Data. `id(u32) != 0`.
    Delivery(DeliveryType, u64, bool),
}

/// main send message for outside channel, send from outside to chamomile.
#[derive(Debug, Clone)]
pub enum SendMessage {
    /// when peer request for join, outside decide connect or not.
    /// params is `delivery_feedback_id`, `peer_id`, `is_connect`, `is_force_close`, `result info`.
    /// if `delivery_feedback_id = 0` will not feedback.
    /// if `is_connect` is true, it will add to white directly list.
    /// we want to build a better network, add a `is_force_close`.
    /// if `is_connect` is false, but `is_force_close` if true, we
    /// will use this peer to build our DHT for better connection.
    /// if false, we will force close it.
    StableResult(u64, PeerId, bool, bool, Vec<u8>),
    /// when need add a peer to stable connect, send to chamomile from outside.
    /// if success connect, will start a stable connection, and add peer to kad, stables,
    /// bootstraps and whitelists. if failure, will send `PeerLeave` to outside.
    /// params is `delivery_feedback_id`, `peer_id`, `socket_addr` and peer `join_info`.
    /// if `delivery_feedback_id = 0` will not feedback.
    StableConnect(u64, PeerId, Option<SocketAddr>, Vec<u8>),
    /// when outside want to close a stable connectioned peer. use it force close.
    /// params is `peer_id`.
    StableDisconnect(PeerId),
    /// (DHT connected) when outside want to connect a peer. will try connect directly.
    /// if connected, chamomile will add to kad and bootstrap.
    /// params is `socket_addr`.
    Connect(SocketAddr),
    /// (DHT connected) when outside donnot want to connect peer. use it to force close.
    /// it will remove from kad and bootstrap list.
    /// params is `socket_addr`.
    DisConnect(SocketAddr),
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
    Stream(u32, StreamType),
    /// Request for return the network current state info.
    /// params is request type, and return channel's sender (async).
    NetworkState(StateRequest, Sender<StateResponse>),
}

/// Network state info response.
#[derive(Debug, Clone)]
pub enum StateRequest {
    Stable,
    DHT,
    Seed,
}

/// Network state info response.
#[derive(Debug, Clone)]
pub enum StateResponse {
    /// response is peer list and peer is relay or directly.
    Stable(Vec<(PeerId, bool)>),
    /// response is peer list.
    DHT(Vec<PeerId>),
    /// response is socket list.
    Seed(Vec<SocketAddr>),
}
