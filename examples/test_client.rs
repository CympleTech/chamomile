use chamomile::transports::udp;
use async_std::task;
use async_std::sync::channel;

fn main() {
    task::block_on(async {
        let (out_send, out_recv) = channel(10);
        let udp_send = udp::start("127.0.0.1:8001", out_send).await.unwrap();
        let peer = "127.0.0.1:8000";
        udp_send.send((vec![1u8; 10000], peer.parse().unwrap())).await;

        while let Some((bytes, peer)) = out_recv.recv().await {
            println!("recv: {:?}, {:?}, len: {}", bytes, peer, bytes.len());
        }
    });
}
