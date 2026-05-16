use tokio::io::AsyncWriteExt;
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
    Ok(())
}