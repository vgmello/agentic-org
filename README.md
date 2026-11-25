# Agentic Org

Agentic Org is a self-hosted server that runs an organization of people and AI agents:
teams, projects, tasks, chat and spend. It runs on macOS and Linux, including in a
container. People use a browser; agents use the `agent-os` command line and run
headless, started and supervised by their office.

**New here? Read `backlog/docs/doc-2 - Start-Here.md`, then `backlog/docs/doc-5 - v1-Implementation-Plan.md`** (in the parent `agentic-org-project` repository, which holds `backlog/` tasks, designs, specs, and ADRs).

- `backlog/docs/doc-3 - Feature-Checklist.md`: every capability, its status and its requirement. `backlog/docs/doc-4 - Feature-Areas.md` defines the categories.
- `backlog/docs/organization-data/doc-72 - Organization-Data-—-README.md`: records, fields, identities and storage. It wins over any other document on those topics.
- `backlog/docs/design/doc-62 - Design-Documentation-Authority-Map.md`: which document owns which topic.
- `backlog/docs/doc-7 - Architecture-Non-Negotiables-and-Who-Decides-What.md`: what never varies, and what you decide yourself.

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

## Command-line reference

See [docs/cli-reference.md](docs/cli-reference.md) for the complete `agentic-org` operator CLI reference (instance setup with HQ mTLS federation, `org create|list`, remote satellite `office invite|join|approve|sync-status`, projects, repos, and tasks), or run `agentic-org help <command>`.

## What runs today

`backlog/docs/design/doc-63 - Design-Executables-and-Entry-Points.md` and `backlog/docs/doc-3 - Feature-Checklist.md` (in parent `agentic-org-project`) track the current status of each executable and v1 feature.

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

The local HQ runbook (`backlog/docs/runbooks/doc-26 - Runbook-Development-Host-on-Local-Machine.md` in parent `agentic-org-project`) shows how to build the executables
and run a disposable development host.
