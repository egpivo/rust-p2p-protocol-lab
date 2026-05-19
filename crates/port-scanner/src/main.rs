use std::net::IpAddr;
use std::str::FromStr;
use std::time::Duration;
use tokio::net::TcpStream;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let ip = IpAddr::from_str(&args[1]).expect("Invalid IP address");
    let start_port: u16 = args[2].parse().expect("Invalid start port");
    let end_port: u16 = args[3].parse().expect("Invalid end port");

    for port in start_port..=end_port {
        let result = tokio::time::timeout(
            Duration::from_millis(500),
            TcpStream::connect((ip, port)),
        ).await;

        match result {
            Ok(Ok(_)) => println!("Port {port}/tcp is open"),
            Ok(Err(_)) => {},//println!("Port {port}/tcp is closed"),
            Err(_) => {}//println!("Port {port}/tcp is filtered"),
        }
    }
}