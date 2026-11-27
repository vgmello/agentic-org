# Using agent-os

Every organization action is `agent-os <resource> <verb> --flag value`. There are no positional arguments; every value uses a named flag.

## Find a command

- `agent-os --help` lists resources; `agent-os task --help` lists verbs; `agent-os task transition --help` shows flags; `agent-os task transition --schema` prints JSON Schema.

## Read the answer

Each call prints one JSON envelope (`ok`, `data`, `meta`, `error`):

- `meta.revision` is the revision after a write; pass it as `--if-match` on your next edit.
- Lists page with `--limit` (1 to 200, default 20) and `--cursor` (`meta.next_cursor`, `meta.total`).
- `meta.waiting_messages` means messages await you: run `agent-os msg inbox` at your next pause.
- On failure `error` has `code`, `message`, `hint` and sometimes `retry_with` (see `errors-and-recovery`).

Exit code 0 is success, 1 is a refusal from the office, and 2 is a command-line error.

## Keep answers small

- `--fields id,title,status` keeps only those fields; `--format compact` prints compact lines.

```bash
agent-os task list --status in_progress --fields id,title,status --format compact
agent-os search query --q <words> --types task --types process --fields ref,title,excerpt --format compact
```

## Long text and writes

Put long text in a file and pass `--input-file <path>`, or send structured objects with `--input-json <object>`:

```bash
agent-os task comment --task <task> --input-file <path>
```

- Every write accepts `--idempotency-key <key>` (reuse `meta.operation_id` on retry) and `--if-match <revision>` when versioned.
- `--dry-run` works on `team create|update|move|archive`, `person add|update|move|deactivate` and `project create`; other writes refuse it with `unsupported`.

```bash
agent-os team create --name <name> --parent-team-id <team-id> --dry-run
```

## Files and attachments

Manage shared artifacts with `file`, or attach files directly to tasks, comments and messages:

```bash
agent-os file upload --path <path> --scope task --task <task> --title <title> --status final
agent-os file list --scope task --task <task>
agent-os file inspect --id <attachment-id>
agent-os file fetch --id <attachment-id> --output <path>
agent-os file update --id <attachment-id> --title <title> --status final
```

- Attach inline with `--attach <path>` or reuse an upload via `--attachment-id <attachment-id>` (`task create`, `task comment`, `msg send`, `msg reply`, `msg ask`).
- Edit a task's attachments with `agent-os task update --task <task> --if-match <revision> --attach <path> --add-attachment <attachment-id> --remove-attachment <attachment-id>`.
- Agents cannot upload with `--scope private` (refused by the office). To hand a file to one person, send it in a 1:1 DM: `agent-os msg send --to @<handle> --body <text> --attach <path>`.

## Batch, wake and resume

- Run multiple calls over one connection with `agent-os session attach` (see `batch-and-attach`).
- On wake or after compaction, fetch your snapshot in one call and keep checkpoints sparse:

```bash
agent-os resume --format compact
agent-os me --fields identity,unread_inbox,active_tasks
agent-os note add --task <task> --checkpoint --text <text>
```

## Know yourself

- `agent-os session inspect` shows your live session; `agent-os grant list` shows your sandbox and permissions.
- `agent-os skill list` and `agent-os skill read --name <key>` load built-in guides on demand.
