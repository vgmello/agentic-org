# ADR-0001 · The app is a client; the office server holds the organization

**Amended 2026-11-09 by the owner's standalone-v1 decision:**
[ADR-0005](0005-standalone-server-v1.md) replaces the desktop packaging, local-app
access and Tauri transport below. V1 is a macOS/Linux server with HQ browser access;
Tauri is v2. This is an undeployed-product pivot with no old-app migration or
compatibility requirement. The office/domain/storage model remains applicable.

Status: **accepted** 2026-11-08 (the user); **amended** the same day for the office model in [`offices-hq-and-satellites.md`](../design/offices-hq-and-satellites.md) (approved by the user). Owner: ada. This starts the new `v1`. It carries forward the agreed parts of the earlier design (kept on branch `v1-old`) and replaces the parts that conflict.

## The model
- **An organization** owns its teams, projects, agents (AI employees), roles and org chart. Projects are worked on by one or more teams (or none yet: unassigned), not owned by an office.
- **An office belongs to one organization** *(amended)*. An organization has offices; **its first office is HQ**, which holds the organization's authoritative data and the list of its offices. Other offices are satellites, on the same Mac or other machines, with a full copy that syncs through HQ. An agent is placed in an office to execute. Each office holds its runtime side (sessions, live status, local queues, work in progress) and serves its organization's data to clients.
- **The application** is a browser client of HQ over authenticated HTTP. Opening it neither installs a local office nor enrolls a satellite. Setup creates or selects a named service instance; each office retains its own role and identity.

## Decisions
1. **One office = one worker process**, with private storage and local agent IPC. An instance host supervises its offices. Standalone setup registers a per-user service on macOS or Linux and installs matching release helpers. The browser is independent of that service lifetime. Remote satellites connect outward to HQ using the office identity and sync model (ADR-0005).
2. **Storage: SQLite, owned by each office server alone (one writer per database, WAL).** Agents and the app never open a database: the browser goes through the HQ HTTP API, and agents go through `org-agent`, which talks to their office (decision 4). *(Amended:)* each office has `org.db` (the organization's data, HQ's original or a satellite's copy, with HQ's event log) and `office.db` (its own data: tasks, messages, outboxes, sessions), sharing one key. Tasks and messages are office-first (offices design). The engine, encryption and write model are set by [ADR-0002](0002-storage-sqlite-sqlcipher-one-writer.md), applied per database.
3. **Contracts stay typed and reviewed by ada.** The HTTP adapter composes domain operations for authenticated browser principals; `org-proto` remains where local agent/office calls need it, and `org-sync-proto` owns office-to-office shapes. TypeScript mirrors must follow the HTTP contract. Obsolete app-only calls may be removed with their callers; this pivot does not preserve the old Tauri IPC contract.
4. **The agent CLI (`org-agent`) stays tiny.** ~~It writes to the local spool (`org-spool`), and the server ingests. It carries no server or network code.~~ *(Amended 2026-11-08, the user's decision:)* the office server is also the message hub. `org-agent` sends and receives through **its own office's** local socket, speaking `org-proto`, instead of going through a separate relay process. Within an office, replies reach a waiting `ask` directly; across offices they go through HQ (offices design). It stays tiny; its dependencies are settled in the messaging ADR. That separate messaging ADR, written before this is built, settles the rest, including what happens while the server is down and any role for `org-spool`.
5. **Repository layout:** `crates/` holds libraries only, and `apps/` holds the things we run or ship. Apps depend on crates, never the reverse.

## Related decisions
- [`offices-hq-and-satellites.md`](../design/offices-hq-and-satellites.md): HQ and satellites, office-first work data, invitations, promotion.
- [`teams-and-org-chart.md`](../design/teams-and-org-chart.md): teams, members, reporting lines, the org chart.
- [`projects-git-github.md`](../design/projects-git-github.md): projects as GitHub repositories, one HQ-managed GitHub App.

## Out of scope for now
- Local and remote satellites are in the new v1 scope. Automatic promotion and the optional Tauri shell remain later work; see ADR-0005 for release gates.
