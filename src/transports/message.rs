use smol::channel::{Receiver, Sender};
use std::fmt::{Debug, Formatter, Result as FmtResult};
use std::net::SocketAddr;

/// Endpoint can receied this message channel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EndpointSendMessage {
    /// connect to a socket address.
    /// params is `socket_addr`, `remote_pk_info`, `is_stable_data`.
    Connect(SocketAddr, Vec<u8>, Option<Vec<u8>>),
    /// close a connection.
    /// params is `socket_addr`.
    Close(SocketAddr),
}

/// when endpoint get a incoming connection, will send to outside.
/// params: `socket_addr`, `endpoint_stream_receiver`,
/// `endpoint_stream_sender` and `is_start_connect_by_self`.
#[derive(Clone)]
pub struct EndpointIncomingMessage(
    pub SocketAddr,
    pub Receiver<EndpointStreamMessage>,
    pub Sender<EndpointStreamMessage>,
    pub Option<Vec<u8>>,
);

/// StreamMessage use in endpoint and outside.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EndpointStreamMessage {
    /// transfer bytes.
    Bytes(Vec<u8>),
    /// closed.
    Close,
}

impl Debug for EndpointIncomingMessage {
    fn fmt(&self, f: &mut Formatter) -> FmtResult {
        write!(f, "Ednpoint incoming: {}.", self.0)
    }
}
