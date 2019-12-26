use async_std::task;
use chamomile::{new_channel, start, Config, Message};
use std::env::args;
use std::net::SocketAddr;

fn main() {
    task::block_on(async {
        let self_addr: SocketAddr = args()
            .nth(1)
            .expect("missing path")
            .parse()
            .expect("invalid addr");

        let (out_send, out_recv) = new_channel();
        let send = start(out_send, Config::default(self_addr)).await.unwrap();

        if args().nth(2).is_some() {
            let remote_addr: SocketAddr = args().nth(2).unwrap().parse().expect("invalid addr");
            println!("start connect to remote: {}", remote_addr);
            send.send(Message::Connect(remote_addr)).await;
        }

        while let Some(message) = out_recv.recv().await {
            match message {
                Message::Data(peer_id, bytes) => {
                    println!("recv data from: {}, {:?}", peer_id.short_show(), bytes);
                }
                Message::PeerJoin(peer_id) => {
                    println!("peer join: {:?}", peer_id);
                    send.send(Message::PeerJoinResult(peer_id, true)).await;
                    println!("Debug: when join send message test: {:?}", vec![1, 2, 3, 4]);
                    send.send(Message::Data(peer_id, vec![1, 2, 3, 4])).await;
                }
                Message::PeerLeave(peer_id) => {
                    println!("peer_leave: {:?}", peer_id);
                }
                _ => break,
            }
        }
    });
}
