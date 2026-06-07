use p2p_core::{Message, NodeId};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

const MAX_PEERS: usize = 8;

type PeerList = Arc<Mutex<Vec<SocketAddr>>>;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    // args[1] = listen port (e.g., 9001)
    // args[2..] = seed peers (e.g., 127.0.0.1:9000)
    let port: u16 = args[1].parse().unwrap();
    let seeds: Vec<SocketAddr> = args[2..].iter()
        .map(|s| s.parse().unwrap())
        .collect();
    
    let node_id = NodeId::random();
    let peers: PeerList = Arc::new(Mutex::new(Vec::new()));

    println!("[{port}] NodeId={:?}", node_id);
    let listen_addr = format!("127.0.0.1:{port}");
    let listener = TcpListener::bind(&listen_addr).await.unwrap();
    println!("[{port}] listening");
    
    // connect to seed peers
    for seed in seeds {
        let peers = peers.clone();
        tokio::spawn(async move {
            match TcpStream::connect(seed).await {
                Ok(mut stream) => {
                    let known = peers.lock().unwrap().clone();
                    send_msg(&mut stream, &Message::Hello {
                        node_id,
                        listen_addr: format!("127.0.0.1:{port}").parse().unwrap(),
                        peers: known,
                    }).await.ok();
                    
                    // split stream so we can read and write separately
                    let (read_half, mut write_half) = stream.into_split();
                    let mut reader = BufReader::new(read_half);
                    let mut line = String::new();

                    // read their Hello reply
                    if reader.read_line(&mut line).await.unwrap_or(0) == 0 { return; }
                    if let Ok(Message::Hello { node_id: rid, .. }) = serde_json::from_str(line.trim()) {
                        peers.lock().unwrap().push(seed);
                        println!("[{port}] -> connected to {:?} at {seed}", rid);
                    }

                    // keep alive loop
                    let mut tick = 0u32;
                    loop {
                        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                        tick += 1;

                        let mut ping = serde_json::to_string(&Message::Ping).unwrap();
                        ping.push('\n');
                        if write_half.write_all(ping.as_bytes()).await.is_err() { break; }

                        // read Pong
                        line.clear();
                        if reader.read_line(&mut line).await.unwrap_or(0) == 0 { break; }

                        if tick % 2 == 0 {
                            line.clear();
                            let mut get = serde_json::to_string(&Message::GetPeers).unwrap();
                            get.push('\n');
                            if write_half.write_all(get.as_bytes()).await.is_err() { break; }
                            if reader.read_line(&mut line).await.unwrap_or(0) == 0 { break; }
                            if let Ok(Message::Peers(list)) = serde_json::from_str(line.trim()) {
                                let mut p = peers.lock().unwrap();
                                for addr in list {
                                    if p.len() < MAX_PEERS && !p.contains(&addr) {
                                        p.push(addr);
                                        println!("[{port}] learned new peer: {addr}");
                                    }
                                }
                            }
                        }
                    }
                }
                Err(e) => eprintln!("[{port}] seed {seed} failed: {e}"),
            }

        });
    }

    // accept inbound connection
    let peers_clone = peers.clone();
    tokio::spawn(async move {
        loop {
            if let Ok((stream, peer_addr)) = listener.accept().await {
                let peers = peers_clone.clone();
                tokio::spawn(handle_inbound(stream, peer_addr, node_id, port, peers));
            }
        }
    });
    tokio::signal::ctrl_c().await.unwrap();
}

async fn send_msg(stream: &mut TcpStream, msg: &Message) -> std::io::Result<()> {
    let mut line = serde_json::to_string(msg).unwrap();
    line.push('\n');
    stream.write_all(line.as_bytes()).await
}


async fn handle_inbound(
    stream: TcpStream,
    peer_addr: SocketAddr,
    node_id: NodeId,
    port: u16,
    peers: PeerList,
) {
    // receive their Hello first
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let mut line = String::new();

    if reader.read_line(&mut line).await.unwrap_or(0) == 0 { return; }

    let remote_id = match serde_json::from_str::<Message>(line.trim()) {
        Ok(Message::Hello { node_id: rid, listen_addr, peers: their_peers }) => {
            let mut p = peers.lock().unwrap();
            if listen_addr.port() != 0 && p.len() < MAX_PEERS && !p.contains(&listen_addr) {
                p.push(listen_addr);
            }
            for addr in their_peers {
                if p.len() < MAX_PEERS && !p.contains(&addr) {
                    p.push(addr);
                }
            }
            rid
        }
        _ => return,
    };

    // reply with our Hello
    let known = peers.lock().unwrap().clone();
    let reply = Message::Hello { node_id, listen_addr: format!("127.0.0.1:{port}").parse().unwrap(), peers: known };
    let mut reply_line = serde_json::to_string(&reply).unwrap();
    reply_line.push('\n');
    if write_half.write_all(reply_line.as_bytes()).await.is_err() { return; }

    println!("[{}] <- handshake from {:?} at {}", node_id.0, remote_id, peer_addr);
    
    // keep connection alive. handle Ping/Pong/GetPeers
    loop {
        line.clear();
        if reader.read_line(&mut line).await.unwrap_or(0) == 0 { break; }
        match serde_json::from_str::<Message>(line.trim()) {
            Ok(Message::Ping) => {
                let mut pong = serde_json::to_string(&Message::Pong).unwrap();
                pong.push('\n');
                if write_half.write_all(pong.as_bytes()).await.is_err() { break; }
            }
            Ok(Message::GetPeers) => {
                let known = peers.lock().unwrap().clone();
                let mut msg = serde_json::to_string(&Message::Peers(known)).unwrap();
                msg.push('\n');
                if write_half.write_all(msg.as_bytes()).await.is_err() { break; }
            }
            _ => {}
        }
    }

    println!("[{}] connection closed from {}", node_id.0, peer_addr);
}
