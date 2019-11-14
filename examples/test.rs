use chamomile::transports::udp;
use async_std::task;
use async_std::sync::channel;

fn main() {
    task::block_on(async {
        let (out_send, out_recv) = channel(10);
        let udp_send = udp::start("127.0.0.1:8000", out_send).await.unwrap();

        while let Some((bytes, peer)) = out_recv.recv().await {
            println!("recv: {:?}, {:?}", bytes, peer);
            udp_send.send((bytes, peer)).await;
        }
    });
}
