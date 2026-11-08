# ADR-0001 · The app is a client; the office server holds the organization

Status: **accepted** 2026-11-08 (the user). Owner: ada. This starts the new `v1`. It carries forward the agreed parts of the earlier design (kept on branch `v1-old`) and replaces the parts that conflict.

## The model
- **An organization** owns its teams, projects, agents (AI employees), roles and org chart. Projects belong to a team in the organization, not to an office.
- **An office** is a node: where work actually runs. An agent is placed in an office to execute. The office holds the runtime side (sessions, live status, local queues, work in progress) and serves the organization's data to clients.
- **The app (agentic.org)** is a client. Its first screen is *Open organization* or *Create organization*. It connects to one or more office servers, lives in the menu bar (and can hide from the Dock), and lists the servers it knows.

## Decisions
1. **`org-office-server` is a daemon registered as a macOS login service** (per-user, through SMAppService). It keeps running when the app quits. This replaces the earlier "daemon stops with the app" decision. It runs as the user, never as root, and listens on a local Unix socket (mode 0600, local only). Remote access comes later, with authentication.
2. **Storage: SQLite, owned by the server alone (single writer, WAL).** Agents and the app never open the database: the app goes through `org-client`, and agents go through `org-agent` and the spool. For now SQLite indexes the workspace files, which stay the source of truth for what agents read and write. Operational records (messages, tasks, queues, sessions) may move into the database later, in a separate decision.
3. **The contract is `org-proto`:** versioned, additive-only, reviewed by ada. TypeScript for the app is generated from it.
4. **The agent CLI (`org-agent`) stays tiny.** It writes to the local spool (`org-spool`), and the server ingests. It carries no server or network code. This keeps the design proven on `v1-old` and its measured ~3 ms sends.
5. **Repository layout:** `crates/` holds libraries only, and `apps/` holds the things we run or ship. Apps depend on crates, never the reverse.

## Out of scope for now
- Multi-office sync, cross-office addressing (`agent@office`), teams UI and the org chart all come after create-organization works end to end.
