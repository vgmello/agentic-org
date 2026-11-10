# Agentic Org

**V1 direction changed 2026-11-09:** the target is a standalone Rust server
on macOS/Linux serving the browser application from HQ. Tauri is deferred to v2.
See the [server design](docs/design/standalone-server.md) and [deletion inventory](docs/design/standalone-server-removal-inventory.md).
This documentation change specifies the pivot; the source described below is the
inspected pre-pivot implementation. Its Tauri/bundle commands are historical
development instructions, not installation instructions for the new v1. Do not
claim the new CLI, HTTP server or remote sync exists until implemented. No migration
of the undeployed desktop app or its data is required.

The same proposal includes [employee supervision and native delegates](docs/specs/employee-supervision-ux.md),
[process delivery and standards lookup](docs/specs/processes-ux.md), and
[channel questions and DM follow-up](docs/specs/chat-ux.md). Their runtime behavior
belongs to the offices; the browser accesses it through HQ.

## Proposed v1 layout

The browser application is **org-ui** at `apps/org-ui/`. The Rust server
application, Cargo package and worker executable are **org-server** at
`apps/org-server/`. The host still supervises one worker per office.

```text
apps/
  org-ui/             Svelte 5 + TypeScript, static SvelteKit 2 application
  org-server/         Per-office worker composition and local transport
  org-gateway/        Planned instance browser HTTP/TLS helper
  org-setup/          Planned setup/lifecycle CLI (binary agentic-org)
  org-office-host/    Instance and office-process supervision
  org-agent/          Small employee CLI
crates/               Cohesive libraries; domain modules contain feature slices
docs/
  architecture-rules.md
  design/project-structure.md
```

Follow the [architecture rules](docs/architecture-rules.md) and
[proposed project structure](docs/design/project-structure.md) for domain/feature
organization, shared code ownership, and the source-to-target implementation plan.
These names/layout are proposed implementation work; this PR changes docs only.

## Inspected source layout (pre-pivot)

A macOS desktop app for running an AI organization: its teams, projects and agents (AI employees) and the work they do. The app is a **client**. Each organization runs on one or more **office servers**, the nodes where agents actually work. Agents talk to their office through a tiny CLI.

### Historical layout

```
crates/                         libraries only
  org-proto/                    the contract between app, agent CLI, host and office servers (types)
  org-client/                   connects to the host and office servers (used by the app and org-agent)
  org-office/                   the office's domain and storage (organizations, agents, SQLite, files)
  org-spool/                    an agent's local queue (its role beside the message hub: see the messaging ADR, to come)
apps/                           things you run or ship
  desktop/                      Agentic Org, the app (Tauri 2 + SvelteKit)
  org-office-server/            one office's server (one process per office)
  org-office-host/              the host: the one macOS login service, which runs the office servers (to come)
  org-agent/                    the CLI agents use (messages now; tasks and queues later)
docs/
  architecture-rules.md         how we write code here
  adr/                          decisions for this codebase
  design/                       flow designs (start with create-organization.md)
  specs/                        UX specs (Eden's, moving in)
```

Dependencies point one way, and apps depend on crates, never the reverse:

```
apps/desktop          ─▶ org-client ─▶ org-proto
apps/org-office-server ─▶ org-office ─▶ org-proto
apps/org-agent        ─▶ org-spool
apps/org-agent        ─▶ org-proto   (planned)
```
The office server is also the agents' message hub (ADR-0001, decision 4): `org-agent` will talk to it over the local socket in `org-proto`. A messaging ADR, to be written before that is built, sets the details, including what happens while the server is down.

## Historical development commands

```
cargo check --workspace          # Rust crates and servers
cd apps/desktop && npm i && npm run tauri dev   # the app, once scaffolded
```

## Working here

Read [AGENTS.md](AGENTS.md) before changing anything. The code is MIT-licensed (see [LICENSE](LICENSE)). The app's UI uses licensed Tailwind Plus components *inside the app only*. See AGENTS.md.
