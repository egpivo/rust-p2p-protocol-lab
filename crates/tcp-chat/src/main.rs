use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tokio::sync::broadcast::error::RecvError;
use std::net::SocketAddr;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let addr = "127.0.0.1:7001";
    let listener = TcpListener::bind(addr).await?;
    let (tx, _rx) = broadcast::channel::<(SocketAddr, String)>(100);

    println!("Listening on {addr}");

    loop {
        let (mut stream, client_addr) = listener.accept().await?;
        println!("client connected: {client_addr}");

        let tx = tx.clone();
        let mut rx = tx.subscribe();

        tokio::spawn(async move {
            let mut buf = [0u8; 1024];

            loop {
                tokio::select! {
                    n = stream.read(&mut buf) => {
                        let n = match n {
                            Ok(n) => n,
                            Err(e) => {
                                eprintln!("failed to read from {client_addr}: {e}");
                                break;
                            }
                        };
                        if n == 0 {
                            println!("client disconnected: {client_addr}");
                            break;
                        }
                        let msg = String::from_utf8_lossy(&buf[..n]);
                        println!("received {n} bytes from {client_addr}:{msg}");

                        let line = format!("{client_addr}: {msg}");
                        if let Err(e) = tx.send((client_addr, line)) {
                            eprintln!("broadcast send failed: {e}");
                        }
                    }
                    packet = rx.recv() => {
                        match packet {
                            Ok((from, msg)) => {
                                if from == client_addr { continue; }
                                if let Err(e) = stream.write_all(msg.as_bytes()).await {
                                    eprintln!("failed to write to {client_addr}: {e}");
                                    break;
                                }
                            }
                            Err(RecvError::Lagged(skipped)) => {
                                eprintln!(
                                    "client {client_addr} lagged, skipped {skipped} messages"
                                );
                            }
                            Err(RecvError::Closed) => break,
                        }
                    }
                }
            }
        });
    }
}
