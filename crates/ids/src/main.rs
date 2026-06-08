use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::net::TcpListener;

#[derive(Debug, Clone)]
struct ConnectionRecord {
    ip: IpAddr,
    port: u16,
    timestamp: Instant,
}

type Records = Arc<Mutex<Vec<ConnectionRecord>>>;

#[tokio::main]
async fn main() {
    let records: Records = Arc::new(Mutex::new(Vec::new()));

    let ports = vec![22, 80, 443, 3306, 6379, 8080];

    for port in ports {
        let records = records.clone();
        tokio::spawn(async move {
            let addr = format!("127.0.0.1:{port}");
            let listener = match TcpListener::bind(&addr).await {
                Ok(l) => {
                    println!("IDS listening on {addr}");
                    l
                }
                Err(e) => {
                    eprintln!("Cannot bind {addr}: {e}");
                    return;
                }
            };
            loop {
                if let Ok((_, peer)) = listener.accept().await {
                    let ip = peer.ip();
                    println!("connection from {ip} → port {port}");
                    records.lock().unwrap().push(ConnectionRecord {
                        ip,
                        port,
                        timestamp: Instant::now(),
                    });
                }
            }
        });
    }

    let records_clone = records.clone();
    tokio::spawn(async move {
        let mut alerted_scan: std::collections::HashSet<IpAddr> = std::collections::HashSet::new();
        let mut alerted_brute: std::collections::HashSet<(IpAddr, u16)> =
            std::collections::HashSet::new();
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

            let now = Instant::now();
            let records = records_clone.lock().unwrap();
            let recent: Vec<&ConnectionRecord> = records
                .iter()
                .filter(|r| now.duration_since(r.timestamp).as_secs() < 30)
                .collect();

            let mut ip_ports: std::collections::HashMap<IpAddr, std::collections::HashSet<u16>> =
                std::collections::HashMap::new();
            let mut ip_ports_count: std::collections::HashMap<(IpAddr, u16), usize> =
                std::collections::HashMap::new();

            for r in &recent {
                ip_ports.entry(r.ip).or_default().insert(r.port);
                *ip_ports_count.entry((r.ip, r.port)).or_default() += 1;
            }

            for (ip, ports) in &ip_ports {
                if ports.len() >= 3 && !alerted_scan.contains(ip) {
                    alerted_scan.insert(*ip);
                    println!("ALERT: Port scan from {ip}");
                }
            }

            for ((ip, port), count) in &ip_ports_count {
                if *count >= 5 && !alerted_brute.contains(&(*ip, *port)) {
                    alerted_brute.insert((*ip, *port));
                    println!(
                        "ALERT: Brute force detected from {ip} on port {port} - {count} attempts"
                    );
                }
            }
        }
    });

    tokio::signal::ctrl_c().await.unwrap();
}
