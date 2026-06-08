use ipnet::IpNet;
use std::net::IpAddr;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let start_port: u16 = args[2].parse().expect("Invalid start port");
    let end_port: u16 = args[3].parse().expect("Invalid end port");
    let target = &args[1];
    let ips: Vec<IpAddr> = if target.contains('/') {
        // CIDR
        let net: IpNet = target.parse().expect("invalid CIDR");
        net.hosts().collect()
    } else {
        // Single IP
        vec![IpAddr::from_str(target).expect("invalid IP")]
    };

    let sem = Arc::new(Semaphore::new(100));

    for ip in ips {
        let mut set = JoinSet::new();

        for port in start_port..=end_port {
            let sem = sem.clone();
            set.spawn(async move {
                let _permit = sem.acquire().await.unwrap();
                let result = tokio::time::timeout(
                    Duration::from_millis(500),
                    TcpStream::connect((ip, port)),
                )
                .await;
                match result {
                    Ok(Ok(mut stream)) => {
                        let mut buf = vec![0u8; 256];
                        let mut banner = match tokio::time::timeout(
                            Duration::from_millis(500),
                            stream.read(&mut buf),
                        )
                        .await
                        {
                            Ok(Ok(n)) if n > 0 => Some(
                                String::from_utf8_lossy(&buf[..n])
                                    .lines()
                                    .next()
                                    .unwrap_or("")
                                    .trim()
                                    .to_string(),
                            ),
                            _ => None,
                        };
                        if banner.is_none() {
                            let probe = b"GET / HTTP/1.0\r\n\r\n";
                            if stream.write_all(probe).await.is_ok() {
                                let mut buf2 = vec![0u8; 256];
                                banner = match tokio::time::timeout(
                                    Duration::from_millis(500),
                                    stream.read(&mut buf2),
                                )
                                .await
                                {
                                    Ok(Ok(n)) if n > 0 => Some(
                                        String::from_utf8_lossy(&buf2[..n])
                                            .lines()
                                            .next()
                                            .unwrap_or("")
                                            .trim()
                                            .to_string(),
                                    ),
                                    _ => None,
                                };
                            }
                        }
                        (port, true, banner)
                    }
                    _ => (port, false, None),
                }
            });
        }
        while let Some(res) = set.join_next().await {
            if let Ok((port, true, banner)) = res {
                match banner {
                    Some(b) => println!("{port}/tcp open {b}"),
                    None => println!("{port}/tcp open"),
                }
            }
        }
    }
}
