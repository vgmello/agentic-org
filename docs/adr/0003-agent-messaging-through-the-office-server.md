# ADR-0003 · Agent messaging through the office server

Status: **proposed** 2026-11-08. Owner: ada. Builds on ADR-0001 decision 4 (the user: the office server is the agents' message hub). It is the "separate decision" ADR-0001 decision 2 asks for: messages move into the database, each organization's into its own (ADR-0002 decision 0). It follows ADR-0002 (encrypted SQLite per organization, one writer each, all SQL in `store.rs`). It replaces the relay design running today (`org-agent` + `org-relay` from branch `v1-old`).

## Context
- Agents run in different CLIs (Claude Code, Antigravity, Codex) and use `org-agent msg send | ask | wait | inbox | read | reply | register | deregister` and `org-agent whoami`.
  - Exit codes today: 0 ok, 1 usage, 2 timeout, 3 unknown recipient, 4 I/O error, 130 interrupted.
  - `wait` doesn't mark a message read, and it takes `--from`.
- **Today:** `org-agent` writes each message into a spool folder under `~/.agency`. `org-relay` moves it into the recipient's inbox folder and wakes listeners through per-listener sockets, with a polling backstop. Measured: `send` p50 2.75 ms, delivery 4.6 ms, `ask` round trip 12.3 ms.
  - Those sends use a plain `fsync`, which on macOS doesn't flush the drive's cache.
  - Messages are unencrypted files.
- **The user's direction:** the office server is the hub. It holds the organizations, their encrypted databases (ADR-0002) and long-lived client connections, so it can hand a message straight to a waiting connection.

## Decisions

### 1. Components
- **`org-agent` → `org-client` → `org-proto`, plus `org-spool`** for the offline queue (decision 7).
  - `org-client` is the one blocking client library, shared with the app's Tauri shell. It keeps `subscribe` and reconnect logic in code the CLI never calls.
  - Message ids are ULIDs from the `ulid` crate (exact pin), used by `org-client` and `org-office`.
  - Target: `org-agent` release binary < 1.5 MB.
- **The mailbox is an `org-office` capability.** It holds the rules: the directory of agents, sending, read state, reply matching, limits, expiry. It also holds the **waiters**: a held `wait` or `ask` is a handle the mailbox completes when a matching message commits. The server only parses requests, holds connections and passes them to the mailbox. SQL stays in `store.rs`.
- The **server binary** reads the offline spool through `org-spool` and gives each entry to the mailbox. `org-office` doesn't touch the spool.

### 2. Identity and organization
- After `hello`, a messaging connection calls **`msg.bind { org?, agent? }`**. Every later `msg.*` call works in that organization and, where it needs one, acts as that agent.
  - `agent` comes from `--as` or `$ORG_AGENT`, as today. `register`, `deregister` and the app's owner view need only the organization. Every other call needs an agent and fails with `not_bound` without one.
  - `org` comes from `$ORG_ORGANIZATION` and holds the organization's **id**. If it's absent, it defaults to the server's only organization, and it fails clearly if there are several. So once a second organization exists, every agent must set `$ORG_ORGANIZATION`, and the hire flow will set it for agents it starts.
  - Binding never registers. As today, only `wait` and `ask` register their caller automatically, and `msg.register` registers explicitly. The name `office` is reserved and can't be bound or registered.
- The socket is 0600, so only the user's own processes connect, but any of them can claim any agent name. That's cooperative, not a security boundary, the same as today. Per-agent credentials come with remote access.
- Phase 1 delivers only within one organization (open question 1).

### 3. Calls (added to `org-proto`; API version stays 1)
- **`msg.bind`:** see above. **`msg.register { agent }` / `msg.deregister { agent }`** maintain the directory until the organization's roster replaces it.
- **`msg.send { id, to, kind, subject?, body, meta?, reply_to?, expires_at? } → { id, seq }`.**
  - `id` is a ULID the client makes. A repeated `id` from the same sender in the same organization, with the same `to`, `kind`, `subject`, `body`, `meta` and `reply_to`, returns the original result. Other fields, such as `timeout_ms` and `expires_at`, don't count. The same `id` with different content fails with `duplicate_id`. Ids are remembered as long as their message is kept.
  - `seq` is the server's commit order. Each recipient sees messages in `seq` order. There's no order across recipients. A send that went to the offline spool gets its `seq` at ingest, so it can land after the same sender's later live sends.
  - `kind` is free text, except `delivery_failed`, which only the server sends (decision 7).
- **`msg.reply { id, to_id, body, … } → { id, seq }`:** the server finds the original and addresses the reply to its sender. Only the original's recipient may reply (`not_found` otherwise, as today). `id` dedups a retried reply, like `send`.
- **`msg.wait { timeout_ms, from? } → { message | null }`** (`null` at the final deadline: the CLI exits 2, as today):
  - It returns the next unread message for the caller (optionally from one sender) **without** marking it read, as today. It returns at once if one is already waiting, and otherwise holds until one commits or the timeout passes.
  - To miss nothing, the mailbox registers the waiter (with the last `seq` it checked) **before** it checks for unread messages, then checks again.
  - Several waits for the same agent all see the same message. Marking it read is `msg.read`'s job.
- **`msg.read { id? } → { message | null }`:** marks the message read and returns it. With no `id`, it takes the next unread one, or returns `null` if there are none, as `read --next` does today. Picking and marking happen in one step on the writer thread, so two concurrent readers never get the same message. Marking read is a durable commit.
- **`msg.inbox { unread_only?, cursor?, limit≤100 } → { headers, next_cursor? }`:** headers only (id, seq, from, kind, subject, reply_to, created_at, size, read). Bodies come only through `read`, so a listing always fits a frame.
- **`msg.ask { …send fields, timeout_ms } → { reply }`:**
  - It sends, then holds until a **reply from the original recipient** (`reply_to` = this id, `from` = the asked agent) commits. Other messages with that `reply_to` are ordinary messages.
  - The reply is **not hidden** from other calls. As today, `wait` and `read` with no id can see it too. Agents that run a background `wait` alongside an `ask` may see the reply twice, which is the same as today.
  - The server returns the reply to the ask first, then marks it read in the next commit, so the hand-off doesn't wait for a flush. If the server crashes in between, the reply is still unread afterwards: delivery is **at least once**.
  - `ask` is idempotent by id. If the connection drops or the server restarts, the CLI reconnects with backoff until its own deadline and re-asks with the **same id** and the remaining time. That re-attaches without sending again, and returns a reply that already arrived, even one already marked read. An `ask` longer than an hour is split the same way.
  - An interrupted `ask` (Ctrl-C) just disconnects. Nothing is held for it.
  - On timeout it fails with `timeout`.
- **For the app (owner view):** `msg.list { org, agent?, cursor?, limit≤100 } → { headers, next_cursor? }` and `msg.get { org, id } → { message }`. Both are read-only, need no `msg.bind`, and never change read state. Plus the event `message.received { org, id, msg_seq, from, to, kind, subject }` (`msg_seq` so it isn't confused with the event's own `seq`). Bodies stay out of events.
- **New error codes:** `unknown_recipient`, `timeout`, `too_large`, `not_found`, `duplicate_id`, `not_bound`.

### 4. Limits
- Every field is capped **as encoded JSON**. JSON escaping can grow text up to 6x, so the cap is on what goes over the wire:
  - `body` ≤ 256 KiB
  - `meta` ≤ 64 KiB
  - `subject` ≤ 1 KiB
  - `kind`, agent names and ids ≤ 128 B
  - organization names ≤ 256 B (64 characters of up to 4 bytes)

  So a whole message fits in the 1 MiB frame. A page of 100 headers fits too: each header is under 2 KiB.
- `timeout_ms` ≤ 1 hour per request, and at most 16 held requests per connection. The client's own timeout is `timeout_ms` + 5 s. The CLI splits longer timeouts (e.g. `wait --timeout 2h`) into consecutive requests.
- Retention is open question 3. Unread messages stay until read or expired.

### 5. Durability
A send is acknowledged only after its commit, with `fullfsync` and batched commits per ADR-0002. Marking read is a commit too. A faster "accepted, not yet durable" mode is open question 4, not a default.

### 6. CLI compatibility
- **Unchanged:** commands, flags, exit codes 0–4 and 130, and `whoami` (local, no server).
- **New:** exit **5, "office server unavailable"**.
- **Error codes map to exit codes:**

  | Error | Exit |
  | --- | --- |
  | `timeout` | 2 |
  | `unknown_recipient` | 3 |
  | `invalid_request`, `too_large`, `duplicate_id`, `not_found`, `not_bound` | 1 |
  | `storage_error`, `unknown_method`, unknown codes, and replies that can't be decoded (e.g. an older or newer server) | 4 |
  | connection failure | 5 |

  `read --next` with nothing unread still exits 0 and prints `null`.
- **Deliberate changes:**
  - `inbox --json` lists headers, not full messages; `read` gives the body.
  - `register` and `deregister` now need an organization (bound by default, decision 2).
  - The 256 KiB body cap is now measured as encoded JSON, so bodies with many escaped characters that pass today may be rejected.
- A send queued offline (decision 7) prints its normal result, plus `"queued": true` in `--json` and a note on stderr.

### 7. When the server isn't reachable
- **Only `send` falls back to the spool**, and only when connecting fails (`ENOENT`, `ECONNREFUSED`): never after a request was written, and never on an API error.
  - If the connection drops after the request was written, the CLI retries over the socket with the same id for up to 10 seconds. Dedup makes that safe.
  - If it still can't reach the server, it spools with the same id.
- **`reply`, `ask`, `wait`, `read` and `inbox`** exit 5. `reply` needs the original message to address it, and `ask`/`wait` need the server.
- **Unknown recipient while offline:** the CLI can't check, so it exits 0, and the server reports it later. This is the one weaker guarantee offline, and it's documented.
- **The spool format** is a second contract, so it is defined in `org-proto` as a versioned `SpooledSend { v, org, from, …send fields }`.
  - The folder is mode 0700, under the office home.
  - The CLI writes into `spool/tmp/` (which ingest never scans), flushes the file with `F_FULLFSYNC`, renames it into `spool/new/`, then full-flushes that folder. Only then does it exit 0. Ingest never sees half a file, and a power cut can't undo a send that already reported "queued".
  - `org` may be `null` when `$ORG_ORGANIZATION` is unset, and the server resolves it at ingest, as `bind` does.
  - Files are read with a size cap and `O_NOFOLLOW`, and deleted only after the commit that stores them.
- **Ingest:**
  - The server drains the spool **before it starts listening**, then scans it every 5 s as a backstop. That's for sends that raced a server start. It isn't delivery polling: delivery to waiters is immediate.
  - Spooled sends keep their ids, so dedup applies, and they get `seq` in ingest order.
- **Undeliverable messages** (an unknown recipient, expired, unreadable) are kept in the database with a reason. The sender is told by a `delivery_failed` message from the reserved sender `office`. Nothing ever generates one in reply to another `delivery_failed`, and clients can't send that kind.

### 8. Migration from the running relay
Prerequisite: the server can create an organization (the create-organization work).

1. The new `org-agent` passes the old integration tests, adapted, against a real server.
2. Stop `org-relay`. Old `wait`/`ask` processes still running stop receiving and hang until their timeout, so agents restart them.
3. The server imports the old spool (`org-office-server import-spool --org <id>`), keeping read state. That covers:
   - registrations
   - inbox messages
   - anything waiting in the old `outbox/new`, routed as sends
4. Install the new `org-agent`, then run the import again to catch writes from old CLIs in between. Dedup makes repeats safe.
5. Rollback: keep the old binaries beside the new one (`org-agent.v1-old`, `org-relay.v1-old`) until the new path has run cleanly for a week. Messages sent through the server in that week don't go back to the old system. Rolling back loses them unless an export is written first.
6. When the rollback window closes, **delete the old plaintext copies** under `~/.agency/spool`.

### 9. Targets (measured on a spike before the build is approved)
- `send` p50 < 20 ms (process start + connect + `hello` + `bind` + one durable commit).
- `read` p50 < 20 ms.
- `wait`/`ask`: hand-off to a waiter < 1 ms after the commit (read state is committed afterwards).
- `ask` round trip p50 < 50 ms, the same target as today's design, not counting the replier's own latency. It includes two flushes in series (the ask and the reply).

A full flush (`F_FULLFSYNC`) is reported at roughly 5 ms to over 20 ms on Apple SSDs, and a request may also wait for a batch already flushing. So these targets are at risk. The spike reports the cost per flush. If it misses the targets, open question 4 decides. We don't quietly weaken durability.

## Options considered
| Option | Why not |
| --- | --- |
| Keep the spool and relay, with the server taking over the relay's job | File-per-message storage (unencrypted, a second source of truth) and the wake-socket machinery stay. |
| `org-agent` writes into the database itself | Breaks ADR-0001/0002: only the server opens the database, and the key stays with it. |
| An async runtime in `org-agent` | One blocking request at a time is enough. It would cost start-up time and size. |
| No offline fallback | Simpler, but fire-and-forget would fail during a server restart. Offline queueing was the user's original requirement ("local queue → server → destination"). |
| `wait` marks messages read | It changes today's behaviour, and a message pushed just as the connection drops would be lost. |
| Hide an `ask`'s reply from `wait`/`read` until the ask returns | It needs stored deadlines, a timer to un-hide, a cancel path and long-ask handling, and it changes today's behaviour. Idempotent re-ask already makes `ask` reliable. |

## Consequences
- Messages live in one place, encrypted. The app shows them through `msg.list`/`msg.get` and `message.received`, without impersonating agents.
- `org-relay` and the wake sockets go away.
- `send` and `read` get slower but truly durable. Waiters wake with no polling.
- `org-proto` grows by the calls above and `SpooledSend`, `org-office` by the mailbox (with its tables in `store.rs`), and `org-client` by typed message calls.

## Open questions for the user
1. Phase 1 delivers only within one organization. Is cross-organization messaging ever needed? (Proposal: no for now.)
2. Should you be a participant (send to and receive from agents in the app), or only a viewer for now? (Proposal: viewer now, participant next.)
3. Keep read messages for 30 days, or keep everything as the organization's history? (Proposal: keep everything, encrypted, and add a retention setting if size becomes a problem.)
4. If durable sends turn out too slow, may a send be acknowledged once the server has accepted it in memory, before it's on disk? (Proposal: no, unless the spike shows a real problem. A power cut could then lose the last messages.)
