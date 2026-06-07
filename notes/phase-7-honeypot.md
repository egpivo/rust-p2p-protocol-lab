# Phase 7: Honeypot

## What We Built

A fake SSH server (honeypot) that lures attackers into entering credentials, logs everything, and always rejects them.

```bash
cargo run -p honeypot
```

Sample log output:
```
2026-06-07 10:17:56 | ip=127.0.0.1:62496 | user="root" | pass="123456"
```

---

## How It Works

```
Attacker connects to port 2222
      ↓
Honeypot sends fake SSH banner: "SSH-2.0-OpenSSH_9.6"
      ↓
Honeypot prompts: "login: "
      ↓
Attacker types username → logged
      ↓
Honeypot prompts: "Password: "
      ↓
Attacker types password → logged
      ↓
Honeypot always replies: "Authentication failed."
      ↓
Credentials written to honeypot.log
```

The attacker believes they found a real SSH server. They reveal their credentials — common passwords, attack patterns, tools — without gaining any access.

---

## Key Code

### Fake Protocol Simulation

```rust
async fn handle(mut stream: TcpStream, peer: SocketAddr) {
    stream.write_all(b"SSH-2.0-OpenSSH_9.6\r\n").await?;
    stream.write_all(b"login: ").await?;
    let user = read_line(&mut stream).await?;
    stream.write_all(b"Password: ").await?;
    let pass = read_line(&mut stream).await?;
    // log user + pass
    stream.write_all(b"Authentication failed.\n").await?;
}
```

### Reading One Line at a Time

```rust
async fn read_line(stream: &mut TcpStream, buf: &mut Vec<u8>) -> Option<String> {
    let mut byte = [0u8; 1];
    loop {
        match stream.read(&mut byte).await {
            Ok(1) => {
                if byte[0] == b'\n' { break; }
                buf.push(byte[0]);
            }
            _ => return None,
        }
    }
    Some(String::from_utf8_lossy(buf).trim().to_string())
}
```

Reads byte by byte until `\n` — gives us a clean string instead of raw bytes.

### Persistent Logging

```rust
let mut file = OpenOptions::new()
    .create(true)   // create if not exists
    .append(true)   // don't overwrite, add to end
    .open("honeypot.log")
    .await?;
file.write_all(log_line.as_bytes()).await?;
```

`append(true)` is important — without it, each connection would overwrite the log.

---

## IDS vs Honeypot

| | IDS | Honeypot |
|--|-----|----------|
| Strategy | Passive — watches real traffic | Active — attracts attackers |
| Trigger | Attacker touches real services | Attacker touches fake service |
| Data collected | Connection patterns | Credentials, tools, behavior |
| False positives | Possible (legitimate tools) | Almost none (nobody should connect) |

IDS says: "someone suspicious is knocking on the door."
Honeypot says: "come in — I'll record everything you do."

---

## Real-world Honeypots

| Tool | Description |
|------|-------------|
| **Cowrie** | Full SSH/Telnet honeypot — logs keystrokes, file transfers |
| **Kippo** | SSH honeypot, predecessor to Cowrie |
| **OpenCanary** | Multi-service honeypot (SSH, HTTP, FTP, SMB) |
| **T-Pot** | Full honeypot platform with dashboards |

Your implementation is the same concept as Cowrie, just simpler.

---

## Limitations

- **nc only** — real SSH clients send binary handshake data, not plain text login prompts. Our honeypot only works with raw TCP clients like `nc`.
- **No persistence across restarts** — log survives, but in-memory state is lost.
- **Port 22 requires root** — running on 2222 instead of 22 means fewer accidental connections.
- **Single prompt** — real SSH honeypots (Cowrie) fake the full SSH protocol and even simulate a shell session.

To catch real SSH client attacks, you'd need to implement the SSH wire protocol — much more complex.

---

## Blockchain Relevance

Blockchain nodes communicate over known ports (Bitcoin: 8333, Ethereum: 30303).

A honeypot on these ports can:
1. Detect Eclipse Attack probing — attacker repeatedly connects to map network topology
2. Collect attacker's node ID / public key (revealed during handshake)
3. Identify scanning tools by their connection patterns
4. Alert before the real node is targeted

Same principle: fake a peer node, let the attacker reveal themselves.

---

## Testing

```bash
# Terminal A
cargo run -p honeypot

# Terminal B — simulate attacker
nc 127.0.0.1 2222
# type username, then password

# Check log
cat honeypot.log
```

---

## Concepts Learned

### Fake Protocol Simulation
A honeypot doesn't need to implement the real protocol — just enough to fool automated tools and curious attackers. Send the right banner, prompt for input, collect data.

### Read Line from Raw TCP
TCP gives you a stream of bytes with no message boundaries. To read "one line," you read byte by byte and stop at `\n`. This is the same pattern used in HTTP, SMTP, and other text-based protocols.

### `append(true)` for Log Files
Always use `append` mode for log files — `write` mode truncates the file on each open, destroying previous records.

### `chrono` for Human-readable Timestamps
`Instant` (used in IDS) measures elapsed time — good for intervals, not for logs.
`chrono::Local::now()` gives wall clock time — good for log entries humans will read.
