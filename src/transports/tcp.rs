use smol::{
    channel::{Receiver, Sender},
    future,
    io::Result,
    lock::Mutex,
    net::{TcpListener, TcpStream},
    prelude::*,
};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use super::message::{EndpointIncomingMessage, EndpointSendMessage, EndpointStreamMessage};
use super::new_endpoint_stream_channel;

/// TCP Endpoint.
pub struct TcpEndpoint {
    streams: HashMap<SocketAddr, Sender<EndpointStreamMessage>>,
}

impl TcpEndpoint {
    /// Init and run a UdpEndpoint object.
    /// You need send a socketaddr str and udp send message's addr,
    /// and receiver outside message addr.
    pub async fn start(
        bind_addr: SocketAddr,
        send: Sender<EndpointIncomingMessage>,
        recv: Receiver<EndpointSendMessage>,
    ) -> Result<()> {
        let endpoint = TcpEndpoint {
            streams: HashMap::new(),
        };

        let m1 = Arc::new(Mutex::new(endpoint));
        let m2 = m1.clone();

        // TCP listen
        smol::spawn(run_listen(bind_addr, send.clone(), m1)).detach();

        // TCP listen from outside
        smol::spawn(run_self_recv(recv, send, m2)).detach();

        Ok(())
    }
}

async fn run_listen(
    socket_addr: SocketAddr,
    out_send: Sender<EndpointIncomingMessage>,
    endpoint: Arc<Mutex<TcpEndpoint>>,
) -> Result<()> {
    let listener = TcpListener::bind(socket_addr).await.map_err(|e| {
        error!("Chamomile TCP listen {:?}", e);
        std::io::Error::new(std::io::ErrorKind::Other, "TCP Listen")
    })?;

    let mut incoming = listener.incoming();

    while let Some(Ok(stream)) = incoming.next().await {
        smol::spawn(process_stream(
            stream,
            out_send.clone(),
            endpoint.clone(),
            false,
        ))
        .detach();
    }

    drop(incoming);
    drop(listener);
    Ok(())
}

async fn run_self_recv(
    recv: Receiver<EndpointSendMessage>,
    out_send: Sender<EndpointIncomingMessage>,
    endpoint: Arc<Mutex<TcpEndpoint>>,
) -> Result<()> {
    while let Ok(m) = recv.recv().await {
        match m {
            EndpointSendMessage::Connect(addr, mut data, is_stable) => {
                if let Ok(mut stream) = TcpStream::connect(addr).await {
                    info!("TCP connect to {:?}", addr);
                    let (is_stable_byte, mut stable_bytes) = if let Some(stable_bytes) = is_stable {
                        (1u8, stable_bytes)
                    } else {
                        (0u8, vec![])
                    };

                    let pk_len = data.len() as u16;
                    let stable_len = stable_bytes.len() as u32;
                    let mut bytes = vec![];
                    bytes.push(1u8); // now setup version is 1u8.
                    bytes.extend(&pk_len.to_be_bytes()[..]); // pk len is [0u8; 2].
                    bytes.push(is_stable_byte); // is stable byte.
                    bytes.extend(&stable_len.to_be_bytes()[..]); // stable len is [0u8; 4].
                    bytes.append(&mut data); // append pk info.
                    bytes.append(&mut stable_bytes); // append stable info.

                    let _ = stream
                        .write_all(&bytes)
                        .await
                        .map_err(|e| error!("TCP WRITE ERROR {:?}", e));

                    smol::spawn(process_stream(
                        stream,
                        out_send.clone(),
                        endpoint.clone(),
                        true,
                    ))
                    .detach();
                } else {
                    info!("TCP cannot connect to {:?}", addr);
                }
            }
            EndpointSendMessage::Close(ref addr) => {
                if let Some(sender) = endpoint.lock().await.streams.remove(addr) {
                    sender
                        .send(EndpointStreamMessage::Close)
                        .await
                        .expect("Endpoint to Server (Close)");
                }
            }
        }
    }

    Ok(())
}

