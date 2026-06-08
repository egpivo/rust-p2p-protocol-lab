use framed_core::{MAX_PAYLOAD, encode_frame};
use std::collections::HashMap;
use std::str::from_utf8;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let addr = "127.0.0.1:7003";
    let listener = TcpListener::bind(addr).await?;
    println!("Listening on {addr}");
    let store = Arc::new(Mutex::new(HashMap::<String, String>::new()));

    loop {
        let (mut stream, client_addr) = listener.accept().await?;
        println!("client connected: {client_addr}");
        let store = store.clone();

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
                let cmd = from_utf8(&body).ok().map(str::trim);

                let Some(text) = cmd else {
                    eprintln!("invalid utf-8 from {client_addr}");
                    continue;
                };
                let parts: Vec<&str> = text.split_whitespace().collect();

                let response: Vec<u8> = match parts.as_slice() {
                    ["PING"] => b"PONG".to_vec(),
                    ["GET", key] => {
                        let guard = store.lock().unwrap();
                        match guard.get(*key) {
                            Some(v) => format!("VALUE {v}").into_bytes(),
                            None => b"NOT_FOUND".to_vec(),
                        }
                    }
                    ["PUT", key, value] => {
                        store
                            .lock()
                            .unwrap()
                            .insert(key.to_string(), value.to_string());
                        b"OK".to_vec()
                    }
                    other => {
                        eprintln!("unknown command from {client_addr}: {:?}", other);
                        continue;
                    }
                };
                let reply = encode_frame(&response);
                if let Err(e) = stream.write_all(&reply).await {
                    eprintln!("write to {client_addr}: {e}");
                    return;
                }
            }
        });
    }
}
