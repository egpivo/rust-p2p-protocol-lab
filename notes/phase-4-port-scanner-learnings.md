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

---

## Semaphore — Concurrency Limiter

A semaphore is a **counter** that limits how many tasks run simultaneously.

Real-world analogy: a parking lot with 100 spaces.
- Car enters → spaces -1 (`acquire`)
- Car leaves → spaces +1 (`release`)
- Lot full → next car waits
- A space opens → waiting car enters

```rust
let sem = Arc::new(Semaphore::new(100));

set.spawn(async move {
    let _permit = sem.acquire().await.unwrap();
    // scan happens here
    // _permit auto-releases when it goes out of scope
});
```

**Why needed:** spawning 1024 tasks at once overwhelms OS connection limits.
Some connections queue past the timeout → wrong results + slower overall.
Semaphore caps at 100 simultaneous connections → stable and predictable.

**Semaphore vs Mutex:**

| | Mutex | Semaphore |
|--|-------|-----------|
| Count | 1 (only one at a time) | N (up to N at a time) |
| Use case | Protect shared data | Limit concurrency |

Mutex = parking lot with 1 space.
Semaphore = parking lot with N spaces.

---

## HTTP Probe

Some services don't send a banner — they wait for the client to speak first.
HTTP is the most common example. After connecting, send a probe request:

```
GET / HTTP/1.0\r\n\r\n
```

The server responds with its status line:
```
HTTP/1.0 200 OK
HTTP/1.1 403 Forbidden
HTTP/1.1 302 Found
```

We only take the first line — enough to identify the service and response code.

**Response codes tell you a lot:**
| Code | Meaning | Security implication |
|------|---------|---------------------|
| 200 OK | Page exists, accessible | Normal web server |
| 403 Forbidden | Server exists, access denied | Still confirms service is running |
| 302/307 Redirect | Redirecting elsewhere | Often HTTP → HTTPS redirect |
| 400 Bad Request | Server doesn't speak HTTP | Wrong protocol, try another probe |

Even a 403 is useful — it confirms a web server is running on that port.

---

## TLS Certificate Fingerprinting

Connecting to HTTPS exposes the server's TLS certificate — even without logging in.
The certificate reveals:

```bash
curl -k -v https://<ip> 2>&1 | grep -i "subject\|issuer\|CN"
```

Example output:
```
subject: CN=SN-382079000317; O=Ruckus Wireless Inc.; L=Sunnyvale; ST=California; C=US
issuer:  CN=RuckusPKI-DeviceSubCA-1; O=Ruckus Wireless Inc.
```

What this tells an attacker:
- **Brand**: Ruckus Wireless → look up Ruckus CVEs
- **Serial number**: can identify exact model
- **Cert age**: issued 2020, expires 2045 → device is ~5 years old
- **TLS version**: TLSv1.2 → older, may have known weaknesses

This is passive reconnaissance — no login needed, just reading the handshake.

---

## Network Layers (OSI simplified)

```
Application layer  → your data (HTTP, SSH, framed-chat)
Transport layer    → TCP/UDP — port-to-port delivery, reliability
Network layer      → IP — routing between machines (IP address lives here)
Link layer         → WiFi/Ethernet — physical transmission
```

**TCP responsibilities:**
1. Port addressing — finds the right service on the machine
2. Reliable delivery — guarantees packets arrive complete, in order, no duplicates

**TCP vs UDP:**

| | TCP | UDP |
|--|-----|-----|
| Reliable | ✅ retransmits lost packets | ❌ fire and forget |
| Ordered | ✅ | ❌ |
| Speed | slower (overhead) | faster |
| Use cases | HTTP, SSH, chat | video streaming, DNS, games |

**Where TLS fits:**
```
your data → TLS encrypts → TCP → IP → WiFi
```
WiFi only sees encrypted bytes — can't read the content.
WPA2/WPA3 encrypts the WiFi signal itself (different layer from TLS).

---

## Checking Your Own Machine

Useful commands to audit your own machine's network state:

```bash
# What ports are listening (waiting for connections)
netstat -an | grep LISTEN

# All active connections
netstat -an | grep ESTABLISHED

# Which process owns each connection
lsof -i -n -P | grep ESTABLISHED

# Who is using a specific port
lsof -i :22
lsof -i :7070
```

**Key distinction:**
- `LISTEN` = your machine is waiting for incoming connections (you are the server)
- `ESTABLISHED` = an active connection exists (could be inbound or outbound)

**Inbound vs outbound:**
```
Outbound (you initiated): your_ip.HIGH_PORT → remote_ip.443   (normal — browser, app)
Inbound  (they initiated): remote_ip.HIGH_PORT → your_ip.PORT  (investigate this)
```

Almost all legitimate connections are outbound on port 443 (HTTPS).
Investigate anything that looks like an external IP connecting TO your machine.

---

## Real-world Recon Workflow (Attacker's Perspective)

Understanding this helps you think like a defender:

