use p2p_core::{Message, NodeId};
use std::collections::{HashSet, VecDeque};
use std::net::SocketAddr;
use tokio::net::TcpStream;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args[1].as_str() {
        "crawl" => {
            let seed: SocketAddr = args[2].parse().unwrap();
            crawl(seed).await;
        }
        "sybil" => {
            let target: SocketAddr = args[2].parse().unwrap();
            let count: usize = args[3].parse().unwrap();
            sybil_attack(target, count).await;
        }
        "monitor" => {
            let target: SocketAddr = args[2].parse().unwrap();
            monitor(target).await;
        }
        _ => eprintln!("Usage: p2p-lab <crawl|sybil> <addr> [count]"),
    }
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

async fn sybil_attack(target: SocketAddr, count: usize) {
    println!("Launching {count} Sybil nodes against {target}...");

    let mut handles = vec![];

    for i in 0..count {
        handles.push(tokio::spawn(async move {
            let node_id = NodeId::random();
            match TcpStream::connect(target).await {
                Ok(mut stream) => {
                    let msg = Message::Hello {
                        node_id,
                        listen_addr: format!("127.0.0.1:{}", 19000 + i).parse().unwrap(),
                        peers: vec![],
                    };
                    let mut line = serde_json::to_string(&msg).unwrap();
                    line.push('\n');
                    if stream.write_all(line.as_bytes()).await.is_err() { return; }
                    
                    println!(" sybil-{i} connected ({node_id:?})");

                    // hold connection open forever
                    tokio::time::sleep(tokio::time::Duration::from_secs(3600)).await;
                }
                Err(e) => eprintln!(" sybil-{i} failed: {e}"),
            }
        }));
    }

    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    // crawl victim to measure peer occupation
    let peers = query_peers(target).await.unwrap_or_default();
    let sybil_count = peers.iter()
        .filter(|a| a.port() >= 19000) // sybil ports start at 19000
        .count();

    println!("\n=== Sybil Attack Result ===");
    println!("Victim peer list: {:?}", peers);
    println!("Total peers: {}", peers.len());
    println!("Sybil peers: {}", sybil_count);
    println!("Occupancy:  {:.0}%", sybil_count as f64 / peers.len().max(1) as f64 * 100.0);

    tokio::signal::ctrl_c().await.unwrap();
    for h in handles { h.abort() };
}

async fn monitor(target: SocketAddr) {
    println!("Monitoring {target}...");
    
    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

        let peers = match query_peers(target).await {
            Some(p) => p,
            None => {
                println!("[WARN] target unreachable");
                continue;
            }
        };

        if peers.is_empty() {
            println!("peer list empty");
            continue
        }

        // count how many peers share the same IP
        let mut ip_count: std::collections::HashMap<std::net::IpAddr, usize> = 
            std::collections::HashMap::new();
        
        for peer in &peers {
            *ip_count.entry(peer.ip()).or_default() += 1;
        }

        // find the dominant IP
        let (dominant_ip, dominant_count) = ip_count
            .iter()
            .max_by_key(|(_, c)| *c)
            .unwrap();
        
        let concentration = *dominant_count as f64 / peers.len() as f64 * 100.0;

        println!(
            "[monitor] peers={} dominant_ip={} ({}/{} = {:.0}%)",
            peers.len(),
            dominant_ip,
            dominant_count,
            peers.len(),
            concentration
        );

        if concentration >= 50.0 {
            println!("[WARN] Peer table concentration: {:.0}% from {}", concentration, dominant_ip);
        }
        if concentration >= 100.0 {
            println!("[ALERT] Possible Eclipse setup - all peers from same IP!");
        }

    }   

}