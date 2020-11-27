use smol::{
    channel::{Receiver, Sender},
    future,
    io::Result,
    net::{TcpListener, TcpStream},
    prelude::*,
};
use std::net::SocketAddr;

use crate::keys::SessionKey;

use super::{
    new_endpoint_channel, EndpointMessage, RemotePublic, TransportRecvMessage, TransportSendMessage,
};

/// Init and run a TcpEndpoint object.
/// You need send a socketaddr str and tcp send message's addr,
/// and receiver outside message addr.
pub async fn start(
    bind_addr: SocketAddr,
    send: Sender<TransportRecvMessage>,
    recv: Receiver<TransportSendMessage>,
) -> Result<()> {
    let listener = TcpListener::bind(bind_addr).await.map_err(|e| {
        error!("Chamomile TCP listen {:?}", e);
        std::io::Error::new(std::io::ErrorKind::Other, "TCP Listen")
    })?;

    // TCP listen incoming.
    smol::spawn(run_listen(listener, send.clone())).detach();

    // TCP listen from outside.
    smol::spawn(run_self_recv(recv, send)).detach();

    Ok(())
}

async fn run_listen(listener: TcpListener, out_send: Sender<TransportRecvMessage>) -> Result<()> {
    let mut incoming = listener.incoming();

    while let Some(Ok(stream)) = incoming.next().await {
        let (self_sender, self_receiver) = new_endpoint_channel();
        let (out_sender, out_receiver) = new_endpoint_channel();

        smol::spawn(process_stream(
            stream,
            out_sender,
            self_receiver,
            OutType::DHT(out_send.clone(), self_sender, out_receiver),
            None,
        ))
        .detach();
    }

    error!("Tcp endpoint incoming failure.");
    Ok(())
}

async fn run_self_recv(
    recv: Receiver<TransportSendMessage>,
    out_send: Sender<TransportRecvMessage>,
) -> Result<()> {
    while let Ok(m) = recv.recv().await {
        match m {
            TransportSendMessage::Connect(addr, remote_pk, session_key) => {
                if let Ok(mut stream) = TcpStream::connect(addr).await {
                    info!("TCP connect to {:?}", addr);
                    let bytes = EndpointMessage::Handshake(remote_pk).to_bytes();
                    let _ = stream.write(&(bytes.len() as u32).to_be_bytes()).await;
                    let _ = stream.write_all(&bytes[..]).await;

                    let (self_sender, self_receiver) = new_endpoint_channel();
                    let (out_sender, out_receiver) = new_endpoint_channel();

                    smol::spawn(process_stream(
                        stream,
                        out_sender,
                        self_receiver,
                        OutType::DHT(out_send.clone(), self_sender, out_receiver),
                        Some(session_key),
                    ))
                    .detach();
                } else {
                    info!("TCP cannot connect to {:?}", addr);
                }
            }
            TransportSendMessage::StableConnect(out_sender, self_receiver, addr, remote_pk) => {
                if let Ok(mut stream) = TcpStream::connect(addr).await {
                    info!("TCP stable connect to {:?}", addr);
                    let bytes = EndpointMessage::Handshake(remote_pk).to_bytes();
                    let _ = stream.write(&(bytes.len() as u32).to_be_bytes()).await;
                    let _ = stream.write_all(&bytes[..]).await;

                    smol::spawn(process_stream(
                        stream,
                        out_sender,
                        self_receiver,
                        OutType::Stable,
                        None,
                    ))
                    .detach();
                } else {
                    info!("TCP cannot stable connect to {:?}", addr);
                    let _ = out_sender.send(EndpointMessage::Close).await;
                }
            }
        }
    }

    Ok(())
}

enum OutType {
    DHT(
        Sender<TransportRecvMessage>,
        Sender<EndpointMessage>,
        Receiver<EndpointMessage>,
    ),
    Stable,
}

async fn process_stream(
    stream: TcpStream,
    out_sender: Sender<EndpointMessage>,
    self_receiver: Receiver<EndpointMessage>,
    out_type: OutType,
    has_session: Option<SessionKey>,
) -> Result<()> {
    let addr = stream.peer_addr()?;
    let mut reader = stream.clone();
    let mut writer = stream;

    let mut read_len = [0u8; 4];
    let handshake: std::result::Result<RemotePublic, ()> = async {
        match reader.read(&mut read_len).await {
            Ok(size) => {
                if size != 4 {
                    return Err(());
                }

                let len: usize = u32::from_be_bytes(read_len) as usize;
                let mut read_bytes = vec![0u8; len];
                let mut received: usize = 0;

                while let Ok(bytes_size) = reader.read(&mut read_bytes[received..]).await {
                    received += bytes_size;
                    if received < len {
                        continue;
                    }

                    if let Ok(EndpointMessage::Handshake(remote_pk)) =
                        EndpointMessage::from_bytes(read_bytes)
                    {
                        return Ok(remote_pk);
                    } else {
                        return Err(());
                    }
                }

                Err(())
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

    let remote_pk = handshake.unwrap();

    match out_type {
        OutType::Stable => {
            out_sender
                .send(EndpointMessage::Handshake(remote_pk))
                .await
                .map_err(|_e| {
                    std::io::Error::new(std::io::ErrorKind::Other, "endpoint channel missing")
                })?;
        }
        OutType::DHT(sender, self_sender, out_receiver) => {
            sender
                .send(TransportRecvMessage(
                    addr,
                    remote_pk,
                    has_session,
                    out_sender.clone(),
                    out_receiver,
                    self_sender,
                ))
                .await
                .map_err(|_e| {
                    std::io::Error::new(std::io::ErrorKind::Other, "server channel missing")
                })?;
        }
    }

    let a = async move {
        loop {
            match self_receiver.recv().await {
                Ok(msg) => {
                    let is_close = match msg {
                        EndpointMessage::Close => true,
                        _ => false,
                    };

                    let bytes = msg.to_bytes();
                    if writer
                        .write(&(bytes.len() as u32).to_be_bytes())
                        .await
                        .is_ok()
                    {
                        let _ = writer.write_all(&bytes[..]).await;
                    }

                    if is_close {
                        break;
                    }
                }
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
                            .send(EndpointMessage::Close)
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

                        if let Ok(msg) = EndpointMessage::from_bytes(read_bytes) {
                            out_sender
                                .send(msg)
                                .await
                                .expect("Endpoint to Session (Bytes)");
                        }

                        break;
                    }
                    read_len = [0u8; 4];
                    received = 0;
                }
                Err(_e) => {
                    out_sender
                        .send(EndpointMessage::Close)
                        .await
                        .expect("Endpoint to Session (Close)");
                    break;
                }
            }
        }

        Err::<(), ()>(())
    };

    let _ = future::try_zip(a, b).await;

    debug!("close stream: {}", addr);

    Ok(())
}
