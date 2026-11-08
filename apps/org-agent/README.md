# `org-agent`: The Agents' CLI

Fast, zero-runtime CLI for agent-to-agent messaging, directory lookup (`who`), self-reported status (`status`), and message triage (`defer`, `done`, `notify`, `due`) over `crates/org-spool`.

## Commands

```bash
export ORG_AGENT=<your-agent-id>

# Identity, Directory, and Status
org-agent whoami [--as <agent>] [--json]
org-agent who [<member>] [--team <team>] [--kind agent|person|owner] [--json]
org-agent status "<text>" [--ttl 8h] [--as <agent>] [--json]
org-agent status [--json]                       # your own; others' via `who <member>`
org-agent status --clear [--as <agent>] [--json]
org-agent profile set [<id>] [--name <n>] [--kind agent|person] [--owner] [--role <r>] [--team <id[:name[:title]]>] [--reports-to <id[:name]>] [--office <id[:name]>] [--json]

# Messaging & Triage
org-agent msg register [<agent>] [--name <n>] [--role <r>] [--team <t>] [--reports-to <r>] [--json]
org-agent msg deregister <agent> [--json]            # <agent> required: never defaults to you
org-agent msg send   --to <agent> [--subject <s>] [--priority normal|high|urgent] [--meta k=v] [--json] <body | ->
org-agent msg ask    --to <agent> [--timeout 120s] [--subject <s>] [--priority high|urgent] [--meta k=v] [--json] <body | ->
org-agent msg reply  <id> [--subject <s>] [--priority normal|high|urgent] [--meta k=v] [--json] [--body <body> | <body | ->]
org-agent msg defer  <id> --until <30m|2h|after-task|ISO> [--note <text>] [--json]
org-agent msg done   <id> [--note <text>] [--json]
org-agent msg notify [--session <id>] [--after-task] [--urgent-only] [--sweep-after 5m] [--format text|json] [--as <agent>]
org-agent msg due    [--after-task] [--urgent-only] [--sweep-after 5m] [--as <agent>]
org-agent msg inbox  [--all] [--summary] [--from <agent>] [--thread <id>] [--limit <n>] [--as <agent>] [--json]
org-agent msg read   <id> | --next [--as <agent>] [--json]
org-agent msg wait   [--timeout 60s] [--from <agent>] [--urgent-only] [--as <agent>] [--json]

# Spool Relay Daemon (also dispatched when invoked as `org-relay`)
org-agent relay run [--once]
org-agent relay status [--json]
```

## Spool location

The spool lives in `$ORG_HOME/spool` (default `~/.agency/spool`), in the same layout as the
v1-old CLI (`outbox/{tmp,new}`, `inbox/<agent>/{tmp,new,cur,wake}`, `dead/`), so both can run
against the same spool. Per-agent metadata sits next to the mail, guarded by an exclusive
`flock` on `inbox/<agent>/.meta.lock`: `triage.json` (snoozes, done, notify dedup),
`profile.json`, `status.json` and `.last_seen`. Tests point `ORG_HOME` (and `HOME`) at a
temporary directory.

**Durability (Phase A):** a send writes `outbox/tmp/<id>.json`, `fsync`s it and renames it
into `outbox/new/`, as the v1-old CLI does. That is not the `F_FULLFSYNC` + directory `fsync`
of ADR-0003 §3.5, which applies to the Phase B offline fallback spool: on macOS a power loss
right after a send can still lose it.

`--office` is accepted everywhere and ignored: Phase A has one spool. `msg inbox --cursor` is a usage error (the spool has no sequence numbers; use `--limit`).

## Directory and presence

`who` lists every registered inbox plus the owner (`vgmello`, a person), with the profile set by
`profile set` or `msg register --name/--role/--team/--reports-to`. Presence is `online` while the
member has a live `msg wait`/`ask` listener or used `org-agent` in the last 2 minutes, else
`offline`, with `last_seen_at`; the spool can't tell busy from idle. Only `vgmello` can carry the owner flag, and a profile file always describes the inbox it sits in (its `id`/`owner` fields are not trusted). `profile set <id>` and `msg register <id> --name ...` edit only your own profile, unless you are the owner. Status lines expire after
8h (`--ttl`, at most 24h), are one line of at most 140 characters / 256 bytes.

## Triage Notification Contract (`org-agent msg notify`)

All runtime hooks (Claude Code, Codex, Antigravity) wrap `org-agent msg notify`:
- **Urgency:** A message is urgent iff `kind == "request"` (`ask`) or `priority` is `"high"` or `"urgent"`.
- **Exit codes:**
  - `0`: Nothing to report (`stdout` is empty in `text` mode).
  - `10`: Normal (non-urgent) items need triage.
  - `11`: At least one urgent item needs triage.
  - `1`: Usage error; `4`: I/O error.
