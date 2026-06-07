# Phase 6: IDS (Intrusion Detection System)

## What We Built

A basic IDS that monitors network connections and alerts on suspicious behavior.

```bash
cargo run -p ids
```

Detects:
1. **Port scan** — same IP connects to 3+ different ports within 30 seconds
2. **Brute force** — same IP connects to the same port 5+ times within 30 seconds

---

## Architecture

```
TcpListener (per port)
      ↓
each connection → push to Records (Arc<Mutex<Vec<ConnectionRecord>>>)
      ↓
Analyzer task (every 5 seconds)
      ↓
check rules → ALERT
```

---

## Key Data Structures

```rust
struct ConnectionRecord {
    ip: IpAddr,
    port: u16,
    timestamp: Instant,   // when the connection happened
}

type Records = Arc<Mutex<Vec<ConnectionRecord>>>;
```

`Arc<Mutex<...>>` — multiple tasks (one per port) write to the same Vec.
- `Arc` — shared ownership across tasks
- `Mutex` — only one task writes at a time

---

## Port < 1024 Requires Root

Unix/macOS restricts binding to ports below 1024 to root only.
Port 22 (SSH), 80 (HTTP), 443 (HTTPS) → `Permission denied` without sudo.

Workaround for development: use high ports instead:
- 22 → 2222
- 80 → 8080
- 443 → 8443

Real IDS tools run as root or use kernel hooks (not just `bind`).

---

## Detection Rules

### Port Scan Detection

```rust
// Group by IP, count distinct ports in last 30 seconds
let mut ip_ports: HashMap<IpAddr, HashSet<u16>> = HashMap::new();
for r in recent {
    ip_ports.entry(r.ip).or_default().insert(r.port);
}
// 3+ different ports from same IP = port scan
if ports.len() >= 3 { ALERT }
```

### Brute Force Detection

```rust
// Group by (IP, port), count connection attempts
let mut ip_port_count: HashMap<(IpAddr, u16), usize> = HashMap::new();
for r in recent {
    *ip_port_count.entry((r.ip, r.port)).or_default() += 1;
}
// 5+ attempts to same port from same IP = brute force
if count >= 5 { ALERT }
```

### Deduplication

Without deduplication, analyzer fires every 5 seconds for the same event.
Fix: `HashSet` of already-alerted IPs/ports:

```rust
let mut alerted_scan: HashSet<IpAddr> = HashSet::new();
let mut alerted_brute: HashSet<(IpAddr, u16)> = HashSet::new();

if condition && !alerted.contains(&ip) {
    alerted.insert(ip);
    println!("ALERT: ...");
}
```

---

## Testing

```bash
# Port scan detection
cargo run -p port-scanner -- 127.0.0.1 1 9000

# Brute force detection
for i in {1..10}; do nc -z 127.0.0.1 3306; done
```

---

## IDS vs IPS

| | IDS | IPS |
|--|-----|-----|
| Full name | Intrusion Detection System | Intrusion Prevention System |
| Action | Detects and alerts | Detects and blocks |
| Position | Passive (monitors) | Active (inline) |

Your implementation is IDS — it logs alerts but doesn't block.
To make it IPS, you'd add firewall rules (e.g. `iptables`) when an alert fires.

---

## Limitations of This IDS

- **False positives** — legitimate tools (health checks, monitoring) can trigger alerts
- **Evasion** — attacker can scan slowly (1 port per minute) to stay below threshold
- **No persistence** — records lost when process restarts
- **localhost only** — needs to bind on `0.0.0.0` to monitor real network traffic
- **Port < 1024** — needs root to monitor standard service ports

Real IDS tools (Snort, Suricata) work at the kernel/packet level, not application level.

---

## Blockchain Relevance

Bitcoin node default port: **8333**
Ethereum node default port: **30303**

An attacker preparing an Eclipse Attack would:
1. Scan for nodes running on these ports
2. Connect repeatedly to map the network topology
3. Gradually replace all peer connections with malicious nodes

Your IDS would detect step 1 (port scan) and step 2 (repeated connections).

---

## Concepts Learned

### `Arc<Mutex<T>>` — Shared Mutable State

Multiple async tasks need to write to the same data:

```rust
// Won't compile — can't share mutable reference across tasks
let mut records = Vec::new();
tokio::spawn(async { records.push(...) });  // ← can't move into two tasks

// Solution: Arc<Mutex<T>>
let records = Arc::new(Mutex::new(Vec::new()));
let r = records.clone();  // clone the Arc (not the data)
tokio::spawn(async move { r.lock().unwrap().push(...) });
```

- `Arc::clone()` — cheap (just increments a counter)
- `Mutex::lock()` — blocks until no other task holds the lock
- Lock auto-releases when the `MutexGuard` goes out of scope

### `ctrl_c()` — Keep the Process Alive

`tokio::spawn` runs tasks in the background. Without waiting, main exits
immediately and kills all tasks.

```rust
for port in ports {
    tokio::spawn(...)  // background
}
tokio::signal::ctrl_c().await.unwrap();  // wait here until Ctrl+C
```
