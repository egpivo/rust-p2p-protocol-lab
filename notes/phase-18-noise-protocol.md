# Phase 18: Noise Protocol

## What We Built

A P2P node using Noise_XX handshake for encrypted, mutually authenticated communication. No CA, no certificates — identity is a static keypair.

```bash
# Terminal A
cargo run -p p2p-noise -- 9000

# Terminal B — connect with expected pubkey
cargo run -p p2p-noise -- 9001 127.0.0.1:9000 <A's pubkey>
```

Output:
```
[9001] -> handshake OK, remote=3677805f...
[9001] peer identity verified ✓
[9001] <- {"Hello":{"node_id":...}}
```

With wrong key (MITM simulation):
```
[9001] -> handshake OK, remote=3677805f...
[9001] MITM DETECTED: expected 3677805f...cd0
[9001]                got     3677805f...cd064ab9d20
```

Connection is dropped. No Hello is sent or received.

---

## Why Noise Instead of TLS

Phase 17 showed that TLS with `NoVerifier` is trivially intercepted — the MITM presents its own cert and the client accepts it. The fix in standard TLS is a CA: a trusted third party that signs certs. But P2P networks have no central authority.

Noise solves this without a CA: each node has a **static keypair**, and the handshake cryptographically proves both sides hold their private keys. Identity is the public key itself.

This is the approach used by:
- **Ethereum devp2p** (RLPx)
- **Lightning Network** (BOLT #8)
- **libp2p** (used by Ethereum 2, IPFS)

---

## Noise_XX Handshake

`XX` means both sides exchange static keys:

```
Initiator (A)               Responder (B)
     |                           |
     |  → e                      |   A sends ephemeral pubkey
     |  ← e, ee, s, es           |   B sends ephemeral pubkey + static pubkey (encrypted)
     |  → s, se                  |   A sends static pubkey (encrypted)
     |                           |
     |  ══ encrypted transport ══|   handshake complete
```

- `e` = ephemeral keypair, generated fresh per connection
- `s` = static keypair, the node's long-term identity
- `ee`, `es`, `se` = Diffie-Hellman operations that mix key material into the cipher state

After the handshake, both sides have verified each other's static public key through DH — not through trust.

---

## Why DH Prevents MITM

The security property of DH:

```
DH(A_private, B_public) == DH(B_private, A_public)
```

Both sides compute the same shared secret using their own private key and the other's public key. An attacker with only the public key cannot compute this value.

In Noise_XX, the `es` step mixes `DH(initiator_ephemeral, responder_static)` into the cipher state. To decrypt B's static key in message 2, you need B's private key. A MITM doesn't have it.

Even if a MITM inserts its own keypair, the initiator ends up with the MITM's static public key — not the real target's. If the initiator knows the expected pubkey in advance, it detects the mismatch immediately.

---

## Identity Verification

After `handshake_initiator` completes, the remote's static public key is available:

```rust
let remote = transport.remote_static().unwrap();
let remote_hex = hex(remote);

if let Some(ref expected) = expected_key {
    if remote_hex != *expected {
        eprintln!("[{port}] MITM DETECTED");
        return;  // drop connection
    }
    println!("[{port}] peer identity verified ✓");
}
```

This is the step `NoVerifier` skipped in Phase 16. Here we do it explicitly using the static public key exchanged during the handshake.

---

## Implementation: `NoiseTransport`

After the handshake, `snow` returns a `TransportState`. We wrap it:

```rust
pub struct NoiseTransport {
    state: TransportState,
}

impl NoiseTransport {
    pub fn send(&mut self, plaintext: &[u8], buf: &mut [u8]) -> usize {
        self.state.write_message(plaintext, buf).unwrap()
    }

    pub fn recv<'a>(&mut self, ciphertext: &[u8], buf: &'a mut [u8]) -> &'a [u8] {
        let len = self.state.read_message(ciphertext, buf).unwrap();
        &buf[..len]
    }

    pub fn remote_static(&self) -> Option<&[u8]> {
        self.state.get_remote_static()
    }
}
```

All messages are framed with a 2-byte length prefix — TCP is a stream, not a message protocol:

```
[2 bytes: length as u16 big-endian][N bytes: Noise-encrypted payload]
```

The buf size of 65535 matches the Noise Protocol spec maximum message size (u16 max).

---

## Comparison: TLS vs Noise in P2P Context

| | TLS + CA | TLS + NoVerifier | Noise_XX |
|--|---------|-----------------|---------|
| Encryption | ✅ | ✅ | ✅ |
| Server authentication | ✅ | ❌ | ✅ |
| Mutual authentication | optional | ❌ | ✅ |
| Requires CA | ✅ | — | ❌ |
| MITM resistant | ✅ | ❌ | ✅ (with known pubkey) |
| Used in blockchain | — | — | Ethereum, Lightning, libp2p |

TLS needs a CA to bind identity to a certificate. In a P2P network with no central authority, this doesn't scale. Noise binds identity directly to a keypair — the same model blockchain wallets and node keys already use.

---

## The Attack Series

```
Phase 16   TLS + NoVerifier
            encryption ✓, authentication ✗
            ↓
Phase 17   TLS MITM
            attacker intercepts encrypted traffic, reads plaintext
            ↓
Phase 18   Noise_XX + identity verification
            MITM detected, connection dropped
            encryption ✓, authentication ✓
```

The progression shows exactly why certificate validation exists and what happens when you skip it.
