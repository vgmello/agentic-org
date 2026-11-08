# `org-agent`: The Agents' CLI

Fast, zero-runtime CLI for agent-to-agent messaging, directory lookup (`who`), self-reported status (`status`), and message triage (`defer`, `done`, `notify`, `due`) over `crates/org-spool`.

## Commands

```bash
export ORG_AGENT=<your-agent-id>

# Identity, Directory, and Status
org-agent whoami [--as <agent>] [--json]
org-agent who [<member>] [--team <team>] [--kind agent|person|owner] [--json]
org-agent status "<text>" [--ttl 8h] [--as <agent>] [--json]
org-agent status --clear [--as <agent>] [--json]
org-agent profile set [<id>] [--name <n>] [--kind agent|person] [--owner] [--role <r>] [--team <id[:name[:title]]>] [--reports-to <id[:name]>] [--office <id[:name]>] [--json]

# Messaging & Triage
org-agent msg register [<agent>] [--name <n>] [--role <r>] [--team <t>] [--reports-to <r>] [--json]
org-agent msg send   --to <agent> [--subject <s>] [--priority normal|high|urgent] [--meta k=v] [--json] <body | ->
org-agent msg ask    --to <agent> [--timeout 120s] [--subject <s>] [--priority high|urgent] [--meta k=v] [--json] <body | ->
org-agent msg reply  <id> [--subject <s>] [--priority normal|high|urgent] [--meta k=v] [--json] [--body <body> | <body | ->]
org-agent msg defer  <id> --until <30m|2h|after-task|ISO> [--note <text>] [--json]
org-agent msg done   <id> [--note <text>] [--json]
org-agent msg notify [--session <id>] [--after-task] [--urgent-only] [--sweep-after 5m] [--format text|json] [--as <agent>]
org-agent msg due    [--after-task] [--urgent-only] [--sweep-after 5m] [--as <agent>] [--json]
org-agent msg inbox  [--all] [--summary] [--from <agent>] [--limit <n>] [--as <agent>] [--json]
org-agent msg read   <id> | --next [--as <agent>] [--json]
org-agent msg wait   [--timeout 60s] [--from <agent>] [--urgent-only] [--as <agent>] [--json]

# Spool Relay Daemon (also dispatched when invoked as `org-relay`)
org-agent relay run [--once]
org-agent relay status [--json]
```

## Triage Notification Contract (`org-agent msg notify`)

All runtime hooks (Claude Code, Codex, Antigravity) wrap `org-agent msg notify`:
- **Urgency:** A message is urgent iff `kind == "request"` (`ask`) or `priority` is `"high"` or `"urgent"`.
- **Exit codes:**
  - `0`: Nothing to report (`stdout` is empty in `text` mode).
  - `10`: Normal (non-urgent) items need triage.
  - `11`: At least one urgent item needs triage.
  - `1`: Usage error; `4`: I/O error.
- **`--session <id>`:** Atomically deduplicates `(message_id, triage_gen)` per session under `inbox/<agent>/.meta.lock`. Snoozing a message (`msg defer`) or releasing a due snooze increments `triage_gen` so the message resurfaces once when due.
- **Read-then-defer:** If an agent runs `org-agent msg read <id>` during triage and then decides to snooze it (`org-agent msg defer <id> --until ...`), `defer` moves `<id>.json` from `cur/` back to `new/` so it remains unread until due.

## Wake Socket Tags (`'U'` vs `'N'`)

When `org-relay` delivers a message into `inbox/<to>/new/<id>.json`, it notifies each listener socket in `inbox/<to>/wake/*.sock` with a 1-byte tag:
- `'U'` (`0x55`): Urgent message (`kind == "request"` or `priority` in `high | urgent`).
- `'N'` (`0x4e`): Normal message (`priority == "normal"`).

`org-agent msg wait --urgent-only` wakes and exits `0` only when an unsnoozed urgent message is present in `new/`.
