# Phase 10: Network Crawler

## What We Built

A network crawler that starts from one seed node and discovers all reachable nodes in the P2P network using BFS.

```bash
# First start the network
cargo run -p p2p-node -- 9000
cargo run -p p2p-node -- 9001 127.0.0.1:9000
cargo run -p p2p-node -- 9002 127.0.0.1:9000

# Then crawl from seed
cargo run -p p2p-lab -- 127.0.0.1:9000
```

Output:
```
Crawling from seed 127.0.0.1:9000...
visiting 127.0.0.1:9000
  127.0.0.1:9000 → [127.0.0.1:9001, 127.0.0.1:9002]
visiting 127.0.0.1:9001
  127.0.0.1:9001 → [127.0.0.1:9000, 127.0.0.1:9002]
visiting 127.0.0.1:9002
  127.0.0.1:9002 → [127.0.0.1:9000, 127.0.0.1:9001]
Done. Discovered 3 nodes.
```

---

## How It Works

```
queue = [seed]
visited = {}

while queue not empty:
    addr = queue.pop_front()       ← BFS: take from front
    if addr in visited: skip
    visited.add(addr)

    peers = connect(addr) → GET_PEERS
    for peer in peers:
        if peer not in visited:
            queue.push_back(peer)  ← add to back of queue

print discovered count
```

---

## BFS vs DFS

Both work for graph traversal. BFS is better for network crawling because:

| | BFS | DFS |
|--|-----|-----|
| Data structure | Queue (VecDeque) | Stack (or recursion) |
| Explores | Nearest nodes first | One path to the end first |
| Risk | None | Stack overflow on cycles |
| For crawling | Discovers core network fast | May go deep into one branch |

P2P networks have cycles (A→B→C→A). DFS with recursion would infinite loop without a visited set. BFS with a queue is naturally safe.

---

## Key Code

### BFS Loop

```rust
let mut visited: HashSet<SocketAddr> = HashSet::new();
let mut queue: VecDeque<SocketAddr> = VecDeque::new();

queue.push_back(seed);

while let Some(addr) = queue.pop_front() {
    if visited.contains(&addr) { continue; }
    visited.insert(addr);

    if let Some(peers) = query_peers(addr).await {
        for peer in peers {
            if peer.port() == 0 { continue; }
            if !visited.contains(&peer) {
                queue.push_back(peer);
            }
        }
    }
}
```

### query_peers

```rust
async fn query_peers(addr: SocketAddr) -> Option<Vec<SocketAddr>> {
    let mut stream = TcpStream::connect(addr).await.ok()?;

    // handshake with port=0 (crawler has no listen port)
    send Hello { listen_addr: "127.0.0.1:0" }
    read Hello reply (discard)

    // ask for peers
    send GetPeers
    read Peers(list)

    Some(list)
}
```

Crawler uses `listen_addr: 127.0.0.1:0` to signal it is not a real node — it has no listen port.

---

## Pitfalls We Hit

### 1. Crawler's fake listen_addr pollutes peer lists

Crawler sends `Hello { listen_addr: "127.0.0.1:0" }`. Honest nodes added `127.0.0.1:0` to their peer list and returned it to other crawlers.

**Fix in p2p-node:** reject `listen_addr` with port 0 when processing Hello:
```rust
if listen_addr.port() != 0 && !p.contains(&listen_addr) {
    p.push(listen_addr);
}
```

**Fix in p2p-lab:** skip port 0 addresses from peer lists:
```rust
if peer.port() == 0 { continue; }
```

### 2. Node returns itself in peer list

Nodes include their own `listen_addr` in their peer list. Not a bug — it gets filtered by the `visited` set since the crawler already visited that node.

---

## P2P Network as a Graph

```
node  = vertex
connection = edge

9000 ── 9001
  \      /
   9002
```

Crawling = graph traversal. The crawler reconstructs the **observable topology** — what each node is willing to share, not necessarily the full network.

This matters for security:
- An attacker uses a crawler to map the network before attacking
- Nodes with many connections (high degree) are high-value targets
- Nodes with few connections are easier to eclipse

---

## What the Crawler Does NOT See

- Connections nodes haven't shared yet (peer exchange is eventually consistent)
- Nodes behind NAT (they can't accept inbound connections)
- Nodes that refuse crawler connections (rate limiting, IP filtering)
- Private connections (some nodes connect without advertising their peers)

The crawler sees the **observable** graph, not the true topology.

---

## Blockchain Relevance

Before a Sybil or Eclipse attack, an attacker maps the network:

```
Step 1: Crawl from known seed nodes (Bitcoin: dns seeds, Ethereum: bootnodes)
Step 2: Build graph of all reachable nodes
Step 3: Identify target nodes (low peer count = easier to eclipse)
Step 4: Launch Sybil identities targeting the victim
```

Our crawler (Phase 10) covers Step 1 and 2.
Phase 11 (Sybil) covers Step 4.

---

## Testing

```bash
# Start network
cargo run -p p2p-node -- 9000
cargo run -p p2p-node -- 9001 127.0.0.1:9000
cargo run -p p2p-node -- 9002 127.0.0.1:9000

# Wait 10s for peer discovery, then crawl
cargo run -p p2p-lab -- 127.0.0.1:9000
```
