use tokio::io::{AsyncReadExt,AsyncWriteExt};
use tokio::net::TcpStream;
use framed_protocol::{MAX_PAYLOAD, encode_frame};

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let mut stream = TcpStream::connect("127.0.0.1:7002").await?;
    let frame = encode_frame(b"hello");
    stream.write_all(&frame).await?;

    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let n = u32::from_be_bytes(len_buf);

    if n > MAX_PAYLOAD {
        eprintln!("frame too large: {n} bytes (max {MAX_PAYLOAD})");
        return Ok(());
    }
    let mut body = vec![0u8; n as usize];

    stream.read_exact(&mut body).await?;
    println!("echo: {}", String::from_utf8_lossy(&body));
    Ok(())
}