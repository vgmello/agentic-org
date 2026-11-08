# ADR-0003 · Agent messaging: office-first, high-volume storage, directory, presence, triage, and cross-office routing

Status: **proposed** 2026-11-08. Author: `agy-senior-dev` (reviewed by `reviewer` and `ac-staff-engineer-ada`).
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
  - **Directory, Presence, Humans, and Triage:** Agents need to discover who is in the organization and what they are doing (`org-agent who`, `org-agent status`), communicate seamlessly with humans through the Agentic Org desktop app, and triage incoming messages without being interrupted during focused work except for urgent requests (`--priority high|urgent`, `msg defer`, `msg done`, `msg due`).
- **Heavy traffic requirement:** In an active organization, dozens of agents exchange prompts, review diffs, logs, and `ask`/`reply` calls continuously (10,000–100,000+ messages/day, bodies up to 256 KiB). Storing uncompressed bodies inline with headers or scanning full tables for unread/due messages would bloat SQLCipher page I/O and slow down both the CLI and the desktop app.

---

## Decisions

### 1. High-Volume Storage Architecture in `office.db`

All message tables live in each office's **`office.db`** (never `org.db`, so replacing `org.db` with an HQ snapshot never touches the office's messages or queues). All SQL and `rusqlite` types remain private inside `crates/org-office/src/store.rs` (ADR-0002 decision 5), with `PRAGMA foreign_keys = ON` on every connection.

#### 1.1 Table Separation: Hot Unread/Snooze Index, Fixed-Width Headers, Cold Compressed Bodies, and Threads

To keep `inbox`, `due`, `wait`, `read --next`, and UI thread views sub-millisecond even at $10^5$–$10^6$ stored messages, message storage in `office.db` is split into six tables plus the shared per-origin work log:

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
   - Participant junction table for fast `msg.threads { participant }` filtering without table scans:
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
     - `expires_at TEXT` (set on `request` / `ask` messages)
     - `ttl_ms INTEGER` (original timeout duration in ms for `request` / `ask`, used to bound clock skew across offices — see §3.3)
     - `body_bytes INTEGER NOT NULL` (uncompressed UTF-8 byte length of `body`)
     - `meta_bytes INTEGER NOT NULL DEFAULT 0` (uncompressed UTF-8 byte length of `meta`)
     - `content_hash BLOB NOT NULL` (32-byte digest of canonical `(to_member_id, kind, priority, subject, body, meta, reply_to)` for $O(1)$ retry deduplication without reading `msg_bodies`, surviving body pruning)
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

