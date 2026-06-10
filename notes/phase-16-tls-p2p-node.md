# Phase 16: TLS P2P Node

## What We Built

Added TLS encryption to the P2P node. All communication — Hello handshake, Ping/Pong, peer discovery — now runs over an encrypted channel.

```bash
# Terminal A
cargo run -p p2p-node -- 9000

# Terminal B
cargo run -p p2p-node -- 9001 127.0.0.1:9000
```

Output:
```
[9000] <- handshake from NodeId(9590...) at 127.0.0.1:51037
[9001] -> connected to NodeId(1284...) at 127.0.0.1:9000
[9001] learned new peer: 127.0.0.1:9001
```

The P2P protocol is unchanged. The only difference: all bytes on the wire are now encrypted.

---

## What Changed

### Before (plaintext TCP)
```
TcpListener::accept() → TcpStream → read/write JSON
```

### After (TLS)
```
TcpListener::accept() → TcpStream
  → TlsAcceptor::accept() → TlsStream → read/write JSON
```

The JSON protocol (Hello, Ping, GetPeers) is identical. TLS is a transparent layer underneath.

---

## Key Design: Generic Stream

`handle_inbound` was changed from a concrete `TcpStream` to a generic:

```rust
// Before
pub async fn handle_inbound(stream: TcpStream, ...) { ... }

// After
pub async fn handle_inbound<S>(stream: S, ...)
where S: AsyncRead + AsyncWrite + Unpin + Send + 'static
{ ... }
```

`TcpStream`, `TlsStream<TcpStream>`, and any future stream type all satisfy `AsyncRead + AsyncWrite`. The function body doesn't change.

The same pattern applies to `send_msg`:

```rust
pub async fn send_msg<W>(writer: &mut W, msg: &Message) -> std::io::Result<()>
where W: AsyncWriteExt + Unpin
```

### Why `tokio::io::split` instead of `into_split`

`TcpStream::into_split()` only works on `TcpStream`. To split a generic `S: AsyncRead + AsyncWrite`, use:

```rust
let (read_half, mut write_half) = tokio::io::split(stream);
```

`tokio::io::split` works on any type implementing both traits.

---

## TLS Setup

### Server side (accept)

```rust
// generate self-signed cert
let (server_config, _, _) = make_tls_config();
let tls_acceptor = TlsAcceptor::from(Arc::new(server_config));

// in accept loop
let (tcp_stream, peer_addr) = listener.accept().await?;
let tls_stream = tls_acceptor.accept(tcp_stream).await?;
handle_inbound(tls_stream, ...).await;
```

### Client side (connect)

```rust
let client_config = ClientConfig::builder()
    .dangerous()
    .with_custom_certificate_verifier(Arc::new(NoVerifier))
    .with_no_client_auth();
let tls_connector = TlsConnector::from(Arc::new(client_config));

let tcp_stream = TcpStream::connect(seed).await?;
let server_name = ServerName::try_from("localhost").unwrap();
let tls_stream = tls_connector.connect(server_name, tcp_stream).await?;
let (read_half, mut write_half) = tokio::io::split(tls_stream);
```

### Self-signed cert generation

```rust
pub fn make_tls_config() -> (ServerConfig, CertificateDer<'static>, Vec<u8>) {
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
    let cert_der = CertificateDer::from(cert.cert.der().to_vec());
    let key_der = PrivateKeyDer::try_from(cert.key_pair.serialize_der()).unwrap();

    let server_config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert_der.clone()], key_der)
        .unwrap();

    (server_config, cert_der, cert_der.to_vec())
}
```

Each node generates a fresh self-signed cert on startup. No CA required — this is a lab.

---

## `NoVerifier` — Skipping Certificate Validation

Because every node has its own self-signed cert, the client can't verify it against a CA. We implement a custom verifier that accepts everything:

```rust
#[derive(Debug)]
struct NoVerifier;

impl ServerCertVerifier for NoVerifier {
    fn verify_server_cert(&self, ...) -> Result<ServerCertVerified, TlsError> {
        Ok(ServerCertVerified::assertion())  // always accept
    }
    // ...
}
```

**This is intentionally insecure.** `NoVerifier` means a MITM attacker can intercept the connection by presenting their own cert — the client won't notice. Phase 17 will exploit this.

---

## What TLS Provides Here

| Property | Status |
|----------|--------|
| Encryption | ✅ All P2P messages are encrypted on the wire |
| Integrity | ✅ TLS MAC prevents tampering |
| Server authentication | ❌ `NoVerifier` skips cert validation |
| Forward secrecy | ✅ TLS 1.3 uses ECDHE by default |

The missing piece is **authentication**. Without cert validation, the client has no proof it's talking to the real server. This is the attack surface Phase 17 exploits.

---

## Why Not Just Use `native-tls`?

`rustls` vs `native-tls`:

| | `rustls` | `native-tls` |
|--|---------|-------------|
| Implementation | Pure Rust | C (OpenSSL/Secure Transport/SChannel) |
| TLS versions | 1.2, 1.3 only | 1.0–1.3 |
| Dependencies | Zero C deps | OpenSSL on Linux |
| Memory safety | Guaranteed | Depends on C library |

`rustls` is the standard choice for new Rust projects. It only supports modern TLS — no deprecated 1.0/1.1 — which is a security feature.

---

## The Upgrade Path: What Each Phase Adds

```
Phase 9–13   TCP, plaintext JSON
              ↓ anyone with Wireshark can read all P2P messages

Phase 16     TLS, encrypted JSON
              ↓ wire traffic is encrypted, but no cert validation

Phase 17     TLS MITM attack
              ↓ exploit NoVerifier to intercept encrypted traffic

Phase 18     Noise Protocol
              ↓ no CA needed, mutual authentication, forward secrecy
              (the actual approach used by Ethereum devp2p, libp2p)
```

---

## Blockchain Relevance

Modern blockchain nodes use encrypted transport:

| Protocol | Transport |
|----------|-----------|
| Bitcoin | No encryption on P2P layer (historical) |
| Ethereum devp2p | RLPx (similar to Noise Protocol) |
| libp2p (used by Ethereum 2, IPFS) | Noise Protocol or TLS 1.3 |
| Bitcoin Lightning | Noise Protocol (BOLT #8) |

Bitcoin's original P2P layer has no encryption — all messages (transactions, blocks, peer addresses) are transmitted in plaintext. This is a known weakness. Proposals like BIP 151 attempted to add encryption but were never fully adopted.

Ethereum learned from this and built RLPx with mandatory encryption and mutual authentication from the start.
