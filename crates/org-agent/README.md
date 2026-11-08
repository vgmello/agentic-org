# `org-agent` & `org-relay` (`org-msg` v0)

Fast, Maildir-style spool messaging CLI (`org-agent`) and relay daemon (`org-relay`) for cross-CLI agent communication (ADR-0009).

## Architecture & Dependency Direction

```text
org-agent (bin, Ce = 1) ─┐
                         ├─▶ org-msg (lib, Ce = 2: serde + serde_json, Ca = 2, I ≈ 0.2)
org-relay (bin, Ce = 1) ─┘
```

- **`org-msg`:** Owns the v1 `Envelope` contract, monotonic Crockford Base32 ULID generator, and `Spool` behavioural operations (`register`, `deregister`, `is_registered`, `enqueue`, `deliver_one`, `deliver_all`, `list_inbox`, `claim`, `claim_next`, `find_for_reply`, `wait_for_reply_with_listener`, `wait_for_inbox`, `prune_old_cur`).
- **`org-agent`:** Stateless CLI that writes only to the local outbox (`outbox/tmp` -> `fsync` -> `outbox/new`), nudges `relay.sock`, and reads/watches its own inbox (`inbox/<agent>/{new,cur,wake}`).
- **`org-relay`:** Single-instance relay daemon (`flock` on `relay.lock`) that drains `outbox/new` into `inbox/<to>/new` (or `dead/<id>.json` + `<id>.reason` when expired, unregistered, or malformed) and wakes active listeners in `inbox/<to>/wake/<pid>-<ulid>.sock`.

## Benchmark Results (`scripts/bench-msg.sh`, Apple Silicon macOS)

| Metric | Target (ADR-0009) | Measured |
| :--- | :--- | :--- |
| **`org-agent` release binary (stripped)** | `< 2 MB` | **554.8 KiB** (`568,064` bytes) |
| **`org-relay` release binary (stripped)** | `< 2 MB` | **502.9 KiB** (`515,008` bytes) |
| **`org-agent msg send` (1,000 runs, fire-and-forget)** | `p50 < 5 ms` | **p50: 2.75 ms** · **p95: 3.11 ms** |
| **`send` $\rightarrow$ delivered in `inbox/<to>/new` (200 runs)** | `p50 < 20 ms` | **p50: 4.60 ms** · **p95: 4.81 ms** |
| **`ask` $\leftrightarrow$ `reply` round trip (100 runs)** | `p50 < 50 ms` | **p50: 12.33 ms** · **p95: 14.75 ms** |
