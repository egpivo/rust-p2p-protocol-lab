# Phase 14: P2P Security Gym

## What We Built

An extensible simulation environment for blockchain P2P network attacks — all in Rust, runnable with one command.

```bash
# Sybil attack on 4-node network
cargo run -p p2p-env -- --honest 4 --attack sybil --sybil 10

# Eclipse attack
cargo run -p p2p-env -- --honest 4 --attack eclipse --sybil 20
```

Output:
```
=== P2P Security Gym ===
Spawning 4 honest nodes...
Network ready.

Attack: Eclipse
Target node: 0
Sybil count: 20
[victim] tip: height=9999 hash=attacker_tip_FAKE   ← fake state received

=== Result ===
Eclipse: 5/8 peers Sybil, state_diverged=false
Success: false
  occupancy: 62.5
  honest_peers: 3.0
```

---

## Architecture

```
crates/
├── p2p-core/    ← Message, NodeId (shared types)
├── p2p-node/
│   ├── src/lib.rs   ← handle_inbound, PeerList, MAX_PEERS (reusable)
│   └── src/main.rs  ← binary entry point
├── p2p-lab/     ← standalone attack tools (crawl, sybil, eclipse, monitor)
└── p2p-env/
    ├── src/lib.rs   ← NetworkEnv, Attack trait, SybilAttack, EclipseAttack
    └── src/main.rs  ← CLI runner
```

### Key Design Decision: p2p-node as Library

`handle_inbound` and `PeerList` live in `p2p-node/src/lib.rs`, not just `main.rs`. This lets `p2p-env` reuse the honest node logic without duplicating code.

```rust
// p2p-env/Cargo.toml
p2p-node = { path = "../p2p-node" }

// p2p-env/src/lib.rs
use p2p_node::{handle_inbound, PeerList, MAX_PEERS};
```

---

## Core Interfaces

### Attack Trait

```rust
pub trait Attack: Send + Sync {
    fn name(&self) -> &str;
}
```

Adding a new attack = implementing one trait. The `Send + Sync` bounds ensure attacks can be safely passed across async tasks.

### NetworkEnv

```rust
pub struct NetworkEnv {
    config: EnvConfig,
    nodes: Vec<NodeHandle>,   // honest nodes running in-process
}

impl NetworkEnv {
    pub async fn reset(&mut self)  // kill old nodes, spawn fresh network
}
```

`reset()` aborts all running node tasks and spawns a fresh network — enabling repeatable experiments.

### AttackResult

```rust
pub struct AttackResult {
    pub success: bool,
    pub metrics: HashMap<String, f64>,
    pub summary: String,
}
```

Quantitative output: `occupancy`, `sybil_peers`, `honest_peers`, `state_diverged`.

---

## Shared Helpers

Two module-level functions shared by all attacks:

### `query_peers`

```rust
async fn query_peers(addr: SocketAddr) -> Option<Vec<SocketAddr>>
```

Connects to a node, sends GetPeers, returns its peer list. Used to measure occupancy after an attack.

### `connect_as_sybil`

```rust
async fn connect_as_sybil(
    target: SocketAddr,
    port: u16,
) -> Option<(BufReader<OwnedReadHalf>, OwnedWriteHalf)>
```

Opens a TCP connection with a fake identity, completes the Hello handshake, returns the live stream halves.

Returns `(reader, writer)` so the caller can:
- **SybilAttack**: hold the connection open (peer slot occupied)
- **EclipseAttack**: send a fake Tip, then hold open

---

## How SybilAttack Works

```rust
// spawn N fake identities
for i in 0..count {
    tokio::spawn(async move {
        let Some((_reader, _writer)) =
            connect_as_sybil(target_addr, 19000 + i as u16).await
        else { return };

        // _reader/_writer kept alive → connection stays open → slot occupied
        tokio::time::sleep(Duration::from_secs(3600)).await;
    });
}

// measure
let peers = query_peers(target_addr).await;
let sybil_count = peers.iter().filter(|a| a.port() >= 19000).count();
let occupancy = sybil_count as f64 / peers.len() as f64 * 100.0;
```

