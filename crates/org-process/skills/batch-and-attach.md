# Batching with session attach

Each `agent-os` call opens a connection to the office. When you need several calls in a row, run them through one connection with `agent-os session attach`: it is faster and keeps your turn short.

## How it works

`agent-os session attach` binds to the office once, then reads standard input one line at a time. Each line is one JSON object:

```text
{"command":"task inspect","input":{"task":"T-A1"}}
{"command":"msg inbox","options":{"limit":5,"fields":"id,from_display_name,subject"}}
{"command":"task transition","input":{"task":"T-A1","status":"in_progress"},"options":{"if_match":3}}
```

- `command` is `<resource> <verb>` without `agent-os`. The shorthands `who`, `note` (`task note`, `task-note`), `context`, `resume` and `me` work as on the command line.
- `input` is the command's input, exactly what `--input-json` would take.
- `options` holds the common options by name: `limit`, `cursor`, `fields`, `if_match`, `idempotency_key`, `durable`. A value of `true` gives a yes-or-no option; `false` leaves it off. Answers are always JSON, so leave `format` out.
- A line has only these three keys. A line with another key, a `format` other than `json`, `stdin` or `help` is refused, and nothing is sent for it: standard input is the stream of lines, so give long text in `input` or with `input_file`.

For every line it writes exactly one line to standard output: the same JSON envelope the single command prints, compacted to one line. Answers come in the order of the lines.

## Multi-query search expansion in one batch

When searching for prior tasks, processes or messages across alternate terms or synonyms, send 2 to 3 `search query` calls in one batch instead of separate shells:

```text
{"command":"search query","input":{"q":"auth timeout","types":["task","process"]},"options":{"limit":5,"fields":"ref,title,excerpt"}}
{"command":"search query","input":{"q":"token expiry","types":["task","message"]},"options":{"limit":5,"fields":"ref,title,excerpt"}}
{"command":"search query","input":{"q":"session refresh","types":["private_decision"]},"options":{"limit":5,"fields":"title,excerpt"}}
```

Keep private types (`private_note`, `private_decision`) in their own search line, never mixed with shared types, and inspect `meta.coverage` on each reply.

## Files and attachments

To upload a local file or attach files to tasks and messages, pass local paths via `file upload --path <path>` or `--attach <path>`, or link an existing upload by ID (`--attachment-id <id>`, `--add-attachment <id>`, `--remove-attachment <id>`):

```bash
agent-os file upload --path <path> --scope task --task <task> --title <title> --status final
agent-os file list --scope task --task <task>
agent-os file fetch --id <attachment-id> --output <path>
```

Agents cannot upload with `--scope private`; to send a file to one person, use a 1:1 DM with `agent-os msg send --to @<handle> --body <text> --attach <path>`.

## Running a batch

Write the lines to a file, then feed it in:

```bash
agent-os session attach < <path>
```

Read the answers line by line. Line N of the output answers line N of the input.

## Rules

- A line that fails writes a failure envelope and the batch goes on. Check `ok` on every answer; do not assume a later line saw an earlier change succeed.
- Put a read before a dependent write in the same batch only when the write does not need a value from the read. If it does, run the read first, then build the next batch from its answer.
- `--help` and `--schema` are not available inside a batch; run them as single commands.
- Writes in a batch follow the same rules as single writes: give `if_match` where the record has a revision, and reuse an `idempotency_key` when you retry a line.
- Keep batches short and purposeful. A batch is for a handful of related calls, not for polling: never loop on `agent-os session attach` to wait for something.
