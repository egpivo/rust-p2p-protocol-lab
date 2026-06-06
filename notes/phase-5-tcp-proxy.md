# Phase 5: TCP Proxy

## What We Built

A transparent TCP proxy that sits between client and server, forwarding all traffic and logging it.

```bash
# Plain TCP (can see content)
cargo run -p tcp-chat                          # Terminal A - server port 7001
cargo run -p tcp-proxy -- 8888 127.0.0.1:7001 # Terminal B - proxy
nc 127.0.0.1 8888                             # Terminal C - client

# TLS (sees encrypted bytes only)
cargo run -p framed-chat --bin framed-chat     # Terminal A - server port 7004
cargo run -p tcp-proxy -- 8888 127.0.0.1:7004 # Terminal B - proxy
cargo run -p framed-chat --bin client -- alice # Terminal C - client (connect to 8888)
```

---

## How It Works

```
Normal connection:
Client ──────────────────────→ Server

Through proxy:
Client → Proxy → Server
          ↑
     sees all traffic, can log or modify
```

The proxy:
1. Listens on a port (e.g. 8888)
2. Client connects thinking it's the server
3. Proxy connects to the real server
4. Proxy copies bytes in both directions
5. Logs everything it sees

---

## Key Concept: `copy_bidirectional`

The core of the proxy — copies bytes between two streams simultaneously:

```rust
io::copy_bidirectional(&mut client, &mut server).await?
```

Why not `select!`? TLS handshake needs multiple round trips. `select!` drops
one direction when the other finishes — this breaks the handshake midway.
`copy_bidirectional` keeps both directions alive until both are done.

For logging, split into two tasks manually:

```rust
let (mut cr, mut cw) = client.into_split();
let (mut sr, mut sw) = server.into_split();

// client → server task (logs what client sends)
tokio::spawn(async move {
    loop {
        let n = cr.read(&mut buf).await?;
        println!("→ server: {:?}", String::from_utf8_lossy(&buf[..n]));
        sw.write_all(&buf[..n]).await?;
    }
});

// server → client task (logs what server sends)
tokio::spawn(async move { ... });
```

---

## TLS vs Plain TCP — What the Proxy Sees

**Plain TCP (no TLS):**
```
→ server: "hello\n"          ← readable plaintext
← client: "127.0.0.1: hello" ← readable plaintext
```

**With TLS:**
```
→ server: "\u{16}\u{3}\u{1}..."  ← TLS ClientHello (handshake)
← client: "\u{16}\u{3}\u{3}..."  ← TLS ServerHello + certificate
→ server: "\u{14}\u{3}\u{3}..."  ← TLS Finished
← client: "\u{17}\u{3}\u{3}..."  ← encrypted application data (unreadable)
```

TLS record types (first byte):
| Byte | Meaning |
|------|---------|
| `0x16` (22) | Handshake |
| `0x17` (23) | Application Data (encrypted) |
| `0x14` (20) | Change Cipher Spec |
| `0x15` (21) | Alert |

The proxy can see the handshake structure but cannot read the application data.
This is exactly why HTTPS (HTTP + TLS) protects you — even a MITM proxy only sees ciphertext.

---

## Man-in-the-Middle (MITM) Attack

This proxy demonstrates the basic MITM principle:

```
Without MITM protection:
You type "password" → Proxy sees "password" → Server receives "password"

With TLS:
You type "password" → TLS encrypts → Proxy sees "Xk9#mQ2..." → Server decrypts → "password"
```

**Real-world MITM tools use this exact architecture:**
- **Burp Suite** — intercepts HTTP/HTTPS traffic for web app security testing
- **mitmproxy** — scriptable MITM proxy for security research
- **Charles Proxy** — used by developers to debug mobile app traffic

The difference between a proxy and a MITM attack is **authorization** — you are
allowed to intercept traffic on your own systems and in authorized testing.

---

## TLS Termination Proxy (Advanced)

To read TLS-encrypted content, a proxy needs to do TLS termination:

```
Client → [TLS] → Proxy → [TLS] → Server
                  ↑
            decrypts here,
            reads plaintext,
            re-encrypts to server
```

This requires the proxy to have its own certificate that the client trusts.
Burp Suite installs its own CA cert on your machine to do this.
This is why browsers warn you when a new CA is added — it could enable MITM.

Certificate pinning (from Phase 3) defeats TLS termination proxies:
- Client checks exact cert hash, not just "trusted CA"
- Even if proxy has a trusted CA cert, hash won't match → connection refused

---

## Why the 🔒 in Your Browser Matters

When you see the padlock:
1. TLS is active — traffic is encrypted
2. The server's certificate was verified by a trusted CA
3. A proxy between you and the server can only see encrypted bytes

When you don't see it (HTTP):
- Any proxy, router, or ISP can read everything
- Passwords, cookies, messages — all in plaintext
- This is what you demonstrated with tcp-chat + tcp-proxy

---

## `into_split()` — Why We Need It

`TcpStream` is a single object but needs to be used in two tasks simultaneously
(one reads, one writes). Rust's ownership rules prevent this.

`into_split()` splits it into independent read and write halves:

```rust
let (read_half, write_half) = stream.into_split();
// now each half can be moved into a separate task
```

This is the same pattern you'll see in any bidirectional async IO code.
