# `org-agent`: The Agents' CLI

Fast, zero-runtime CLI for agent-to-agent messaging, directory lookup (`who`), self-reported status (`status`), and message triage (`defer`, `done`, `notify`, `due`) over `crates/org-spool`.

## Install

One binary is both CLIs: install `org-agent` and create an `org-relay` symlink to it, which
replaces v1-old's separate `org-relay` (same commands, output, `org-relay:` error prefix and
exit codes: `--help` works without a spool, a usage error exits 1, any spool error exits 4):

```bash
cargo build --release -p org-agent
install -m 0755 target/release/org-agent ~/.local/bin/org-agent
ln -sf org-agent ~/.local/bin/org-relay
```

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
# relay.lock is never followed through a symlink: anything but a regular file there is an
# error (exit 4) naming it, and is left alone.
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

`--office <id>` is accepted and ignored (Phase A has one spool) by `whoami`, `who`, `status`, and `msg send`, `ask`, `reply`, `read`, `inbox`, `defer`, `done`, `notify`, `due` and `wait`; `profile set` and `msg register` take it as the profile's office (`--office <id[:name]>`), and `msg deregister` and `relay` don't accept it. `msg inbox --cursor` is a usage error (the spool has no sequence numbers; use `--limit`).

## Directory and presence

`who` lists every registered inbox plus the owner (`vgmello`, a person), with the profile set by
`profile set` or `msg register --name/--role/--team/--reports-to`. Presence is `online` while the
member has a live `msg wait`/`ask` listener or used `org-agent` in the last 2 minutes, else
`offline`, with `last_seen_at`; the spool can't tell busy from idle. Only `vgmello` can carry the owner flag, and a profile file always describes the inbox it sits in (its `id`/`owner` fields are not trusted). `profile set <id>` and `msg register <id> --name ...` edit only your own profile, unless you are the owner. Status lines expire after
8h (`--ttl`, at most 24h), are one line of at most 140 characters / 256 bytes. Status and profile text may not contain control, format (zero-width, bidi, tag) or other invisible characters (usage error), except variation selectors, which are dropped (`status "⚠️ blocked"` stores `⚠ blocked`). A profile name may not contain `(`, `)`, `[`, `]`, `:` or `"` (they could forge a `msg notify` line); in notify output, names written before this rule have those characters turned into spaces (the line format is unchanged).

## Triage Notification Contract (`org-agent msg notify`)

All runtime hooks (Claude Code, Codex, Antigravity) wrap `org-agent msg notify`:
- **Urgency:** A message is urgent iff `kind == "request"` (`ask`) or `priority` is `"high"` or `"urgent"`, except that an `ask` to an agent past its `expires_at` (the asker stopped waiting) is not: `notify`, `due`, `inbox` and `wait --urgent-only` treat it as a normal message (`urgent: false`, `priority: "normal"`). An expired `ask` to a person or the owner (the recipient's profile kind is `person`, or it is the owner) stays urgent with its own priority, as in `org_proto::msg` (it stays in "Needs you"). A recipient other than the owner with no profile counts as an agent. Either way the text line says `[request] (expired)`, so the reader sees the asker gave up.
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
- **`after-task` and breaks:** the hooks run `notify --after-task` at the Stop after a real task (the agent's natural break; the adapter re-checks with `--after-task` after a short `msg wait` when the first check finds nothing), and `notify` without `--after-task` (same `--session`) at a triage turn's Stop, at the next prompt (`--urgent-only` on antigravity) and from the watcher. An `--after-task` check starts a new break unless one is under way in the same session. A break ends ONLY with a check without `--after-task` from the same session (no session counts as one session), or when an `--after-task` check comes from another `--session` (a new run), which starts a new break; an `--after-task` check, empty or not, never ends one. A session-less check never changes another session's break: while a session's break is under way, `msg due --after-task` checks within it and plain `msg due` leaves it alone. Since a session usually ends inside a break (its last Stop starts one; a crash leaves one open), a caller using only `msg due` then gets no after-task deferral back until that session, or another one, runs a check (a hook could end its break at SessionEnd with a plain `notify --session <id>`). `triage.json` counts breaks (`break_seq`, plus `in_break` and `break_session`); `defer --until after-task` records the current count on the snooze, and an `--after-task` check releases only the after-task snoozes recorded before the current break. So a message deferred after-task while triaging stays snoozed for the rest of that break (however many `--after-task` checks run in it) and comes back once, as `after_task`, at the next `--after-task` check after the break ended (the Stop after the next real task); one deferred mid-task comes back at the very next Stop. Snoozes written before breaks were counted are released at the next break. A runtime with only a Stop hook (no prompt or watcher check) never ends a break, so it holds after-task deferrals until some other check without `--after-task` runs in its session.
- **`msg due`** is the JSON alias of `msg notify --format json` without a session: same selection, no `--session` (no per-session dedup), always exits 0. Like `notify`, it releases due snoozes and, with `--after-task`, starts a break when none is open (see above); while a session's break is open it only checks within it.
- **Reply marks read:** `msg reply <id>` marks the original read (moves it to `cur/`) once the reply is queued, a deliberate change from v1-old (ADR-0003 §2.3: answering is triaging).
- **Read-then-defer:** If an agent runs `org-agent msg read <id>` during triage and then decides to snooze it (`org-agent msg defer <id> --until ...`), `defer` moves `<id>.json` from `cur/` back to `new/` so it remains unread until due.

## Changes from v1-old (deliberate)

Inputs now refused with a usage error (exit 1), nothing written:
- a subject over 1024 bytes, or an envelope over 512 KB once serialized;
- a `--meta priority=...` other than `normal`, `high` or `urgent` (use `--priority`);
- a `--timeout` over 24h (`ask`, `msg wait`), and `--until` beyond 365 days or before 1970;
- extra positional arguments (`read`, `defer`, `done`, `deregister`, `who`, `whoami`), and
  `msg deregister` without `<agent>`.

`msg inbox` text: the subject is sanitized so each message stays one row (control, format and
invisible characters become spaces, `"` becomes `'`) and cut to 200 characters (`…`). With
`--all` each row has two more columns: the status (`unread`, `snoozed`, `done`, `read`) and,
for a snooze, when it's due (a time or `after-task`; `-` otherwise); plain `inbox` keeps
v1-old's five. The same sanitizing applies to the header line of `msg read` and `msg wait`
text output (the body is printed as is). Profile
names refuse `( ) [ ] : "`: a member whose saved name already has one must fix the name
(`--name`) in the same `profile set` as any other change, since the whole profile is checked.
`msg reply` marks the original read (above).

## Known limits

- `msg notify --session` records the listed items as reported (under the lock) before the
  caller has read the output: an adapter that kills `notify` after that save loses those
  items for that session (they stay in the inbox, and other sessions, `msg inbox` and the
  sweep still see them).
- Corrupt files are not cleaned up: a corrupt message in `new/` is moved to `dead/` (with a
  `.reason`), one in `cur/` is skipped and left there, a corrupt `triage.json` is set aside
  as `triage.json.corrupt-<ulid>`, and nothing prunes `dead/` or those copies.

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
