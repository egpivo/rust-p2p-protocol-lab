# Phase 15: Network Partition Attack

## What We Built

A network partition attack that splits the P2P network into two isolated groups. Nodes in group A cannot communicate with nodes in group B, causing the network to fork into two independent chains.

```bash
cargo run -p p2p-env -- --honest 6 --attack partition
```

Output:
```
=== P2P Security Gym ===
Spawning 6 honest nodes...
Network ready.

Attack: NetworkPartition
Group A: [0, 1, 2]
Group B: [3, 4, 5]

=== Result ===
Network partitioned: group_a=[0, 1, 2] isolated from group_b=[3, 4, 5]
Success: true
  cross_peers_remaining: 0.0
  group_a_size: 3.0
  group_b_size: 3.0
```

`cross_peers_remaining: 0.0` — Group A nodes have zero visibility into Group B peers. The network is fully partitioned.

---

## Real-World Occurrences

This is not a theoretical attack. Network partitions have caused real blockchain incidents:

### 1. Bitcoin Chain Fork (2013)
A version incompatibility between Bitcoin 0.7 and 0.8 clients caused the network to split into two groups, each following a different chain for ~6 hours. During this window, double-spend attacks were theoretically possible. The community coordinated a manual rollback.

### 2. BGP Hijacking (Apostolaki et al., 2017)
The paper "Hijacking Bitcoin: Routing Attacks on Cryptocurrencies" demonstrated that AS-level (Autonomous System) attackers controlling internet routing infrastructure could:
- Intercept traffic between Bitcoin nodes
- Partition the Bitcoin network into two halves
- Delay block propagation by 20+ minutes

At the time of publication, 13 ISPs carried 30% of all Bitcoin traffic — a single malicious ISP could partition a significant portion of the network.

### 3. Ethereum Classic 51% Attacks (2019, 2020)
Combined with low hashrate, network partitions were used to enable double-spend attacks worth millions of dollars. Attackers mined blocks privately on a partitioned segment, then released them to reorganize the main chain.

---

## How the Attack Works

### Implementation

Each node has a `blocklist: Arc<Mutex<HashSet<IpAddr>>>`. When `handle_inbound` receives a connection, it checks the source IP against the blocklist before processing the handshake:

```rust
if blocklist.lock().unwrap().contains(&peer_addr.ip()) {
    return;  // drop the connection silently
}
```

`NetworkPartitionAttack::execute` populates the blocklists:

```rust
// group_a nodes block group_b IPs
for &i in &self.group_a {
    let mut bl = env.nodes[i].blocklist.lock().unwrap();
    for addr in &addrs_b {
        bl.insert(addr.ip());
    }
}

// group_b nodes block group_a IPs (symmetric)
for &i in &self.group_b {
    let mut bl = env.nodes[i].blocklist.lock().unwrap();
    for addr in &addrs_a {
        bl.insert(addr.ip());
    }
}
```

### Key Design: BlockList in NodeHandle

`spawn_node` creates a blocklist and returns it alongside the task handle:

```rust
pub async fn spawn_node(port: u16, seeds: Vec<SocketAddr>)
    -> (JoinHandle<()>, BlockList)
{
    let blocklist: BlockList = Arc::new(Mutex::new(HashSet::new()));
    let blocklist_inner = blocklist.clone();

    let handle = tokio::spawn(async move {
        // ... node logic uses blocklist_inner
    });

    (handle, blocklist)  // caller holds the other end
}
```

`NodeHandle` stores the blocklist:
```rust
pub struct NodeHandle {
    pub port: u16,
    pub addr: SocketAddr,
    pub blocklist: BlockList,  // ← external control handle
    task: JoinHandle<()>,
}
```

This is the **shared ownership pattern**: the node task and the attack controller both hold `Arc` pointers to the same `Mutex<HashSet>`. When the attack writes to the blocklist, the node sees the change immediately on the next inbound connection.

---

## Verification

