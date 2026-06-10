use p2p_core::{Message, NodeId};
use p2p_node::{MAX_PEERS, NoVerifier, PeerList, handle_inbound, send_msg};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::rustls::ClientConfig;
use tokio_rustls::rustls::pki_types::ServerName;
use tokio_rustls::{TlsAcceptor, TlsConnector};

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    // args[1] = listen port (e.g., 9001)
    // args[2..] = seed peers (e.g., 127.0.0.1:9000)
    let port: u16 = args[1].parse().unwrap();
    let seeds: Vec<SocketAddr> = args[2..].iter().map(|s| s.parse().unwrap()).collect();

    let node_id = NodeId::random();
    let peers: PeerList = Arc::new(Mutex::new(Vec::new()));

    println!("[{port}] NodeId={:?}", node_id);
    let listen_addr = format!("127.0.0.1:{port}");
    let listener = TcpListener::bind(&listen_addr).await.unwrap();
    let (server_config, _cert_der, _cert_bytes) = p2p_node::make_tls_config();
    let tls_acceptor = TlsAcceptor::from(Arc::new(server_config));
    let client_config = ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(NoVerifier))
        .with_no_client_auth();
    let tls_connector = TlsConnector::from(Arc::new(client_config));
    println!("[{port}] listening");

    // connect to seed peers
    for seed in seeds {
        let peers = peers.clone();
        let tls_connector = tls_connector.clone();
        tokio::spawn(async move {
            match TcpStream::connect(seed).await {
                Ok(tcp_stream) => {
                    let server_name = ServerName::try_from("localhost").unwrap();
                    let tls_stream = tls_connector
                        .connect(server_name, tcp_stream)
                        .await
                        .unwrap();

                    // split stream so we can read and write separately
                    let (read_half, mut write_half) = tokio::io::split(tls_stream);
                    let mut reader = BufReader::new(read_half);
                    let mut line = String::new();

                    let known = peers.lock().unwrap().clone();
                    send_msg(
                        &mut write_half,
                        &Message::Hello {
                            node_id,
                            listen_addr: format!("127.0.0.1:{port}").parse().unwrap(),
                            peers: known,
                        },
                    )
                    .await
                    .ok();

                    // read their Hello reply
                    if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
                        return;
                    }
                    if let Ok(Message::Hello { node_id: rid, .. }) =
                        serde_json::from_str(line.trim())
                    {
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
                        if write_half.write_all(ping.as_bytes()).await.is_err() {
                            break;
                        }

                        // read Pong
                        line.clear();
                        if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
                            break;
                        }

                        if tick.is_multiple_of(2) {
                            line.clear();
                            let mut get = serde_json::to_string(&Message::GetPeers).unwrap();
                            get.push('\n');
                            if write_half.write_all(get.as_bytes()).await.is_err() {
                                break;
                            }
                            if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
                                break;
                            }
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
            if let Ok((tcp_stream, peer_addr)) = listener.accept().await {
                let acceptor = tls_acceptor.clone();
                let peers = peers_clone.clone();
                let blocklist: p2p_node::BlockList =
                    Arc::new(Mutex::new(std::collections::HashSet::new()));
                let bl = blocklist.clone();
                tokio::spawn(async move {
                    match acceptor.accept(tcp_stream).await {
                        Ok(tls_stream) => {
                            handle_inbound(tls_stream, peer_addr, node_id, port, peers, bl).await;
                        }
                        Err(e) => eprintln!("[{port}] TLS accept failed: {e}"),
                    }
                });
            }
        }
    });

    let peers_for_tip = peers.clone();
    tokio::spawn(async move {
        let mut height = 100u64;
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
            let _known = peers_for_tip.lock().unwrap().clone();
            println!("[{port}] broadcasting tip height={height}");
            // tip broadcast happens via direct connection - skip for now
            height += 1;
        }
    });
    tokio::signal::ctrl_c().await.unwrap();
}
