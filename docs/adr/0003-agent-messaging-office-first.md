# ADR-0003 · Agent messaging: office-first, high-volume storage, directory, presence, triage, and cross-office routing

Status: **proposed** 2026-11-08. Author: `agy-senior-dev`.
Builds on [ADR-0001](0001-app-is-a-client-office-is-a-server.md) (decision 4: the office server is the agents' message hub), [ADR-0002](0002-storage-sqlite-sqlcipher-one-writer.md) (SQLCipher, `org.db` + `office.db`, one batching writer with `fullfsync`), [`offices-hq-and-satellites.md`](../design/offices-hq-and-satellites.md) ("Office-first: tasks and messages"), and [`teams-and-org-chart.md`](../design/teams-and-org-chart.md) (who agents may interact with).
Supersedes the `v1-old` file-spool relay (`org-relay` + `~/.agency/spool`) and the pre-offices ADR-0003 draft.

---

## Context

- Agents run across multiple CLIs (Claude Code, Antigravity, Codex) and coordinate continuously using `org-agent`.
- Under the approved **offices model** ([`offices-hq-and-satellites.md`](../design/offices-hq-and-satellites.md)):
  - An organization has **offices**; the first is **HQ** (authoritative organization structure in `org.db`) and the rest are **satellites** (local or remote) holding a full copy of `org.db`.
  - **One office = one `org-office-server` process**, supervised by `org-office-host` (the single macOS login service). Each office owns `<office_home>/offices/<office_id>/{office.json, org.db, office.db, workspace/}` and listens on `<office_home>/run/<handle>.sock` (mode `0600`).
  - **Agent messages are office-first work data:** an agent runs on its office's machine and talks only to its own office. Same-office messages commit and wake waiting recipients locally in milliseconds, even if HQ is unreachable. Cross-office messages commit locally first, stream to HQ over the office's persistent connection, and are forwarded by HQ to the recipient's office (`office → HQ → office`).
  - **Recipient rules** ([`teams-and-org-chart.md`](../design/teams-and-org-chart.md)): an agent may message or `ask` another **agent** of the organization, a **person** added to the organization, or the **owner** (`owner: true`). Anyone else is rejected with `unknown_recipient`.
  - **Directory, Presence, Humans, and Triage:** Agents need to discover who is in the organization and what they are doing (`org-agent who`, `org-agent status`), communicate seamlessly with humans through the Agentic Org desktop app, and triage incoming messages without being interrupted during focused work except for urgent requests (`--priority high|urgent`, `msg defer`, `msg done`, `msg notify`).
- **Heavy traffic requirement:** In an active organization, dozens of agents exchange prompts, review diffs, logs, and `ask`/`reply` calls continuously (10,000–100,000+ messages/day, bodies up to 256 KiB). Storing uncompressed bodies inline with headers or scanning full tables for unread/due messages would bloat SQLCipher page I/O and slow down both the CLI and the desktop app.

---

## Decisions

### 1. High-Volume Storage Architecture in `office.db`

All message tables live in each office's **`office.db`** (never `org.db`, so replacing `org.db` with an HQ snapshot never touches the office's messages or queues). All SQL and `rusqlite` types remain private inside `crates/org-office/src/store.rs` (ADR-0002 decision 5), with `PRAGMA foreign_keys = ON` on every connection.

#### 1.1 Table Separation: Hot Unread/Snooze Index, Fixed-Width Headers, Cold Compressed Bodies, and Threads

To keep `inbox`, `notify`, `wait`, `read --next`, and UI thread views sub-millisecond even at $10^5$–$10^6$ stored messages, message storage in `office.db` is split into five messaging tables plus the office runtime's `member_status` table and the shared per-origin work stream owned by `crates/org-sync`:

1. **`msg_threads` (conversation summary per thread) and `msg_thread_participants`**
   - Every top-level message (`reply_to IS NULL`) starts a thread with `thread_id = id`.
   - Every reply (`reply_to = parent_id`) inherits `thread_id = parent.thread_id` while keeping `reply_to = parent_id`. Deeply nested `ask`/`reply` chains stay grouped in one conversation thread while preserving exact parent-child causality.
   - Columns of `msg_threads`:
     - `thread_id TEXT PRIMARY KEY` (ULID of the root message)
     - `subject TEXT` (subject of the root message, or earliest known message in the thread on a partial satellite)
     - `created_at TEXT NOT NULL` (RFC 3339 UTC, `YYYY-MM-DDTHH:MM:SS.mmmZ`)
     - `last_msg_id TEXT NOT NULL`
     - `last_msg_at TEXT NOT NULL`
     - `message_count INTEGER NOT NULL`
     - `participants_json TEXT NOT NULL` (compact JSON array of distinct member IDs in the thread)
   - Index: `INDEX idx_msg_threads_last ON msg_threads(last_msg_at DESC, thread_id DESC)`.
   - Participant junction table for fast app `msg.threads { participant }` filtering without table scans:
     - `msg_thread_participants(member_id TEXT NOT NULL, thread_id TEXT NOT NULL REFERENCES msg_threads(thread_id) ON DELETE CASCADE, PRIMARY KEY (member_id, thread_id))`
   - **Out-of-order / partial-scope upsert rules:** Because a satellite may receive a mid-thread message without the root message, or receive backfilled messages after live messages:
     - Inserting a new row into `msg_headers` creates the `msg_threads` row if absent.
     - On conflict (`thread_id` exists), `message_count` increments by 1 (only when `msg_headers` actually inserts a new row, never on a deduplicated replay), `created_at = MIN(msg_threads.created_at, excluded.created_at)`, `subject` is updated if `excluded.id == thread_id` or `msg_threads.subject IS NULL`, and `(last_msg_at, last_msg_id)` updates only when `(excluded.created_at, excluded.id) > (msg_threads.last_msg_at, msg_threads.last_msg_id)`.

