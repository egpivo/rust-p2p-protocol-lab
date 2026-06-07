# Phase 8: Password Brute Forcer

## What We Built

A concurrent password brute forcer that tries credential pairs from a wordlist against a target service.

```bash
# Terminal A — target
cargo run -p honeypot

# Terminal B — attack
cargo run -p brute-forcer
```

Sample output:
```
Loaded 8 credentials pairs
failed: root / 123456
failed: admin / admin
...
```

---

## How It Works

```
wordlist.txt
  root,123456
  admin,admin
  ...
      ↓
Read into Vec<(String, String)>
      ↓
JoinSet — one task per credential pair
      ↓
try_login(addr, user, pass)
  → connect
  → read banner
  → send username
  → read Password prompt
  → send password
  → drain response until \n
  → check if "Authentication failed" in response
      ↓
Print SUCCESS / failed
```

---

## Pitfalls We Hit (Important)

### 1. TCP Has No Message Boundaries

TCP is a byte stream — the receiver has no guarantee about how data is chunked.

```
Sender writes:
  write("SSH-2.0-OpenSSH_9.6\r\n")
  write("login: ")

Receiver may read:
  "SSH-2.0-OpenSSH_9.6\r\nlogin: "   ← both together
  OR
  "SSH-2.0-OpenSSH_9.6\r\n"           ← banner only
  "login: "                            ← login prompt later
```

If you assume one `read()` = one message, you will get wrong results.

**Fix:** Don't rely on individual reads matching individual writes. Either:
- Read until a specific delimiter (`\n`)
- Drain until connection closes
- Use a framed protocol (like Phase 3)

### 2. Stale Read Discarding Data

```rust
// Bug: reads data and throws it away
let n = match stream.read(&mut buf).await {
    Ok(n) if n > 0 => n,
    _ => return false,
};
// buf contains "Authentication failed.\n" but n is never used
// next loop reads nothing → empty response → wrong result
```

Always use the data you read. If you do a `read()`, consume the result.

### 3. Inverted Logic

```rust
response.contains("Authentication failed")   // returns true when auth FAILED
!response.contains("Authentication failed")  // returns true when login SUCCEEDED
```

`try_login` should return `true` when login succeeds (server didn't reject).
Honeypot always rejects → all results should be `false` (failed).

### 4. `while let Some(Ok(...))` Silently Drops Errors

```rust
// Bug: if one task panics, Err(...) doesn't match Ok(...) → loop exits early
while let Some(Ok((user, pass, result))) = set.join_next().await { ... }

// Fix: handle all cases
while let Some(result) = set.join_next().await {
    match result {
        Ok((user, pass, true))  => println!("SUCCESS: {} / {}", user, pass),
        Ok((user, pass, false)) => println!("failed:  {} / {}", user, pass),
        Err(e) => println!("task error: {}", e),
    }
}
```

If any task panics, `join_next()` returns `Some(Err(JoinError))`. The `Ok` pattern
doesn't match → loop exits → remaining results never printed.

### 5. Concurrent Reads Cause False Positives

With multiple simultaneous connections, timing becomes unpredictable:

```
Connection A: read 2 gets "login: "   ← from its own earlier buffer
Connection A: read 3 (response loop) gets "Password: "  ← not "Authentication failed"
→ "Password: " doesn't contain "Authentication failed" → false positive SUCCESS
```

**Fix:** Drain the full response in a loop until `\n` or connection close:

```rust
let mut response = String::new();
loop {
    let n = match stream.read(&mut buf).await {
        Ok(0) | Err(_) => break,
        Ok(n) => n,
    };
    response.push_str(&String::from_utf8_lossy(&buf[..n]));
    if response.contains('\n') { break; }
}
```

This accumulates all data until a complete line arrives, regardless of how many
TCP segments it took.

---

## Key Concepts

### `Vec<(String, String)>` — Vector of Tuples

```rust
// Wrong — Vec takes ONE type parameter
Vec<String, String>

// Correct — tuple is one type
Vec<(String, String)>
```

### `splitn(2, ',')` — Split with Limit

```rust
"root,123,456".split(',')        // ["root", "123", "456"]  ← splits all commas
"root,123,456".splitn(2, ',')    // ["root", "123,456"]     ← splits only first comma
```

`splitn(2, ',')` is safer for passwords that contain commas.

### `filter_map` — Parse and Skip Errors

```rust
content.lines().filter_map(|line| {
    let mut parts = line.splitn(2, ',');
    let user = parts.next()?.to_string();  // ? returns None if missing
    let pass = parts.next()?.to_string();
    Some((user, pass))
})
// lines that don't have a comma are silently skipped
```

### Semaphore — Concurrency Limiter

```rust
let sem = Arc::new(Semaphore::new(5));  // max 5 concurrent connections

set.spawn(async move {
    let _permit = sem.acquire().await.unwrap();  // wait for a slot
    let result = try_login(...).await;
    // _permit drops here → slot freed → next task can proceed
});
```

Without a semaphore: all 8 tasks connect simultaneously → TCP timing issues → wrong results.
With Semaphore(5): at most 5 concurrent connections → more predictable timing.

---

## Real Brute Force Tools

| Tool | Protocol | Notes |
|------|----------|-------|
| **Hydra** | SSH, HTTP, FTP, many more | Most common, very fast |
| **Medusa** | SSH, HTTP, FTP | Similar to Hydra |
| **Burp Suite** | HTTP/HTTPS | Web app focused, proxy-based |
| **hashcat** | Offline hash cracking | CPU/GPU based, no network |

Your implementation is the same concept as Hydra, just for one protocol.

---

## Defensive Countermeasures

| Attack | Defense |
|--------|---------|
| Brute force | Rate limiting — delay after failed attempts |
| Credential stuffing | Account lockout after N failures |
| Tool identification | Check connection patterns (bots connect fast, humans slow) |
| Password guessing | Require strong passwords, MFA |

Our IDS (Phase 6) already detects brute force:
```
same IP, same port, 5+ attempts in 30s → ALERT
```

---

## Blockchain Relevance

Blockchain RPC endpoints (like Ethereum's `eth_sendTransaction`) are HTTP-based.
An attacker might brute force:
- Node admin API passwords
- Wallet unlock passphrases
- JSON-RPC authentication tokens

Defending a blockchain node means:
1. Rate limiting RPC calls per IP
2. Requiring authentication (API keys, JWT)
3. Firewall: only allow known IPs to reach RPC port (8545 for Ethereum)

---

## Testing

```bash
# Start honeypot
cargo run -p honeypot

# Run brute forcer
cargo run -p brute-forcer

# Check what honeypot captured
cat honeypot.log
```

To test SUCCESS detection: modify the honeypot to accept one credential pair
and verify the brute forcer reports it as SUCCESS.
