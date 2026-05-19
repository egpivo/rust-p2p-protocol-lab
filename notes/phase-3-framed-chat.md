# Phase 3: Framed Chat with TLS + Certificate Pinning

## What We Built

A multi-client TCP chat server with:
- Length-prefixed framing (4-byte big-endian header + body)
- Broadcast messaging via `tokio::sync::broadcast`
- TLS encryption via `tokio-rustls`
- Certificate pinning for server identity verification

## Architecture

```
Client A ──TLS──┐
Client B ──TLS──┼── TcpListener
Client C ──TLS──┘       │
                    accept loop
                         │
                    tokio::spawn (per client)
                         │
                    tokio::select!
                    ┌────┴────┐
               read_frame   rx.recv()
               (commands)  (broadcast)
```

## Protocol Commands

| Client → Server | Server → Client |
|-----------------|-----------------|
| `JOIN <name>`   | `OK`            |
| `SAY <msg>`     | `OK` / `ERR join first` |
| `PING`          | `PONG`          |
| `ACK`           | (no reply, server logs) |
| —               | `MSG <name> <msg>` (broadcast) |

## Key Concepts

### `tokio::select!` — the core pattern

`select!` races multiple async operations and runs whichever completes first.
Used in both server (read frame vs. receive broadcast) and client (read frame vs. stdin).

```rust
tokio::select! {
    body = read_frame(&mut stream) => { /* handle inbound */ }
    packet = rx.recv()            => { /* forward broadcast */ }
}
```

**Why only one `stream` reference**: both arms share the same `stream`, so
the read and write sides must live inside the same `select!` loop — never split
across separate tasks.

### Length-prefixed framing

Raw TCP is a byte stream with no message boundaries. A 4-byte length header solves this:

```
[ u32 big-endian length ][ payload bytes ]
```

`encode_frame` prepends the header. `read_frame` reads the header first,
then reads exactly that many bytes — no partial reads, no delimiter scanning.

### Broadcast channel

```rust
let (tx, _rx) = broadcast::channel::<(SocketAddr, String)>(100);
```

Each client gets its own `rx` via `tx.subscribe()`. When any client sends `SAY`,
the server broadcasts `MSG <name> <msg>` to all subscribers. Each task filters
out its own messages (`if from == client_addr { continue; }`).

### Why `display_name` needs no `Mutex`

Each client lives in its own `tokio::spawn` task. `display_name: Option<String>`
is local to that task — no shared state, no synchronization needed.
`Arc<Mutex<...>>` would only be necessary if multiple tasks read/write the same variable.

### Generic `read_frame` for TLS

TLS streams (`TlsStream<TcpStream>`) are a different type from `TcpStream`.
Making `read_frame` generic lets it work with both:

```rust
async fn read_frame<S>(stream: &mut S) -> io::Result<Vec<u8>>
where
    S: AsyncRead + AsyncWrite + Unpin,
```

## TLS

### How TLS works here

1. Server generates a self-signed cert at startup (`rcgen`)
2. Client connects → TLS handshake → server sends cert
3. Client verifies cert (custom verifier) → encrypted channel established
4. All `read_frame` / `write_all` calls happen over the encrypted stream

### Certificate Pinning

Instead of trusting a CA, the client trusts a specific cert by its hash.

```
Server startup:
  cert_der → SHA-256 → hex → write to cert.hash

Client startup:
  read cert.hash → decode hex → store as expected_hash

TLS handshake:
  received cert → SHA-256 → compare with expected_hash
  match   → Ok(ServerCertVerified::assertion())
  mismatch → Err(rustls::Error::General("cert hash mismatch"))
```

This prevents MITM: even if an attacker intercepts the connection and presents
a different cert, the hash won't match and the client rejects it.

### Current limitation

`cert.hash` is distributed via shared filesystem — only works when client and
server are on the same machine. Real-world alternatives:

| Method | Used by |
|--------|---------|
| Hardcoded in binary | Mobile apps, CLI tools |
| Trust On First Use (TOFU) | SSH (`~/.ssh/known_hosts`) |
| Out-of-band (QR code, Signal) | E2E encrypted messengers |

## ACK — Delivery Confirmation

When a client receives a `MSG` frame, it sends back `ACK`.
Server logs it. In a production system this enables:
- Guaranteed delivery (retransmit if no ACK within timeout)
- Dead client detection
- Replay attack defense (ACK with sequence number)

## Encoding: `b"OK"` vs `"OK"`

`encode_frame` takes `&[u8]`. `b"OK"` is a byte string literal with type `&[u8]`
directly — no conversion needed. Under the hood it is ASCII bytes `[79, 75]`.
`"OK".as_bytes()` produces the same result but requires an explicit call.

## u8, u16, u32 — Quick Reference

- `u` = unsigned (0 and positive only)
- The number = bit width
- `u8` = 1 byte = 0–255 — the atom of network data
- `u32` = 4 bytes = 0–4,294,967,295 — used for frame length header

## Unicode vs UTF-8

- **Unicode**: the standard assigning a number (code point) to every character
- **UTF-8**: one encoding of Unicode into bytes (1–4 bytes per character)
- ASCII is a subset of UTF-8 — bytes 0–127 are identical in both
- UTF-16 uses 2–4 bytes per character; common in Windows/Java internals
- Network protocols almost always use UTF-8
