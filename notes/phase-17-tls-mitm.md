# Phase 17: TLS MITM Attack

## What We Built

A TLS Man-in-the-Middle proxy that intercepts encrypted P2P traffic. The attacker sits between two nodes, terminates TLS on both sides, and reads all plaintext messages in the middle.

```bash
# Terminal A — real target node
cargo run -p p2p-node -- 9000

# Terminal B — MITM proxy
cargo run -p p2p-lab -- mitm 9001 127.0.0.1:9000

# Terminal C — victim node (thinks it's connecting to 9000)
cargo run -p p2p-node -- 9002 127.0.0.1:9001
```

Output on Terminal B:
```
[mitm] victim connected from 127.0.0.1:52837
[mitm] TLS established with victim
[mitm] TLS established with target
[mitm][victim->target] {"Hello":{"node_id":15029507781090924506,"listen_addr":"127.0.0.1:9002","peers":[]}}
[mitm][target->victim] {"Hello":{"node_id":7734789368817993465,"listen_addr":"127.0.0.1:9000","peers":["127.0.0.1:9002"]}}
[mitm][victim->target] "Ping"
[mitm][target->victim] "Pong"
[mitm][victim->target] "GetPeers"
[mitm][target->victim] {"Peers":["127.0.0.1:9002"]}
```

Victim believes it has an encrypted connection to 9000. It does not.

---

## How It Works

```
victim (9002)  ──TLS──►  MITM (9001)  ──TLS──►  target (9000)
                         ^
                         reads plaintext here
```

Two separate TLS tunnels:
1. Victim → MITM: victim initiates TLS, MITM presents its own self-signed cert
2. MITM → Target: MITM connects to real target as a TLS client

In the middle, all data passes through MITM in plaintext before being re-encrypted and forwarded. Neither side detects anything wrong.

---

## The Root Cause: `NoVerifier`

Phase 16 added TLS but used `NoVerifier` to skip certificate validation:

```rust
impl ServerCertVerifier for NoVerifier {
    fn verify_server_cert(&self, ...) -> Result<ServerCertVerified, TlsError> {
        Ok(ServerCertVerified::assertion())  // accept any cert
    }
}
```

The victim connects to MITM's port, receives MITM's cert, and accepts it unconditionally. From the victim's perspective, the TLS handshake succeeded — it has no way to know the cert belongs to an attacker instead of the real node.

TLS without certificate validation provides **encryption but not authentication**. The channel is private from passive eavesdroppers, but cannot resist an active MITM.

---

## Implementation: `proxy_direction`

```rust
async fn proxy_direction<R, W>(label: &'static str, mut reader: R, mut writer: W)
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line).await.unwrap_or(0) == 0 { break; }
        println!("[mitm][{label}] {}", line.trim());
        writer.write_all(line.as_bytes()).await.ok();
    }
}
```

`tokio::io::split()` returns `ReadHalf<T>` which only implements `AsyncRead`. `BufReader::new()` wraps it to get `AsyncBufRead`, which is required for `read_line`.

```rust
let (victim_read, victim_write) = tokio::io::split(victim_tls);
let victim_read = BufReader::new(victim_read);  // ReadHalf → AsyncBufRead
```

---

## What TLS Actually Guarantees

| Property | TCP (Phase 9) | TLS + NoVerifier (Phase 16) | TLS + Validation |
|----------|--------------|----------------------------|-----------------|
| Encryption | ❌ | ✅ | ✅ |
| Integrity | ❌ | ✅ | ✅ |
| Server authentication | ❌ | ❌ | ✅ |
| MITM resistant | ❌ | ❌ | ✅ |

Phase 16 moved from the first column to the second. Phase 17 shows that the second column is still vulnerable. Phase 18 (Noise Protocol) moves to the third.

---

## Blockchain Relevance

Bitcoin's original P2P layer has no encryption — every message (transactions, blocks, peer addresses) is visible to any network observer. BIP 151 proposed encryption but was never fully adopted.

Ethereum's RLPx protocol and Lightning Network's BOLT #8 both use the **Noise Protocol** instead of TLS. The key difference: Noise uses **static public keys for mutual authentication** — no CA, no cert validation, no `NoVerifier`. Each node has a long-term keypair; the handshake cryptographically proves both sides hold their private keys.

This is exactly what Phase 18 implements.
