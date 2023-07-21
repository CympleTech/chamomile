use std::{collections::HashMap, net::SocketAddr, sync::Arc, time::Instant};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, Result},
    join,
    net::{TcpListener, TcpStream},
    select,
    sync::{
        mpsc::{Receiver, Sender},
        RwLock,
    },
    task::JoinHandle,
};

use crate::session_key::SessionKey;

use super::{
    new_endpoint_channel, EndpointMessage, RemotePublic, TransportRecvMessage,
    TransportSendMessage, CONNECTING_WAITING,
};

/// Init and run a TcpEndpoint object.
/// You need send a socketaddr str and tcp send message's addr,
/// and receiver outside message addr.
pub async fn start(
    bind_addr: SocketAddr,
    send: Sender<TransportRecvMessage>,
    recv: Receiver<TransportSendMessage>,
    both: bool,
) -> Result<SocketAddr> {
    let (addr, task) = if both {
        let listener = TcpListener::bind(bind_addr).await.map_err(|e| {
            error!("TCP listen {:?}", e);
            std::io::Error::new(std::io::ErrorKind::Other, "TCP Listen")
        })?;
        let addr = listener.local_addr()?;
        info!("TCP listening at: {:?}", addr);

        // TCP listen incoming.
        let task = tokio::spawn(run_listen(listener, send.clone()));
        (addr, Some(task))
    } else {
        (bind_addr, None)
    };

    // TCP listen from outside.
    tokio::spawn(run_self_recv(recv, send, task));

    Ok(addr)
}

async fn run_listen(listener: TcpListener, out_send: Sender<TransportRecvMessage>) -> Result<()> {
    loop {
        let (stream, _) = listener.accept().await?;
        let (self_sender, self_receiver) = new_endpoint_channel();
        let (out_sender, out_receiver) = new_endpoint_channel();

        tokio::spawn(process_stream(
            stream,
            out_sender,
            self_receiver,
            OutType::DHT(out_send.clone(), self_sender, out_receiver),
            None,
            None,
        ));
    }
}

async fn run_self_recv(
    mut recv: Receiver<TransportSendMessage>,
    out_send: Sender<TransportRecvMessage>,
    task: Option<JoinHandle<Result<()>>>,
) -> Result<()> {
    let connecting: Arc<RwLock<HashMap<SocketAddr, Instant>>> =
        Arc::new(RwLock::new(HashMap::new()));

    while let Some(m) = recv.recv().await {
        match m {
            TransportSendMessage::Connect(addr, remote_pk, session_key) => {
                let read_lock = connecting.read().await;
                if let Some(time) = read_lock.get(&addr) {
                    if time.elapsed().as_secs() < CONNECTING_WAITING {
                        drop(read_lock);
                        continue;
                    }
                }
                drop(read_lock);
                let mut lock = connecting.write().await;
                lock.insert(addr, Instant::now());
                drop(lock);
                let new_connecting = connecting.clone();

                let server_send = out_send.clone();
                tokio::spawn(async move {
                    if let Ok(mut stream) = TcpStream::connect(addr).await {
                        info!("TCP connect to {:?}", addr);
                        let bytes = EndpointMessage::Handshake(remote_pk).to_bytes();
                        let _ = stream.write(&(bytes.len() as u32).to_be_bytes()).await;
                        let _ = stream.write_all(&bytes[..]).await;

                        let (self_sender, self_receiver) = new_endpoint_channel();
                        let (out_sender, out_receiver) = new_endpoint_channel();

                        let _ = process_stream(
                            stream,
                            out_sender,
                            self_receiver,
                            OutType::DHT(server_send, self_sender, out_receiver),
                            Some(session_key),
                            Some(new_connecting),
                        )
                        .await;
                    } else {
                        info!("TCP cannot connect to {:?}", addr);
                    }
                });
            }
            TransportSendMessage::StableConnect(out_sender, self_receiver, addr, remote_pk) => {
                let read_lock = connecting.read().await;
                if let Some(time) = read_lock.get(&addr) {
                    if time.elapsed().as_secs() < CONNECTING_WAITING {
                        drop(read_lock);
                        continue;
                    }
                }
                drop(read_lock);
                let mut lock = connecting.write().await;
                lock.insert(addr, Instant::now());
                drop(lock);
                let new_connecting = connecting.clone();

                tokio::spawn(async move {
                    if let Ok(mut stream) = TcpStream::connect(addr).await {
                        info!("TCP stable connect to {:?}", addr);
                        let bytes = EndpointMessage::Handshake(remote_pk).to_bytes();
                        let _ = stream.write(&(bytes.len() as u32).to_be_bytes()).await;
                        let _ = stream.write_all(&bytes[..]).await;

                        let _ = process_stream(
                            stream,
                            out_sender,
                            self_receiver,
                            OutType::Stable,
                            None,
                            Some(new_connecting),
                        )
                        .await;
                    } else {
                        info!("TCP cannot stable connect to {:?}", addr);
                        let _ = out_sender.send(EndpointMessage::Close).await;
                    }
                });
            }
            TransportSendMessage::Stop => {
                if let Some(task) = task {
                    task.abort();
                }
                break;
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
    mut stream: TcpStream,
    out_sender: Sender<EndpointMessage>,
    mut self_receiver: Receiver<EndpointMessage>,
    out_type: OutType,
    has_session: Option<SessionKey>,
    connectiongs: Option<Arc<RwLock<HashMap<SocketAddr, Instant>>>>,
) -> Result<()> {
    let addr = stream.peer_addr()?;
    let (mut reader, mut writer) = stream.split();

    let mut read_len = [0u8; 4];
    let handshake: std::result::Result<RemotePublic, ()> = select! {
        v = async {
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
        } => v,
        v = async {
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            Err(())
        } => v
    };

    if handshake.is_err() {
        // close it. if is_by_self, Better send outside not connect.
        debug!("Transport: connect read publics timeout, close it.");
        return Ok(());
    }

    let remote_pk = handshake.unwrap(); // safe. checked.

    if let Some(connectiongs) = connectiongs {
        let mut lock = connectiongs.write().await;
        lock.remove(&addr);
        drop(lock);
        drop(connectiongs);
    }

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
                Some(msg) => {
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
                None => break,
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
                        let _ = out_sender.send(EndpointMessage::Close).await;
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
                            let _ = out_sender.send(msg).await;
                        }

                        break;
                    }
                    read_len = [0u8; 4];
                    received = 0;
                }
                Err(_e) => {
                    let _ = out_sender.send(EndpointMessage::Close).await;
                    break;
                }
            }
        }

        Err::<(), ()>(())
    };

    let _ = join!(a, b);

    debug!("close stream: {}", addr);

    Ok(())
}
