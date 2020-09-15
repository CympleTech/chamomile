use futures::select;
use rand::{thread_rng, RngCore};
use smol::{
    channel::{Receiver, Sender},
    io::{BufReader, Result},
    lock::Mutex,
    net::UdpSocket,
    prelude::*,
};
use std::collections::{BTreeMap, HashMap};
use std::net::SocketAddr;
use std::sync::Arc;

use super::{new_channel, new_stream_channel, EndpointMessage, StreamMessage};

/// 576(MTU) - 8(Head) - 20(IP) - 8(ID + Head) = 540
const UDP_UINT: usize = 540;

/// save splited messages buffers.
type Buffers = HashMap<u32, (u32, BTreeMap<u32, Vec<u8>>)>;

/// UDP Endpoint.
/// Provide a simple recombine and resend function.
pub struct UdpEndpoint {
    streams: HashMap<SocketAddr, Sender<StreamMessage>>,
}

// TODO how to connected
// TODO how to connected verify

#[async_trait]
impl UdpEndpoint {
    /// Init and run a UdpEndpoint object.
    /// You need send a socketaddr str and udp send message's addr,
    /// and receiver outside message addr.
    async fn start(
        socket_addr: SocketAddr,
        out_send: Sender<EndpointMessage>,
    ) -> Result<Sender<EndpointMessage>> {
        let socket: Arc<UdpSocket> = Arc::new(UdpSocket::bind(socket_addr).await?);
        let (send, recv) = new_channel();
        let endpoint = UdpEndpoint {
            streams: HashMap::new(),
        };

        let m1 = Arc::new(Mutex::new(endpoint));
        let m2 = m1.clone();

        smol::spawn(run_self_recv(socket.clone(), recv));
        smol::spawn(run_listen(socket, out_send, m2));
        Ok(send)
    }
}

/// Listen for outside send job.
/// Split message to buffers, if ok, send to remote.
async fn run_self_recv(socket: Arc<UdpSocket>, recv: Receiver<EndpointMessage>) -> Result<()> {
    let mut send_buffers = Buffers::new();

    while let Some(m) = recv.recv().await {
        let peer = match m {
            EndpointMessage::Connect(addr, _bytes) => addr, // TODO send connect bytes
            EndpointMessage::Disconnect(addr) => addr,
            _ => return Ok(()),
        };
        let mut bytes = vec![1]; // TODO

        let buffer_key = thread_rng().next_u32();
        let total_size = bytes.len();
        let mut new_buffer = BTreeMap::new();
        let mut i = 1;
        loop {
            if bytes.len() < UDP_UINT {
                new_buffer.insert(i, bytes);
                break;
            }

            let next_bytes = bytes.split_off(UDP_UINT);
            new_buffer.insert(i, bytes);
            bytes = next_bytes;
            i += 1;
        }

        send_buffers.insert(buffer_key, (total_size as u32, new_buffer));

        let send_tasks = send_buffers.get(&buffer_key).unwrap();
        let buffer_key_bytes = buffer_key.to_be_bytes();

        let mut head_bytes = [0u8; 12];
        head_bytes[0..4].copy_from_slice(&buffer_key_bytes);
        head_bytes[8..12].copy_from_slice(&send_tasks.0.to_be_bytes());
        socket.send_to(&head_bytes, peer).await?;

        for (k, v) in send_tasks.1.iter() {
            let mut bytes = [0u8; 8 + UDP_UINT];
            bytes[0..4].copy_from_slice(&buffer_key_bytes);
            bytes[4..8].copy_from_slice(&k.to_be_bytes());
            bytes[8..8 + v.len()].copy_from_slice(v);
            socket.send_to(&bytes[..8 + v.len()], peer).await?;
        }

        let mut tail_bytes = [255u8; 8];
        tail_bytes[0..4].copy_from_slice(&buffer_key_bytes);
        socket.send_to(&tail_bytes, peer).await?;
    }

    drop(send_buffers);
    Ok(())
}

/// UDP listen. If receive bytes, handle it.
/// Handle receiver bytes, first check if bytes is completed.
/// If not completed, save to buffers, and waiting.
/// If timeout, send request to remote, call send again or drop it.
/// If completed. send to outside.
async fn run_listen(
    socket: Arc<UdpSocket>,
    send: Sender<EndpointMessage>,
    endpoint: Arc<Mutex<UdpEndpoint>>,
) -> Result<()> {
    let mut recv_buffers = Buffers::new();

    let mut buf = vec![0u8; 8 + UDP_UINT];
    while let Ok((n, peer)) = socket.recv_from(&mut buf).await {
        if buf.len() < 8 {
            continue;
        }

        let id = bytes_to_u32(&buf[0..4]);

        // start new id. and save length
        if buf[4..8] == [0u8; 4] {
            if buf.len() < 12 {
                continue;
            }

            let total_size = bytes_to_u32(&buf[8..12]);
            recv_buffers
                .entry(id)
                .and_modify(|(size, _)| {
                    *size = total_size;
                })
                .or_insert((total_size, Default::default()));
            continue;
        }

        // end id
        if buf[4..8] == [255u8; 4] {
            // TODO check if all data received

            if let Some((_total_size, body)) = recv_buffers.remove(&id) {
                let data: Vec<Vec<u8>> = body.iter().map(|(_, v)| v).cloned().collect();
                let data = data.concat();
                let endpoint = endpoint.lock().await;
                if endpoint.streams.contains_key(&peer) {
                    endpoint
                        .streams
                        .get(&peer)
                        .unwrap()
                        .send(StreamMessage::Bytes(data))
                        .await;
                }

                // clear buf
            }
            continue;
        }

        let no = bytes_to_u32(&buf[4..8]);
        recv_buffers
            .entry(id)
            .and_modify(|(_, body)| {
                body.insert(no, buf[8..n].to_vec());
            })
            .or_insert((0, {
                let mut body = BTreeMap::new();
                body.insert(no, buf[8..n].to_vec());
                body
            }));
    }

    drop(recv_buffers);
    Ok(())
}

fn bytes_to_u32(buf: &[u8]) -> u32 {
    let mut id_bytes = [0u8; 4];
    id_bytes.copy_from_slice(buf);
    u32::from_be_bytes(id_bytes)
}
