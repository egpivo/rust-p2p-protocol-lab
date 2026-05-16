use tokio::io::{AsyncReadExt,AsyncWriteExt};
use tokio::net::TcpStream;

fn encode_frame(payload: &[u8]) -> Vec<u8> {
    let len = payload.len() as u32;
    let mut out = Vec::with_capacity(4 + len as usize);
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(payload);
    out
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let mut stream = TcpStream::connect("127.0.0.1:7002").await?;
    let frame = encode_frame(b"hello");
    stream.write_all(&frame).await?;

    let mut len_buf = [0u8; 4];
    println!("sent frame, waiting for echo length ...");
    stream.read_exact(&mut len_buf).await?;
    let n = u32::from_be_bytes(len_buf);
    let mut body = vec![0u8; n as usize];
    println!("got length {n}");
    stream.read_exact(&mut body).await?;
    println!("echo: {}", String::from_utf8_lossy(&body));
    Ok(())
}