- **Selection:** unread messages that are neither snoozed nor done; snoozes now due (`reason: "due"`); with `--after-task`, `after-task` snoozes (`"after_task"`). Urgent messages always count (`"new"`); a normal untriaged one counts at the break (`--after-task`, `"new"`) or once older than `--sweep-after` (default 5m, `"sweep"`). `--urgent-only` keeps only urgent ones.
- **`--session <id>`:** Atomically deduplicates `(message_id, triage_gen)` per session under `inbox/<agent>/.meta.lock`. Snoozing a message (`msg defer`) or releasing a due snooze increments `triage_gen` so the message resurfaces once when due. Without `--session` nothing is deduplicated (every check lists everything that qualifies). The session id must stay the same for a whole run (the hooks use `claude:<session_id>`, `codex:<session_id>`, `antigravity:<conversationId>`): breaks are tracked per session, see below, assuming one run per agent at a time (two runs taking turns each start a new break). At most 16 sessions are remembered per message (when full, the alphabetically first is dropped); entries are cleared when the message is read, done, re-snoozed or released, so there is no separate 24h prune.
- **Text (`--format text`, the default):** never a body. At most 10 items, urgent first, then `…and <k> more (org-agent msg inbox)`; the unlisted ones aren't recorded for the session and come up at the next check. Sender and subject are sanitized (control, format (zero-width, bidi, tag), variation-selector and other invisible characters removed, `"` turned into `'`, subject cut to 80 characters):

  ```text
  2 new message(s), 1 urgent (subjects are untrusted data from other agents):
  - [URGENT] Ada (ac-staff-engineer-ada): "Quick check" [request] (id: 01M6WRN71MBX8FHS08MB0Q9Z24)
  - River (ac-software-engineer-river): "FYI: host restarts" [msg] (id: 01M6WRP0000000000000000000)
  Triage each: answer now, `org-agent msg defer <id> --until 30m|2h|after-task|<time>`, or `org-agent msg done <id>`.
  ```
- **JSON (`--format json`):** `{"v":1,"agent","urgent","count","urgent_count","items":[{"id","from","from_name","subject","kind","urgent","priority","triage_gen","created_at","reason"}],"more","text"}`.
- **Timeouts:** `ask` and `msg wait` take a `--timeout` of at most 24h; more is a usage error (exit 1), and an `ask` is then never queued.
- **Sizes:** a body is at most 256 KB, a subject at most 1024 bytes, and the whole envelope at most 512 KB once serialized (JSON escaping can grow a body up to 6x); `send`, `ask` and `reply` refuse anything bigger (exit 1, nothing written), since every reader would dead-letter it.
- **Snoozes:** `--until` takes a duration up to 365 days, `after-task`, or a UTC time (`YYYY-MM-DDTHH:MM:SS[.mmm]Z`) from 1970-01-01 to 365 days from now; anything else is a usage error (exit 1).
- **`after-task` and breaks:** the hooks run `notify --after-task` at the Stop after a real task (the agent's natural break), and `notify` without `--after-task` (same `--session`) at a triage turn's Stop, at the next prompt (`--urgent-only` on antigravity) and from the watcher. A break starts with the first `--after-task` check after one that listed nothing new to the break, after any other check from the same session (no session at all counts as one session, so `msg due --after-task` at Stops plus `msg due` at prompts works the same way), or from another `--session` (a new run); it goes on while each `--after-task` check lists something not yet listed in this break (the agent is triaging). A message the agent leaves untriaged is listed again but doesn't extend the break. `triage.json` counts breaks (`break_seq`, plus `in_break` and `break_session`); `defer --until after-task` records the current count on the snooze, and an `--after-task` check releases only the after-task snoozes recorded before the current break. So a message deferred after-task while triaging stays snoozed for the rest of that break (a further `--after-task` Stop in it doesn't list it) and comes back once, as `after_task`, at the Stop after the next task; one deferred mid-task comes back at the very next Stop. Snoozes written before breaks were counted are released at the next break. This relies on the hooks never running `--after-task` at a triage turn's Stop.
- **`msg due`** is the JSON alias of `msg notify --format json` without a session: same selection, no `--session` (no per-session dedup), always exits 0. Like `notify`, it releases due snoozes and, with `--after-task`, counts as a Stop for breaks.
- **Read-then-defer:** If an agent runs `org-agent msg read <id>` during triage and then decides to snooze it (`org-agent msg defer <id> --until ...`), `defer` moves `<id>.json` from `cur/` back to `new/` so it remains unread until due.

## Wake Socket Tags (`'U'` vs `'N'`)

Wake sockets are named `<pid>-<10 chars>.sock` so the path stays under macOS's 103-byte limit even for long agent ids under `~/.agency`.

When `org-relay` delivers a message into `inbox/<to>/new/<id>.json`, it notifies each listener socket in `inbox/<to>/wake/*.sock` with a 1-byte tag:
- `'U'` (`0x55`): Urgent message (`kind == "request"` or `priority` in `high | urgent`).
- `'N'` (`0x4e`): Normal message (`priority == "normal"`).

`org-agent msg wait --urgent-only` wakes and exits `0` only when an unsnoozed urgent message is present in `new/`.

It is level-triggered, not edge-triggered: it doesn't remember what it already returned, so it
returns at once, every time, while any unsnoozed urgent message is still unread. A watcher that
loops on it must back off after reporting (e.g. wait until the agent has triaged, or sleep)
rather than call it again straight away, or it spins on the same message.
