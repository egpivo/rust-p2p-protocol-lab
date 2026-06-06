# Phase 4: Port Scanner — Learnings

## What We Built

A concurrent TCP port scanner with banner grabbing and CIDR support.

```bash
# Single IP
cargo run -p port-scanner -- 127.0.0.1 1 1024

# CIDR range
cargo run -p port-scanner -- 192.168.1.0/24 22 22
```

Sample output:
```
22/tcp open  SSH-2.0-OpenSSH_9.6
88/tcp open
445/tcp open
631/tcp open
```

---

## TCP Connect Scan

The most basic scan type — attempt a full TCP 3-way handshake:

```
Client → SYN       → Server
Client ← SYN-ACK  ← Server   → port OPEN
Client → ACK       → Server

Client → SYN       → Server
Client ← RST-ACK  ← Server   → port CLOSED (instant)

Client → SYN       → ...      → port FILTERED (timeout, firewall blocked)
```

- Open: `TcpStream::connect` succeeds
- Closed: connection refused immediately (fast)
- Filtered: silence until timeout (slow — 500ms per port)

This is what Wireshark would show if you captured the traffic.

---

## Sequential vs Concurrent

| Version | 1024 ports on localhost |
|---------|------------------------|
| Sequential | ~0.7s (all closed = instant RST) |
| Concurrent (JoinSet) | ~2.3s (with banner grabbing) |
| Against remote with filtered ports | Sequential = minutes, Concurrent = seconds |

Sequential worst case: `n_ports × timeout_ms` — for 65535 ports at 500ms = **9 hours**.
Concurrent: all ports fire simultaneously, limited by OS connection limits.

### Semaphore (concurrency limiter)

Spawning 65535 tasks at once hits OS limits and causes unexpected timeouts.
Fix: `tokio::sync::Semaphore` caps simultaneous connections:

```rust
let sem = Arc::new(Semaphore::new(100));
// each task acquires a permit before connecting
```

---

## Banner Grabbing

After connecting to an open port, read the first bytes the service sends.
Many services announce themselves immediately:

| Port | Service | Example Banner |
|------|---------|---------------|
| 22 | SSH | `SSH-2.0-OpenSSH_9.6` |
| 21 | FTP | `220 FTP server ready` |
| 25 | SMTP | `220 mail.example.com ESMTP` |
| 80/443 | HTTP/HTTPS | (silent — need to send a request first) |
| 3306 | MySQL | (silent) |

**Not all services send banners.** Services that wait for a client request
(HTTP, databases) require sending a probe packet first — that's what
`nmap -sV` does for deep service detection.

### Security implications of banners

```
SSH-2.0-OpenSSH_9.6
```

An attacker uses this to:
1. Look up the version in CVE databases
2. Find matching exploits
3. Target the attack

**Defensive countermeasures:**
- Hide or modify the banner (e.g. SSH `VersionAddendum none` in `sshd_config`)
- Keep software updated (no known CVEs for the version)
- Use `fail2ban` to detect and block scanning IPs

---

## Port 22 — Why Attackers Target It

Port 22 = SSH (Secure Shell) — remote login to a server.

Attacker workflow after finding open port 22:
1. Grab banner → get SSH version
2. Check CVEs for that version
3. Try brute force (common passwords: admin, root, 123456)
4. Or exploit known vulnerabilities

**Defensive measures for SSH:**
- Disable root login (`PermitRootLogin no`)
- Use key-based auth, disable password auth
- Change default port (limited value — scanners check all ports anyway)
- `fail2ban` — auto-block IPs with repeated failed logins
- Firewall whitelist — only allow known IPs to reach port 22

---

## CIDR Notation

Classless Inter-Domain Routing — a way to express a range of IPs:

```
192.168.1.0/24
           ^^
           bits that are FIXED (network portion)
           remaining bits vary (host portion)
```

| CIDR | Hosts | Use case |
|------|-------|---------|
| `/32` | 1 | Single IP |
| `/24` | 254 | Small office LAN |
| `/16` | 65534 | Large enterprise |
| `/8` | 16M | ISP-scale |

`ipnet` crate handles parsing and host iteration:
```rust
let net: IpNet = "192.168.1.0/24".parse().unwrap();
net.hosts()  // iterator over 192.168.1.1 .. 192.168.1.254
```

Always use `/24` or smaller for local testing. `/8` = 16M IPs = hours/days to scan.

---

## Reconnaissance — The Attacker's First Step

Port scanning is **Reconnaissance** — gathering information before attacking.

What open ports reveal:
| Port | Implication |
|------|-------------|
| 22 open | SSH server — try credential attacks |
| 80/443 open | Web server — look for web vulnerabilities |
| 3306 open | MySQL exposed — often misconfigured, try default creds |
| 6379 open | Redis — unauthenticated by default in older versions |

**Defenders detect reconnaissance by:**
- IDS (Intrusion Detection System) — flags many connection attempts from one IP
- Honeypots — fake open ports that alert when touched
- Rate limiting — slow down scanners

---

## Your Scanner vs nmap

| Feature | Your scanner | nmap |
|---------|-------------|------|
| TCP Connect scan | ✅ | ✅ |
| Concurrent scanning | ✅ | ✅ |
| Banner grabbing | ✅ basic | ✅ deep (probe packets) |
| SYN scan (half-open) | ❌ | ✅ (needs root) |
| OS fingerprinting | ❌ | ✅ |
| CIDR support | ✅ | ✅ |

SYN scan uses raw sockets (directly crafts IP packets, no full handshake).
Harder to detect, requires root/admin. Uses `unsafe` in Rust — learn later.
