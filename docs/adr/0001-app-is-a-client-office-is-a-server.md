# ADR-0001 · The app is a client; the office server holds the organization

Status: **accepted** 2026-11-08 (the user). Owner: ada. This starts the new `v1`. It carries forward the agreed parts of the earlier design (kept on branch `v1-old`) and replaces the parts that conflict.

## The model
- **An organization** owns its teams, projects, agents (AI employees), roles and org chart. Projects belong to a team in the organization, not to an office.
- **An office** is a node: where work actually runs. An agent is placed in an office to execute. The office holds the runtime side (sessions, live status, local queues, work in progress) and serves the organization's data to clients.
- **The app (Agentic Org)** is a client. Its first screen is *Open organization* or *Create organization*. It connects to one or more office servers, lives in the menu bar (and can hide from the Dock), and lists the servers it knows.

## Decisions
1. **`org-office-server` is a daemon registered as a macOS login service** (per-user, through SMAppService). The app ships the server inside its bundle and registers it (confirmed by the user 2026-11-08). It keeps running when the app quits. This replaces the earlier "daemon stops with the app" decision. It runs as the user, never as root, and listens on a local Unix socket (mode 0600, local only). Remote access comes later, with authentication.
2. **Storage: SQLite, owned by the server alone (single writer, WAL).** Agents and the app never open the database: the app goes through `org-client`, and agents go through `org-agent`, which talks to the server (decision 4). SQLite first holds the office's registry of organizations and, once something indexes, an index of the workspace files; the files stay the source of truth for what agents read and write. Operational records (messages, tasks, queues, sessions) may move into the database later, in a separate decision. The engine, encryption and write model are set by [ADR-0002](0002-storage-sqlite-sqlcipher-one-writer.md).
3. **The contract is `org-proto`:** versioned, additive-only, reviewed by ada. *(Amended 2026-11-08:)* for phase 1 the app keeps a hand-written TypeScript mirror in one file, `apps/desktop/src/lib/proto.ts`, owned by the app agent and kept in step with every `org-proto` change. Generating it from the Rust types comes later. The app's WebView never talks to the server: Rust in the Tauri shell owns the socket, through `org-client`, and the UI calls Tauri commands.
4. **The agent CLI (`org-agent`) stays tiny.** ~~It writes to the local spool (`org-spool`), and the server ingests. It carries no server or network code.~~ *(Amended 2026-11-08, the user's decision:)* the office server is also the message hub. `org-agent` sends and receives through the office server's local socket, speaking `org-proto`, instead of going through a separate relay process, so replies reach a waiting `ask` directly. It stays tiny; its dependencies are settled in the messaging ADR. That separate messaging ADR, written before this is built, settles the rest, including what happens while the server is down and any role for `org-spool`.
5. **Repository layout:** `crates/` holds libraries only, and `apps/` holds the things we run or ship. Apps depend on crates, never the reverse.

## Out of scope for now
- Multi-office sync, cross-office addressing (`agent@office`), teams UI and the org chart all come after create-organization works end to end.
