use async_std::task;
use chamomile::{new_channel, start, Config, Message};

fn main() {
    task::block_on(async {
        let (out_send, out_recv) = new_channel();
        let send = start(out_send, Config::default("127.0.0.1:8000".parse().unwrap()))
            .await
            .unwrap();

        while let Some(message) = out_recv.recv().await {
            match message {
                Message::Data(peer_id, _bytes) => {
                    println!("recv data from: {:?}", peer_id);
                }
                Message::PeerJoin(peer_id) => {
                    println!("peer join: {:?}", peer_id);
                    send.send(Message::PeerJoinResult(peer_id, true)).await;
                }
                Message::PeerLeave(peer_id) => {
                    println!("peer_leave: {:?}", peer_id);
                }
                _ => break,
            }
        }

        drop(send);
        drop(out_recv);
    });
}
