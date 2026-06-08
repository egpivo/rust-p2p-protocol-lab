# Phase 13: Eclipse Attack

## What We Built

A full Eclipse Attack demonstration — Sybil nodes fill the victim's peer slots and feed it false chain state, causing state divergence from the honest network.

```bash
# Terminal A — honest network
cargo run -p p2p-node -- 9000

# Terminal B — eclipse attack
cargo run -p p2p-lab -- eclipse 127.0.0.1:9000
```

Result:
```
=== Eclipse Attack Scenario ===

[Before attack]
Victim peers: []

[After attack]
Honest peers remaining: 0
Sybil peers:            8
Victim received fake tip: height=9999 hash=attacker_tip_FAKE
Honest tip:               height=100  hash=honest
State diverged:           true
```

---

## The Full Attack Chain

```
Phase 9:  Build P2P node (the target)
Phase 10: Crawler maps the network (attacker's recon)
Phase 11: Sybil fills peer slots (attacker gains presence)
Phase 12: IDS detects the pattern (defender's view)
Phase 13: Eclipse — victim receives fake chain state
```

Each phase builds on the previous. Eclipse is the final outcome of a successful Sybil attack.

---

## How Eclipse Differs from Sybil

| | Sybil | Eclipse |
|--|-------|---------|
| Definition | Many fake identities | Victim isolated from honest network |
| Measure | % of fake peers | 0 honest peers remaining |
| Goal | Gain presence | Control victim's information |
| Consequence | Victim hears from attacker | Victim sees attacker's reality |

**Sybil is the method. Eclipse is the result.**

When Sybil occupancy reaches 100%, the victim is eclipsed:
- All incoming data comes from attacker-controlled nodes
- Victim cannot verify against honest peers
- Attacker can feed any chain state they want

---

## What State Divergence Means

```
Honest network:   height=100  hash=honest_tip
Victim sees:      height=9999 hash=attacker_tip_FAKE
```

The victim's view of the blockchain is completely controlled by the attacker.

**Real-world consequences:**

1. **Double spend**
   ```
   Attacker sends TX to merchant on honest network
   Merchant sees TX confirmed (from honest chain)
   Attacker's eclipse prevents victim from seeing the TX
   Attacker reverses the TX on honest network
   Victim (merchant) loses money
   ```

2. **Selfish mining**
   ```
   Attacker mines blocks privately
   Releases them to eclipsed victim only
   Victim wastes mining power on attacker's chain
   ```

3. **Transaction censorship**
   ```
   Attacker filters which transactions victim sees
   Victim can never include certain TXs in blocks
   ```

---

## Eclipse Attack Flow

```
Step 1: Recon (crawler)
  seed → GET_PEERS → map all nodes
  identify victim (low peer count = easier target)

Step 2: Sybil (fill slots)
  spawn 20 fake identities
  all connect to victim simultaneously
  8/8 peer slots filled

Step 3: Eclipse (send fake data)
  Sybil nodes send Tip { height: 9999, hash: "attacker" }
  victim receives and logs fake tip
  honest nodes broadcast real tip — victim never hears it

Step 4: State divergence confirmed
  victim tip ≠ honest network tip
  State diverged: true
```

---

## Key Code

### Sybil + Fake Tip

```rust
for i in 0..20 {
    tokio::spawn(async move {
        // connect with fake identity
        let mut stream = TcpStream::connect(target).await?;
        send Hello { node_id: NodeId::random(), listen_addr: "127.0.0.1:1900x" }
        read Hello reply

        // send fake chain state
        send Tip { height: 9999, hash: "attacker_tip_FAKE" }

        // hold connection to block honest peers
        sleep(3600s).await;
    });
}
```

### Measuring Divergence

```rust
let peers = query_peers(target).await;
let sybil = peers.iter().filter(|a| a.port() >= 19000).count();
let diverged = sybil == peers.len();  // 100% sybil = eclipsed
```

---

## What This Lab Does NOT Prove

This is a controlled simulation. Important limitations:

1. **No real consensus** — `Tip { height, hash }` is a simple message, not validated blockchain headers with proof of work or proof of stake.

2. **Random NodeId has no cost** — In this lab, identity is free. Real Sybil resistance requires proof of work, proof of stake, or signed persistent identities.

3. **Localhost IP** — All nodes share `127.0.0.1`. In a real network, Sybil nodes from different IPs are harder to detect but the attack principle is the same.

4. **No NAT or routing** — Real P2P attacks involve NAT traversal, IP spoofing, and BGP-level routing manipulation.

5. **Static network** — Our network doesn't churn. Real networks have constant peer rotation which makes Eclipse harder to maintain.

The lab demonstrates the **attack principle**, not a production exploit.

---

## Defensive Lessons

| Attack step | Defense |
|-------------|---------|
| Sybil fills slots | IP diversity scoring (Bitcoin addrman) |
| Fake identities | Persistent signed identities (Ed25519) |
| Peer slot exhaustion | Reserve slots for outbound connections |
| Fake chain state | Validate headers (PoW/PoS) before accepting |
| State divergence | Cross-check with multiple independent sources |

**Key insight:** Authentication (NodeId) does not create identity scarcity. Without a cost to creating identities, peer limits alone cannot ensure peer diversity.

---

## Blockchain Protocol Responses

| Protocol | Defense against Eclipse |
|----------|------------------------|
| **Bitcoin** | addrman bucketing by /16 subnet, feeler connections, anchor connections |
| **Ethereum** | discv5 with Kademlia DHT, ENR records with signed identity |
| **Tendermint** | Validator set known in advance — peer discovery less critical |

Bitcoin's **anchor connections** (introduced after the Eclipse Attack paper, 2015) maintain a small set of long-lived trusted peers that survive node restarts — making it much harder to fully eclipse a node.

---

## The Complete Experiment Output

```
=== Eclipse Attack Scenario ===
Target: 127.0.0.1:9000

[Before attack]
Victim peers: []

[Launching Sybil nodes...]

[After attack]
Victim peers:     [127.0.0.1:19000, :19002, :19003, :19004, :19005, :19006, :19011, :19012]
Honest peers remaining: 0
Sybil peers:            8
Victim received fake tip: height=9999 hash=attacker_tip_FAKE
Honest tip:               height=100  hash=honest
State diverged:           true
```

---

## Article Series Summary

| Phase | Topic | Key Lesson |
|-------|-------|-----------|
| 9 | P2P Node | Peer discovery is the foundation |
| 10 | Network Crawler | Attacker maps before attacking |
| 11 | Sybil Attack | Identity is cheap without cost |
| 12 | IDS Monitor | Peer concentration reveals the attack |
| 13 | Eclipse Attack | Information control = chain control |

**The throughline:** A node can only verify what its peers tell it. Control the peers, control the node's reality.
