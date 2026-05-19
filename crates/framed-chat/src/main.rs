use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt, AsyncRead, AsyncWrite};
use framed_core::{MAX_PAYLOAD, encode_frame};
use std::str::from_utf8;
use tokio::sync::broadcast;
use tokio::sync::broadcast::error::RecvError;
use std::net::SocketAddr;
use std::io::{Result, Error, ErrorKind};
use rustls::ServerConfig;
use std::sync::Arc;
use tokio_rustls::TlsAcceptor;
use sha2::{Sha256, Digest};
use std::fs;

async fn read_frames<S>(stream: &mut S) -> Result<(Vec<u8>)> 
where
    S: AsyncRead + AsyncWrite+ Unpin,
{
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let payload_len = u32::from_be_bytes(len_buf);
    if payload_len > MAX_PAYLOAD {
        return Err(Error::new(
             ErrorKind::InvalidData,
            format!("frame too large: {payload_len} bytes (max {MAX_PAYLOAD})"),
        ));
    }
    let mut body = vec![0u8; payload_len as usize];
    stream.read_exact(&mut body).await?;
    Ok(body)
}

fn make_tls_acceptor() -> TlsAcceptor {
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
    let cert_der = rustls::pki_types::CertificateDer::from(cert.cert);
    let hash = Sha256::digest(&cert_der);
    let hash_hex = hex::encode(hash);
    fs::write("cert.hash", &hash_hex).unwrap();
    println!("cert hash: {hash_hex}");
    let key_der = rustls::pki_types::PrivateKeyDer::try_from(
        cert.key_pair.serialize_der()
    ).unwrap();
    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert_der], key_der)
        .unwrap();
    TlsAcceptor::from(Arc::new(config))
}


#[tokio::main]
async fn main() -> std::io::Result<()> {
    let addr = "127.0.0.1:7004";
    let listener = TcpListener::bind(addr).await?;
    let (tx, _rx) = broadcast::channel::<(SocketAddr, String)>(100);
    println!("Listening on {addr}");

    let acceptor = make_tls_acceptor();
    loop {
        let (mut stream, client_addr) = listener.accept().await?;
        let mut stream = acceptor.accept(stream).await?;
        println!("client connected: {client_addr}");

        let tx = tx.clone();
        let mut rx = tx.subscribe();
        tokio::spawn(async move {
            let mut display_name: Option<String> = None;
            loop {
                tokio::select! {
                    body = read_frames(&mut stream) => {
                        let body = match body {
                            Ok(b) => b,
                            Err(e) => {
                                eprintln!("failed to read frame from {client_addr}");
                                break;
                            }
                        };
                        let text = from_utf8(&body).unwrap_or("").trim();
                        if let Some(name) = text.strip_prefix("JOIN ") {
                            display_name = Some(name.to_string());
                            stream.write_all(&encode_frame(b"OK")).await.ok();
                        } else if let Some(msg) = text.strip_prefix("SAY ") {
                            if let Some(ref name) = display_name {
                                tx.send((client_addr, format!("MSG {name} {msg}"))).ok();
                                stream.write_all(&encode_frame(b"OK")).await.ok();
                            } else {
                                stream.write_all(&encode_frame(b"ERR join first")).await.ok();
                            }
                        } else if text == "PING" {
                            stream.write_all(&encode_frame(b"PONG")).await.ok();
                        } else if text == "ACK" {
                            println!("ACK from {client_addr}");
                        }
                    }
                    packet = rx.recv() => {
                        match packet {
                            Ok((from, msg)) => {
                                if from == client_addr { continue; }
                                if let Err(e) = stream.write_all(&encode_frame(msg.as_bytes())).await {
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