async fn process_stream(
    stream: TcpStream,
    sender: Sender<EndpointIncomingMessage>,
    endpoint: Arc<Mutex<TcpEndpoint>>,
    is_self: bool,
) -> Result<()> {
    let addr = stream.peer_addr()?;
    let mut reader = stream.clone();
    let mut writer = stream;

    // bytes[0] is version, bytes[1..3] is public_info, bytes[3] is stable or not, bytes[4..8] is stable bytes.
    let mut read_len = [0u8; 8];

    let handshake: std::result::Result<(Vec<u8>, bool, Vec<u8>), ()> = async {
        match reader.read(&mut read_len).await {
            Ok(size) => {
                if size != 8 {
                    return Err(());
                }
                let mut pk_len_bytes = [0u8; 2];
                let mut stable_len_bytes = [0u8; 4];
                pk_len_bytes.copy_from_slice(&read_len[1..3]);
                let is_stable = read_len[3] == 1u8;
                stable_len_bytes.copy_from_slice(&read_len[4..8]);
                let pk_len: u16 = u16::from_be_bytes(pk_len_bytes);
                let stable_len: u32 = u32::from_be_bytes(stable_len_bytes);

                let next_len = pk_len as usize + stable_len as usize;
                let mut read_bytes = vec![0u8; next_len];
                let mut received: usize = 0;

                while let Ok(bytes_size) = reader.read(&mut read_bytes[received..]).await {
                    received += bytes_size;
                    if received >= next_len {
                        break;
                    }
                }
                let pk: Vec<u8> = read_bytes.drain(0..pk_len as usize).collect();
                Ok((pk, is_stable, read_bytes))
            }
            Err(e) => {
                error!("TCP READ ERROR: {:?}", e);
                Err(())
            }
        }
    }
    .or(async {
        smol::Timer::after(std::time::Duration::from_secs(10)).await;
        Err(())
    })
    .await;

    if handshake.is_err() {
        // close it. if is_by_self, Better send outside not connect.
        debug!("Transport: connect read publics timeout, close it.");
        return Ok(());
    }
    let (pk_info, is_stable, stable_bytes) = handshake.unwrap();
    let stable_info = if is_stable { Some(stable_bytes) } else { None };

    let (self_sender, self_receiver) = new_endpoint_stream_channel();
    let (out_sender, out_receiver) = new_endpoint_stream_channel();

    endpoint
        .lock()
        .await
        .streams
        .entry(addr)
        .and_modify(|s| *s = self_sender.clone())
        .or_insert(self_sender.clone());

    sender
        .send(EndpointIncomingMessage(
            addr,
            pk_info,
            stable_info,
            is_self,
            out_receiver,
            self_sender,
        ))
        .await
        .expect("Endpoint to Server (Incoming)");

    let a = async move {
        loop {
            match self_receiver.recv().await {
                Ok(msg) => match msg {
                    EndpointStreamMessage::Handshake(mut data) => {
                        let pk_len = data.len() as u16;
                        let stable_len = 0u32;
                        let mut bytes = vec![];
                        bytes.push(1u8); // now setup version is 1u8.
                        bytes.extend(&pk_len.to_be_bytes()[..]); // pk len is [0u8; 2].
                        bytes.push(0u8); // default when handshake is not stable.
                        bytes.extend(&stable_len.to_be_bytes()[..]); // stable len is [0u8; 4].
                        bytes.append(&mut data); // append pk info.
                        let _ = writer.write_all(&bytes[..]).await;
                    }
                    EndpointStreamMessage::Bytes(bytes) => {
                        let len = bytes.len() as u32;
                        let _ = writer.write(&(len.to_be_bytes())).await;
                        let _ = writer.write_all(&bytes[..]).await;
                    }
                    EndpointStreamMessage::Close => break,
                },
                Err(_e) => break,
            }
        }

        Err::<(), ()>(())
    };

    let b = async move {
        let mut read_len = [0u8; 4];
        let mut received: usize = 0;

        loop {
            match reader.read(&mut read_len).await {
                Ok(size) => {
                    if size == 0 {
                        // when close or better when many Ok(0)
                        out_sender
                            .send(EndpointStreamMessage::Close)
                            .await
                            .expect("Endpoint to Session (Size Close)");
                        break;
                    }

                    let len: usize = u32::from_be_bytes(read_len) as usize;
                    let mut read_bytes = vec![0u8; len];
                    while let Ok(bytes_size) = reader.read(&mut read_bytes[received..]).await {
                        received += bytes_size;
                        if received > len {
                            break;
                        }

                        if received != len {
                            continue;
                        }

                        out_sender
                            .send(EndpointStreamMessage::Bytes(read_bytes.clone()))
                            .await
                            .expect("Endpoint to Session (Bytes)");

                        break;
                    }
                    read_len = [0u8; 4];
                    received = 0;
                }
                Err(_e) => {
                    out_sender
                        .send(EndpointStreamMessage::Close)
                        .await
                        .expect("Endpoint to Session (Close)");
                    break;
                }
            }
        }

        Err::<(), ()>(())
    };

    let _ = future::try_zip(a, b).await;

    //stream.close().await?;
    endpoint.lock().await.streams.remove(&addr);
    debug!("close stream: {}", addr);

    Ok(())
}
