# Phase 9: P2P Node

## What We Built

A minimal P2P node where every node is both a server and a client — no master/slave.

```bash
# Start seed node
cargo run -p p2p-node -- 9000

# Join the network
cargo run -p p2p-node -- 9001 127.0.0.1:9000
cargo run -p p2p-node -- 9002 127.0.0.1:9000
```

After ~10 seconds, nodes discover each other through peer exchange:
```
[9001] learned new peer: 127.0.0.1:9002
[9002] learned new peer: 127.0.0.1:9001
```

---

## Architecture

```
Each node simultaneously:
  ├── listens for inbound connections (TcpListener)
  └── connects outbound to seed peers (TcpStream::connect)

Every connection:
  ├── HELLO handshake (exchange NodeId + listen_addr + known peers)
  ├── Ping/Pong keepalive (every 5s)
  └── GET_PEERS / PEERS exchange (every 10s)
```

---

## Protocol Messages

Framing: **newline-delimited JSON** — one message per line.

```json
{"Hello":{"node_id":12345,"listen_addr":"127.0.0.1:9001","peers":[]}}
{"Ping"}
{"Pong"}
{"GetPeers"}
{"Peers":["127.0.0.1:9001","127.0.0.1:9002"]}
{"Tip":{"height":100,"hash":"abc123"}}
```

Newline JSON is simpler than length-prefix framing (Phase 3) and easier to debug — you can inspect messages with `nc`.

---

## Key Data Structures

```rust
// p2p-core
struct NodeId(u64);  // random identifier per node

enum Message {
    Hello { node_id: NodeId, listen_addr: SocketAddr, peers: Vec<SocketAddr> },
    Ping,
    Pong,
    GetPeers,
    Peers(Vec<SocketAddr>),
    Tip { height: u64, hash: String },
}

// p2p-node
type PeerList = Arc<Mutex<Vec<SocketAddr>>>;
const MAX_PEERS: usize = 8;
```

---

## Handshake Flow

### Outbound (you initiated)
```
you → Hello{node_id, listen_addr, known_peers}
    ← Hello{their_node_id, their_listen_addr, their_peers}
add their listen_addr to peer list
keep alive: Ping every 5s, GET_PEERS every 10s
```

### Inbound (they initiated)
```
    ← Hello{their_node_id, their_listen_addr, their_peers}
add their listen_addr + their_peers to peer list
you → Hello{node_id, listen_addr, known_peers}
keep alive: respond to Ping with Pong, GetPeers with Peers
```

---

## Peer Discovery

Starting from one seed, a node can discover the full network:

```
node A connects to seed S
  → GET_PEERS → S returns [B, C]
  → A now knows B and C without directly connecting to them
  → A can connect to B, C and repeat
```

This is how `net-crawler` (Phase 10) will work.

---

## Pitfalls We Hit

### 1. listen_addr vs ephemeral port

TCP connections have two ports:
- **listen port** — the stable port a server binds to (e.g. 9001)
- **ephemeral port** — a random temporary port the OS assigns for each outbound connection (e.g. 61558)

When node B connects to node A, `peer_addr` from `listener.accept()` gives the **ephemeral port**, not B's listen port.

```
B connects to A → A sees peer_addr = 127.0.0.1:61558 (useless)
A doesn't know B is listening on 127.0.0.1:9001
```

**Fix:** Include `listen_addr` in the `Hello` message. Each node announces its own listen address during handshake.

```rust
Hello { node_id, listen_addr: "127.0.0.1:9001", peers: [...] }
```

Without this, peer discovery breaks: nodes share ephemeral ports that no longer exist after the connection closes.

### 2. Ping response not consumed

Outbound keep-alive loop sends Ping and then GET_PEERS in the same tick. Inbound handler responds to Ping with Pong, then GET_PEERS with Peers. But the outbound was reading the GET_PEERS response without first consuming the Pong:

```
outbound sends: Ping
inbound sends:  Pong       ← outbound didn't read this
outbound sends: GetPeers
inbound sends:  Peers
outbound reads: Pong       ← reads wrong message, Peers parse fails
```

**Fix:** Read and discard Pong before sending GetPeers.

### 3. Peer list empty on GET_PEERS

Without `listen_addr` in Hello, the inbound handler had no stable address to add to the peer list. So GET_PEERS always returned `[]`.

**Fix:** Same as pitfall 1 — add `listen_addr` to Hello.

---

## Inbound vs Outbound

Every P2P node handles both directions simultaneously:

```
node A                    node B
  │                         │
  ├── outbound ──────────→ accept (inbound)
  │                         │
accept (inbound) ←──────── outbound ←┘
```

- **Outbound**: you called `TcpStream::connect(peer)` — you initiated
- **Inbound**: they connected to you via `listener.accept()` — they initiated

Both sides do a handshake, but in opposite order:
- Outbound sends Hello first, then reads Hello reply
- Inbound reads Hello first, then sends Hello reply

---

## Redundant Logic (Known Technical Debt)

`handle_inbound` and the outbound seed-connect block do similar things:
- Both do handshake
- Both maintain a keep-alive loop

They differ in who speaks first. In a future refactor, both could share a `handle_peer(reader, writer, ...)` function. Deferred until Phase 10 makes the boundaries clearer.

---

## Blockchain Relevance

This is exactly how Bitcoin and Ethereum nodes bootstrap:

| Concept | Our Lab | Bitcoin |
|---------|---------|---------|
| NodeId | random u64 | 8-byte nonce in `version` message |
| Hello | `Hello` message | `version` + `verack` |
| listen_addr | `listen_addr` field | `addr_from` in `version` |
| GET_PEERS | `GetPeers` | `getaddr` |
| PEERS | `Peers` | `addr` |
| MAX_PEERS | 8 | ~125 outbound + inbound |

The key insight: **peer discovery is the foundation of every P2P network**. An attacker who can manipulate what peers a node learns about can eventually control what information that node receives — this is the basis of Eclipse attacks (Phase 13).

---

## What's Next

Phase 10: Network Crawler
- Start from one seed
- GET_PEERS recursively
- Build a complete map of the observable network topology
- This is what an attacker does before launching a Sybil or Eclipse attack