2. **`msg_headers` (small, fixed-width metadata — never holds `body` or `meta`)**
   - Columns:
     - `id TEXT PRIMARY KEY` (client-made ULID)
     - `thread_id TEXT NOT NULL`
     - `reply_to TEXT` (parent message ULID, or `NULL`)
     - `origin_office_id TEXT NOT NULL` (the sender office's ID)
     - `origin_incarnation TEXT NOT NULL` (the sender office's enrollment/restore incarnation ID, per `offices-hq-and-satellites.md`)
     - `origin_seq INTEGER NOT NULL` (monotonic work-change sequence allocated by the sender office in the same transaction)
     - `local_seq INTEGER NOT NULL UNIQUE` (monotonic commit sequence at *this* office, used for local waiter watermarks and pagination cursors)
     - `org_epoch INTEGER NOT NULL`, `org_seq INTEGER NOT NULL` (the `Position` the sender office had applied in `org.db` when accepting the message)
     - `from_member_id TEXT NOT NULL` (member ID, or reserved `'office'` for `delivery_failed` notices)
     - `from_display_name TEXT NOT NULL` (sender's display name at send time)
     - `to_member_id TEXT NOT NULL` (member ID, or raw addressed recipient if dead-lettered from offline spool)
     - `to_display_name TEXT NOT NULL` (recipient's display name at send time, or raw `--to` string as addressed by the sender, preserved so `delivery_failed` notices and the UI can show the exact name even after the member is removed)
     - `kind TEXT NOT NULL` (`msg`, `request`, `reply`, `delivery_failed`, or custom tag)
     - `priority TEXT NOT NULL DEFAULT 'normal'` (`'normal'`, `'high'`, or `'urgent'`; for `kind = 'request'`, defaults to `'urgent'`)
     - `subject TEXT`
     - `created_at TEXT NOT NULL`
     - `expires_at TEXT` (set on `request` / `ask` messages from `created_at + ttl_ms`)
     - `ttl_ms INTEGER` (request deadline duration in ms from creation for `request` / `ask` — see §3.3)
     - `body_bytes INTEGER NOT NULL` (uncompressed UTF-8 byte length of `body`)
     - `meta_bytes INTEGER NOT NULL DEFAULT 0` (uncompressed UTF-8 byte length of `meta`)
     - `content_hash BLOB NOT NULL` (32-byte SHA-256 digest of the canonical length-prefixed encoding defined in §3.4 for $O(1)$ retry deduplication without reading `msg_bodies`, surviving body pruning)
     - `flags INTEGER NOT NULL DEFAULT 0` (bitmask: `1 = EXPIRED`, `2 = UNRESOLVED_REF`, `4 = DEAD_LETTER`, `8 = EXPIRED_WAITING_HUMAN`)
     - `dead_reason TEXT` (`NULL` unless `DEAD_LETTER` or `EXPIRED` is set)
   - Indexes:
     - `UNIQUE INDEX idx_msg_origin ON msg_headers(origin_office_id, origin_incarnation, origin_seq)` (idempotent cross-office stream dedup)
     - `UNIQUE INDEX idx_msg_delivery_failed_once ON msg_headers(reply_to) WHERE kind = 'delivery_failed'` (guarantees at most one `delivery_failed` notice per original message ID)
     - `INDEX idx_msg_to_seq ON msg_headers(to_member_id, local_seq DESC)` (recipient inbox history)
     - `INDEX idx_msg_thread_seq ON msg_headers(thread_id, local_seq DESC)` (thread pagination by `local_seq`)
     - `INDEX idx_msg_reply_to ON msg_headers(reply_to) WHERE reply_to IS NOT NULL` (fast `ask` reply lookup on reconnect)
     - `INDEX idx_msg_unresolved ON msg_headers(org_epoch, org_seq) WHERE (flags & 2) != 0` (messages waiting for `org.db` to reach `(org_epoch, org_seq)`)

3. **`msg_bodies` (cold payload table, stored on separate B-tree pages from `msg_headers`)**
   - Columns:
     - `id TEXT PRIMARY KEY REFERENCES msg_headers(id) ON DELETE CASCADE`
     - `compression INTEGER NOT NULL` (`0 = raw UTF-8`, `1 = raw DEFLATE via flate2/miniz_oxide`)
     - `body_blob BLOB NOT NULL`
     - `meta_blob BLOB` (`NULL` when `meta` is empty)
   - Keeping `msg_bodies` separate ensures that scanning 100 rows in `msg_headers` (`~260 B/row` $\approx 26\text{ KiB}$, ~7 SQLite pages) never touches or decrypts overflow pages containing 256 KiB message bodies.

4. **`msg_unread` (tiny hot table of currently open unread & snoozed messages, owned by the recipient's office)**
   - Columns:
     - `to_member_id TEXT NOT NULL`
     - `local_seq INTEGER NOT NULL`
     - `id TEXT NOT NULL UNIQUE REFERENCES msg_headers(id) ON DELETE CASCADE`
     - `from_member_id TEXT NOT NULL`
     - `kind TEXT NOT NULL`
     - `priority TEXT NOT NULL DEFAULT 'normal'`
     - `urgent INTEGER NOT NULL DEFAULT 0` (`1` iff `kind == 'request'` or `priority IN ('high', 'urgent')`)
     - `triage_gen INTEGER NOT NULL DEFAULT 0` (monotonic per-message triage generation counter; starts at `0` on arrival and increments whenever a snooze is set or released so hooks can deduplicate `(id, triage_gen)` per snooze cycle)
     - `reply_to TEXT`
     - `created_at TEXT NOT NULL`
     - `expires_at TEXT`
     - `snoozed_until TEXT` (`NULL` unless snoozed to an RFC 3339 UTC timestamp via `msg defer --until <duration|ISO>`)
     - `snooze_after_task INTEGER NOT NULL DEFAULT 0` (`1` when snoozed via `msg defer --until after-task`, `0` otherwise — kept separate from `snoozed_until` so timestamp comparisons on `snoozed_until` never mix RFC 3339 strings with string literals)
     - `snooze_note TEXT` (optional note from `msg defer --note`)
     - `triaged_at TEXT` (`NULL` when new/untriaged; set when snoozed or when a due snooze is released)
     - `PRIMARY KEY (to_member_id, local_seq)`
   - Indexes:
     - `INDEX idx_msg_unread_due ON msg_unread(to_member_id, snoozed_until) WHERE snoozed_until IS NOT NULL`
     - `INDEX idx_msg_unread_after_task ON msg_unread(to_member_id) WHERE snooze_after_task = 1`
   - **Why a separate table instead of `WHERE read = 0`:** Even after 500,000 messages are read or marked done, `msg_unread` contains only currently open (unread or snoozed) messages (typically $0$–$50$ rows, fitting in a single 4 KiB page in the WAL cache).
   - `msg.wait`, `msg.notify`, `msg.read --next`, `msg.inbox`, and unread/needs-you counts in the UI touch only `msg_unread` (+ `msg_headers` for subject/names).

5. **`msg_state` (current lifecycle state per message, upserted by the recipient's owning office)**
   - Per [`offices-hq-and-satellites.md`](../design/offices-hq-and-satellites.md), a message is created by the sender's office and its header/body never change; **the recipient's owning office owns its post-creation lifecycle state** (for people and the owner, HQ is their office; if a recipient was already removed in `org.db` before delivery, HQ owns the dead-letter transition).
   - Because triage allows reading a message (`msg read <id>`) and then either snoozing it (`msg defer <id>`, which restores it to `msg_unread`) or marking it done (`msg done <id>`), `msg_state` stores the message's **current** lifecycle state via **UPSERT** (`INSERT INTO msg_state ... ON CONFLICT(id) DO UPDATE`) rather than insert-once:
     - `id TEXT PRIMARY KEY REFERENCES msg_headers(id) ON DELETE CASCADE`
     - `state TEXT NOT NULL` (`'unread'`, `'snoozed'`, `'read'`, `'done'`, `'expired'`, or `'dead_letter'`)
     - `triage_gen INTEGER NOT NULL DEFAULT 0`
     - `snoozed_until TEXT` (`NULL` unless `state = 'snoozed'` with a UTC deadline)
     - `snooze_after_task INTEGER NOT NULL DEFAULT 0` (`1` iff `state = 'snoozed'` until `after-task`)
     - `reason TEXT` (optional `snooze_note` when `'snoozed'`/`'unread'`, optional `done` note when `'done'`, or `'expired'` / `'unknown_recipient'`)
     - `updated_at TEXT NOT NULL`
     - `owner_office_id TEXT NOT NULL`
     - `origin_incarnation TEXT NOT NULL`
     - `origin_seq INTEGER NOT NULL` (latest work-change sequence that mutated this row)
     - `org_epoch INTEGER NOT NULL`, `org_seq INTEGER NOT NULL` (the `Position` in `org.db` at which the owning office recorded this transition)
   - **Initial row & allowed lifecycle transitions (`L-b`):**
     - **Initial arrival:** Seeded in `msg_state` with `state = 'unread', triage_gen = 0`, using the `MessageCreated` origin triple `(origin_office_id, origin_incarnation, origin_seq)` and `(org_epoch, org_seq)` from `msg_headers` (so `idx_msg_state_origin` always has a valid unique key even at non-owning offices; if a reply is claimed directly by a live `msg.ask` waiter before commit per §2.2, its initial state at the asker's office is `'read'` and it is not inserted into `msg_unread`).
     - `unread | snoozed → read` (`msg.read` or `msg.reply` on an unread or snoozed message): deleted from `msg_unread`, upserted in `msg_state` (`state = 'read'`).
     - `unread | read | snoozed → snoozed` (`msg.defer`): upserted into `msg_unread` (restoring the row if it was in `'read'`) with `triage_gen = triage_gen + 1` and `snoozed_until` / `snooze_after_task`, and upserted in `msg_state` (`state = 'snoozed'`).
     - `snoozed → unread` (snooze timer due or `--after-task` release): `snoozed_until = NULL, snooze_after_task = 0, triage_gen = triage_gen + 1, triaged_at = now_utc` in `msg_unread`, and upserted in `msg_state` (`state = 'unread'`).
     - `unread | read | snoozed → done` (`msg.done`): deleted from `msg_unread`, upserted in `msg_state` (`state = 'done'`, terminal).
     - `unread | snoozed → expired | dead_letter`: deleted from `msg_unread`, upserted in `msg_state` (`state = 'expired'` or `'dead_letter'`, terminal).
   - **Cross-owner total ordering key (`M-E`):** Every state transition is ordered by the tuple `(org_epoch, org_seq, origin_seq, owner_office_id, origin_incarnation)`. An incoming `WorkChange::MessageStateChanged` (including across an agent move from Office A to Office B or across an office restore incarnation) updates `msg_state` and `msg_unread` iff its `(org_epoch, org_seq, origin_seq, owner_office_id, origin_incarnation)` is strictly greater than the stored row's tuple.
   - Index: `UNIQUE INDEX idx_msg_state_origin ON msg_state(owner_office_id, origin_incarnation, origin_seq)`.

6. **`member_status` (agent self-reported status lines in `office.db`, owned by `org-office` runtime)**
   - Stores each local agent's short status line set via `org-agent status "<text>"` so it survives local `org-office-server` restarts without polluting `org.db`'s permanent event log:
     - `member_id TEXT PRIMARY KEY`
     - `status_text TEXT NOT NULL` ($\le 140$ chars / $256$ UTF-8 bytes, no control characters)
     - `current_task_id TEXT` (optional, populated when working on a task)
     - `updated_at TEXT NOT NULL`
     - `expires_at TEXT NOT NULL` (defaults to `updated_at + 8 hours`)

7. **Integration with the Per-Origin Work Stream (`crates/org-sync`, owned by `ac-software-engineer-river`)**
   - Per [`offices-hq-and-satellites.md`](../design/offices-hq-and-satellites.md) (lines 74–79, 210–213), each office numbers every work change it originates from one monotonic counter `origin_seq` in the **same transaction** as the change.
   - The stream mechanics (the `work_origin_log` and `work_stream_cursors` tables, acks, backfill cursors, and promotion-republish retention) belong to **`crates/org-sync`** (`ac-software-engineer-river`).
   - Messaging contributes two work-change kinds to that stream:
     - `'msg_created'` (referencing `target_id = msg_headers.id` without duplicating the body in the log table — `org-sync` reads the immutable header and compressed body blobs from `msg_headers` / `msg_bodies` by `id`), and
     - `'msg_state_changed'` (carrying the compact `SyncedMessageState` payload so intermediate state transitions stream in exact `origin_seq` order).

#### 1.2 Body Compression Strategy and Decompression Safety

- **Fixed algorithm (`compression = 1`):** Pure-Rust raw DEFLATE via `flate2` (`default-features = false, features = ["rust_backend"]` using `miniz_oxide` at `Compression::fast()`, level 1 — 100% safe Rust, zero C dependencies, exact version pin in `Cargo.toml`).
- **Threshold (`512` bytes) and incompressible fallback:** Payloads with `body.len() < 512` bytes (or `meta` JSON `< 512` bytes) are stored raw (`compression = 0`). Payloads $\ge 512$ bytes are compressed **before** sending the write request to the single writer thread; if `compressed.len() >= raw.len()` (incompressible payload), the raw bytes are stored with `compression = 0` so payloads never expand. Decompression runs on the reader thread after `SELECT`.
- **Decompression bomb protection:** Every decompression reader wraps the slice in `.take(MAX_BYTES + 1)` (`256 KiB + 1` for `body`, `64 KiB + 1` for `meta`) and verifies that the decompressed byte length exactly matches `msg_headers.body_bytes` (or `meta_bytes`), rejecting any mismatch with `StorageError`.
- Doing compression/decompression off the writer thread keeps the single SQLite writer thread CPU-free while shrinking typical 4–64 KiB markdown/JSON/diff payloads by 4–7×, cutting SQLCipher AES-256 page encryption and WAL `fullfsync` write volume proportionally.

#### 1.3 Scope, Retention, Batching, and Measurements

- **What each office stores** ([`offices-hq-and-satellites.md`](../design/offices-hq-and-satellites.md) decision 3):
  - **HQ** keeps every message and lifecycle state across the organization (and owns all human inboxes).
  - **Each satellite** keeps messages where `from_member_id` or `to_member_id` is an agent placed in that satellite, or part of a thread/project that satellite follows.
- **Retention:**
  - `msg_headers` (including `content_hash`), `msg_threads`, and `msg_state` (~280 bytes/message) are kept as the organization's conversation index ($10^6$ headers $\approx 280\text{ MB}$). Because retry deduplication uses `msg_headers.content_hash`, pruning old `msg_bodies` never breaks deduplication.
  - Open messages in `msg_unread` and their `msg_bodies` are kept until resolved (`read`, `done`, `expired`, or `dead_letter`).
  - Resolved `msg_bodies` are kept by default (encrypted at rest); a per-organization retention policy (e.g. 90 days for resolved bodies, configurable later) can prune old `msg_bodies` rows (`DELETE FROM msg_bodies WHERE ...`) without breaking thread history or deduplication.
- **Write batching (ADR-0002 decision 4):**
  - The office server's single writer thread drains all queued write requests (up to 64 per batch) into one SQLite transaction, wrapping each request in its own `SAVEPOINT` and executing a single `COMMIT` (`synchronous = FULL`, `fullfsync = ON`).
- **Measurements to take in the implementation spike:**
  1. `send` latency (p50/p95) at concurrency 1, 8, and 32 with `fullfsync = ON`.
  2. Compression ratio, CPU time, and WAL byte growth over 10,000 synthetic agent messages (mix of 200 B short pings and 16 KiB code review bodies).
  3. `msg.inbox`, `msg.notify`, and `msg.threads` query time at 100,000 stored rows in `office.db`.

---

### 2. Local Delivery, Authentication & Authorization, Triage & Snooze, Directory & Presence, and CLI Surface (`org-agent`)

#### 2.1 How `org-agent` Finds Its Office and Authenticates (`office.json`, `<handle>.sock`, Keychain `app_token`, and Call Authorization)

1. Each office on a Mac owns `<office_home>/offices/<office_id>/office.json` and, while running, listens on `<office_home>/run/<handle>.sock` (`org_proto::location::office_socket_path`).
2. When an agent runs in an office, `ORG_OFFICE=<office_id>` and `ORG_AGENT=<member_id_or_name>` identify its office and member identity.
3. `org-agent` resolves its target `office_id` deterministically (never guessing from which `.sock` files happen to be open in `run/`):
   - If `--office <id>` or `$ORG_OFFICE` is set, it uses that `office_id`.
   - Otherwise, it scans `<office_home>/offices/*/office.json`: if there is **exactly one** office directory on this Mac, it uses that `office_id`; if there are zero or multiple offices on this Mac, it exits `1` with a clear message instructing the caller to set `ORG_OFFICE` or pass `--office`.
4. **Authentication, Accepted Risks, and Call-by-Call Authorization (`H1a`, `H1b`, `M-A`, `M-B`):**
   - **Human / App Capability (`app_token` in macOS Keychain + `SecCode` peer check, never a file on disk):**
     - Because agents run under the same macOS user account as the owner ([`offices-hq-and-satellites.md`](../design/offices-hq-and-satellites.md)), a mode-`0600` token file on disk could be read by an agent to forge human replies.
     - Therefore, the per-office **`app_token`** (256-bit random secret) is stored **only in the macOS Keychain** with its Keychain access control list restricted to the code-signed `org-office-host` and desktop app binaries (the same code-signature-bound Keychain protection used by [ADR-0002](0002-storage-sqlite-sqlcipher-one-writer.md) for the database keys) — **never written to a file on disk**. When the desktop app requests `app_token` from `org-office-host` over `host.sock`, `org-office-host` also verifies the connecting peer's kernel `audit_token_t` (`LOCAL_PEERTOKEN`) via `SecCodeCopyGuestWithAttributes` + `SecCodeCheckValidityWithErrors` to confirm the caller is the signed Agentic Org desktop app binary before returning the token.
     - Binding as a `Person` or `owner` (`msg.bind { agent: "owner" | "<person_id>", app_token }`), or calling app-only methods, requires a valid `app_token`. Any connection without `app_token` attempting to bind as a `Person` or `owner` is rejected with `ErrorCode::InvalidRequest` ("human binding requires app token").
     - **Residual risk (v1):** An unsandboxed local process that can drive the macOS GUI (Accessibility) or attach to an unsigned dev build of the app could still act through the UI; OS-level sandboxing of agent child processes is deferred to v2.
   - **Agent Identity (Accepted Risk Until Office Runner Spawns Agents, `M-A`):**
     - **Accepted risk for v1:** Until the office runner spawns every agent session directly, agents are launched by `bin/agent` or developer terminals outside `org-office-server`. Therefore, any local process on the office machine can bind as any **`MemberKind::Agent`** via `--as` / `ORG_AGENT` (never as a `Person` or `owner`). Once the office runner spawns agents directly, `org-office-server` will issue a per-session capability token (`ORG_SESSION_TOKEN`) in the spawned child's environment and require it on `msg.bind` for agent identities.
   - **Parent Verification on `reply_to` and No Retroactive Thread Escalation (`H1b`):**
     - When `msg.send` includes `reply_to: Some(parent_id)` or when `msg.reply { to_id: parent_id }` is called on an agent-bound connection, the server verifies that `bound_member_id` is authorized to read `parent_id` (`parent.from_member_id == bound_member_id || parent.to_member_id == bound_member_id`); otherwise it rejects the call with `ErrorCode::NotFound`. An agent cannot attach a message to a foreign thread by passing an arbitrary message ULID.
     - Furthermore, an agent-bound connection can read **only messages where `from_member_id == bound_member_id` or `to_member_id == bound_member_id`** (never earlier or third-party messages in a thread between other members).
   - **Call-by-Call Authorization and Row Visibility Table (`M-B`):**

| Wire Call | Who May Call It | Row / Target Authorization Rule |
| :--- | :--- | :--- |
| `who` | `agent-bound` or `app-bound` | Reads organization directory cards (`org.db` + runtime status). |
| `status.set` | `agent-bound` | Updates `member_status` for `bound_member_id` only. |
| `msg.bind` | Any local client | `MemberKind::Agent`: allowed on office socket. `MemberKind::Person` or `owner`: **requires `app_token`**. `"office"`: always rejected. |
| `msg.send`, `msg.ask` | `agent-bound` or `app-bound` | Sends with `from_member_id = bound_member_id`. If `reply_to` is set on an agent-bound call, requires `parent.from_member_id == bound_member_id \|\| parent.to_member_id == bound_member_id` (else `NotFound`). |
| `msg.reply`, `msg.defer`, `msg.done` | `agent-bound` or `app-bound` | Requires `target.to_member_id == bound_member_id` (else `NotFound`). |
| `msg.inbox`, `msg.read`, `msg.wait`, `msg.notify` | `agent-bound` or `app-bound` | Operates strictly on rows where `to_member_id == bound_member_id`. |
| `msg.get` | `agent-bound` or `app-bound` | `app-bound` (`app_token`): any message in the office. `agent-bound`: requires `msg.from_member_id == bound_member_id \|\| msg.to_member_id == bound_member_id` (else `NotFound`). |
| `msg.list` | `agent-bound` or `app-bound` | `app-bound` (`app_token`): any thread/participant in the office. `agent-bound`: restricted to rows where `from_member_id == bound_member_id \|\| to_member_id == bound_member_id`. |
| `msg.threads`, `msg.human_needs_you` | **`app-bound` (`app_token`) only** | Used by the Agentic Org desktop app to render org/human thread lists and the menu-bar "Needs you" queue. |

#### 2.2 In-Memory Waiter Registry, Snooze Timer Heap, and Claim-Before-Commit Reply Hand-Off (`M1`, `M2`, `M-C`, `L-a`)

`crates/org-mailbox`'s `Mailbox` struct owns the in-memory `WaiterRegistry` and snooze deadline min-heap, keeping all waiter and wake-up logic inside `org-mailbox` so `crates/org-office/src/store.rs` (`MessageStore`) remains a pure storage implementation that never calls back into `org-mailbox`:

- **Boundary rule:** Every message delivery and state transition — local `send`/`ask`/`reply`, local `defer`/`done`/`read`, synced `WorkChange` application, `resolve_pending_references`, and offline spool ingest — is invoked through `Mailbox` methods. After `store.commit_message(...)` or `store.update_state_local(...)` returns `Ok(...)`, **`Mailbox` notifies `WaiterRegistry` in memory (`< 1 ms`)**.
- **Snooze timer min-heap & startup rebuild (`L-a`):** `WaiterRegistry` maintains a `BTreeSet<(Instant, String, String)>` of active time-based `(snoozed_until, to_member_id, msg_id)` deadlines.
  - On `org-office-server` startup, `Mailbox::new` initializes this heap from `store.list_active_snooze_deadlines()`, and updates it whenever `msg.defer` or `apply_synced_state` sets or clears a `snoozed_until` timestamp.
  - When a `msg.wait` call blocks for `to_member_id`, its wait timeout is bounded by `MIN(request_timeout, next_snooze_deadline_for_member)`. When the snooze deadline arrives, `Mailbox` runs `store.check_notify(...)` to release the due snooze in `msg_unread` (`snoozed → unread`, `triage_gen += 1`) and returns the newly due message to the waiting `msg.wait` caller immediately.

**Race-free "register before check" and "claim before commit" protocol (`M-C`):**
1. **`msg.wait { timeout_ms, from?, urgent_only? }`:**
   - **Step 1 (Register):** Inserts an inbox waiter handle into `WaiterRegistry`.
   - **Step 2 (Check):** Releases any due time-based snoozes (`snoozed_until <= now_utc`) and queries `peek_next_unread` for the oldest unsnoozed unread message for `to_member_id` (matching `from` and `urgent_only` if given).
     - If `peek_next_unread` encounters rows with `adjusted_expires_at + skew_bound <= now_utc`, it skips them and enqueues `update_state_local(..., Expired)` so they are cleaned out of `msg_unread`.
   - **Step 3 (Complete or block):** If a matching unread message is found (or if a concurrent `Mailbox` commit completed the waiter handle during Step 2), removes the waiter handle and returns that message immediately **without** marking it read. Otherwise, blocks on the waiter handle until woken by `Mailbox` or until `timeout_ms` elapses (returning `null`, which makes `org-agent msg wait` exit `2`).
2. **Claim-before-commit on `msg.ask` replies (zero double-wake window, `M-C`):**
   - **Step 1 (Register ask waiter):** `Mailbox::ask` registers a reply waiter for `(ask.id, recipient_member_id)` in `WaiterRegistry` **before** calling `store.commit_message` (or `store.find_reply_for_ask` on an idempotent re-ask).
   - **Step 2 (Claim before commit when reply arrives):** When a reply with `reply_to == Some(ask_id)` is submitted to `Mailbox` (locally or via sync), `Mailbox` checks `WaiterRegistry::claim_reply_waiter(ask_id, reply.from_member_id)` **before** calling `store.commit_message`:
     - **If a live reply waiter is claimed:** `Mailbox` sets `initial_state = MessageLifecycleState::Read` on `NewStoredMessage`. `store.commit_message` inserts the reply into `msg_headers`, `msg_bodies`, and `msg_state(state = 'read')` in a **single transaction** and **never inserts the reply into `msg_unread`**. Because the row is never in `msg_unread`, no concurrent `msg.wait`, `msg.notify`, or `msg.inbox` reader can ever observe it as unread or double-wake the asking agent's background hook!
     - **Step 3 (Deliver or fallback on caller disconnect):** Immediately after `store.commit_message` returns `Ok`, `Mailbox` sends the committed `Message` through the claimed reply waiter's oneshot channel. In the rare case that the `msg.ask` socket disconnected during the millisecond `commit_message` was flushing, the oneshot send returns `Err(message)` and `Mailbox` immediately runs `store.update_state_local(Unread)` to place the reply into `msg_unread` so it is never lost.

#### 2.3 Message Triage, Priority, Snooze, and `notify` (`--priority`, `defer`, `done`, `notify`)

To prevent agents from being interrupted mid-task by routine messages while guaranteeing immediate response to urgent messages across Claude Code, Codex, and Antigravity:

1. **Interrupt Only for Urgent (`urgent = true`):**
   - `send`, `ask`, and `reply` take `--priority normal|high|urgent` (stored in `msg_headers.priority` and `msg_unread.priority`, and mirrored in `meta.priority` for spool compatibility).
   - A message is **urgent** (`urgent = true`) iff `kind == "request"` (`ask`) OR `priority` is `"high"` or `"urgent"`.
   - **Wake socket & `wait --urgent-only`:** In Phase A's spool relay, delivering an urgent message writes a 1-byte `'U'` (`0x55`) tag to `inbox/<to>/wake/*.sock`, whereas a normal message writes `'N'` (`0x4e`). `org-agent msg wait --urgent-only` (used by a busy agent's background watcher) wakes and exits `0` **only** when an unsnoozed urgent message is present; normal messages wait until the agent's natural break (`Stop`).
2. **Triage at the Break, Per Message:**
   At a break (`Stop`), the runtime hook runs `org-agent msg notify --after-task`, which surfaces sanitized header summaries only (count, sender, subject, kind, urgent marker — **never bodies**), instructing the agent to triage each message. The agent reads each message (`org-agent msg read <id>`) and chooses one of three actions:
   - **Answer now:** `org-agent msg reply <id> "…"` (transitions `<id>` from `unread` or `snoozed` to `read` and sends the reply).
   - **Snooze (`defer`):** `org-agent msg defer <id> --until <30m|2h|after-task|ISO-time> [--note "<why>"] [--json]`.
     - Restores `<id>` into `msg_unread` (even if the agent just ran `msg read <id>` during triage), increments `triage_gen`, sets `snoozed_until` (or `snooze_after_task = 1`) and `snooze_note`, sets `triaged_at = now_utc`, and upserts `msg_state` (`state = 'snoozed'`).
     - While snoozed, the message is hidden from default `msg inbox` so the agent has a clean slate.
     - `"after-task"` means due at the agent's next natural break (`--after-task`).
   - **Done without a reply (`done`):** `org-agent msg done <id> [--note "<why>"] [--json]`.
     - Upserts `msg_state` (`state = 'done'`, `reason = note`) and removes `<id>` from `msg_unread` (or `new/`). Done messages never resurface.
3. **Shared Triage Notification Command (`org-agent msg notify`, with `msg due` as a CLI alias, `L-f`):**
   - All three runtime adapters (Claude Code, Codex, Antigravity) are thin wrappers (~30 lines) around a single command in `org-agent`:
     `org-agent msg notify [--session <id>] [--after-task] [--urgent-only] [--sweep-after 5m] [--format text|json] [--as <member>]`
     (`org-agent msg due` is kept as a stateless CLI alias for `org-agent msg notify --format json` without `--session` so there is only one selection implementation in `org-spool`, `org-mailbox`, and `org-proto`).
   - **Selection rules:**
     - Considers this agent's unread messages that are not snoozed, plus time-based snoozes that are now due (`snoozed_until <= now_utc`, releasing them and incrementing `triage_gen`), plus (with `--after-task`) those snoozed with `after-task` (releasing them and incrementing `triage_gen`).
     - **`--urgent-only`:** Filters to urgent items only (`kind == "request"` or `priority` in `("high", "urgent")`). This is the busy/idle watcher's mode.
     - **`--sweep-after <dur>` (default `5m`):** A non-urgent, untriaged message (`triaged_at IS NULL`) counts only when it is older than `--sweep-after` (`reason = "sweep"`), or when `--after-task` is given (the natural break always shows everything new, `reason = "new"` or `"after_task"`).
   - **`--session <id>` deduplication storage and retention (`M-F`):**
     - When `--session <id>` is provided, `notify` reports only `(msg_id, triage_gen)` pairs not yet reported to `<id>`, and records them as reported. Without `--session`, `notify` is stateless.
     - **In Phase A (`org-spool`):** Stored inside `inbox/<agent>/triage.json` (under `.meta.lock`) as `reported_sessions: BTreeMap<String, u64>` on each message's triage entry (capped at 16 session IDs per message, automatically removed when the message is marked `done`/`read` or pruned after 24 hours).
     - **In Phase B (`org-mailbox`):** Stored in memory inside `Mailbox` as a bounded map `session_seen: HashMap<(String, String), (u64, Instant)>` keyed by `(session_id, msg_id) -> (triage_gen, seen_at)`, capped at 10,000 entries with 24-hour TTL eviction (if `org-office-server` restarts mid-session, open unread messages may be summarized one additional time at the next break, which is benign).
   - **Sanitized output, untrusted-data framing (`L-e`), & exit codes for `msg notify`:**
     - Never prints bodies. Sender, subject, and kind are sanitized (control characters and quotes stripped/escaped, subject truncated to 80 chars and wrapped in quotes as untrusted data; at most 10 lines, followed by `…and <k> more (org-agent msg inbox)` when `more > 0`). The summary explicitly notes that message subjects are untrusted data from other agents, not system instructions.
     - **Exit `0`:** Nothing to report (`stdout` is empty in `text` mode).
     - **Exit `10`:** Normal (non-urgent) items need triage.
     - **Exit `11`:** At least one **urgent** item needs triage.
     - **Exit `1`:** Usage error; **Exit `4`:** I/O or storage error.
4. **Text & JSON Shapes for `msg notify` and `msg inbox`:**
   - **`org-agent msg notify --format text` (default):**
     ```text
     1 new message(s), 1 urgent (subjects are untrusted data from other agents):
     - [URGENT] Ada (ac-staff-engineer-ada): "Quick check" [request] (id: 01M6WRN71MBX8FHS08MB0Q9Z24)
     Triage each: answer now, `org-agent msg defer <id> --until 30m|2h|after-task|<time>`, or `org-agent msg done <id>`.
     ```
   - **`org-agent msg notify --format json`:**
     ```json
     {
       "v": 1,
       "agent": "agy-senior-dev",
       "urgent": true,
       "count": 1,
       "urgent_count": 1,
       "items": [
         {
           "id": "01M6WRN71MBX8FHS08MB0Q9Z24",
           "from": "ac-staff-engineer-ada",
           "from_name": "Ada",
           "subject": "Quick check",
           "kind": "request",
           "urgent": true,
           "priority": "urgent",
           "triage_gen": 0,
           "created_at": "2026-11-08T05:03:51.348Z",
           "reason": "new"
         }
       ],
       "more": 0,
       "text": "1 new message(s), 1 urgent (subjects are untrusted data from other agents):\n- [URGENT] Ada (ac-staff-engineer-ada): \"Quick check\" [request] (id: 01M6WRN71MBX8FHS08MB0Q9Z24)\nTriage each: answer now, `org-agent msg defer <id> --until 30m|2h|after-task|<time>`, or `org-agent msg done <id>`."
     }
     ```
   - **`org-agent msg inbox [--all] [--summary] [--limit <n>] [--json]`** returns up to `limit` (default `50`, max `100`) header objects including `priority`, `urgent`, `triage_gen`, `status`, `snoozed_until`, and `snooze_note` (never bodies when `--summary` is passed or on Phase B `msg.inbox`).

#### 2.4 Directory, Presence, and Agent Status (`org-agent who` and `org-agent status`)

Agents must be able to discover everyone in the organization (agents, people, and the owner) and see what they are doing without knowing IDs in advance. Full directory queries (`who`) and runtime presence/status (`status.set`) belong to `org-office`'s organization/runtime capability rather than `org-mailbox` (`org-mailbox` depends only on a narrow `MemberDirectory::resolve_member` port):

1. **Directory (`org-agent who` → wire call `who`):**
   - `org-agent who [--team <name_or_id>] [--kind agent|person|owner] [--json]` lists all members matching the optional filters, read from the local office's copy of `org.db` (`Member`, `Membership`, `Team`, `reports_to_member_id`, `office_id`) combined with live runtime presence and `member_status`.
   - `org-agent who <id_or_name> [--json]` returns one member's full card (`role_title` is `null` in Phase B until the role catalogue is added to `org_proto::org::Member`, while per-team titles are carried in `teams[].title_in_team`, `L-d`):
     ```json
     {
       "id": "01M6W...",
       "name": "agy-senior-dev",
       "kind": "agent",
       "owner": false,
       "role_title": null,
       "teams": [{"id": "01M6T...", "name": "Platform", "title_in_team": "Messaging Engineer"}],
       "reports_to": {"id": "01M6A...", "name": "ac-staff-engineer-ada"},
       "office": {"id": "01M6O...", "name": "HQ"},
       "presence": "busy",
       "last_seen_at": "2026-11-08T05:08:00.000Z",
       "status": "Drafting ADR-0003 office-first messaging",
       "status_expires_at": "2026-11-08T13:08:00.000Z",
       "current_task_id": null
     }
     ```
2. **Presence (`online | busy | idle | offline`):**
   - Derived from each office's runtime state (`busy` while executing a turn, `idle` while holding `msg.wait` or between turns, `offline` when no session is active).
   - **How presence propagates across offices:** Per [`offices-hq-and-satellites.md`](../design/offices-hq-and-satellites.md) ("Runtime summaries are not logged"), presence is **ephemeral runtime data**, never written to `org.db`'s event log. Each office sends HQ passing `MemberPresenceUpdate` frames over its open connection when an agent's presence or status changes (debounced to at most 1 update/second per agent). HQ keeps the latest map in memory and forwards updates to connected offices and app subscribers (`member.presence_changed` event), avoiding any `fullfsync` per status change.
3. **Self-Reported Status Line (`org-agent status` → wire call `status.set`):**
   - `org-agent status "<text>" [--ttl 8h] [--json]` sets the calling agent's status line; `org-agent status --clear` clears it.
   - **Limits & staleness:** `text` is trimmed, single-line (no control characters), at most **140 characters / 256 UTF-8 bytes**. It expires automatically after **8 hours** (default `--ttl 8h`, max `24h`). Expired status lines are treated as `null` on read.
   - **Storage classification:** Stored in the owning office's `office.db` (`member_status` table, so it survives a local office server restart) and broadcast via the passing runtime summary stream to HQ and other offices (not logged in `org.db` or `work_origin_log`).

#### 2.5 Talking to Humans (Owner and Added People)

Per [`teams-and-org-chart.md`](../design/teams-and-org-chart.md), agents can message, `ask`, or assign tasks to any **person** added to the organization (`MemberKind::Person`) and always to the **owner** (`owner: true`):

1. **HQ Owns All Human Inboxes:**
   - People are not placed in offices (`office_id: None`). **HQ is the owning office for every person's and the owner's inbox** (`office.db` at HQ), because humans interact through the Agentic Org desktop app.
   - If the desktop app is running on a Mac that only has a satellite office, the satellite proxies human inbox reads, replies, and sends over its connection to HQ (authenticated with the user's Keychain `app_token`).
2. **How Messages Reach a Human in the Desktop App:**
   - **Menu-Bar "Needs you" Rows & Indicator Dot (Eden's UX spec):**
     - When an agent sends an `ask` (`kind == "request"`) or an urgent message (`priority` in `("high", "urgent")`) to the owner (or a person), HQ emits `message.received` (`urgent: true`).
     - The Agentic Org menu-bar icon lights its indicator dot, posts a macOS notification, and shows a **"Needs you"** row in the menu-bar dropdown with the agent's name, subject/preview, and an inline reply box.
   - **App Inbox & Threads View:**
     - All messages to/from the owner and people (urgent and normal) appear in the app's Inbox view, grouped by thread (`msg.threads` / `msg.list`), where the human can read, reply (`msg.reply`), snooze (`msg.defer`), or mark done (`msg.done`).
3. **How a Human Replies:**
   - When the owner (or person) replies in the app, the app authenticates with its Keychain `app_token`, binds as that human member at HQ, and calls `msg.reply`.
   - HQ commits the reply in HQ's `office.db`, transitions the agent's original message to `state = 'read'`, and streams the reply immediately to the asking agent's office (`HQ → satellite`), waking a waiting `msg.ask` or surfacing at the agent's next break.
4. **What an `ask` to a Human Does While They Are Away (`ttl_ms` vs per-call `wait_ms`):**
   - Humans answer on human timescales (minutes to hours, not milliseconds).
   - `msg.ask` separates the request's overall deadline **`ttl_ms`** (which sets `expires_at = created_at + ttl_ms` once when the request is created; up to `86_400_000` ms / 24h) from the per-socket-call hold time **`wait_ms`** ($\le 3\_600\_000$ ms / 1h per socket call).
   - When `org-agent msg ask --to <human>` targets a `Person` or `owner` and the caller does not pass an explicit `--timeout`, the default `ttl_ms` is **`24h`** (vs `120s` for agent recipients), automatically held across 1-hour `wait_ms` socket calls using idempotent re-ask (`§3.3.4`) without altering `expires_at`.
   - **Graceful Expiry ("Late Human Reply"):** Only when the full `expires_at` (`created_at + ttl_ms`) elapses before the human replies:
     - `org-agent msg ask` exits `2` (`timeout`) so the calling agent can move on or fallback.
     - Instead of hiding the request as dead-lettered, HQ sets `flags |= FLAG_EXPIRED_WAITING_HUMAN` (`8`) and **keeps the request visible in the human's "Needs you" / Inbox queue** marked *"Agent timed out waiting — your reply will still be delivered"*.
     - When the human later replies in the app, the reply is delivered normally into the agent's inbox (`msg_unread`) with `reply_to = ask.id` and `priority = "high"`, waking the agent if idle or surfacing at its next `Stop`.

#### 2.6 CLI Surface and Exit Codes

```text
org-agent whoami [--as <member>] [--json]
org-agent who    [<member>] [--team <team>] [--kind agent|person|owner] [--office <id>] [--json]
org-agent status [<text> | --clear] [--ttl 8h] [--office <id>] [--as <member>] [--json]
org-agent msg send   --to <member> [--subject <s>] [--priority normal|high|urgent] [--meta k=v] [--office <id>] [--as <member>] [--json] <body | ->
org-agent msg ask    --to <member> [--timeout 120s] [--subject <s>] [--priority high|urgent] [--meta k=v] [--office <id>] [--as <member>] [--json] <body | ->
org-agent msg reply  <id> [--subject <s>] [--priority normal|high|urgent] [--meta k=v] [--office <id>] [--as <member>] [--json] [--body <body> | <body | ->]
org-agent msg defer  <id> --until <30m|2h|after-task|ISO> [--note <text>] [--office <id>] [--as <member>] [--json]
org-agent msg done   <id> [--note <text>] [--office <id>] [--as <member>] [--json]
org-agent msg notify [--session <id>] [--after-task] [--urgent-only] [--sweep-after 5m] [--format text|json] [--office <id>] [--as <member>]
org-agent msg due    [--after-task] [--urgent-only] [--sweep-after 5m] [--office <id>] [--as <member>] [--json]
org-agent msg inbox  [--all] [--summary] [--from <member>] [--thread <id>] [--limit <n>] [--cursor <seq>] [--office <id>] [--as <member>] [--json]
org-agent msg read   <id> | --next [--office <id>] [--as <member>] [--json]
org-agent msg wait   [--timeout 60s] [--from <member>] [--urgent-only] [--office <id>] [--as <member>] [--json]
```

- **Addressing (`--to`, `--from`, `--as`):** Accepts a member's ULID `id`, their unique `name` (case-insensitive match against `org.db`'s `members` table), or `"owner"` (which resolves to the organization's `owner: true` member). If a display name matches more than one member in `org.db`, the server rejects it with `ErrorCode::InvalidRequest` instructing the caller to pass the member's ULID.
- **Reserved names & kinds:** `"office"` is reserved for the server (`msg.bind { agent: "office" }`, `to: "office"`, or a client sending `kind: "delivery_failed"` fails with `ErrorCode::InvalidRequest`).
- **Exit codes:**

| Exit Code | Meaning | `org-proto` Error Codes / Conditions |
| :--- | :--- | :--- |
| `0` | Success (`read --next` on empty inbox prints `null` in `--json` and exits `0`; `notify` when nothing is due exits `0`) | `ok: true` (or offline spool queued on `send`) |
| `1` | Usage, validation, or authorization error | `invalid_request`, `invalid_name`, `too_large`, `already_exists`, `not_found`, `not_bound` |
| `2` | Timeout waiting for message or reply | `timeout` (or `msg.wait` returning `null` at deadline) |
| `3` | Unknown recipient | `unknown_recipient` |
| `4` | Storage or protocol error (including any future error code that decodes as `ErrorCode::Unknown` on an older CLI) | `storage_error`, `unknown_method`, `unknown`, frame decode error |
| `5` | Office server unavailable | Socket connect failure on `ask`, `reply`, `defer`, `done`, `notify`, `due`, `wait`, `read`, `inbox`, `who`, `status` |
| `10` | Normal (non-urgent) messages need triage (`msg notify` only) | `msg notify` found $>0$ actionable normal messages and $0$ urgent messages |
| `11` | Urgent messages need triage (`msg notify` only) | `msg notify` found $>0$ actionable urgent messages |
| `130` | Interrupted | `SIGINT` / `SIGTERM` |

---

### 3. Cross-Office Routing, Ownership Hand-Off, Deadlines, Dedup, and Offline Outbox

#### 3.1 Per-Origin Work Stream (`office → HQ → office`) and Agent Move Ownership Hand-Off (`M-E`)

Per [`offices-hq-and-satellites.md`](../design/offices-hq-and-satellites.md) ("Office-first: tasks and messages"):

1. **Local commit + sequence allocation:**
   - When an agent in Office A sends a message (`msg.send`, `msg.reply`, `msg.ask`) or when the recipient's owning office updates message lifecycle state (`msg.defer`, `msg.done`, `msg.read`, `expired`, or `dead_letter`), that office allocates the next monotonic `origin_seq` for `(office_id, incarnation)` in `work_origin_log` (`crates/org-sync`) **in the same SQLite transaction**.
   - If the recipient is also placed in Office A, local delivery is already complete and any local `wait`/`ask` listener wakes immediately after commit.
2. **Streaming to HQ and HQ forwarding:**
   - Office A streams new work changes over its persistent outbound connection to HQ.
   - HQ commits the work change in HQ's `office.db` (deduplicating by `(origin_office_id, origin_incarnation, origin_seq)` via `idx_msg_origin` / `idx_msg_state_origin`), sends `WorkAck` back to Office A, and **immediately after commit** pushes the work change to:
     - the recipient's office (if `to_member_id` is an agent placed in Satellite B), and
     - any other satellite following that thread/project.
   - If `to_member_id` is the owner or a person (`kind == Person`), HQ is their owning office.
   - Satellite B commits the incoming message into its `office.db` (inserting into `msg_unread` when `to_member_id` is placed in Satellite B) and wakes any local `wait` or `ask` listener for that agent.
3. **Deterministic Ownership Hand-Off and Reconciliation on an Agent Move (`M-E`):**
   - Which office owns an agent $M$'s inbox is a deterministic function of the organization event log `Position { epoch, seq }` in `org.db`.
   - Every `WorkChange::MessageStateChanged` carries the `org_position: Position` that the acting office had applied when it recorded the state change.
   - When an organization event at `move_pos` moves agent $M$ from Office A to Office B (or removes $M$):
     1. As soon as Office A applies `move_pos` in `org.db`, Office A **stops resolving or mutating state** for $M$, cancels any active `WaiterRegistry` waiters for $M$ (`ErrorCode::NotBound`), and removes $M$'s rows from Office A's `msg_unread`.
     2. HQ and all following offices validate that any `MessageStateChanged` from Office A for $M$ has `change.org_position < move_pos`, and any from Office B has `change.org_position >= move_pos`.
     3. Every office (HQ, Office B, and thread followers) applies `MessageStateChanged` using the total ordering tuple **`(org_epoch, org_seq, origin_seq, origin_office_id, origin_incarnation)`** (`§1.1.5`): an incoming transition overwrites `msg_state` (and updates `msg_unread`) iff its tuple is strictly greater than the stored row's tuple.
     4. If Office A was disconnected when `move_pos` occurred, HQ backfills Office B from HQ's current state at `move_pos`. When Office A later reconnects and streams its pre-move changes (`change.org_position < move_pos`), HQ forwards those late pre-move changes to Office B; because `(org_epoch, org_seq)` is the leading key of the comparison tuple, Office B applies a late pre-move change from A iff Office B has not yet recorded a newer post-move transition (`>= move_pos`) for that `msg_id`, ensuring HQ, Office B, and all followers converge to the exact same state.

#### 3.2 Two-Stage Recipient Check, Structure Position (`org_epoch, org_seq`), and Single-Owner Dead-Lettering

Per [`teams-and-org-chart.md`](../design/teams-and-org-chart.md) ("Who agents may interact with") and [`offices-hq-and-satellites.md`](../design/offices-hq-and-satellites.md) ("References to structure"):

1. **Stage 1 — At the sender's office (synchronous before commit):**
   - The sender's office checks `to` against its local `org.db` copy at the `Position { epoch, seq }` it has currently applied (never against unapplied commands still in its org-command outbox).
   - `to` must match an active `Member` (`kind: Agent | Person`, or `owner: true`). If not, the office rejects the call immediately with `ErrorCode::UnknownRecipient` (exit code `3`).
   - The committed message records `org_epoch` and `org_seq` from that applied `Position`, plus `to_display_name` (the recipient's current display name in `org.db`).
2. **Stage 2 — When a message arrives at HQ or a satellite (`FLAG_UNRESOLVED_REF`):**
   - If a synced message arrives with `(org_epoch, org_seq) > local_org_position` (the work stream arrived slightly ahead of the organization-event stream), the receiving office commits the message to `msg_headers`/`msg_bodies` with `flags |= FLAG_UNRESOLVED_REF` and **withholds it from `msg_unread` and `WaiterRegistry`** so the work stream never stalls and the message never wakes an agent before its structure position is applied.
   - Whenever `org.db` advances to a new `Position`, `Mailbox::resolve_pending_references(applied_pos)` runs: for every message in `idx_msg_unresolved` with `(org_epoch, org_seq) <= applied_pos`, it validates `from_member_id` and `to_member_id`. If valid, it clears `FLAG_UNRESOLVED_REF`, inserts into `msg_unread` (if this office owns the recipient and the message is still unread), and notifies `WaiterRegistry`.
3. **Single-owner lifecycle & `delivery_failed` notices (with recipient ID + display name at send time):**
   - **Rule:** Only the **recipient's owning office at `org_position`** (or **HQ alone** if the recipient member was removed in `org.db` or has no office) may transition a message to `read`, `done`, `expired`, or `dead_letter` in `msg_state` and emit a `delivery_failed` notice. Other offices following the thread apply the synced `WorkChange::MessageStateChanged` event and never emit their own `delivery_failed`.
   - When the owning office (or HQ on `member.remove`) dead-letters an open message:
     - It upserts `state = 'dead_letter'`, `reason = 'unknown_recipient'` in `msg_state`, sets `flags |= FLAG_DEAD_LETTER` in `msg_headers`, and removes the row from `msg_unread`.
     - If `original.kind != "delivery_failed"` and `original.from_member_id != "office"` and `original.from_member_id` is still an active member in `org.db`, it emits a single `delivery_failed` message (`from_member_id = "office"`, `to_member_id = original.from_member_id`, `kind = "delivery_failed"`, `priority = "high"`, `reply_to = original.id`), guarded by `idx_msg_delivery_failed_once`.
     - **`delivery_failed` payload (for agents and Eden's UX):** The `delivery_failed` message carries structured `meta` (and human-readable `body`) with the **recipient as addressed at send time** (`recipient_id`: `original.to_member_id`, `recipient_name`: `original.to_display_name`) and the **reason code** (`reason`: `"unknown_recipient"` | `"expired"`), so both the sending agent and the desktop UI can display who the message was addressed to even after that member has been removed.

#### 3.3 Cross-Office `ask` with Deadlines, Bounded Clock-Skew Grace, and Idempotent Re-Ask

1. **Sending a cross-office `ask`:**
   - Agent 1 in Office A runs `org-agent msg ask --to agent-2 --timeout 120s "…"`.
   - Office A records `ttl_ms = 120_000` and `expires_at = created_at + 120s`, registers the reply waiter `(ask.id, agent_2_id)` in Office A's `WaiterRegistry`, commits the `request` locally, and streams it to HQ $\rightarrow$ Office B.
2. **Deadline enforcement & clock-skew protection (`H3`, `L-c` — never reviving stale queued requests):**
   - At the receiving office, a `request` message is considered live iff `now_utc < expires_at + skew_bound`, where `skew_bound = MIN(ttl_ms / 4, 2_000 ms)` (at most 2 seconds past `expires_at` to tolerate minor NTP drift between machines, and **never** computed relative to `local_received_at`).
   - Consequently, a 120 s `ask` that sat for 1 hour in an offline queue arrives with `expires_at + 2s` an hour in the past and is **immediately marked `expired` upon arrival, never acted on** (matching [`offices-hq-and-satellites.md`](../design/offices-hq-and-satellites.md) L88).
   - When an agent-targeted request is expired on arrival (or when `agent-2` calls `msg.read` / `msg.wait`), the recipient's owning office upserts `state = 'expired'` in `msg_state`, removes it from `msg_unread`, and emits `delivery_failed` (`reason = "expired"`, `recipient_id`, `recipient_name`, `reply_to = ask.id`) back to Office A.
3. **Reply path (`Office B → HQ → Office A`):**
   - When `agent-2` in Office B runs `org-agent msg reply <ask.id> "…"`, Office B commits the `reply` (`reply_to = ask.id`, `thread_id = ask.thread_id`), upserts `ask.id` to `state = 'read'` in Office B's `msg_state`, and streams both to HQ $\rightarrow$ Office A.
   - When Office A receives the incoming reply (or an incoming `delivery_failed` with `reply_to = ask.id`) via `Mailbox`, `Mailbox` claims the live reply waiter before commit (`§2.2.2`), commits the reply directly as `state = 'read'` (never touching `msg_unread`), and wakes the held `msg.ask` connection immediately (`< 1 ms`). If the wake-up is a `delivery_failed`, `msg.ask` returns the corresponding error (`unknown_recipient` or `timeout`).
4. **Race-Free Idempotent Re-Ask on Disconnect:**
   - `org-agent` generates the `ask`'s ULID `id` before sending.
   - If the local socket between `org-agent` and Office A drops (or when a 24h human `ask` starts its next 1-hour `wait_ms` chunk) before `expires_at`, `org-agent` reconnects and calls `msg.ask` with the **same `id`**, the same content, the original `ttl_ms`, and the per-call `wait_ms`.
   - Office A looks up `id` in `msg_headers`:
     - If `id` exists with a different `from_member_id` or different `content_hash`, it returns `ErrorCode::AlreadyExists`.
     - If `id` exists with matching `(from_member_id, content_hash)`:
       1. Office A preserves the original `expires_at` on the stored message and **first registers** a reply waiter `(id, to_member_id)` in `WaiterRegistry` (preventing any race if the reply commits concurrently).
       2. Office A then queries `find_reply_for_ask(id, to_member_id)` on a reader connection (which checks for a reply from `to_member_id` or a `delivery_failed` for `id`, even if already marked read).
       3. If found, it removes the waiter and returns the reply (or error) immediately; otherwise it waits on the registered waiter handle for `MIN(wait_ms, remaining_until_expires_at)` without re-sending the request.

#### 3.4 Canonical `content_hash` Contract (`M-D`) and Ordering Guarantees

- **Canonical `content_hash` (computed in `crates/org-mailbox` via `sha2` crate):**
  - Only the office (`crates/org-mailbox`) computes `content_hash`; `org-agent` and `org-proto` never need to hash messages.
  - Algorithm: **SHA-256** via the standard RustCrypto **`sha2`** crate (exact version pin in workspace `Cargo.toml`, depended on by `crates/org-mailbox`, `Ce = 4`).
  - Canonical input byte stream: version byte `0x01` followed by seven `u32` big-endian length-prefixed UTF-8 fields in exact order:
    `0x01 || u32be(len(to_member_id)) || to_member_id || u32be(len(kind)) || kind || u32be(len(priority)) || priority || u32be(len(subject_or_empty)) || subject_or_empty || u32be(len(reply_to_or_empty)) || reply_to_or_empty || u32be(len(body)) || body || u32be(len(canonical_meta_json)) || canonical_meta_json`
  - **Explicit recursive key sorting for `canonical_meta_json` (`M-D`):** Rather than relying on `serde_json::Value`'s internal map representation (which changes if any workspace member enables `serde_json/preserve_order`), `org-mailbox` formats `canonical_meta_json` using an explicit recursive canonical serializer that sorts every JSON object's keys by UTF-8 byte order at all nesting levels and emits compact RFC 8785 / JSON bytes (or `""` when `meta` is empty).
  - **Test vector (with nested unsorted keys):**
    For `to_member_id = "01M6WAGENT2"`, `kind = "request"`, `priority = "urgent"`, `subject = Some("Check")`, `reply_to = None`, `body = "ping"`, and `meta = {"z": 1, "a": {"d": true, "b": "x"}}` (canonical meta bytes `{"a":{"b":"x","d":true},"z":1}`), `org-mailbox` includes a unit test locking the exact 32-byte SHA-256 digest against regressions.
- **Deduplication & Ordering:**
  - Client retries at the local office look up `id` by primary key in `msg_headers` and compare `(from_member_id, content_hash)` in $O(1)$ (`ErrorCode::AlreadyExists` on mismatch; original commit result on match).
  - Cross-office stream items are deduplicated by `(origin_office_id, origin_incarnation, origin_seq)` via `idx_msg_origin` and `idx_msg_state_origin`.
  - Work changes originating from any single office are delivered and applied in strict `origin_seq` order; each office presents its inbox and `msg.wait` stream in monotonic `local_seq` commit order.

#### 3.5 Offline Behavior and Spool Safety (Two Distinct Boundaries)

1. **Satellite Office Disconnected from HQ (Network Offline):**
   - Same-office messaging (`send`, `ask`, `reply`, `defer`, `done`, `notify`, `due`, `wait`, `read`, `inbox`, `who`, `status`) works at full local speed.
   - Cross-office `send` and `reply` commit locally in `office.db` and stream to HQ automatically when the connection resumes. Because the work stream is separate from the organization-command outbox (`offices-hq-and-satellites.md`), a blocked organization command never holds back agent messages.
   - Cross-office `ask` commits locally and waits; if HQ remains unreachable past `expires_at`, the local waiter times out with exit code `2`, and when HQ reconnects later, the request's `expires_at` prevents a remote agent from acting on it.
2. **Local Office Server Temporarily Unreachable (`org-spool` Offline Fallback & Security Rules):**
   - If `org-agent` cannot connect to `<office_home>/run/<handle>.sock` (`ENOENT` or `ECONNREFUSED`), it retries briefly (up to 500 ms with backoff) in case `org-office-host` is restarting the server.
   - If still unreachable:
     - **Fire-and-forget `send`** writes a versioned `SpooledSend` JSON file into `<office_home>/offices/<office_id>/spool/tmp/` (mode `0700` directory, `0600` file), flushes the file with `F_FULLFSYNC`, atomically renames it into `<office_home>/offices/<office_id>/spool/new/<id>.json`, flushes the `new/` directory with `F_FULLFSYNC`, prints `"queued": true` in `--json` (with a stderr notice), and exits `0`.
     - **`ask`, `reply`, `defer`, `done`, `notify`, `due`, `wait`, `read`, `inbox`, `who`, and `status`** require live office state and exit `5` (`office server unavailable`).
   - **Spool Ingest, FIFO/Symlink Protection, and Plaintext Lifetime:**
     - `org-office-server` drains `<office_home>/offices/<office_id>/spool/new/` through `crates/org-spool` on startup **before** accepting client connections, and re-scans every 5 seconds as a backstop.
     - **Non-blocking regular-file verification (`M4`):** Every entry in `spool/new/` must have a valid `<26-char ULID>.json` filename, is opened with `O_RDONLY | O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC`, and is immediately checked with `fstat`: the descriptor must satisfy `S_ISREG(st_mode)` (rejecting FIFOs, sockets, symlinks, and device nodes so startup ingest can never block on a named pipe) and `st_uid == geteuid()`, with size $\le 512\text{ KiB}$. Non-regular or oversized files are unlinked or moved to `spool/dead/` without reading.
     - **Time-bounded plaintext exception:** Spooled files exist on disk only while `org-office-server` is mid-restart and are ingested into SQLCipher-encrypted `office.db` and `unlink`ed immediately upon server startup (any corrupt file in `spool/dead/` is purged after 24 hours). Similarly, Phase A's `~/.agency/spool` directory (`0700`/`0600`) is a temporary pre-cutover spool that is imported into HQ's encrypted `office.db` at Phase B cutover and deleted after the 1-week rollback window.

---

### 4. Contract Additions (`org-proto` & `org-sync-proto`) and Code Ownership

#### 4.1 `org-proto` Additions (`crates/org-proto`, owned by `ada`)

`API_VERSION` remains `1` (all additions are additive). Because each `org-office-server` serves a single office (and therefore a single organization), calls on an office socket do **not** need an `org` parameter. Call-by-call authorization and row visibility are governed by the table in §2.1.4.

- **Directory & Presence Calls (handled by `org-office` runtime):**
  - `who { member?: String, team?: String, kind?: MemberKind } → { members: Vec<DirectoryCard> }`
    - Returns directory cards (`id`, `name`, `kind`, `owner`, `role_title`, `teams`, `reports_to`, `office`, `presence`, `last_seen_at`, `status`, `status_expires_at`, `current_task_id`).
  - `status.set { text?: String, ttl_ms?: u64 } → { status: Option<String>, expires_at: Option<String> }` (`text: None` clears the caller's status line).
- **Agent & Shared Messaging Calls (handled by `org-mailbox`):**
  - `msg.bind { agent?: String, app_token?: String } → { member_id?: String, office_id: String }`
    - Resolves `agent` (member ID, unique member name, or `"owner"`; `"office"` is rejected) against `org.db`, requires a valid Keychain `app_token` when binding as a `Person`/`owner` (or for app-wide queries), and binds the connection's acting member.
  - `msg.send { id: String, to: String, kind: String, priority?: MessagePriority, subject?: String, body: String, meta?: BTreeMap<String, Value>, reply_to?: String, expires_at?: String, ttl_ms?: u64 } → { id: String, thread_id: String, local_seq: u64, origin_seq: u64 }`
  - `msg.reply { id: String, to_id: String, priority?: MessagePriority, subject?: String, body: String, meta?: BTreeMap<String, Value> } → { id: String, thread_id: String, local_seq: u64, origin_seq: u64 }`
  - `msg.ask { id: String, to: String, priority?: MessagePriority, subject?: String, body: String, meta?: BTreeMap<String, Value>, ttl_ms: u64, wait_ms: u64 } → { reply: Message }`
  - `msg.defer { id: String, until: SnoozeTarget, note?: String } → { id: String, triage_gen: u64, snoozed_until: Option<String>, snooze_after_task: bool, snooze_note: Option<String> }` (`SnoozeTarget` = `UntilUtc(String)` | `AfterTask`)
  - `msg.done { id: String, note?: String } → { id: String, resolved_at: String }`
  - `msg.notify { session_id?: String, after_task?: bool, urgent_only?: bool, sweep_after_ms?: u64 } → NotifyResult { v: u32, agent: String, urgent: bool, count: u32, urgent_count: u32, items: Vec<NotifyItem>, more: u32, text: String }` (powers both `org-agent msg notify` and `org-agent msg due`)
  - `msg.wait { timeout_ms: u64, from?: String, urgent_only?: bool } → { message: Option<Message> }` (returns the oldest unsnoozed unread message without marking it read; `None` on timeout)
  - `msg.read { id?: String } → { message: Option<Message> }` (atomically claims and marks read a specific message or the oldest unsnoozed unread message)
  - `msg.inbox { include_all?: bool, from?: String, thread_id?: String, cursor?: u64, limit?: u32 } → { headers: Vec<MessageHeader>, next_cursor: Option<u64> }` (returns up to `limit <= 100` headers ordered by `local_seq DESC`, including `priority`, `urgent`, `triage_gen`, `status`, `snoozed_until`, `snooze_after_task`, `snooze_note`, `read_at`; hides currently snoozed messages unless `include_all = true`; never includes `body` or `meta`)
  - `msg.list { thread_id?: String, participant?: String, unread_only?: bool, include_snoozed?: bool, cursor?: u64, limit?: u32 } → { headers: Vec<MessageHeader>, next_cursor: Option<u64> }` (visibility governed by §2.1.4)
  - `msg.get { id: String } → { message: Message }` (visibility governed by §2.1.4)
- **App-Only Calls (require `app_token`, §2.1.4):**
  - `msg.human_needs_you { member_id?: String, limit?: u32 } → { items: Vec<MessageHeader>, total_urgent_unread: u32 }` (powers the menu-bar "Needs you" rows and indicator dot for the owner or specified person; defaults to the organization's `owner: true` member)
  - `msg.threads { participant?: String, cursor?: ThreadCursor, limit?: u32 } → { threads: Vec<ThreadSummary>, next_cursor: Option<ThreadCursor> }` (where `ThreadCursor` is `{ last_msg_at: String, thread_id: String }`)
- **Events (`impl EventKind`, pushed to `subscribe` clients):**
  - `message.received { header: MessageHeader }` (no body in events so frames stay small)
  - `message.state_changed { id: String, thread_id: String, to_member_id: String, state: MessageLifecycleState, triage_gen: u64, snoozed_until: Option<String>, snooze_after_task: bool, reason: Option<String>, updated_at: String }`
  - `message.delivery_failed { id: String, original_id: String, thread_id: String, from_member_id: String, recipient_id: String, recipient_name: String, reason: String }` (carries both the recipient's `id` and `display_name` as addressed at send time and the `reason` code such as `unknown_recipient` or `expired`)
  - `member.presence_changed { member_id: String, presence: MemberPresence, last_seen_at: String, status: Option<String>, status_expires_at: Option<String>, current_task_id: Option<String> }`
- **Error codes added to `org_proto::ErrorCode`:**
  - `Timeout`, `TooLarge`, `NotBound` (`AlreadyExists`, `UnknownRecipient`, `NotFound`, `InvalidRequest`, `StorageError`, `Unknown` already exist in `crates/org-proto/src/error.rs`).
- **Limits (measured on JSON-encoded wire bytes, fitting inside `MAX_FRAME_BYTES = 1 MiB`):**
  - `body` $\le 256\text{ KiB}$, `meta` $\le 64\text{ KiB}$, `subject` $\le 1\text{ KiB}$, `status` $\le 256\text{ B}$ ($140$ chars), `snooze_note` / `done` note $\le 1\text{ KiB}$, `kind` / member names / IDs $\le 128\text{ B}$.
  - `wait_ms` / `timeout_ms` $\le 3\_600\_000$ (1 hour per socket call), `ttl_ms` $\le 86\_400\_000$ (24 hours request deadline), max 16 held `wait`/`ask` requests per connection.
- **Offline spool contract (`org_proto::msg::SpooledSend`):**
  - `{ v: 1, office_id: String, from: String, id: String, to: String, kind: String, priority: MessagePriority, subject?: String, body: String, meta?: BTreeMap<String, Value>, reply_to?: String, expires_at?: String, ttl_ms?: u64, created_at: String }`

#### 4.2 `org-sync-proto` Additions (Office ↔ HQ Stream, owned by `ada`, stream mechanics in `crates/org-sync` owned by `ac-software-engineer-river`)

In the office-to-office protocol (`crates/org-sync-proto`):

- **Messaging work-stream items (`WorkChange`):**
  - `WorkChange::MessageCreated(SyncedMessage)`: carries `origin_office_id`, `origin_incarnation`, `origin_seq`, `org_position: Position`, all `MessageHeader` fields (including `from_display_name`, `to_display_name`, `priority`, `content_hash`, and `ttl_ms`), and `(compression, body_blob, meta_blob)`.
  - `WorkChange::MessageStateChanged(SyncedMessageState)`: carries `origin_office_id`, `origin_incarnation`, `origin_seq`, `org_position: Position`, `msg_id`, `to_member_id`, `state: Unread | Snoozed | Read | Done | Expired | DeadLetter`, `triage_gen: u64`, `snoozed_until: Option<String>`, `snooze_after_task: bool`, `reason: Option<String>`, `updated_at: String`.
- **Passing runtime updates (not logged in `org.db` or `work_origin_log`):**
  - `RuntimeUpdate::MemberPresence { office_id: String, members: Vec<MemberRuntimeState> }` (carries `member_id`, `presence: Online | Busy | Idle | Offline`, `last_seen_at`, `status`, `status_expires_at`, `current_task_id`).

#### 4.3 Where the Code Lives, Coupling Metrics (`docs/architecture-rules.md`), and the `MemberDirectory` & `MessageStore` Traits

Per `AGENTS.md`:

```text
apps/org-agent (agy-senior-dev) ──┬──▶ crates/org-client (server-dev) ──┐
                                  └──▶ crates/org-spool  (agy-senior-dev)├──▶ crates/org-proto (ada)
                                                                         │
apps/org-office-server (server-dev) ─▶ crates/org-office (server-dev) ───┤
                                            │                            │
                                            ▼                            │
                                       crates/org-mailbox (agy-senior-dev)┘
```

| Component | Owner | Role | Dependencies (`Ce`) | Instability (`I`) |
| :--- | :--- | :--- | :--- | :--- |
| `crates/org-proto` | `ada` | Client ↔ Office wire & spool contract | `serde`, `serde_json` (`Ce = 2`) | `~0.15` (Stable contract) |
| `crates/org-sync-proto` | `ada` | Office ↔ HQ sync & runtime contract | `org-proto`, `serde`, `serde_json` (`Ce = 3`) | `~0.25` (Stable contract) |
| `crates/org-mailbox` | `agy-senior-dev` | Pure messaging & triage/snooze domain (`WaiterRegistry` + snooze timer heap, `sha2` canonical `content_hash`, `flate2` compression + bounded decompression, `notify` formatter + session dedup, idempotent `ask`, dead-letter rules) over `MemberDirectory` + `MessageStore` traits | `org-proto`, `org-sync-proto`, `flate2`, `sha2` (`Ce = 4`, **no SQLite**) | `~0.40` (Domain capability) |
| `crates/org-spool` | `agy-senior-dev` | Crash-safe local spool (`tmp` + `F_FULLFSYNC` + `rename` into `new`, `O_NOFOLLOW | O_NONBLOCK` regular-file ingest; powers Phase A CLI and Phase B offline fallback) | `org-proto`, `serde`, `serde_json` (`Ce = 3`) | `~0.35` (Platform primitive) |
| `apps/org-agent` | `agy-senior-dev` | Fast stateless CLI (`who`, `status`, `msg` with `defer`/`done`/`notify`/`due`) | `org-client`, `org-spool`, `org-proto` (`Ce = 3`) | `1.00` (Edge binary) |
| `crates/org-office` (`store.rs`) | `server-dev` | Owns `who` / `status.set` runtime capability and implements `MemberDirectory` + `MessageStore` inside `store.rs` on the single batching SQLCipher writer and read pool | Owns `rusqlite` / SQLCipher | Domain + Store |

**Dependency Inversion Traits (`MemberDirectory` and `MessageStore` in `crates/org-mailbox`):**
`crates/org-mailbox` defines two narrow traits that `crates/org-office/src/store.rs` implements:

```rust
/// Narrow read-only port into `org.db` (shared with tasks; full `who` and `status.set` live in `org-office`).
pub trait MemberDirectory {
    /// Resolve an active member by ULID, unique case-insensitive name, or "owner" at the office's applied Position.
    /// Returns `Err(StoreError::AmbiguousName)` if a display name matches >1 member in `org.db`.
    fn resolve_member(&self, id_or_name: &str) -> Result<Option<(Member, Position)>, StoreError>;
}

pub trait MessageStore {
    /// Persist a new local or synced message (or return the existing commit if deduplicated by
    /// `id` + `content_hash` or `(origin_office_id, origin_incarnation, origin_seq)`),
    /// updating `msg_threads`, `msg_headers`, `msg_bodies`, `msg_state` (using `msg.initial_state`:
    /// `Unread` normally, or `Read` when claimed before commit by a live `msg.ask` reply waiter),
    /// `msg_unread` (only when `initial_state == Unread`), and `work_origin_log`.
    fn commit_message(&self, msg: NewStoredMessage) -> Result<CommitMessageOutcome, StoreError>;

    /// Atomically claim and mark read a specific message `id` or the oldest unsnoozed unread for `to_member_id`,
    /// upserting `state = 'read'` in `msg_state`, removing from `msg_unread`, and allocating a local `origin_seq`.
    fn claim_read(&self, to_member_id: &str, id: Option<&str>, now_utc: &str) -> Result<Option<StoredMessage>, StoreError>;

    /// Transition a message's lifecycle state (`Snoozed`, `Unread`, `Read`, `Done`, `Expired`, or `DeadLetter`)
    /// at the owning office, upserting `msg_state`, syncing `msg_unread` (including restoring a read message to
    /// `msg_unread` on `Snoozed` and incrementing `triage_gen`), and allocating a local `origin_seq` in `work_origin_log`.
    fn update_state_local(&self, change: LocalStateTransition) -> Result<MessageHeader, StoreError>;

    /// Apply a synced lifecycle state transition (`WorkChange::MessageStateChanged`) ordered by
    /// `(org_epoch, org_seq, origin_seq, origin_office_id, origin_incarnation)`, upserting `msg_state` and syncing `msg_unread`.
    fn apply_synced_state(&self, synced: SyncedMessageState) -> Result<bool, StoreError>;

    /// Evaluate and return actionable unread messages for `to_member_id` (releasing snoozes with
    /// `snoozed_until <= now_utc` and, when `after_task` is true, `snooze_after_task = 1`, bumping `triage_gen`),
    /// powering `msg.notify` and `msg.due`.
    fn check_notify(&self, to_member_id: &str, after_task: bool, urgent_only: bool, sweep_before_utc: &str, now_utc: &str) -> Result<Vec<NotifyCandidate>, StoreError>;

    /// List all active time-based `(snoozed_until, to_member_id, id)` rows in `msg_unread` to seed the in-memory snooze timer heap on startup.
    fn list_active_snooze_deadlines(&self) -> Result<Vec<SnoozeDeadline>, StoreError>;

    /// Query oldest non-expired, unsnoozed unread message for `to_member_id` (optionally filtered by `from_member_id`
    /// and `urgent_only`) without marking read. Also returns any expired unread IDs encountered.
    fn peek_next_unread(&self, to_member_id: &str, from_member_id: Option<&str>, urgent_only: bool, now_utc: &str) -> Result<PeekUnreadOutcome, StoreError>;

    /// Look up an existing message by ID (for `msg.get`, `msg.reply` recipient check, or idempotent `msg.ask`).
    fn get_message(&self, id: &str) -> Result<Option<StoredMessage>, StoreError>;

    /// Find a reply (`reply_to == ask_id` and `from_member_id == responder_id`) or `delivery_failed` for `ask_id`.
    fn find_reply_for_ask(&self, ask_id: &str, responder_id: &str) -> Result<Option<StoredMessage>, StoreError>;

    /// Re-evaluate messages with `FLAG_UNRESOLVED_REF` whose `(org_epoch, org_seq) <= applied_pos` when `org.db` advances.
    fn resolve_pending_references(&self, applied_pos: Position, now_utc: &str) -> Result<Vec<DeliveredMessage>, StoreError>;

    /// Paginate headers (`msg.inbox`, `msg.list`, `msg.human_needs_you`) or threads (`msg.threads`) without reading `msg_bodies`.
    fn list_headers(&self, query: HeaderQuery) -> Result<HeaderPage, StoreError>;
    fn list_threads(&self, query: ThreadQuery) -> Result<ThreadPage, StoreError>;
}
```

---

### 5. Performance and Size Targets

Accounting for ADR-0002's `PRAGMA synchronous = FULL` + `PRAGMA fullfsync = ON` and single-writer batching on macOS Apple Silicon:

| Metric | Target | Rationale / How Achieved |
| :--- | :--- | :--- |
| **`org-agent` release binary (stripped)** | **`< 1.0 MB`** | No async runtime, no SQLite, no TLS; depends only on `org-client`, `org-spool`, `org-proto`. |
| **`send` (same office, durable `fullfsync` commit)** | **`p50 < 15 ms` · `p95 < 30 ms`** | Process spawn (~2 ms) + Unix socket `hello`/`bind` (~0.5 ms) + 1 batched `F_FULLFSYNC` commit (~6–10 ms). Under 16-agent burst load, batching shares 1 flush across all 16 sends. |
| **`send` (offline fallback via `org-spool`)** | **`p50 < 12 ms` · `p95 < 25 ms`** | Local `tmp` write + `F_FULLFSYNC` + `rename` into `spool/new/` + dir `F_FULLFSYNC`. |
| **Waiter wake-up after commit (`wait` / `ask`)** | **`< 1 ms`** | In-memory `WaiterRegistry` channel hand-off in `Mailbox` immediately after `commit_message` returns, with `ask` replies claimed before commit (`initial_state = Read`). |
| **`ask` ↔ `reply` round-trip (same office, immediate responder)** | **`p50 < 35 ms` · `p95 < 60 ms`** | Two `F_FULLFSYNC` commits in series (1 for `ask`, 1 for `reply`, which commits directly as `Read` when a live reply waiter is claimed). |
| **`ask` ↔ `reply` round-trip (cross-office, local satellite via HQ)** | **`p50 < 60 ms` · `p95 < 120 ms`** | Four batched `F_FULLFSYNC` commits across persistent open sockets (`Office A → HQ → Office B → HQ → Office A`, ~8–10 ms per flush on Apple Silicon). |
| **`inbox` / `notify` / `list` / `threads` / `who` (at 100,000 stored messages)** | **`p50 < 2 ms` · `p95 < 5 ms`** | Queries touch only `msg_unread`, `msg_headers`, `msg_threads`, `msg_thread_participants`, and `org.db` / in-memory presence (never `msg_bodies`). |

---

## Two-Phase Rollout & Migration (`Phase A` Spool CLI → `Phase B` Office Server)

1. **Phase A (Immediate, Spool-Backed in `v1`):**
   - Port the working `v1-old` spool and CLI into `crates/org-spool` and `apps/org-agent` (with `org-agent relay` + `org-relay` symlink), preserving the `~/.agency/spool` format so no running agent loses its inbox.
   - Add `who`, `status`, `profile`, and message triage/snooze (`--priority`, `msg defer`, `msg done`, `msg notify`, `msg due`, `'U'`/`'N'` wake socket tags, `msg wait --urgent-only`, and snooze-aware `msg inbox`) backed by per-agent `flock`-protected metadata files (`inbox/<agent>/.meta.lock`, `triage.json`, `profile.json`, `status.json`) in `~/.agency/spool/inbox/<agent>/` so agents and all three runtime adapters (Claude Code, Codex, Antigravity) get the full workflow immediately.
2. **Phase B (Office-First Server Cutover):**
   - Implement `crates/org-mailbox` and the `store.rs` `MemberDirectory` + `MessageStore` tables in `org-office`, wire `who`, `status.set`, and `msg.*` in `org-proto` / `org-sync-proto` / `org-client` / `org-office-server`, and switch `apps/org-agent` from the spool transport to `org-client` (keeping `org-spool` as the offline fallback for `send`).
   - Run `org-office-server import-v1-old-spool --office <hq_office_id> --from ~/.agency/spool` to import existing messages, profiles, statuses, and read/snooze states into HQ's `office.db` (deduplicated by ULID).
   - Keep `~/.agency/spool` and `.v1-old` binaries for a 1-week rollback window, then delete the plaintext `~/.agency/spool` directory.
