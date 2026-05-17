use tokio::net::TcpListener;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use framed_core::{MAX_PAYLOAD, encode_frame};
use std::str::from_utf8;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let addr = "127.0.0.1:7003";
    let listener = TcpListener::bind(addr).await?;
    println!("Listening on {addr}");
    loop {

        let (mut stream, client_addr) = listener.accept().await?;
        println!("client connected: {client_addr}");

        tokio::spawn(async move {
            let mut len_buf = [0u8; 4];
            loop {
                if let Err(e) = stream.read_exact(&mut len_buf).await {
                    eprintln!("read length from {client_addr}: {e}");
                    return;
                }
                let payload_len = u32::from_be_bytes(len_buf);
                if payload_len > MAX_PAYLOAD {
                    eprintln!("frame too large: {payload_len} bytes (max {MAX_PAYLOAD})");
                    return;
                }
                let mut body = vec![0u8; payload_len as usize];
                if let Err(e) = stream.read_exact(&mut body).await {
                    eprintln!("read body from {client_addr}: {e}");
                    return;
                }
                println!("from {client_addr}: {payload_len} bytes");

                let cmd = from_utf8(&body)
                    .map_err(|_| ())
                    .ok()
                    .map(str::trim);

                let response: &[u8] = match cmd {
                    Some("PING") => b"PONG",
                    Some(other) => {
                        eprintln!("unknown command from {client_addr}: {other}");
                        continue;
                    }
                    None => {
                        eprintln!("invalid utf-8 from {client_addr}");
                        continue;
                    }
                };
                let reply = encode_frame(response);
                if let Err(e) = stream.write_all(&reply).await {
                    eprintln!("write to {client_addr}: {e}");
                    return;
                }
            }
        });
    }
}
