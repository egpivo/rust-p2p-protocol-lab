use p2p_core::{Message, NodeId};
use p2p_noise::{generate_keypair, handshake_initiator, handshake_responder};
use std::net::SocketAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let port: u16 = args[1].parse().unwrap();
    let seed: Option<SocketAddr> = args.get(2).map(|s| s.parse().unwrap());
    let expected_key: Option<String> = args.get(3).cloned();

    let node_id = NodeId::random();
    let (static_private, static_public) = generate_keypair();

    println!("[{port}] NodeId={:?}", node_id);
    println!("[{port}] static pubkey={}", hex(&static_public));

    let listener = TcpListener::bind(format!("127.0.0.1:{port}"))
        .await
        .unwrap();
    println!("[{port}] listening");

    // connect to seeds
    if let Some(seed) = seed {
        let privkey = static_private;
        let expected_key = expected_key.clone();
        tokio::spawn(async move {
            match TcpStream::connect(seed).await {
                Ok(mut stream) => {
                    match handshake_initiator(&mut stream, &privkey).await {
                        Ok(mut transport) => {
                            let remote = transport.remote_static().unwrap();
                            let remote_hex = hex(remote);
                            println!("[{port}] -> handshake OK, remote={remote_hex}");

                            if let Some(ref expected) = expected_key {
                                if remote_hex != *expected {
                                    eprintln!("[{port}] MITM DETECTED: expected {expected}");
                                    eprintln!("[{port}]                got     {remote_hex}");
                                    return;
                                }
                                println!("[{port}] peer identity verified");
                            }
                            // send Hello
                            let msg = serde_json::to_string(&Message::Hello {
                                node_id,
                                listen_addr: format!("127.0.0.1:{port}").parse().unwrap(),
                                peers: vec![],
                            })
                            .unwrap();
                            noise_send(&mut stream, &mut transport, msg.as_bytes()).await;

                            //recv Hello reply
                            let reply = noise_recv(&mut stream, &mut transport).await;
                            if let Ok(line) = std::str::from_utf8(&reply) {
                                println!("[{port}] <- {}", line.trim());
                            }
                        }
                        Err(e) => eprintln!("[{port}] handshake failed: {e}"),
                    }
                }
                Err(e) => eprintln!("[{port}] connect failed: {e}"),
            }
        });
    }

    // accept inbound
    loop {
        if let Ok((mut stream, peer_addr)) = listener.accept().await {
            let privkey = static_private;
            tokio::spawn(async move {
                match handshake_responder(&mut stream, &privkey).await {
                    Ok(mut transport) => {
                        let remote = transport.remote_static().unwrap();
                        println!(
                            "[{port}] <- handshake OK from {peer_addr} remote={}",
                            hex(remote)
                        );

                        // recv Hello
                        let data = noise_recv(&mut stream, &mut transport).await;
                        if let Ok(line) = std::str::from_utf8(&data) {
                            println!("[{port}] <- {}", line.trim());
                        }

                        // send Hello reply
                        let reply = serde_json::to_string(&Message::Hello {
                            node_id,
                            listen_addr: format!("127.0.0.1:{port}").parse().unwrap(),
                            peers: vec![],
                        })
                        .unwrap();
                        noise_send(&mut stream, &mut transport, reply.as_bytes()).await;
                    }
                    Err(e) => eprintln!("[{port}] handshake failed from {peer_addr}: {e}"),
                }
            });
        }
    }
}

async fn noise_send(
    stream: &mut TcpStream,
    transport: &mut p2p_noise::NoiseTransport,
    plaintext: &[u8],
) {
    let mut buf = vec![0u8; 65535];
    let len = transport.send(plaintext, &mut buf);
    stream.write_all(&(len as u16).to_be_bytes()).await.ok();
    stream.write_all(&buf[..len]).await.ok();
}

async fn noise_recv(stream: &mut TcpStream, transport: &mut p2p_noise::NoiseTransport) -> Vec<u8> {
    let mut len_buf = [0u8; 2];
    stream.read_exact(&mut len_buf).await.unwrap();
    let len = u16::from_be_bytes(len_buf) as usize;
    let mut msg = vec![0u8; len];
    stream.read_exact(&mut msg).await.unwrap();
    let mut buf = vec![0u8; 65535];
    let out = transport.recv(&msg, &mut buf);
    out.to_vec()
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
