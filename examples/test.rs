use chamomile::transports::UdpEndpoint;
use chamomile::transports::Endpoint;
use async_std::task;
use async_std::sync::channel;

fn main() {
    task::block_on(async {
        let (out_send, out_recv) = channel(10);
        let udp_send = UdpEndpoint::start("127.0.0.1:8000".parse().unwrap(), out_send).await.unwrap();

        while let Some((bytes, peer)) = out_recv.recv().await {
            println!("recv: {:?}, {:?}, len: {}", bytes, peer, bytes.len());
            udp_send.send((bytes, peer)).await;
        }
    });
}
