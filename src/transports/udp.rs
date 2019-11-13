use async_std::net::UdpSocket;
use async_std::net::ToSocketAddrs;
use async_std::io::Result;
use async_std::sync::{channel, Sender, Receiver};
use async_std::task;
use async_std::sync::Arc;
use std::net::SocketAddr;

use std::collections::HashMap;

/// max task capacity for udp to handle.
pub const MAX_MESSAGE_CAPACITY: usize = 1024;

/// 576(MTU) - 8(Head) - 20(IP) = 548
const UDP_UINT: usize = 548;

/// udp and outside message type.
pub type MessageType = (Vec<u8>, SocketAddr);

/// save splited messages buffers.
type Buffers = HashMap<u32, HashMap<u32, [u8; UDP_UINT]>>;

/// Init and run a UdpEndpoint object.
/// You need send a socketaddr str and udp send message's addr,
/// and receiver outside message addr.
pub async fn start(socket_addr: impl ToSocketAddrs, out_send: Sender<MessageType>) -> Result<Sender<MessageType>>{
    let socket: Arc<UdpSocket> = Arc::new(UdpSocket::bind(socket_addr).await?);
    let (send, recv) = channel(MAX_MESSAGE_CAPACITY);

    task::spawn(run_self_recv(socket.clone(), recv));
    task::spawn(run_listen(socket, out_send));
    Ok(send)
}

/// Listen for outside send job.
/// Split message to buffers, if ok, send to remote.
async fn run_self_recv(socket: Arc<UdpSocket>, recv: Receiver<MessageType>) -> Result<()> {
    let send_buffers = Buffers::new();

    loop {
        if let Some((bytes, peer)) = recv.recv().await {
            socket.send_to(&bytes[..], peer).await?;
            // let send_tasks = vec![];
            // for t in send_tasks {
            //     socket.send_to(t, peer).await?;
            // }
        }
    }
}

/// UDP listen. If receive bytes, handle it.
/// Handle receiver bytes, first check if bytes is completed.
/// If not completed, save to buffers, and waiting.
/// If timeout, send request to remote, call send again or drop it.
/// If completed. send to outside.
async fn run_listen(socket: Arc<UdpSocket>, send: Sender<MessageType>) -> Result<()> {
    let recv_buffers = Buffers::new();

    let mut buf = vec![0u8; UDP_UINT];
    loop {
        let (n, peer) = socket.recv_from(&mut buf).await?;
        // self.receiver.send(&buf[..n]);
        // endpoint.handle_recv_bytes(&buf[..n], peer);
        send.send((buf[..n].to_vec(), peer)).await;
    }
}
