use async_std::net::UdpSocket;
use async_std::io::Result;
use async_std::sync::{channel, Sender, Receiver};
use async_std::task;
use async_std::sync::Arc;
use async_trait::async_trait;
use std::net::SocketAddr;
//use std::collections::{HashMap, BTreeMap};
//use rand::{RngCore, thread_rng};

use super::{Endpoint, EndpointMessage, MAX_MESSAGE_CAPACITY};

/// TCP Endpoint.
pub struct TcpEndpoint;

#[async_trait]
impl Endpoint for TcpEndpoint {
    /// Init and run a UdpEndpoint object.
    /// You need send a socketaddr str and udp send message's addr,
    /// and receiver outside message addr.
    async fn start(
        socket_addr: SocketAddr,
        out_send: Sender<EndpointMessage>
    ) -> Result<Sender<EndpointMessage>>{
        let socket: Arc<UdpSocket> = Arc::new(UdpSocket::bind(socket_addr).await?);
        let (send, recv) = channel(MAX_MESSAGE_CAPACITY);

        Ok(send)
    }

}
