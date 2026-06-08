use std::net::SocketAddr;
use tokio::fs::OpenOptions;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

#[tokio::main]
async fn main() {
    let listener = TcpListener::bind("127.0.0.1:2222").await.unwrap();
    println!("Honeypot listening on port 2222");

    loop {
        let (stream, peer_addr) = listener.accept().await.unwrap();
        println!("Connection from {peer_addr}");
        tokio::spawn(async move {
            handle(stream, peer_addr).await;
        });
    }
}

async fn handle(mut stream: TcpStream, peer: SocketAddr) {
    // banner
    if stream.write_all(b"SSH-2.0-OpenSSH_9.6\r\n").await.is_err() {
        return;
    }

    // prompt username
    if stream.write_all(b"login: ").await.is_err() {
        return;
    }
    let user = match read_line(&mut stream, &mut Vec::new()).await {
        Some(u) => u,
        None => return,
    };

    // prompt password
    if stream.write_all(b"Password: ").await.is_err() {
        return;
    }
    let pass = match read_line(&mut stream, &mut Vec::new()).await {
        Some(s) => s,
        None => return,
    };

    // log
    let log_line = format!(
        "{} | ip={} | user={:?} | pass={:?}\n",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
        peer,
        user,
        pass
    );

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open("honeypot.log")
        .await
        .unwrap();

    file.write_all(log_line.as_bytes()).await.unwrap();
    println!("HONEYPOT | ip={peer} | user={user:?} | pass={pass:?}");

    // reject
    let _ = stream.write_all(b"Authentication failed\n\n").await;
}

async fn read_line(stream: &mut TcpStream, buf: &mut Vec<u8>) -> Option<String> {
    let mut byte = [0u8; 1];
    loop {
        match stream.read(&mut byte).await {
            Ok(1) => {
                if byte[0] == b'\n' {
                    break;
                }
                buf.push(byte[0]);
            }
            _ => return None,
        }
    }
    Some(String::from_utf8_lossy(buf).trim().to_string())
}
