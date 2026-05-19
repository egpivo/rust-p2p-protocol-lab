# Phase 4: Port Scanner

## Goal

Build a port scanner from scratch in Rust — the same concept as `nmap`.
**Only use against systems you own or have explicit permission to scan.**

---

## Phase 1: Single IP, Port Range Scan (TCP Connect Scan)

### Concepts
- TCP Connect Scan: attempt `TcpStream::connect(ip:port)`
  - connection succeeds → port is **open**
  - connection refused / timeout → port is **closed** or **filtered**
- This is the most basic scan type — no raw sockets needed

### Steps

1. Create the crate:
   ```bash
   cargo new crates/port-scanner --name port-scanner
   ```
   Add to workspace `Cargo.toml`:
   ```toml
   members = [..., "crates/port-scanner"]
   ```

2. Add dependencies in `crates/port-scanner/Cargo.toml`:
   ```toml
   tokio = { version = "1", features = ["full"] }
   ```

3. Read args: `ip`, `start_port`, `end_port`
   ```
   cargo run -p port-scanner -- 127.0.0.1 1 1024
   ```

4. Loop over port range, try `TcpStream::connect` with a timeout:
   ```rust
   tokio::time::timeout(
       Duration::from_millis(500),
       TcpStream::connect((ip, port))
   ).await
   ```
   - `Ok(Ok(_))` → open
   - `Ok(Err(_))` → closed
   - `Err(_)` → timeout (filtered)

5. Print open ports only.

---

## Phase 2: Concurrent Scanning

### Concepts
- Sequential scan of 1024 ports × 500ms timeout = 512 seconds — too slow
- `tokio::spawn` one task per port → all run in parallel
- `JoinSet` collects results

### Steps

1. Add `tokio::task::JoinSet`
2. Spawn one task per port inside the range loop
3. Collect results and print open ports sorted

### Key idea
```rust
let mut set = JoinSet::new();
for port in start..=end {
    set.spawn(scan_port(ip, port));
}
while let Some(result) = set.join_next().await {
    // handle result
}
```

---

## Phase 3: Banner Grabbing

### Concepts
- After connecting to an open port, read the first bytes the service sends
- Many services announce themselves: SSH, FTP, SMTP, HTTP send a banner
- Banner → identify service and version without knowing the port number

### Steps

1. After successful connect, try to read up to 256 bytes with a short timeout
2. Print as UTF-8 if valid, otherwise print hex
3. Example banners:
   - Port 22: `SSH-2.0-OpenSSH_8.9`
   - Port 21: `220 FTP server ready`
   - Port 25: `220 mail.example.com ESMTP`

---

## Phase 4: CIDR Range Support

### Concepts
- CIDR notation: `192.168.1.0/24` means scan all 256 IPs in that subnet
- `/24` = last octet varies (0–255)
- `/16` = last two octets vary (65535 IPs)

### Steps

1. Add `ipnet` crate:
   ```toml
   ipnet = "2"
   ```
2. Parse CIDR input: `IpNet::from_str("192.168.1.0/24")`
3. Iterate over all hosts: `network.hosts()`
4. For each host, run the concurrent port scan

---

## Security Concepts Behind Port Scanning

| Scan Type | How | Detectable? |
|-----------|-----|-------------|
| TCP Connect | Full 3-way handshake | Yes — shows in server logs |
| SYN scan (half-open) | Send SYN, don't complete handshake | Harder to detect |
| UDP scan | Send UDP packet, wait for ICMP | Slow, unreliable |

We implement TCP Connect — the safest and most straightforward.
SYN scan requires raw sockets (need root + unsafe), learn that later.

### What open ports tell an attacker
- Port 22 open → SSH server, try credential attacks
- Port 80/443 → web server, look for vulnerabilities
- Port 3306 → MySQL exposed to network (misconfiguration)
- Port 6379 → Redis, often unauthenticated by default

### What defenders do
- Close unused ports (firewall rules)
- Change default ports (security by obscurity — limited value)
- Detect port scans via IDS (many connection attempts = scan pattern)
- Rate limiting on connection attempts

---

## Test Environment Setup (Legal & Safe)

```bash
# Scan your own machine
cargo run -p port-scanner -- 127.0.0.1 1 1024

# Spin up a local Docker container to scan
docker run -d -p 8080:80 nginx
cargo run -p port-scanner -- 127.0.0.1 8080 8080
```

For more realistic practice: HackTheBox, TryHackMe — legal target machines.