4. **`msg_unread` (tiny hot table of currently unread & snoozed messages, owned by the recipient's office)**
   - Columns:
     - `to_member_id TEXT NOT NULL`
     - `local_seq INTEGER NOT NULL`
     - `id TEXT NOT NULL UNIQUE REFERENCES msg_headers(id) ON DELETE CASCADE`
     - `from_member_id TEXT NOT NULL`
     - `kind TEXT NOT NULL`
     - `priority TEXT NOT NULL DEFAULT 'normal'`
     - `urgent INTEGER NOT NULL DEFAULT 0` (`1` iff `kind == 'request'` or `priority IN ('high', 'urgent')`)
     - `reply_to TEXT`
     - `created_at TEXT NOT NULL`
     - `expires_at TEXT`
     - `snoozed_until TEXT` (`NULL` when active/unsnoozed; RFC 3339 UTC timestamp or literal `'after-task'` when snoozed via `msg defer`)
     - `snooze_note TEXT` (optional note from `msg defer --note`)
     - `triaged_at TEXT` (`NULL` when new/untriaged; set when snoozed or when a due snooze is released)
     - `PRIMARY KEY (to_member_id, local_seq)`
   - Indexes:
     - `INDEX idx_msg_unread_due ON msg_unread(to_member_id, snoozed_until)`
   - **Why a separate table instead of `WHERE read = 0`:** Even after 500,000 messages are read or marked done, `msg_unread` contains only currently open (unread or snoozed) messages (typically $0$–$50$ rows, fitting in a single 4 KiB page in the WAL cache).
   - `msg.wait`, `msg.due`, `msg.read --next`, `msg.inbox`, and unread/needs-you counts in the UI touch only `msg_unread` (+ `msg_headers` for subject/names). Resolving a message (`read`, `done`, `expired`, or `dead_letter`) deletes its row from `msg_unread` and inserts into `msg_state`.

5. **`msg_state` (read/done/expiry/dead-letter resolution, owned by the recipient's office)**
   - Per [`offices-hq-and-satellites.md`](../design/offices-hq-and-satellites.md), a message is created by the sender's office and never changes; **the recipient's office owns its post-creation lifecycle state** (`read`, `done`, `expired`, or `dead_letter`; for people and the owner, HQ is their office; if a recipient was already removed in `org.db` before delivery, HQ owns the dead-letter resolution).
   - Columns:
     - `id TEXT PRIMARY KEY REFERENCES msg_headers(id) ON DELETE CASCADE`
     - `resolution TEXT NOT NULL` (`'read'`, `'done'`, `'expired'`, or `'dead_letter'`)
     - `reason TEXT` (`NULL` for `'read'`; optional note from `msg done --note` for `'done'`; `'expired'` or `'unknown_recipient'` otherwise)
     - `resolved_at TEXT NOT NULL`
     - `owner_office_id TEXT NOT NULL`
     - `origin_incarnation TEXT NOT NULL`
     - `origin_seq INTEGER NOT NULL` (work-change sequence allocated when the owning office resolves the message)
   - Index: `UNIQUE INDEX idx_msg_state_origin ON msg_state(owner_office_id, origin_incarnation, origin_seq)`.

6. **`member_status` (agent self-reported status lines in `office.db`)**
   - Stores each local agent's short status line set via `org-agent status "<text>"` so it survives local `org-office-server` restarts without polluting `org.db`'s permanent event log:
     - `member_id TEXT PRIMARY KEY`
     - `status_text TEXT NOT NULL` ($\le 140$ chars / $256$ UTF-8 bytes, no control characters)
     - `current_task_id TEXT` (optional, populated when working on a task)
     - `updated_at TEXT NOT NULL`
     - `expires_at TEXT NOT NULL` (defaults to `updated_at + 8 hours`)

7. **`work_origin_log` and `work_stream_cursors` (shared per-origin work stream in `office.db`)**
   - Per [`offices-hq-and-satellites.md`](../design/offices-hq-and-satellites.md) (lines 74–79, 210–213), each office numbers every work change it originates (a new message, a message snooze/triage change, a message resolution mark, a task version, a task handover) from one monotonic counter `origin_seq` in the **same transaction** as the change, and retains its origin log so it can stream to HQ or republish from a promoted HQ's position:
     - `work_origin_log(origin_seq INTEGER PRIMARY KEY, kind TEXT NOT NULL, target_id TEXT NOT NULL, payload_json TEXT, created_at TEXT NOT NULL)` (`kind` = `'msg_created'` | `'msg_snoozed'` | `'msg_resolved'` | `'task_version'` | `'task_handover'`)
     - `work_stream_cursors(scope TEXT NOT NULL, origin_office_id TEXT NOT NULL, origin_incarnation TEXT NOT NULL, last_origin_seq INTEGER NOT NULL, PRIMARY KEY (scope, origin_office_id, origin_incarnation))`

#### 1.2 Body Compression Strategy and Decompression Safety

- **Fixed algorithm (`compression = 1`):** Pure-Rust raw DEFLATE via `flate2` (`default-features = false, features = ["rust_backend"]` using `miniz_oxide` at `Compression::fast()`, level 1 — 100% safe Rust, zero C dependencies, exact version pin in `Cargo.toml`).
- **Threshold (`512` bytes):** Payloads with `body.len() < 512` bytes (or `meta` JSON `< 512` bytes) are stored raw (`compression = 0`). Payloads $\ge 512$ bytes are compressed (`compression = 1`) **before** sending the write request to the single writer thread, and decompressed on the reader thread after `SELECT`.
- **Decompression bomb protection:** Every decompression reader wraps the slice in `.take(MAX_BYTES + 1)` (`256 KiB + 1` for `body`, `64 KiB + 1` for `meta`) and verifies that the decompressed byte length exactly matches `msg_headers.body_bytes` (or `meta_bytes`), rejecting any mismatch with `StorageError`.
- Doing compression/decompression off the writer thread keeps the single SQLite writer thread CPU-free while shrinking typical 4–64 KiB markdown/JSON/diff payloads by 4–7×, cutting SQLCipher AES-256 page encryption and WAL `fullfsync` write volume proportionally.

#### 1.3 Scope, Retention, Batching, and Measurements

- **What each office stores** ([`offices-hq-and-satellites.md`](../design/offices-hq-and-satellites.md) decision 3):
  - **HQ** keeps every message, snooze state, and resolution mark across the organization (and owns all human inboxes).
  - **Each satellite** keeps messages where `from_member_id` or `to_member_id` is an agent placed in that satellite, or part of a thread/project that satellite follows.
- **Retention:**
  - `msg_headers` (including `content_hash`), `msg_threads`, and `msg_state` (~280 bytes/message) are kept as the organization's conversation index ($10^6$ headers $\approx 280\text{ MB}$). Because retry deduplication uses `msg_headers.content_hash`, pruning old `msg_bodies` never breaks deduplication.
  - Unread/snoozed messages in `msg_unread` and their `msg_bodies` are kept until resolved (`read`, `done`, `expired`, or `dead_letter`).
  - Resolved `msg_bodies` are kept by default (encrypted at rest); a per-organization retention policy (e.g. 90 days for resolved bodies, configurable later) can prune old `msg_bodies` rows (`DELETE FROM msg_bodies WHERE ...`) without breaking thread history or deduplication.
- **Write batching (ADR-0002 decision 4):**
  - The office server's single writer thread drains all queued write requests (up to 64 per batch) into one SQLite transaction, wrapping each request in its own `SAVEPOINT` and executing a single `COMMIT` (`synchronous = FULL`, `fullfsync = ON`).
- **Measurements to take in the implementation spike:**
  1. `send` latency (p50/p95) at concurrency 1, 8, and 32 with `fullfsync = ON`.
  2. Compression ratio, CPU time, and WAL byte growth over 10,000 synthetic agent messages (mix of 200 B short pings and 16 KiB code review bodies).
  3. `msg.inbox`, `msg.due`, and `msg.threads` query time at 100,000 stored rows in `office.db`.

---

### 2. Local Delivery, Triage & Snooze, Directory & Presence, and CLI Surface (`org-agent`)

#### 2.1 How `org-agent` Finds Its Office (`office.json` and `<handle>.sock`)

1. Each office on a Mac owns `<office_home>/offices/<office_id>/office.json` and, while running, listens on `<office_home>/run/<handle>.sock` (`org_proto::location::office_socket_path`).
2. When an office spawns an agent session, it sets `ORG_OFFICE=<office_id>` and `ORG_AGENT=<member_id_or_name>` in the agent's environment.
3. `org-agent` resolves its target `office_id` deterministically (never guessing from which `.sock` files happen to be open in `run/`, which could point at the wrong office if one of two local offices is restarting):
   - If `--office <id>` or `$ORG_OFFICE` is set, it uses that `office_id`.
   - Otherwise, it scans `<office_home>/offices/*/office.json`: if there is **exactly one** office directory on this Mac, it uses that `office_id`; if there are zero or multiple offices on this Mac, it exits `1` with a clear message instructing the caller to set `ORG_OFFICE` or pass `--office`.
4. Given `office_id`, the socket path is `office_socket_path(office_home, &office_id)` (`<office_home>/run/<handle>.sock`), and the offline spool folder (if needed by `send`) is `<office_home>/offices/<office_id>/spool/`.

#### 2.2 In-Memory Waiter Registry (Zero Polling, Race-Free)

The office's messaging capability (`Mailbox`) holds an in-memory `WaiterRegistry` protected by a short mutex, completely separate from SQLite:

- **Inbox waiters (`msg.wait`):** waiting for an actionable unread message for `to_member_id` (optionally filtered by `from_member_id` or `urgent_only: bool`).
- **Reply waiters (`msg.ask`):** waiting for a reply where `reply_to == ask_id` and `from_member_id == ask_recipient_id` (or a system `delivery_failed` with `reply_to == ask_id`).

Each waiter handle contains a single-completion cell (`OnceLock` / oneshot channel) so that if the writer thread and the initial reader check race on the same message, the handle completes at most once.

**Race-free "register before check" protocol:**
1. **`msg.wait { timeout_ms, from?, urgent_only? }`:**
   - **Step 1 (Register):** Inserts an inbox waiter handle into `WaiterRegistry` (also marking the member's runtime presence as `idle` / listening while held).
   - **Step 2 (Check):** Queries `msg_unread` (via a reader connection, `peek_next_unread`) for the oldest unsnoozed unread message for `to_member_id` (matching `from` and `urgent_only` if given).
     - If `peek_next_unread` encounters rows with `expires_at <= now_utc`, it skips them and enqueues an asynchronous `resolve_local(..., Expired)` command to the writer thread so they are cleaned out of `msg_unread`.
   - **Step 3 (Complete or block):** If a matching unread message is found (or if the writer completed the waiter handle during Step 2), removes the waiter handle and returns that message immediately **without** marking it read. Otherwise, blocks on the waiter handle until woken by the writer thread (either on a newly committed message or when a snoozed message's timer becomes due) or until `timeout_ms` elapses (returning `null`, which makes `org-agent msg wait` exit `2`).
2. **Writer-thread wake-up (`< 0.1 ms` after commit):**
   - Each writer batch collects the messages committed in that transaction (excluding any with `FLAG_UNRESOLVED_REF`, `FLAG_EXPIRED`, or `FLAG_DEAD_LETTER`).
   - **Immediately after `COMMIT` succeeds**, the writer notifies every matching inbox waiter and reply waiter in `WaiterRegistry` in memory.
3. **Same-office `msg.ask`:**
   - **Step 1 (Register):** Registers a reply waiter for `(ask.id, recipient_member_id)` in `WaiterRegistry` **before** submitting the `ask` (or re-ask) to the writer/reader.
   - **Step 2 (Commit or re-attach):** Submits the `request` message to the writer thread (or, on an idempotent re-ask where `ask.id` already exists in `msg_headers`, queries `find_reply_for_ask` on a reader connection — see §3.3.4).
   - **Step 3 (Wake & deferred read-mark):** When the recipient calls `msg.reply { to_id: ask.id, ... }`, the writer commits the reply and immediately hands the full `Message` to the waiting `ask` handle in memory. To avoid making `ask` wait for a second `F_FULLFSYNC` just to mark the reply read, the server returns the reply to the `ask` caller immediately and queues a local `resolve_local(..., Read)` for the next writer batch. If the server crashes in the millisecond between returning the reply and flushing the read mark, the reply remains unread in `msg_unread` (at-least-once semantics).

#### 2.3 Message Triage, Priority, and Snooze (`--priority`, `defer`, `done`, `due`)

To prevent agents from being interrupted mid-task by routine messages while guaranteeing immediate response to urgent messages:

1. **Interrupt Only for Urgent (`urgent = true`):**
   - `send` and `ask` take `--priority normal|high|urgent` (stored in `msg_headers.priority` and `msg_unread.priority`, and mirrored in `meta.priority` for spool compatibility).
   - A message is **urgent** (`urgent = true`) iff `kind == "request"` (`ask`) OR `priority` is `"high"` or `"urgent"`.
   - **Hook behavior:** Urgent messages wake an idle session immediately (`asyncRewake`) and are surfaced at the agent's next `Stop`. Non-urgent messages (`urgent = false`) never interrupt a busy agent; they wait for the agent's natural break (end of turn).
2. **Triage at the Break, Per Message:**
   At a break (`Stop`), the hook surfaces header summaries only (count, sender, subject, kind, urgent marker — **never bodies**), instructing the agent: *"Triage each new message: answer now, snooze it, or mark it done."* The agent reads each message (`org-agent msg read <id>`) and chooses one of three actions:
   - **Answer now:** `org-agent msg reply <id> "…"` (marks `<id>` read and sends the reply).
   - **Snooze (`defer`):** `org-agent msg defer <id> --until <30m|2h|after-task|ISO-time> [--note "<why>"] [--json]`.
     - If `<id>` was already read during this triage turn (`msg read <id>`), `msg defer` moves `<id>` back into `msg_unread` (clearing its `read` row in `msg_state`) with `snoozed_until` set to the target UTC timestamp (or literal `"after-task"`) and `snooze_note`, and sets `triaged_at = now_utc`.
     - While `snoozed_until` is in the future (or `"after-task"`), the message is hidden from default `msg inbox` so the agent has a clean slate.
     - `"after-task"` means due at the agent's next `Stop` (when its current task finishes).
   - **Done without a reply (`done`):** `org-agent msg done <id> [--note "<why>"] [--json]`.
     - Marks `<id>` resolved in `msg_state` (`resolution = 'done'`, `reason = note`) and removes it from `msg_unread`. Done messages never resurface.
3. **Periodic Check for Due Snoozes (`org-agent msg due [--after-task] [--sweep-after 5m] [--json]`):**
   - An idle agent is woken by the periodic watcher only when `msg due` reports `wake: true`, which happens iff:
     1. **Urgent unread (`due_reason = "urgent"`):** An unsnoozed unread message has `urgent == true`.
     2. **Snooze due (`due_reason = "snooze_due"`):** A snoozed message has `snoozed_until <= now_utc`, OR `snoozed_until == "after-task"` when `--after-task` is passed by the `Stop` hook. When a snooze becomes due, its `snoozed_until` is cleared (`NULL`, keeping `snooze_note` and setting `triaged_at = now_utc`) so it resurfaces **once** in both `msg due` and default `msg inbox`.
     3. **Untriaged sweep (`due_reason = "untriaged_sweep"`):** A new, untriaged unread message (`triaged_at IS NULL` and `snoozed_until IS NULL`) has waited longer than `--sweep-after` (default `5m`, configurable).
   - Low-priority snoozed items are never re-evaluated in bulk before their `snoozed_until` time; read or done messages never resurface.
4. **JSON Shapes for Hook & CLI (`--json`):**
   - **`org-agent msg inbox [--all] [--json]`** returns an array of header objects (never bodies when `--summary` is used by the hook; in Phase A/B `--json` on `inbox` includes all triage metadata):
     ```json
     [
       {
         "id": "01M6WRN71MBX8FHS08MB0Q9Z24",
         "from": "ac-staff-engineer-ada",
         "to": "agy-senior-dev",
         "kind": "msg",
         "priority": "normal",
         "urgent": false,
         "subject": "APPROVED: message triage + snooze",
         "reply_to": null,
         "created_at": "2026-11-08T05:03:51.348Z",
         "expires_at": null,
         "status": "unread",
         "snoozed_until": null,
         "snooze_note": null
       }
     ]
     ```
   - **`org-agent msg due [--after-task] [--sweep-after 5m] [--json]`:**
     ```json
     {
       "wake": true,
       "urgent_count": 1,
       "due_snoozed_count": 1,
       "untriaged_sweep_count": 0,
       "messages": [
         {
           "id": "01M6WRN71MBX8FHS08MB0Q9Z24",
           "from": "ac-staff-engineer-ada",
           "to": "agy-senior-dev",
           "kind": "request",
           "priority": "urgent",
           "urgent": true,
           "subject": "Quick check",
           "reply_to": null,
           "created_at": "2026-11-08T05:03:51.348Z",
           "expires_at": "2026-11-08T05:05:51.348Z",
           "snoozed_until": null,
           "snooze_note": "was snoozed until after-task",
           "due_reason": "urgent"
         }
       ]
     }
     ```
   - **`org-agent msg defer <id> --until ... [--note "..."] --json`:**
     `{"id":"...","status":"snoozed","snoozed_until":"2026-11-08T05:33:51.348Z","snooze_note":"..."}`
   - **`org-agent msg done <id> [--note "..."] --json`:**
     `{"id":"...","status":"done","note":"..."}`

#### 2.4 Directory, Presence, and Agent Status (`org-agent who` and `org-agent status`)

Agents must be able to discover everyone in the organization (agents, people, and the owner) and see what they are doing without knowing IDs in advance:

1. **Directory (`org-agent who`):**
   - `org-agent who [--team <name_or_id>] [--kind agent|person|owner] [--json]` lists all members matching the optional filters, read from the local office's copy of `org.db` (`Member`, `Membership`, `Team`, `reports_to_member_id`, `office_id`) combined with live runtime presence and status.
   - `org-agent who <id_or_name> [--json]` returns one member's full card:
     ```json
     {
       "id": "01M6W...",
       "name": "agy-senior-dev",
       "kind": "agent",
       "owner": false,
       "role_title": "Senior Engineer (Messaging)",
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
   - Derived from each office's runtime state:
     - `busy`: agent session is actively executing a turn/command in the office runner.
     - `idle` (or `online`): agent session is live and waiting for work (e.g. holding a `msg.wait` connection or idle between turns).
     - `offline`: no active session or listener for this member, with `last_seen_at` showing their most recent activity.
   - **How presence propagates across offices:** Per [`offices-hq-and-satellites.md`](../design/offices-hq-and-satellites.md) ("Runtime summaries are not logged"), presence is **ephemeral runtime data**, never written to `org.db`'s event log. Each office sends HQ passing `MemberPresenceUpdate` frames over its open connection when an agent's presence or status changes (debounced to at most 1 update/second per agent). HQ keeps the latest map in memory and forwards updates to connected offices and app subscribers (`member.presence_changed` event), avoiding any `fullfsync` per status change.
3. **Self-Reported Status Line (`org-agent status`):**
   - `org-agent status "<text>" [--ttl 8h] [--json]` sets the calling agent's status line; `org-agent status --clear` clears it.
   - **Limits & staleness:** `text` is trimmed, single-line (no control characters), at most **140 characters / 256 UTF-8 bytes**. It expires automatically after **8 hours** (default `--ttl 8h`, max `24h`). Expired status lines are treated as `null` on read.
   - **Storage classification:** Stored in the owning office's `office.db` (`member_status` table, so it survives a local office server restart) and broadcast via the passing runtime summary stream to HQ and other offices (not logged in `org.db` or `work_origin_log`).

#### 2.5 Talking to Humans (Owner and Added People)

Per [`teams-and-org-chart.md`](../design/teams-and-org-chart.md), agents can message, `ask`, or assign tasks to any **person** added to the organization (`MemberKind::Person`) and always to the **owner** (`owner: true`):

1. **HQ Owns All Human Inboxes:**
   - People are not placed in offices (`office_id: None`). **HQ is the owning office for every person's and the owner's inbox** (`office.db` at HQ), because humans interact through the Agentic Org desktop app.
   - If the desktop app is running on a Mac that only has a satellite office, the satellite proxies human inbox reads, replies, and sends over its connection to HQ.
2. **How Messages Reach a Human in the Desktop App:**
   - **Menu-Bar "Needs you" Rows & Indicator Dot (Eden's UX spec):**
     - When an agent sends an `ask` (`kind == "request"`) or an urgent message (`priority` in `("high", "urgent")`) to the owner (or a person), HQ emits `message.received` (`urgent: true`).
     - The Agentic Org menu-bar icon lights its indicator dot, posts a macOS notification, and shows a **"Needs you"** row in the menu-bar dropdown with the agent's name, subject/preview, and an inline reply box.
   - **App Inbox & Threads View:**
     - All messages to/from the owner and people (urgent and normal) appear in the app's Inbox view, grouped by thread (`msg.threads` / `msg.list`), where the human can read, reply (`msg.reply`), snooze (`msg.defer`), or mark done (`msg.done`).
3. **How a Human Replies:**
   - When the owner (or person) replies in the app (or via `ORG_AGENT=vgmello org-agent msg reply <id> "…"` in the CLI), the app calls `msg.reply` bound as that human member at HQ.
   - HQ commits the reply in HQ's `office.db`, marks the agent's original message `read`, and streams the reply immediately to the asking agent's office (`HQ → satellite`), waking a waiting `msg.ask` or surfacing at the agent's next break.
4. **What an `ask` to a Human Does While They Are Away:**
   - Humans answer on human timescales (minutes to hours, not milliseconds).
   - When `org-agent msg ask --to <human>` targets a `Person` or `owner` and the caller does not pass an explicit `--timeout`, the default timeout is **`24h`** (vs `120s` for agent recipients), automatically split by `org-agent` across 1-hour socket calls using idempotent re-ask (`§3.3.4`).
   - **Graceful Expiry ("Late Human Reply"):** If the caller's `--timeout` expires before the human replies:
     - `org-agent msg ask` exits `2` (`timeout`) so the calling agent can move on or fallback.
     - Instead of hiding the request as dead-lettered, HQ sets `flags |= FLAG_EXPIRED_WAITING_HUMAN` (`8`) and **keeps the request visible in the human's "Needs you" / Inbox queue** marked *"Agent timed out waiting — your reply will still be delivered"*.
     - When the human later replies in the app, the reply is delivered normally into the agent's inbox (`msg_unread`) with `reply_to = ask.id` and `priority = "high"`, waking the agent if idle or surfacing at its next `Stop`!

#### 2.6 CLI Surface and Exit Codes

```text
org-agent whoami [--as <member>] [--json]
org-agent who    [<member>] [--team <team>] [--kind agent|person|owner] [--office <id>] [--json]
org-agent status [<text> | --clear] [--ttl 8h] [--office <id>] [--as <member>] [--json]
org-agent msg send  --to <member> [--subject <s>] [--priority normal|high|urgent] [--meta k=v] [--office <id>] [--as <member>] [--json] <body | ->
org-agent msg ask   --to <member> [--timeout 120s] [--subject <s>] [--priority high|urgent] [--meta k=v] [--office <id>] [--as <member>] [--json] <body | ->
org-agent msg reply <id> [--subject <s>] [--priority normal|high|urgent] [--meta k=v] [--office <id>] [--as <member>] [--json] <body | ->
org-agent msg defer <id> --until <30m|2h|after-task|ISO> [--note <text>] [--office <id>] [--as <member>] [--json]
org-agent msg done  <id> [--note <text>] [--office <id>] [--as <member>] [--json]
org-agent msg due   [--after-task] [--sweep-after 5m] [--office <id>] [--as <member>] [--json]
org-agent msg inbox [--all] [--from <member>] [--thread <id>] [--limit <n>] [--cursor <seq>] [--office <id>] [--as <member>] [--json]
org-agent msg read  <id> | --next [--office <id>] [--as <member>] [--json]
org-agent msg wait  [--timeout 60s] [--from <member>] [--urgent-only] [--office <id>] [--as <member>] [--json]
```

- **Addressing (`--to`, `--from`, `--as`):** Accepts a member's ULID `id`, their unique `name` (case-insensitive match against `org.db`'s `members` table), or `"owner"` (which resolves to the organization's `owner: true` member). If a display name matches more than one member in `org.db`, the server rejects it with `ErrorCode::InvalidRequest` instructing the caller to pass the member's ULID.
- **Reserved names & kinds:** `"office"` is reserved for the server (`msg.bind { agent: "office" }`, `to: "office"`, or a client sending `kind: "delivery_failed"` fails with `ErrorCode::InvalidRequest`).
- **`msg.reply`, `msg.defer`, and `msg.done` authorization:** Only the recipient of the original message (`original.to_member_id == caller_member_id`) may reply to, defer, or mark done `<id>`; otherwise the server returns `ErrorCode::NotFound`.
- **`msg register` / `deregister`:** Kept as a transitional CLI command (in Phase A, writes `profile.json` and creates the spool inbox; in Phase B, resolves the member in `org.db`).
- **Exit codes:**

| Exit Code | Meaning | `org-proto` Error Codes / Conditions |
| :--- | :--- | :--- |
| `0` | Success (`read --next` on empty inbox prints `null` in `--json` and exits `0`) | `ok: true` (or offline spool queued on `send`) |
| `1` | Usage or validation error | `invalid_request`, `invalid_name`, `too_large`, `duplicate_id`, `not_found`, `not_bound` |
| `2` | Timeout waiting for message or reply | `timeout` (or `msg.wait` returning `null` at deadline) |
| `3` | Unknown recipient | `unknown_recipient` |
| `4` | Storage or protocol error | `storage_error`, `unknown_method`, `unknown`, frame decode error |
| `5` | Office server unavailable | Socket connect failure on `ask`, `reply`, `defer`, `done`, `due`, `wait`, `read`, `inbox`, `who`, `status` |
| `130` | Interrupted | `SIGINT` / `SIGTERM` |

---

### 3. Cross-Office Routing, Deadlines, Dedup, Offline Outbox, and Recipient Checks

#### 3.1 Per-Origin Work Stream (`office → HQ → office`)

Per [`offices-hq-and-satellites.md`](../design/offices-hq-and-satellites.md) ("Office-first: tasks and messages"):

1. **Local commit + sequence allocation:**
   - When an agent in Office A sends a message (`msg.send`, `msg.reply`, `msg.ask`) or when the recipient's office updates triage/resolution state (`msg.defer`, `msg.done`, `msg.read`, `expired`, or `dead_letter`), that office allocates the next monotonic `origin_seq` for `(office_id, incarnation)` in `work_origin_log` **in the same SQLite transaction**.
   - If the recipient is also placed in Office A, local delivery is already complete and any local `wait`/`ask` listener wakes immediately after commit.
2. **Streaming to HQ and HQ forwarding:**
   - Office A streams new `work_origin_log` entries over its persistent outbound connection to HQ.
   - HQ commits the work change in HQ's `office.db` (deduplicating by `(origin_office_id, origin_incarnation, origin_seq)` via `idx_msg_origin` / `idx_msg_state_origin`), sends `WorkAck` back to Office A, and **immediately after commit** pushes the work change to:
     - the recipient's office (if `to_member_id` is an agent placed in Satellite B), and
     - any other satellite following that thread/project.
   - If `to_member_id` is the owner or a person (`kind == Person`), HQ is their owning office.
   - Satellite B commits the incoming message into its `office.db` (inserting into `msg_unread` when `to_member_id` is placed in Satellite B) and wakes any local `wait` or `ask` listener for that agent.
3. **Reconnect, Backfill, and Agent Moves:**
   - Each office tracks `work_stream_cursors` per `(scope, origin_office_id, origin_incarnation)`. On reconnect, only missing sequence numbers are sent.
   - When an agent is moved from Office A to Office B (or removed via `member.remove` / office removal), Office A cancels any active `WaiterRegistry` waiters for that agent (`ErrorCode::NotBound`) and removes the agent's rows from Office A's `msg_unread`, while HQ backfills the moved agent's unread/snoozed messages and recent threads to Office B.

#### 3.2 Two-Stage Recipient Check, Structure Position (`org_epoch, org_seq`), and Single-Owner Dead-Lettering

Per [`teams-and-org-chart.md`](../design/teams-and-org-chart.md) ("Who agents may interact with") and [`offices-hq-and-satellites.md`](../design/offices-hq-and-satellites.md) ("References to structure"):

1. **Stage 1 — At the sender's office (synchronous before commit):**
   - The sender's office checks `to` against its local `org.db` copy at the `Position { epoch, seq }` it has currently applied (never against unapplied commands still in its org-command outbox).
   - `to` must match an active `Member` (`kind: Agent | Person`, or `owner: true`). If not, the office rejects the call immediately with `ErrorCode::UnknownRecipient` (exit code `3`).
   - The committed message records `org_epoch` and `org_seq` from that applied `Position`, plus `to_display_name` (the recipient's current display name in `org.db`).
2. **Stage 2 — When a message arrives at HQ or a satellite (`FLAG_UNRESOLVED_REF`):**
   - If a synced message arrives with `(org_epoch, org_seq) > local_org_position` (the work stream arrived slightly ahead of the organization-event stream), the receiving office commits the message to `msg_headers`/`msg_bodies` with `flags |= FLAG_UNRESOLVED_REF` and **withholds it from `msg_unread` and `WaiterRegistry`** so the work stream never stalls and the message never wakes an agent before its structure position is applied.
   - Whenever `org.db` advances to a new `Position`, the office runs `resolve_pending_references(applied_pos)`: for every message in `idx_msg_unresolved` with `(org_epoch, org_seq) <= applied_pos`, it validates `from_member_id` and `to_member_id`. If valid, it clears `FLAG_UNRESOLVED_REF`, inserts into `msg_unread` (if this office owns the recipient and the message is unresolved), and notifies `WaiterRegistry`.
3. **Single-owner lifecycle & `delivery_failed` notices (with recipient ID + display name at send time):**
   - **Rule:** Only the **recipient's owning office** (or **HQ alone** if the recipient member was removed in `org.db` or has no office) may resolve a message (`read`, `done`, `expired`, or `dead_letter` in `msg_state`) and emit a `delivery_failed` notice. Other offices following the thread apply the synced `WorkChange::MessageResolved` event and never emit their own `delivery_failed`.
   - When the owning office (or HQ on `member.remove`) dead-letters an unread message:
     - It records `resolution = 'dead_letter'`, `reason = 'unknown_recipient'` in `msg_state`, sets `flags |= FLAG_DEAD_LETTER` in `msg_headers`, and removes the row from `msg_unread`.
     - If `original.kind != "delivery_failed"` and `original.from_member_id != "office"` and `original.from_member_id` is still an active member in `org.db`, it emits a single `delivery_failed` message (`from_member_id = "office"`, `to_member_id = original.from_member_id`, `kind = "delivery_failed"`, `priority = "high"`, `reply_to = original.id`), guarded by `idx_msg_delivery_failed_once`.
     - **`delivery_failed` payload (for agents and Eden's UX):** The `delivery_failed` message carries structured `meta` (and human-readable `body`) with the **recipient as addressed at send time** (`recipient_id`: `original.to_member_id`, `recipient_name`: `original.to_display_name`) and the **reason code** (`reason`: `"unknown_recipient"` | `"expired"`), so both the sending agent and the desktop UI can display who the message was addressed to even after that member has been removed.

#### 3.3 Cross-Office `ask` with Deadlines, Clock-Skew Guard, and Idempotent Re-Ask

1. **Sending a cross-office `ask`:**
   - Agent 1 in Office A runs `org-agent msg ask --to agent-2 --timeout 120s "…"`.
   - Office A records both `ttl_ms = 120_000` and `expires_at = now_utc + 120s`, registers the reply waiter `(ask.id, agent_2_id)` in Office A's `WaiterRegistry`, commits the `request` locally, and streams it to HQ $\rightarrow$ Office B.
2. **Deadline enforcement & clock-skew protection:**
   - Clocks between a remote satellite Mac and HQ/Office B can drift by a few seconds. To prevent a small clock skew from expiring a short-timeout `ask` upon arrival while still bounding stale requests:
     - When Office B receives a cross-office `request` with `ttl_ms`, its effective local expiry is `MAX(expires_at, local_received_at + MIN(ttl_ms, 5_000 ms))` if the sender's `created_at` is within 5 minutes of Office B's clock, and `expires_at` otherwise.
   - If an agent-targeted request is past its effective expiry when it reaches the recipient's owning office (or when `agent-2` calls `msg.read` / `msg.wait`), the recipient's owning office resolves it as `resolution = 'expired'` in `msg_state`, removes it from `msg_unread` so `agent-2` never acts on an expired request, and emits `delivery_failed` (`reason = "expired"`, `recipient_id`, `recipient_name`, `reply_to = ask.id`) back to Office A. (For human-targeted requests, see §2.5.4: the request is flagged `FLAG_EXPIRED_WAITING_HUMAN` and stays actionable in the human's app inbox.)
3. **Reply path (`Office B → HQ → Office A`):**
   - When `agent-2` in Office B runs `org-agent msg reply <ask.id> "…"`, Office B commits the `reply` (`reply_to = ask.id`, `thread_id = ask.thread_id`), marks `ask.id` read in Office B's `msg_state`, and streams both to HQ $\rightarrow$ Office A.
   - When Office A commits the incoming reply (or an incoming `delivery_failed` with `reply_to = ask.id`), Office A's `WaiterRegistry` wakes the held `msg.ask` connection immediately (`< 1 ms`). If the wake-up is a `delivery_failed`, `msg.ask` returns the corresponding error (`unknown_recipient` or `timeout`).
4. **Race-Free Idempotent Re-Ask on Disconnect:**
   - `org-agent` generates the `ask`'s ULID `id` before sending.
   - If the local socket between `org-agent` and Office A drops (e.g. Office A restarts) before the deadline, `org-agent` reconnects with backoff and calls `msg.ask` with the **same `id`**, the same content, and the remaining `timeout_ms`.
   - Office A looks up `id` in `msg_headers`:
     - If `id` exists with a different `from_member_id` or different `content_hash`, it returns `ErrorCode::DuplicateId`.
     - If `id` exists with matching `(from_member_id, content_hash)`:
       1. Office A **first registers** a reply waiter `(id, to_member_id)` in `WaiterRegistry` (preventing any race if the reply commits concurrently).
       2. Office A then queries `find_reply_for_ask(id, to_member_id)` on a reader connection (which checks for a reply from `to_member_id` or a `delivery_failed` for `id`, even if already marked read).
       3. If found, it removes the waiter and returns the reply (or error) immediately; otherwise it waits on the registered waiter handle for the remaining `timeout_ms` without re-sending the request.

#### 3.4 Ordering and Deduplication Guarantees

- **Deduplication:**
  - Client retries at the local office look up `id` by primary key in `msg_headers` and compare `(from_member_id, content_hash)` in $O(1)$ (`DuplicateId` on mismatch; original commit result on match).
  - Cross-office stream items are deduplicated by `(origin_office_id, origin_incarnation, origin_seq)` via `idx_msg_origin` and `idx_msg_state_origin`.
- **Ordering:**
  - Work changes originating from any single office are delivered and applied in strict `origin_seq` order.
  - Each office presents its inbox and `msg.wait` stream in monotonic `local_seq` commit order.

#### 3.5 Offline Behavior (Two Distinct Boundaries)

1. **Satellite Office Disconnected from HQ (Network Offline):**
   - Same-office messaging (`send`, `ask`, `reply`, `defer`, `done`, `due`, `wait`, `read`, `inbox`, `who`, `status`) works at full local speed.
   - Cross-office `send` and `reply` commit locally in `office.db` (`work_origin_log`) and stream to HQ automatically when the connection resumes. Because `work_origin_log` is separate from the organization-command outbox (`offices-hq-and-satellites.md`), a blocked organization command never holds back agent messages.
   - Cross-office `ask` commits locally and waits; if HQ remains unreachable past `timeout_ms`, the local waiter times out with exit code `2`, and when HQ reconnects later, the request's `expires_at` prevents a remote agent from acting on it.
2. **Local Office Server Temporarily Unreachable (`org-spool` Offline Fallback):**
   - If `org-agent` cannot connect to `<office_home>/run/<handle>.sock` (`ENOENT` or `ECONNREFUSED`), it retries briefly (up to 500 ms with backoff) in case `org-office-host` is restarting the server.
   - If still unreachable:
     - **Fire-and-forget `send`** writes a versioned `SpooledSend` JSON file into `<office_home>/offices/<office_id>/spool/tmp/` (mode `0700` directory, `0600` file), flushes the file with `F_FULLFSYNC`, atomically renames it into `<office_home>/offices/<office_id>/spool/new/<id>.json`, flushes the `new/` directory with `F_FULLFSYNC`, prints `"queued": true` in `--json` (with a stderr notice), and exits `0`.
     - **`ask`, `reply`, `defer`, `done`, `due`, `wait`, `read`, `inbox`, `who`, and `status`** require live office state and exit `5` (`office server unavailable`).
   - **Spool Ingest & Poison-File Safety:** `org-office-server` drains `<office_home>/offices/<office_id>/spool/new/` through `crates/org-spool` on startup **before** accepting client connections, and re-scans every 5 seconds as a backstop.
     - Files are opened with `O_NOFOLLOW` and capped at `512 KiB`.
     - Valid sends are committed to `office.db` (deduplicated by `id` + `content_hash`) and then unlinked.
     - If a spooled file has an unknown recipient or is expired at ingest time, it is committed as `FLAG_DEAD_LETTER` (preserving the raw `to` string in `to_display_name` and emitting `delivery_failed` if `from` is a valid member) and unlinked.
     - If a spooled file is corrupt/unparseable JSON or exceeds 512 KiB, it is moved to `spool/dead/<filename>` with a `.reason` file so it never blocks or loops on subsequent 5-second scans.

---

### 4. Contract Additions (`org-proto` & `org-sync-proto`) and Code Ownership

#### 4.1 `org-proto` Additions (`crates/org-proto/src/msg.rs`, owned by `ada`)

`API_VERSION` remains `1` (all additions are additive). Because each `org-office-server` serves a single office (and therefore a single organization), `msg.*` calls on an office socket do **not** need an `org` parameter.

- **Agent & Shared Calls (`impl Call`):**
  - `msg.bind { agent?: String } → { member_id?: String, office_id: String }`
    - Resolves `agent` (member ID, unique member name, or `"owner"`; `"office"` is rejected) against `org.db` and binds the connection's acting member.
  - `msg.who { member?: String, team?: String, kind?: MemberKind } → { members: Vec<DirectoryCard> }`
    - Returns directory cards (`id`, `name`, `kind`, `owner`, `role_title`, `teams`, `reports_to`, `office`, `presence`, `last_seen_at`, `status`, `status_expires_at`, `current_task_id`).
  - `msg.status_set { text?: String, ttl_ms?: u64 } → { status: Option<String>, expires_at: Option<String> }` (`text: None` clears the caller's status line).
  - `msg.send { id: String, to: String, kind: String, priority?: MessagePriority, subject?: String, body: String, meta?: BTreeMap<String, Value>, reply_to?: String, expires_at?: String, ttl_ms?: u64 } → { id: String, thread_id: String, local_seq: u64, origin_seq: u64 }`
  - `msg.reply { id: String, to_id: String, priority?: MessagePriority, subject?: String, body: String, meta?: BTreeMap<String, Value> } → { id: String, thread_id: String, local_seq: u64, origin_seq: u64 }`
  - `msg.ask { id: String, to: String, priority?: MessagePriority, subject?: String, body: String, meta?: BTreeMap<String, Value>, timeout_ms: u64 } → { reply: Message }`
  - `msg.defer { id: String, until: SnoozeTarget, note?: String } → { id: String, snoozed_until: String, snooze_note: Option<String> }` (`SnoozeTarget` = `UntilUtc(String)` | `AfterTask`)
  - `msg.done { id: String, note?: String } → { id: String, resolved_at: String }`
  - `msg.due { after_task?: bool, sweep_after_ms?: u64 } → DueResult { wake: bool, urgent_count: u32, due_snoozed_count: u32, untriaged_sweep_count: u32, messages: Vec<MessageHeader> }`
  - `msg.wait { timeout_ms: u64, from?: String, urgent_only?: bool } → { message: Option<Message> }` (returns the oldest unsnoozed unread message without marking it read; `None` on timeout)
  - `msg.read { id?: String } → { message: Option<Message> }` (atomically claims and marks read a specific message or the oldest unsnoozed unread message)
  - `msg.inbox { include_all?: bool, from?: String, thread_id?: String, cursor?: u64, limit?: u32 } → { headers: Vec<MessageHeader>, next_cursor: Option<u64> }` (returns up to `limit <= 100` headers ordered by `local_seq DESC`, including `priority`, `urgent`, `status`, `snoozed_until`, `snooze_note`, `read_at`; hides currently snoozed messages unless `include_all = true`; never includes `body` or `meta`)
- **App / Human Inbox Calls (used by Agentic Org desktop app):**
  - `msg.human_needs_you { member_id?: String, limit?: u32 } → { items: Vec<MessageHeader>, total_urgent_unread: u32 }` (powers the menu-bar "Needs you" rows and indicator dot for the owner or specified person; defaults to the organization's `owner: true` member)
  - `msg.threads { participant?: String, cursor?: ThreadCursor, limit?: u32 } → { threads: Vec<ThreadSummary>, next_cursor: Option<ThreadCursor> }` (where `ThreadCursor` is `{ last_msg_at: String, thread_id: String }`)
  - `msg.list { thread_id?: String, participant?: String, unread_only?: bool, include_snoozed?: bool, cursor?: u64, limit?: u32 } → { headers: Vec<MessageHeader>, next_cursor: Option<u64> }`
  - `msg.get { id: String } → { message: Message }`
  - To reply, send, defer, or mark done from the app as the owner (or a person), the app calls `msg.bind { agent: "owner" }` (or the person's `member_id`) and uses `msg.reply`, `msg.send`, `msg.defer`, and `msg.done`.
- **Events (`impl EventKind`, pushed to `subscribe` clients):**
  - `message.received { header: MessageHeader }` (no body in events so frames stay small)
  - `message.snoozed { id: String, thread_id: String, to_member_id: String, snoozed_until: Option<String>, snooze_note: Option<String> }`
  - `message.resolved { id: String, thread_id: String, to_member_id: String, resolution: MessageResolution, reason: Option<String>, resolved_at: String }`
  - `message.delivery_failed { id: String, original_id: String, thread_id: String, from_member_id: String, recipient_id: String, recipient_name: String, reason: String }` (carries both the recipient's `id` and `display_name` as addressed at send time and the `reason` code such as `unknown_recipient` or `expired`)
  - `member.presence_changed { member_id: String, presence: MemberPresence, last_seen_at: String, status: Option<String>, status_expires_at: Option<String>, current_task_id: Option<String> }`
- **Error codes added to `org_proto::ErrorCode`:**
  - `Timeout`, `TooLarge`, `DuplicateId`, `NotBound` (`UnknownRecipient`, `NotFound`, `InvalidRequest`, `StorageError` already exist in `org-proto`).
- **Limits (measured on JSON-encoded wire bytes, fitting inside `MAX_FRAME_BYTES = 1 MiB`):**
  - `body` $\le 256\text{ KiB}$, `meta` $\le 64\text{ KiB}$, `subject` $\le 1\text{ KiB}$, `status` $\le 256\text{ B}$ ($140$ chars), `snooze_note` / `done` note $\le 1\text{ KiB}$, `kind` / member names / IDs $\le 128\text{ B}$.
  - `timeout_ms` $\le 3\_600\_000$ (1 hour per socket request), max 16 held `wait`/`ask` requests per connection.
- **Offline spool contract (`org_proto::msg::SpooledSend`):**
  - `{ v: 1, office_id: String, from: String, id: String, to: String, kind: String, priority: MessagePriority, subject?: String, body: String, meta?: BTreeMap<String, Value>, reply_to?: String, expires_at?: String, ttl_ms?: u64, created_at: String }`

#### 4.2 `org-sync-proto` Additions (Office ↔ HQ Stream, owned by `ada`)

In the office-to-office protocol (`crates/org-sync-proto`):

- **Durable work-stream items (`WorkChange`):**
  - `WorkChange::MessageCreated(SyncedMessage)`: carries `origin_office_id`, `origin_incarnation`, `origin_seq`, `org_position: Position`, all `MessageHeader` fields (including `from_display_name`, `to_display_name`, `priority`, `content_hash`, and `ttl_ms`), and `(compression, body_blob, meta_blob)`.
  - `WorkChange::MessageSnoozed(SyncedMessageSnooze)`: carries `origin_office_id`, `origin_incarnation`, `origin_seq`, `org_position: Position`, `msg_id`, `to_member_id`, `snoozed_until: Option<String>`, `snooze_note: Option<String>`, `triaged_at: String`.
  - `WorkChange::MessageResolved(SyncedMessageResolution)`: carries `origin_office_id`, `origin_incarnation`, `origin_seq`, `org_position: Position`, `msg_id`, `to_member_id`, `resolution: Read | Done | Expired | DeadLetter`, `reason: Option<String>`, `resolved_at: String`.
  - `WorkAck { origin_office_id, origin_incarnation, up_to_seq: u64 }` and per-scope backfill cursor `WorkBackfill { scope, from_positions }`.
- **Passing runtime updates (not logged in `org.db` or `work_origin_log`):**
  - `RuntimeUpdate::MemberPresence { office_id: String, members: Vec<MemberRuntimeState> }` (carries `member_id`, `presence: Online | Busy | Idle | Offline`, `last_seen_at`, `status`, `status_expires_at`, `current_task_id`).

#### 4.3 Where the Code Lives, Coupling Metrics (`docs/architecture-rules.md`), and the `MessageStore` Trait

Per `AGENTS.md` (updated by `ada` on `v1`):

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
| `crates/org-mailbox` | `agy-senior-dev` | Pure messaging, triage/snooze, directory & presence domain (`WaiterRegistry`, `flate2` compression + bounded decompression, idempotent `ask`, dead-letter rules) over `MessageStore` trait | `org-proto`, `org-sync-proto`, `flate2` (`Ce = 3`, **no SQLite**) | `~0.40` (Domain capability) |
| `crates/org-spool` | `agy-senior-dev` | Crash-safe local spool (`tmp` + `F_FULLFSYNC` + `rename` into `new`, `O_NOFOLLOW` ingest; powers Phase A CLI and Phase B offline fallback) | `org-proto`, `serde`, `serde_json` (`Ce = 3`) | `~0.35` (Platform primitive) |
| `apps/org-agent` | `agy-senior-dev` | Fast stateless CLI (`who`, `status`, `msg` with `defer`/`done`/`due`) | `org-client`, `org-spool`, `org-proto` (`Ce = 3`) | `1.00` (Edge binary) |
| `crates/org-office` (`store.rs`) | `server-dev` | Implements `MessageStore` trait inside `store.rs` on the single batching SQLCipher writer and read pool | Owns `rusqlite` / SQLCipher | Domain + Store |

**The `MessageStore` Trait (Dependency Inversion):**
`crates/org-mailbox` defines the storage trait that `crates/org-office/src/store.rs` implements:

```rust
pub trait MessageStore {
    /// Resolve a member by ULID, unique case-insensitive name, or "owner" at the office's applied Position.
    /// Returns `Err(StoreError::AmbiguousName)` if a display name matches >1 member in `org.db`.
    fn resolve_member(&self, id_or_name: &str) -> Result<Option<(Member, Position)>, StoreError>;

    /// List directory entries from `org.db` (members, teams, memberships, reports_to, role titles)
    /// combined with stored `member_status` rows in `office.db`.
    fn list_directory(&self, filter: DirectoryFilter, now_utc: &str) -> Result<Vec<StoredDirectoryMember>, StoreError>;

    /// Set or clear an agent's self-reported status line in `office.db` (`member_status`).
    fn set_member_status(&self, member_id: &str, status: Option<NewMemberStatus>) -> Result<(), StoreError>;

    /// Persist a new local or synced message (or return the existing commit if deduplicated by
    /// `id` + `content_hash` or `(origin_office_id, origin_incarnation, origin_seq)`),
    /// updating `msg_threads`, `msg_headers`, `msg_bodies`, `msg_unread`, and `work_origin_log`.
    fn commit_message(&self, msg: NewStoredMessage) -> Result<CommitMessageOutcome, StoreError>;

    /// Atomically claim and mark read a specific message `id` or the oldest unsnoozed unread for `to_member_id`,
    /// allocating a local `origin_seq` in `msg_state` and `work_origin_log`.
    fn claim_read(&self, to_member_id: &str, id: Option<&str>, now_utc: &str) -> Result<Option<StoredMessage>, StoreError>;

    /// Defer (snooze) a message for `to_member_id` until `snoozed_until` (UTC timestamp or `"after-task"`),
    /// keeping or restoring it in `msg_unread` and allocating a local `origin_seq` in `work_origin_log`.
    fn defer_local(&self, id: &str, to_member_id: &str, snoozed_until: &str, note: Option<&str>, now_utc: &str) -> Result<MessageHeader, StoreError>;

    /// Apply a synced snooze update (`WorkChange::MessageSnoozed`) without allocating a local `origin_seq`.
    fn apply_synced_snooze(&self, synced: SyncedMessageSnooze) -> Result<bool, StoreError>;

    /// Evaluate and return due messages for `to_member_id` (urgent unsnoozed, snoozes with `snoozed_until <= now_utc`
    /// or `"after-task"` when `after_task` is true, and untriaged messages older than `sweep_before_utc`),
    /// releasing due snoozes in `msg_unread`.
    fn check_due(&self, to_member_id: &str, after_task: bool, sweep_before_utc: &str, now_utc: &str) -> Result<DueResult, StoreError>;

    /// Record a local resolution (`Read`, `Done`, `Expired`, or `DeadLetter`) in `msg_state`,
    /// removing the row from `msg_unread` and allocating a local `origin_seq`.
    fn resolve_local(&self, id: &str, to_member_id: &str, resolution: MessageResolution, reason: Option<&str>, now_utc: &str) -> Result<bool, StoreError>;

    /// Apply a synced resolution from the recipient's owning office (`WorkChange::MessageResolved`)
    /// using the remote `(origin_office_id, origin_incarnation, origin_seq)` without allocating a local `origin_seq`.
    fn apply_synced_resolution(&self, synced: SyncedMessageResolution) -> Result<bool, StoreError>;

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
| **Waiter wake-up after commit (`wait` / `ask`)** | **`< 1 ms`** | In-memory `WaiterRegistry` channel hand-off immediately after `COMMIT`, with the `ask` reply's read-mark deferred to the next writer batch. |
| **`ask` ↔ `reply` round-trip (same office, immediate responder)** | **`p50 < 35 ms` · `p95 < 60 ms`** | Two `F_FULLFSYNC` commits in series (1 for `ask`, 1 for `reply`; reply read-mark is deferred off the critical path). |
| **`ask` ↔ `reply` round-trip (cross-office, local satellite via HQ)** | **`p50 < 60 ms` · `p95 < 120 ms`** | Four batched `F_FULLFSYNC` commits across persistent open sockets (`Office A → HQ → Office B → HQ → Office A`, ~8–10 ms per flush on Apple Silicon). |
| **`inbox` / `due` / `list` / `threads` / `who` (at 100,000 stored messages)** | **`p50 < 2 ms` · `p95 < 5 ms`** | Queries touch only `msg_unread`, `msg_headers`, `msg_threads`, `msg_thread_participants`, and `org.db` / in-memory presence (never `msg_bodies`). |

---

## Two-Phase Rollout & Migration (`Phase A` Spool CLI → `Phase B` Office Server)

1. **Phase A (Immediate, Spool-Backed in `v1`):**
   - Port the working `v1-old` spool and CLI into `crates/org-spool` and `apps/org-agent` (with `org-agent relay` + `org-relay` symlink), preserving the `~/.agency/spool` format so no running agent loses its inbox.
   - Add `who`, `status`, `profile`, and message triage/snooze (`--priority`, `msg defer`, `msg done`, `msg due`, and snooze-aware `msg inbox`) backed by per-agent metadata files in `~/.agency/spool/inbox/<agent>/` so agents and `codex-dev`'s hook get the full workflow immediately.
2. **Phase B (Office-First Server Cutover):**
   - Implement `crates/org-mailbox` and the `store.rs` `MessageStore` tables in `org-office`, wire `msg.*` in `org-proto` / `org-sync-proto` / `org-client` / `org-office-server`, and switch `apps/org-agent` from the spool transport to `org-client` (keeping `org-spool` as the offline fallback for `send`).
   - Run `org-office-server import-v1-old-spool --office <hq_office_id> --from ~/.agency/spool` to import existing messages, profiles, statuses, and read/snooze states into HQ's `office.db` (deduplicated by ULID).
   - Keep `~/.agency/spool` and `.v1-old` binaries for a 1-week rollback window, then delete the plaintext `~/.agency/spool` directory.
