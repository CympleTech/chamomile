use smol::channel::{Receiver, Sender};
use std::fmt::{Debug, Formatter, Result as FmtResult};
use std::net::SocketAddr;

/// Endpoint can receied this message channel.
pub enum EndpointSendMessage {
    /// connect to a socket address.
    /// params is `socket_addr`, `remote_pk bytes`, `is_stable`.
    Connect(SocketAddr, Vec<u8>, Option<Vec<u8>>),
    /// close a connection.
    /// params is `socket_addr`.
    Close(SocketAddr),
}

/// when endpoint get a incoming connection, will send to outside.
/// params: `socket_addr`, `endpoint_stream_receiver`,
/// `endpoint_stream_sender` and `is_stable`, `remote_pk bytes`.
#[derive(Clone)]
pub struct EndpointIncomingMessage(
    pub SocketAddr,
    pub Vec<u8>,         // remote public info.
    pub Option<Vec<u8>>, // check stable connect.
    pub bool,            // is send by self.
    pub Receiver<EndpointStreamMessage>,
    pub Sender<EndpointStreamMessage>,
);

/// StreamMessage use in endpoint and outside.
#[derive(Clone)]
pub enum EndpointStreamMessage {
    /// Handshake `remote_pk bytes`.
    Handshake(Vec<u8>),
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
