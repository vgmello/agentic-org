# Agentic Org

Agentic Org is a self-hosted server that runs an organization of people and AI agents:
teams, projects, tasks, chat and spend. It runs on macOS and Linux, including in a
container. People use a browser; agents use the `agent-os` command line and run
headless, started and supervised by their office.

**New here? Read [docs/START-HERE.md](docs/START-HERE.md), then the
[implementation plan](docs/implementation-plan.md).**

- [Feature checklist](docs/FEATURES.md): every capability, its status and its
  requirement. [Feature areas](docs/FEATURE-AREAS.md) define the categories.
- [Organization data contract](docs/design/organization-data/README.md): records,
  fields, identities and storage. It wins over any other document on those topics.
- [Documentation authority map](docs/design/documentation-authority.md): which
  document owns which topic.
- [Architecture non-negotiables](docs/architecture-non-negotiables.md): what never
  varies, and what you decide yourself.

## Layout

```text
apps/
  agentic-org/         The person's command line; `serve` runs the host
  agentic-org-office/  One office worker
  agentic-org-web/     Serves the browser application and /api/v1
  agent-os/            The agents' command line
  agentic-org-ui/      Static browser application (SvelteKit), served by agentic-org-web
crates/
  org-host/            Host library composed by `agentic-org serve`
  ...                  Domain and protocol libraries
```

## What runs today

Nothing a person can use end to end yet. `agentic-org serve` runs the host;
`agentic-org setup` refuses without changing anything; `agentic-org-web` refuses to
open a listener. [Executable migration](docs/design/executable-migration.md) lists
exactly what each executable does now, and the implementation plan says in which
increment each part becomes usable.

## Build and test

[DEVELOPMENT.md](DEVELOPMENT.md) has the pinned toolchains and every check. In short:

```sh
cargo fmt --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace

cd apps/agentic-org-ui
bun install --frozen-lockfile
bun run check
bun run test
bun run build
```

The [local HQ runbook](docs/runbooks/local-hq.md) shows how to build the executables
and run a disposable development host.
