# Batching with session attach

Each `agent-os` call opens a connection to the office. When you need several calls in a row, run them through one connection with `agent-os session attach`: it is faster and keeps your turn short.

## How it works

`agent-os session attach` binds to the office once, then reads standard input one line at a time. Each line is one JSON object:

```text
{"command":"task inspect","input":{"task":"T-A1"}}
{"command":"msg inbox","options":{"limit":5,"fields":"id,from_display_name,subject"}}
{"command":"task transition","input":{"task":"T-A1","status":"in_progress"},"options":{"if_match":3}}
```

- `command` is `<resource> <verb>` without `agent-os`. The shorthands `who`, `task note` and `boot` work as on the command line.
- `input` is the command's input, exactly what `--json` would take.
- `options` holds the common options by name: `limit`, `cursor`, `fields`, `if_match`, `idempotency_key`, `durable`. A value of `true` gives a yes-or-no option; `false` leaves it off. Answers are always JSON, so leave `format` out.
- A line has only these three keys. A line with another key, a `format` other than `json`, `stdin` or `help` is refused, and nothing is sent for it: standard input is the stream of lines, so give long text in `input` or with `input_file`.

For every line it writes exactly one line to standard output: the same JSON envelope the single command prints, compacted to one line. Answers come in the order of the lines.

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

## When to use it

- Reading several records you need together, such as a task, its dependencies and your inbox.
- Making a few related changes, such as ticking checklist steps and adding a comment.

For one or two calls, a plain command is simpler.
