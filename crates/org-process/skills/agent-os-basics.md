# Using agent-os

Every organization action is one command: `agent-os <resource> <verb> --flag value`. There are no positional arguments; every value goes with a named flag.

## Find a command

- `agent-os --help` lists the resources.
- `agent-os task --help` lists the verbs of one resource.
- `agent-os task transition --help` shows one command's flags; `agent-os task transition --schema` prints its input and answer as JSON Schema.

## Read the answer

Each call prints exactly one JSON envelope:

```text
{"ok":true,"data":{...},"meta":{"status":"completed","operation_id":"..."},"error":null}
```

- `ok` says whether the call succeeded. `data` is the record or list the command declares.
- `meta.revision` is the record's revision after a write; pass it as `--if-match` on your next change of that record.
- Every list answers one page: `--limit` sets its size, from 1 to 200 (default 20). `meta.next_cursor` means there is another page: repeat the call with `--cursor <value>`. `meta.total`, when a list reports it, counts every match (`msg inbox` and `msg list` report none).
- `meta.waiting_messages` counts messages waiting for you: check `agent-os msg inbox` at your next pause.
- On failure `error` has `code`, `message`, `hint` and sometimes `retry_with`. See the `errors-and-recovery` guide.

Exit code 0 is success, 1 is a refusal from the office, and 2 is a command line that could not be read.

## Keep answers small

- `--fields id,title,status` keeps only those fields of `data`; a dot reaches into a nested field.
- `--format compact` prints `data` alone as compact lines, which is cheaper to read than the full envelope.

```bash
agent-os task list --status in_progress --fields id,title,status --format compact
```

## Long text

Put long bodies in a file and pass `--input-file <path>`; it fills the command's content field, such as a message body or a task body:

```bash
agent-os task comment --task <task> --input-file <path>
```

`--json <object>` gives the whole input, or the part the flags do not give, as one JSON object. Fields with structure, such as the questions of an ask, can only be given that way.

## Writes

- Every write takes `--idempotency-key <key>`. Without one the command makes a key and reports it in `meta.operation_id`; reuse that key when you retry the same change.
- Writes to a record with a revision take `--if-match <revision>`. Some, such as `agent-os task transition`, require it.
- `--dry-run` is read only on writes; a read refuses it as a command line error. Today the office carries out a dry run only for `agent-os team create`, `agent-os team update`, `agent-os team move`, `agent-os team archive`, `agent-os person add`, `agent-os person update`, `agent-os person move`, `agent-os person deactivate` and `agent-os project create`: it decides and reports, and changes nothing. Every other write, tasks and messages included, refuses `--dry-run` with `unsupported` and changes nothing.

```bash
agent-os team create --name <name> --parent-team-id <team-id> --dry-run
```

## Many calls at once

`agent-os session attach` runs many commands over one connection, one JSON line each. See the `batch-and-attach` guide.

## Know yourself

- `agent-os session inspect` shows your live session.
- `agent-os grant list` shows what you may do and your sandbox rules.
- `agent-os skill list` and `agent-os skill read --name <key>` give these guides.
