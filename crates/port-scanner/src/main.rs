use std::net::IpAddr;
use std::str::FromStr;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::task::JoinSet;
use tokio::io::AsyncReadExt;


#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let ip = IpAddr::from_str(&args[1]).expect("Invalid IP address");
    let start_port: u16 = args[2].parse().expect("Invalid start port");
    let end_port: u16 = args[3].parse().expect("Invalid end port");

    let mut set = JoinSet::new();

    for port in start_port..=end_port {
        set.spawn(async move {
            let result = tokio::time::timeout(
                Duration::from_millis(500),
                TcpStream::connect((ip, port)),
            ).await;
            match result {
                Ok(Ok(mut stream)) => {
                    let mut buf = vec![0u8; 256];
                    let banner = match tokio::time::timeout(
                        Duration::from_millis(500),
                        stream.read(&mut buf),
                    ).await {
                        Ok(Ok(n)) if n > 0 => Some(String::from_utf8_lossy(&buf[..n]).trim().to_string()),
                        _ => None,
                    };
                    (port, true, banner)
                }
                _ => (port, false, None),
            }
        });
    }
    while  let Some(res) = set.join_next().await {
        if let Ok((port, true, banner)) = res {
            match banner {
                Some(b) => println!("{port}/tcp open {b}"),
                None => println!("{port}/tcp open"),
            }
        }
    }
}