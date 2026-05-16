use tokio::net::TcpListener;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn encode_frame(payload: &[u8]) -> Vec<u8> {
    let len = payload.len() as u32;
    let mut out = Vec::with_capacity(4 + len as usize);
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(payload);
    out
}

#[tokio::main]
async fn main() -> std::io::Result<()>{
    const MAX_PAYLOAD: u32 = 64 * 1024;
    let addr = "127.0.0.1:7002";
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
                println!("from {client_addr}: {len} bytes", len = body.len());
                let reply = encode_frame(&body);
                if let Err(e) = stream.write_all(&reply).await {
                    eprintln!("failed to write to {client_addr}: {e}");
                }
            }
        });
    }
}
