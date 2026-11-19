# `agent-os`

The agents' command line, and the only way an agent reaches the product
([architecture non-negotiables](../../docs/architecture-non-negotiables.md), rule 7).
An agent talks only to its own office ([ADR-0003](../../docs/adr/0003-agent-messaging-office-first.md)).

What it is to become is set by the
[agent command line design](../../docs/design/agent-os-command-line.md) (one grammar,
`agent-os <resource> <verb>` with named flags, one answer envelope, one table of
commands) and the
[implementation plan](../../docs/implementation-plan.md): every call goes to the office,
with a long-running mode for single-digit-millisecond calls (B6), task commands (B5),
and the binding of an agent's run when the office starts it (C3). This page describes
what the code does today.

## Build

```sh
cargo build -p agent-os
./target/debug/agent-os --help
```

## What it does today

`agent-os --help` prints the full usage. The commands are:

- Identity and directory: `whoami`, `who`, `status`, `profile set`.
- Messages: `msg register | deregister | send | ask | reply | defer | done | notify | due | inbox | read | wait`.
- `relay run | status`, which delivers the file queue (also run when invoked as `org-relay`).

The caller is named by `$ORG_AGENT` or `--as`. Nothing authenticates that name in the
file-queue path: any caller can act as any member. Authentication comes with the run
binding for agents (C3). People then use `agentic-org` after signing in (A5), which
offers the same commands from the same library, and `agent-os` refuses to run without
a run binding.

### Two routes

- **Through an office.** `msg send`, `msg inbox`, `msg read`, `msg wait` and `who` go
  to an office when one is named with `--office <id>` or `$ORG_OFFICE`. The client
  checks that the office exists under the office home (`$ORG_OFFICE_HOME`, else the
  default under `$HOME`), connects to its socket, binds
  as the agent and makes the call (`src/office.rs`). With `$ORG_SESSION_TOKEN` set
  (an agent run by the office), it never falls back to the file queue. Only `who`
  answers there today: the office does not serve messaging until increment B6 rebuilds
  it on the office store, so the four `msg` commands are refused with the office's
  `unsupported` error (exit 4, nothing on standard output, no fallback to the file
  queue).
- **A file queue.** Every other command, and those five when no office is named, read
  and write a file queue under `$ORG_HOME/spool` (default `~/.agency/spool`), through
  `crates/org-spool`. It hard-codes an owner id and delivers through the relay.

The file queue, `crates/org-spool` and the relay are deleted by increment B6, which
also moves every command to the office. Do not build on them.

## Tests

```sh
cargo test -p agent-os
```

`tests/office.rs` runs the office route against a real office worker in temporary
homes. Tests never use a real organization or a real home directory.
