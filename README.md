# agentic.org

A macOS desktop app for running an AI organization: its teams, projects and agents (AI employees) and the work they do. The app is a **client**. Each organization runs on one or more **office servers**, the nodes where agents actually work. Agents talk to their office through a tiny CLI.

## Layout

```
crates/                         libraries only
  org-proto/                    the contract between app, agent CLI and office server (types)
  org-client/                   connects to an office server (used by the app)
  org-office/                   the office's domain and storage (organizations, agents, SQLite, files)
  org-spool/                    an agent's fast local queue, picked up by the office server
apps/                           things you run or ship
  desktop/                      agentic.org, the app (Tauri 2 + SvelteKit)
  org-office-server/            the office daemon (macOS login service)
  org-agent/                    the CLI agents use (messages now; tasks and queues later)
docs/
  architecture-rules.md         how we write code here
  adr/                          decisions for this codebase
  design/                       flow designs (start with create-organization.md)
```

Dependencies point one way, and apps depend on crates, never the reverse:

```
apps/desktop          ─▶ org-client ─▶ org-proto
apps/org-office-server ─▶ org-office ─▶ org-proto
apps/org-agent        ─▶ org-spool
```
At runtime (not a code dependency), the server reads what `org-agent` writes into the spool. It gains a dependency on `org-spool` when it starts ingesting.

## Build

```
cargo check --workspace          # Rust crates and servers
cd apps/desktop && npm i && npm run tauri dev   # the app, once scaffolded
```

## Working here

Read [AGENTS.md](AGENTS.md) before changing anything. The code is MIT-licensed (see [LICENSE](LICENSE)). The app's UI uses licensed Tailwind Plus components *inside the app only*. See AGENTS.md.
