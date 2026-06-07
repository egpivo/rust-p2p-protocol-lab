use p2p_core::{Message, NodeId};
use std::collections::{HashSet, VecDeque};
use std::net::SocketAddr;
use tokio::net::TcpStream;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    // args[1] = seed addr (e.g., 127.0.0.1.9000)
    let seed: SocketAddr = args[1].parse().unwrap();

    println!("Crawling from seed {seed}...");
    crawl(seed).await;
}

async fn crawl(seed: SocketAddr) {
    let mut visited: HashSet<SocketAddr> = HashSet::new();
    let mut queue: VecDeque<SocketAddr> = VecDeque::new();

    queue.push_back(seed);

    while let Some(addr) = queue.pop_front() {
        if visited.contains(&addr) { continue; }
        visited.insert(addr);

        println!("visiting {addr}");
        
        match query_peers(addr).await {
            Some(peers) => {
                println!(" {} -> {:?}", addr, peers);
                for peer in peers {
                    if peer.port() == 0 { continue; }
                    if !visited.contains(&peer) {
                        queue.push_back(peer);
                    }
                }
            }
            None => println!(" {} -> unreachable", addr)
        }
    }
    println!("Done. Discovered {} nodes.", visited.len());
}


async fn query_peers(addr: SocketAddr) -> Option<Vec<SocketAddr>> {
    let mut stream = TcpStream::connect(addr).await.ok()?;

    // send Hello
    let node_id = NodeId::random();
    // note: crawler has no listen port
    let msg = Message::Hello {
        node_id,
        listen_addr: "127.0.0.1:0".parse().unwrap(),
        peers: vec![],
    };
    let mut line = serde_json::to_string(&msg).unwrap();
    line.push('\n');
    stream.write_all(line.as_bytes()).await.ok()?;

    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let mut line = String::new();
    
    // read Hello reply
    reader.read_line(&mut line).await.ok()?;
    line.clear();

    // send GetPeers
    let mut get = serde_json::to_string(&Message::GetPeers).unwrap();
    get.push('\n');
    write_half.write_all(get.as_bytes()).await.ok()?;


    // read Peers response
    reader.read_line(&mut line).await.ok()?;
    match serde_json::from_str(line.trim()).ok()? {
        Message::Peers(list) => Some(list),
        _ => None,
    }
}