```
Step 1: Discover hosts
  → ping sweep or CIDR scan to find live machines

Step 2: Port scan each host
  → TCP connect scan on common ports first (1-1024)
  → Then full range (1-65535) if needed

Step 3: Service identification
  → Banner grabbing (passive — read what the service sends)
  → Probe packets (active — send requests, read responses)
  → TLS cert fingerprinting (passive — read the handshake)

Step 4: Version detection
  → SSH-2.0-OpenSSH_9.6 → check CVE database
  → HTTP/1.1 → try common web vulnerabilities

Step 5: Exploitation
  → Use found version vulnerabilities
  → Try default credentials
  → Brute force if no lockout policy
```

Your scanner currently covers Steps 2, 3, and part of 4.

---

## Ports Worth Knowing

| Port | Service | Common risk |
|------|---------|-------------|
| 21 | FTP | Often allows anonymous login, plaintext passwords |
| 22 | SSH | Brute force target, version exploits |
| 23 | Telnet | Completely unencrypted, almost never safe to expose |
| 25 | SMTP | Open relay misconfiguration → spam server |
| 53 | DNS | DNS amplification attacks if misconfigured |
| 80/443 | HTTP/HTTPS | Web app vulnerabilities (SQLi, XSS, etc.) |
| 3306 | MySQL | Exposed DB = immediate data breach risk |
| 5432 | PostgreSQL | Same as MySQL |
| 6379 | Redis | No auth by default in older versions |
| 27017 | MongoDB | Many deployments left open with no auth |
| 3389 | RDP | Windows remote desktop, brute force target |

---

## HTTP Probe

Some services don't send a banner — they wait for the client to speak first.
HTTP is the most common example. After connecting, send a probe request:

```
GET / HTTP/1.0\r\n\r\n
```

The server responds with its status line:
```
HTTP/1.0 200 OK
HTTP/1.1 403 Forbidden
HTTP/1.1 302 Found
```

We only take the first line — enough to identify the service and response code.

**Response codes tell you a lot:**
| Code | Meaning | Security implication |
|------|---------|---------------------|
| 200 OK | Page exists, accessible | Normal web server |
| 403 Forbidden | Server exists, access denied | Still confirms service is running |
| 302/307 Redirect | Redirecting elsewhere | Often HTTP → HTTPS redirect |
| 400 Bad Request | Server doesn't speak HTTP | Wrong protocol, try another probe |

Even a 403 is useful — it confirms a web server is running on that port.

---

## Checking Your Own Machine

Useful commands to audit your own machine's network state:

```bash
# What ports are listening (waiting for connections)
netstat -an | grep LISTEN

# All active connections
netstat -an | grep ESTABLISHED

# Which process owns each connection
lsof -i -n -P | grep ESTABLISHED

# Who is using a specific port
lsof -i :22
lsof -i :7070
```

**Key distinction:**
- `LISTEN` = your machine is waiting for incoming connections (you are the server)
- `ESTABLISHED` = an active connection exists (could be inbound or outbound)

**Inbound vs outbound:**
```
Outbound (you initiated): your_ip.HIGH_PORT → remote_ip.443   (normal — browser, app)
Inbound  (they initiated): remote_ip.HIGH_PORT → your_ip.PORT  (investigate this)
```

Almost all legitimate connections are outbound on port 443 (HTTPS).
Investigate anything that looks like an external IP connecting TO your machine.

---

## Real-world Recon Workflow (Attacker's Perspective)

Understanding this helps you think like a defender:

```
Step 1: Discover hosts
  → ping sweep or CIDR scan to find live machines

Step 2: Port scan each host
  → TCP connect scan on common ports first (1-1024)
  → Then full range (1-65535) if needed

Step 3: Service identification
  → Banner grabbing (passive — read what the service sends)
  → Probe packets (active — send requests, read responses)

Step 4: Version detection
  → SSH-2.0-OpenSSH_9.6 → check CVE database
  → HTTP/1.1 → try common web vulnerabilities

Step 5: Exploitation
  → Use found version vulnerabilities
  → Try default credentials
  → Brute force if no lockout policy
```

Your scanner currently covers Steps 2 and 3.

---

## Ports Worth Knowing

| Port | Service | Common risk |
|------|---------|-------------|
| 21 | FTP | Often allows anonymous login, plaintext passwords |
| 22 | SSH | Brute force target, version exploits |
| 23 | Telnet | Completely unencrypted, almost never safe to expose |
| 25 | SMTP | Open relay misconfiguration → spam server |
| 80/443 | HTTP/HTTPS | Web app vulnerabilities (SQLi, XSS, etc.) |
| 3306 | MySQL | Exposed DB = immediate data breach risk |
| 5432 | PostgreSQL | Same as MySQL |
| 6379 | Redis | No auth by default in older versions |
| 27017 | MongoDB | Many deployments left open with no auth |
| 3389 | RDP | Windows remote desktop, brute force target |
