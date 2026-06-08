# rust-p2p-protocol-lab

A series of network programming and blockchain security experiments in Rust.

Built as a learning path toward blockchain security engineering — each phase adds one concept on top of the last.

---

## What's Here

| Phase | Crate | What |
|-------|-------|------|
| 1–4 | — | TCP fundamentals, length-prefix framing, proxy |
| 5–6 | — | TCP proxy, IDS |
| 7 | — | Honeypot with fake SSH + credential logging |
| 8 | — | Concurrent password brute forcer |
| 9 | `p2p-node` | P2P node with peer discovery |
| 10–13 | `p2p-lab` | Crawler, Sybil, Eclipse, IDS monitor |
| 14–15 | `p2p-env` | P2P Security Gym |

---

## P2P Security Gym

Simulate blockchain P2P attacks with one command.

```bash
# Sybil attack — fill peer slots with fake identities
cargo run -p p2p-env -- --honest 4 --attack sybil --sybil 10

# Eclipse attack — isolate a node, feed it fake chain state
cargo run -p p2p-env -- --honest 4 --attack eclipse --sybil 20

# Network partition — split the network into two isolated groups
cargo run -p p2p-env -- --honest 6 --attack partition
```

Adding a new attack = implementing one trait:

```rust
pub trait Attack: Send + Sync {
    fn name(&self) -> &str;
}
```

---

## Crate Structure

```
crates/
├── p2p-core/   Message, NodeId
├── p2p-node/   Honest P2P node (lib + binary)
├── p2p-lab/    Attack tools: crawl, sybil, eclipse, monitor
└── p2p-env/    Security gym: NetworkEnv, Attack trait, CLI
```

---

## Run the Network

```bash
# Terminal A — seed node
cargo run -p p2p-node -- 9000

# Terminal B, C — join the network
cargo run -p p2p-node -- 9001 127.0.0.1:9000
cargo run -p p2p-node -- 9002 127.0.0.1:9000

# Attack tools
cargo run -p p2p-lab -- crawl 127.0.0.1:9000
cargo run -p p2p-lab -- sybil 127.0.0.1:9000 20
cargo run -p p2p-lab -- eclipse 127.0.0.1:9000
cargo run -p p2p-lab -- monitor 127.0.0.1:9000
```

---

## Protocol

Newline-delimited JSON over TCP.

```json
{"Hello":{"node_id":12345,"listen_addr":"127.0.0.1:9001","peers":[]}}
{"Ping"}
{"Pong"}
{"GetPeers"}
{"Peers":["127.0.0.1:9001","127.0.0.1:9002"]}
{"Tip":{"height":100,"hash":"abc123"}}
```

---

## Notes

Implementation notes for each phase are in [`notes/`](notes/).
