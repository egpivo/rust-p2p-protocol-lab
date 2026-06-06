use tokio::net::{TcpListener, TcpStream};
use tokio::io;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use std::env;

#[tokio::main]
async fn main() -> io::Result<()> {
    let args: Vec<String> = env::args().collect();
    // args[1] = listen port (e.g., 8888)
    // args[2] = target (e.g., 127.0.0.1:7004)
    let listen_addr = format!("127.0.0.1:{}", &args[1]);
    let target_addr = args[2].clone();

    let listener = TcpListener::bind(&listen_addr).await?;
    println!("Proxy listening on {listen_addr} → {target_addr}");

    loop {
        let (client_stream, client_addr) = listener.accept().await?;
        println!("connection from {client_addr}");

        let target = target_addr.clone();
        tokio::spawn(async move {
            if let Err(e) = handle(client_stream, target).await {
                eprintln!("error: {e}");
            }
        });
    }
}

async fn handle(mut client: TcpStream, target: String) -> io::Result<()> {
    let mut server = TcpStream::connect(&target).await?;
    println!("connected to {target}");

    let (mut cr, mut cw) = client.into_split();
    let (mut sr, mut sw) = server.into_split();

    let c2s = tokio::spawn(async move {
        let mut buf = vec![0u8; 4096];
        loop {
            let n = cr.read(&mut buf).await?;
            if n == 0 { break; }
            println!("→ server: {:?}", String::from_utf8_lossy(&buf[..n]));

            let data = String::from_utf8_lossy(&buf[..n]);
            let modified = data.replace("hello", "HACKED");
            println!("→ server (modified): {:?}", modified);
            sw.write_all(&buf[..n]).await?;
        }
        Ok::<_, io::Error>(())
    });
    
    let s2c = tokio::spawn(async move {
        let mut buf = vec![0u8; 4096];
        loop {
            let n = sr.read(&mut buf).await?;
            if n == 0 { break; }
            println!("← client: {:?}", String::from_utf8_lossy(&buf[..n]));
            cw.write_all(&buf[..n]).await?;
        }
        Ok::<_, io::Error>(())
    });
    let _ = tokio::join!(c2s, s2c);
    Ok(())
}

