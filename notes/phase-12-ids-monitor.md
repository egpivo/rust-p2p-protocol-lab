# Phase 12: IDS Monitor

## What We Built

An independent monitoring tool that periodically crawls a target node and detects Sybil/Eclipse patterns from the peer table.

```bash
# Terminal A — victim
cargo run -p p2p-node -- 9000

# Terminal B — monitor (defender's view)
cargo run -p p2p-lab -- monitor 127.0.0.1:9000

# Terminal C — attack (attacker's view)
cargo run -p p2p-lab -- sybil 127.0.0.1:9000 20
```

Output before attack:
```
peer list empty
peer list empty
```

Output after attack:
```
[monitor] peers=8 dominant_ip=127.0.0.1 (8/8 = 100%)
[WARN] Peer table concentration: 100% from 127.0.0.1
[ALERT] Possible Eclipse setup — all peers from same IP!
```

---

## Architecture

```
Defender runs monitor independently:

monitor → query_peers(target) every 5s
        → analyze IP distribution
        → alert if concentration ≥ 50%
        → alert if concentration = 100%
```

The monitor is **not embedded in the node** — it's a separate tool, like a network observer. This matches how real security monitoring works: defenders watch from outside, not inside the target.

---

## Detection Rules

### Rule 1: Peer Table Concentration

```rust
let mut ip_count: HashMap<IpAddr, usize> = HashMap::new();
for peer in &peers {
    *ip_count.entry(peer.ip()).or_default() += 1;
}

let concentration = dominant_count as f64 / peers.len() as f64 * 100.0;

if concentration >= 50.0  → [WARN]
if concentration >= 100.0 → [ALERT]
```

**What it detects:** When one IP dominates the peer table, it's a sign that either:
- A Sybil attack is underway (many fake identities from one host)
- An Eclipse setup is complete (victim only connected to attacker)

---

## Why Independent IDS?

Two design choices:

| Embedded IDS | Independent IDS (our approach) |
|-------------|-------------------------------|
| Built into p2p-node | Separate tool (p2p-lab monitor) |
| Node detects its own anomalies | Observer detects from outside |
| Attacker may evade by targeting IDS logic | Attacker doesn't know IDS exists |
| Simpler deployment | More realistic security model |

In real blockchain networks, security monitoring is done by:
- Node operators watching dashboards
- Third-party monitoring services
- Automated alerting systems

None of these are inside the node binary.

---

## Attacker vs Defender Tooling

```
Same binary (p2p-lab), different subcommands:

p2p-lab crawl   → attacker: map the network
p2p-lab sybil   → attacker: fill peer slots
p2p-lab monitor → defender: detect the attack
```

This design mirrors real security tools like Metasploit (offense) vs Snort (defense) — the same protocol knowledge powers both sides.

---

## Limitations of This IDS

### 1. localhost bias
All nodes share `127.0.0.1` — concentration is always 100% even for honest networks on localhost.

In the article, note:
> In this local lab, source IP is intentionally shared. The rule demonstrates the telemetry mechanism rather than a production-ready classifier. In a real network, Sybil nodes from different IPs would show lower concentration but still be detectable via other signals.

### 2. Polling vs event-driven
Monitor polls every 5 seconds — it misses events between polls. A production IDS would subscribe to connection events in real time.

### 3. Only one rule
Real IDS tools (Snort, Suricata) have hundreds of rules. Our three signals are:
- Peer table concentration ≥ 50% → WARN
- Peer table concentration = 100% → ALERT

Missing signals:
- Many NodeIds from same IP (requires Hello inspection, not just peer list)
- Rapid successive connections (requires event timestamps)
- Churn rate (connections per minute)

### 4. No response
This is IDS (detection only), not IPS (prevention). It alerts but doesn't block.
To make it IPS: add firewall rules when alert fires, or modify p2p-node to reject connections from flagged IPs.

---

## Detection Timeline

```
t=0s   Sybil attack starts
t=0s   20 Sybil nodes connect to victim
t=0s   Victim peer slots fill to 8/8
t=5s   Monitor polls → sees 8/8 from 127.0.0.1 → ALERT
t=10s  Monitor polls → still 8/8 → ALERT (persistent)
...
```

Each poll cycle confirms the attack is ongoing — not a one-time false positive.

---

## Blockchain Relevance

What real blockchain node monitoring looks like:

| Signal | Our lab | Production |
|--------|---------|------------|
| Peer IP diversity | IP concentration % | Subnet /16 bucket distribution |
| Connection rate | (not implemented) | Connections per minute per IP |
| NodeId churn | (not implemented) | New NodeIds per time window |
| Peer table health | concentration rule | addrman bucket analysis |

Tools like **Ethereum's devp2p** and **Bitcoin's addrman** implement sophisticated peer diversity scoring to resist Sybil attacks at scale. Our lab demonstrates the same principle at minimal scale.

---

## What's Next

Phase 13: Eclipse Attack — demonstrate that 100% Sybil occupancy leads to state divergence.

The victim's observed blockchain tip diverges from the honest network tip, showing the real consequence of a successful eclipse.
