# `agent-os`

Thin composition root for the Agentic Org agent CLI (`org-agent-cli` + `org-command` + `org-client`).

- Every command uses the shared `org-command` grammar (`agent-os <resource> <verb> [--flag <value>]...`) and returns one structured envelope (`ok`, `data`, `meta`, `error`).
- `--help`, `--schema`, and `--version` run without a session binding.
- Every office command requires an office-injected session binding (`AGENT_OS_SOCKET` and `AGENT_OS_SESSION_TOKEN`); there is no file-spool or relay fallback.

## Build & Test

```sh
cargo test -p org-agent-cli -p agent-os
```