Critical detail: `_reader` and `_writer` must be named variables (not `_`). An unnamed `_` drops immediately, closing the connection. Named variables with `_` prefix suppress unused warnings while keeping the value alive until the block ends.

---

## How EclipseAttack Differs

```rust
let Some((mut reader, mut write_half)) =
    connect_as_sybil(target_addr, 19000 + i as u16).await
else { return };

// one extra step: send fake chain state
let mut tip = serde_json::to_string(&Message::Tip {
    height: 9999,
    hash: "attacker_tip_FAKE".to_string(),
}).unwrap();
tip.push('\n');
write_half.write_all(tip.as_bytes()).await.ok();

// then hold connection
tokio::time::sleep(Duration::from_secs(3600)).await;
```

**Sybil is the method. Eclipse is the result.**
The only code difference is the `Tip` message — everything else (connection, handshake, slot occupation) is shared via `connect_as_sybil`.

---

## Experiment Results

### Sybil (10 attackers vs 4-node network)

```
Sybil occupancy: 62% (5/8 peers)
Success: false
```

3 honest nodes connected first → 3/8 slots taken → Sybil only got 5/8.

### Eclipse (20 attackers vs 4-node network)

```
Eclipse: 5/8 peers Sybil, state_diverged=false
occupancy: 62.5
honest_peers: 3.0
```

Same result: honest nodes that connect during `reset()` hold their slots. Fake tips were delivered, but the victim still has honest peers.

**Key insight:** The timing of connections matters. Honest nodes that establish connections before the attack starts hold their peer slots. This is why real Eclipse attacks require sustained effort and network churn.

---

## Why the Attack Didn't Fully Succeed

In this lab, 4 honest nodes connect to the seed (node 0) during network setup:

```
t=0s  reset() spawns nodes 0,1,2,3
t=0s  nodes 1,2,3 connect to node 0 → 3/8 slots filled
t=3s  "Network ready"
t=3s  Eclipse attack starts → only 5 remaining slots
```

To get 100% Eclipse in this lab:
- Use `--honest 1` (only seed node, no other honest nodes)
- Or use `--sybil` count large enough to fill all 8 slots before honest nodes connect

---

## Extensibility

Adding a new attack requires implementing one struct:

```rust
pub struct NetworkPartition {
    pub group_a: Vec<usize>,
    pub group_b: Vec<usize>,
}

impl Attack for NetworkPartition {
    fn name(&self) -> &str { "NetworkPartition" }
}

impl NetworkPartition {
    pub async fn execute(&self, env: &NetworkEnv) -> AttackResult {
        // drop connections between group_a and group_b nodes
        // measure partition persistence
    }
}
```

Then wire it up in `main.rs` with one match arm. No changes to the gym framework itself.

---

## What This Proves

1. **Peer limits alone don't protect diversity** — MAX_PEERS=8 limits connections but not who connects
2. **Timing matters** — connections established before an attack hold their slots
3. **Quantification is possible** — occupancy %, honest_peers, state_diverged are measurable signals
4. **Shared abstraction eliminates duplication** — `connect_as_sybil` is used by both Sybil and Eclipse with zero repeated code

---

## Series Summary

| Phase | What | Key Output |
|-------|------|-----------|
| 9 | P2P Node | Honest network with peer discovery |
| 10 | Crawler | BFS network map |
| 11 | Sybil Attack | 100% peer slot occupancy |
| 12 | IDS Monitor | IP concentration alerting |
| 13 | Eclipse Attack | Fake chain state delivery |
| 14 | Security Gym | Repeatable, quantified attack simulation |

**The throughline:** Identity without cost enables Sybil. Sybil with persistence enables Eclipse. Eclipse with fake data enables chain state control. The gym makes all of this measurable.
