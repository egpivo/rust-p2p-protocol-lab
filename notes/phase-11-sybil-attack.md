# Phase 11: Sybil Attack

## What We Built

A Sybil attacker that spawns N fake identities, each connecting to a target node with a valid handshake, filling up its peer slots.

```bash
# Terminal A — victim
cargo run -p p2p-node -- 9000

# Terminal B — attack
cargo run -p p2p-lab -- sybil 127.0.0.1:9000 20
```

Result:
```
=== Sybil Attack Result ===
Victim peer list: [127.0.0.1:19005, 127.0.0.1:19003, ...]
Total peers:  8
Sybil peers:  8
Occupancy:   100%
```

20 Sybil identities → 8/8 peer slots occupied → 100% malicious occupancy.

---

## What is a Sybil Attack?

A Sybil attack is when one entity creates many fake identities to gain disproportionate influence in a network.

```
Real world:    1 attacker
Network sees:  20 "different" nodes
```

Named after the book "Sybil" (1973) about a person with multiple personality disorder.

In P2P networks:
- Identity = NodeId (just a number)
- Cost of new identity = near zero (call `NodeId::random()`)
- No way to prove each NodeId belongs to a distinct real-world entity

---

## How the Attack Works

```
Attacker spawns 20 tasks, each with:
  - Different NodeId (looks like different node)
  - Different listen_addr port (19000, 19001, ...)
  - Valid Hello handshake

All 20 connect to victim simultaneously
Victim's 8 peer slots fill up with Sybil nodes
Honest nodes try to connect → no slots available → rejected
```

---

## Key Code

```rust
for i in 0..count {
    tokio::spawn(async move {
        let node_id = NodeId::random();  // new identity each time
        let mut stream = TcpStream::connect(target).await?;

        send Hello {
            node_id,
            listen_addr: format!("127.0.0.1:{}", 19000 + i),
            peers: vec![],
        }

        // hold connection open — occupies peer slot
        tokio::time::sleep(Duration::from_secs(3600)).await;
    });
}
```

The critical part: **hold the connection open**. A Sybil attack only works if the fake connections persist and block honest connections.

---

## Measuring the Attack

After spawning Sybil nodes, crawl the victim to check its peer list:

```rust
let peers = query_peers(target).await;
let sybil_count = peers.iter()
    .filter(|a| a.port() >= 19000)  // sybil ports start at 19000
    .count();

let occupancy = sybil_count as f64 / peers.len() as f64 * 100.0;
```

Key metric: `malicious_occupancy = sybil_peers / total_peers`

---

## Why MAX_PEERS Doesn't Help

```
MAX_PEERS = 8 looks like protection
But it only limits HOW MANY peers, not WHO the peers are
```

With 20 Sybil nodes connecting simultaneously:
- All 20 get accepted (first-come-first-served)
- 8 slots fill up before honest nodes connect
- Victim now only hears Sybil traffic

**Peer limits protect resources, not identity diversity.**

---

## Sybil vs Eclipse

These are related but distinct concepts:

| | Sybil | Eclipse |
|--|-------|---------|
| What | Many fake identities | Victim isolated from honest network |
| How | Create N NodeIds | Fill ALL peer slots with malicious nodes |
| Goal | Gain influence | Control victim's view of the network |
| Relationship | Sybil enables Eclipse | Eclipse requires Sybil (or similar) |

Sybil is the **means**, Eclipse is the **outcome**.
After Sybil occupancy reaches 100%, the victim is eclipsed.

---

## Real-world Defenses

| Defense | How it works | Limitation |
|---------|-------------|------------|
| IP diversity | Limit connections per IP | NAT, proxies, botnets |
| Proof of Work | Make identity creation costly | Arms race |
| Proof of Stake | Stake tokens for identity | Wealth concentration |
| Signed identities | Persist identity across restarts | Doesn't prevent Sybil at scale |
| Peer diversity scoring | Prefer peers from diverse subnets | Bitcoin's addrman buckets |

Bitcoin uses **addrman** — a bucketing system that limits how many addresses from the same /16 subnet can be stored. This makes Sybil attacks from one IP range much harder.

Our lab skips these defenses deliberately to demonstrate the vulnerability.

---

## Blockchain Relevance

Why Sybil attacks matter for blockchain:

```
Step 1: Sybil nodes fill victim's peer slots (Phase 11)
Step 2: Victim only receives data from Sybil nodes
Step 3: Sybil nodes send fake chain data (Phase 13)
Step 4: Victim accepts attacker's version of the blockchain
```

Consequences:
- **Double spend**: victim thinks a transaction was confirmed, attacker reverses it
- **Selfish mining**: attacker hides blocks, releases them strategically
- **Transaction censorship**: victim never sees certain transactions

---

## Pitfalls

### Tasks aborted too early

Original code called `h.abort()` after 2 seconds — connections dropped before we could measure occupancy.

**Fix:** Wait for measurement first, then abort on Ctrl+C.

### Port-based Sybil detection

Our measurement uses `port >= 19000` to identify Sybil nodes. In a real attack, Sybil nodes would use random ports — you can't detect them this way.

Real IDS (Phase 12) must detect Sybil by **behavior patterns**, not port numbers:
- Many NodeIds from same source IP
- Rapid successive connections
- Peer table concentration