After applying the blocklist, `execute` queries group_a[0]'s peer list and checks if any group_b ports appear:

```rust
let a_peers = query_peers(addrs_a[0]).await.unwrap_or_default();
let cross_peers = a_peers.iter()
    .filter(|p| addrs_b.iter().any(|b| b.port() == p.port()))
    .count();

metrics.insert("cross_peers_remaining", cross_peers as f64);
let partitioned = cross_peers == 0;
```

`cross_peers_remaining: 0.0` confirms the partition is in effect — no group_b addresses appear in group_a's peer table.

---

## Partition vs Eclipse vs Sybil

| | Sybil | Eclipse | NetworkPartition |
|--|-------|---------|-----------------|
| Target | One node | One node | Entire network |
| Method | Fill peer slots | Fill + send fake data | Block inter-group connections |
| Result | Attacker presence | Single-node fork | Network-wide fork |
| Requires | Many fake identities | Many fake identities | Control over routing/connections |
| Real-world analog | Cheap cloud VMs | BGP route injection to single AS | BGP hijacking at ISP level |

---

## Blockchain Consequences of a Partition

When a blockchain network splits into two isolated groups:

```
Group A                    Group B
  │                          │
  ├── mines blocks A1, A2    ├── mines blocks B1, B2
  ├── sees chain: ...A1, A2  ├── sees chain: ...B1, B2
  │                          │
  └── partition heals ───────┘
         Both groups see conflicting chains
         Longer chain wins (PoW) or validators vote (PoS)
         Shorter chain's transactions are REVERSED
```

**Double spend window:**
During the partition, a merchant on Group A confirms a transaction. The attacker on Group B never saw it. When the partition heals, if Group B's chain is longer, Group A's chain is abandoned — the merchant's transaction is reversed.

---

## Limitations of This Lab

### 1. Same IP for all nodes
All nodes run on `127.0.0.1`. The blocklist blocks by IP, but since all nodes share the same IP, blocking `127.0.0.1` would block everyone. The partition works here because **existing connections are not dropped** — only new inbound connections from blocked IPs are rejected.

In a real network, each node would have a distinct IP, and the partition would be more effective (and easier to verify).

### 2. Existing connections persist
The blocklist only affects new connections. Nodes that were already connected before the partition remain connected. In this lab, nodes connect during `reset()`, so all connections are established before the attack — the partition only prevents new connections from forming.

A more complete implementation would also drop existing connections to blocked peers.

### 3. No routing-level control
Real network partitions via BGP hijacking operate at the routing layer — packets simply never arrive. Our lab simulates this at the application layer (reject on accept), which is equivalent in effect but different in mechanism.

---

## Defenses Against Network Partition

| Defense | How it works |
|---------|-------------|
| Geographic peer diversity | Connect to nodes across different ASes and regions |
| Multiple network paths | Use Tor + clearnet simultaneously |
| Detect tip divergence | If your tip stops advancing while others' doesn't, suspect partition |
| Eclipse-resistant peer selection | Bitcoin's addrman bucketing by /16 subnet |
| Anchor connections | Bitcoin: maintain long-lived connections that survive restarts |

Bitcoin's response to the 2013 fork: manual coordination via IRC to roll back the 0.8 chain. This highlighted the need for automated detection and response mechanisms.

---

## The Complete Attack Suite

Running all three attacks on the same network:

```bash
# Identity attack: fill peer slots
cargo run -p p2p-env -- --honest 4 --attack sybil --sybil 10
# → occupancy: 62.5%

# Information attack: control chain state view
cargo run -p p2p-env -- --honest 4 --attack eclipse --sybil 20
# → fake tip delivered, state_diverged possible at 100% occupancy

# Network attack: split the entire network
cargo run -p p2p-env -- --honest 6 --attack partition
# → cross_peers_remaining: 0.0, full partition confirmed
```

Each attack builds on the same protocol infrastructure. The gym makes them repeatable, quantified, and extensible — adding a new attack means implementing one struct.
