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

The [organization data documentation](docs/design/organization-data/README.md) defines the proposed durable organization/project/agent data model. Its model-profile and agent-execution document is a linked implementation specification; application migrations are not applied by those SQL drafts.


Application directories, Cargo packages and executable names now match:

```text
apps/
  agentic-org/         Human CLI and serve host composition
  agentic-org-office/  One office worker
  agentic-org-web/     Browser entry point and HTTP library
  agent-os/            Small employee CLI
  agentic-org-ui/      Static browser assets
crates/
  org-host/            Host lifecycle/key-custody library
  ...                  Domain and protocol libraries
```

agentic-org serve runs the existing host lifecycle. **HQ setup/service registration and production web handoff remain unavailable:** their protected bootstrap/authentication integration has not landed in v1. setup returns an error without creating state; agentic-org-web refuses to open a listener without its unfinished production composition. Help/version commands are available. The existing test-only CSP harness remains a test example, not a production bypass. See [executable migration](docs/design/executable-migration.md) for migration scope and checks. This code change does not install services, update running agents or reset data.

Follow the [architecture rules](docs/architecture-rules.md) and
[proposed project structure](docs/design/project-structure.md) for domain/feature
organization, shared code ownership, and the source-to-target implementation plan.
The application/package naming migration and Tauri-shell removal are implemented. Remaining standalone deployment features are listed above. Browser areas without a gateway adapter show unavailable
in both ordinary development and release; test fakes require explicit test composition.
The historical shell commands below no longer run against this tree.

Run the UI checks from `apps/agentic-org-ui`:

```sh
cd apps/agentic-org-ui
bun install --frozen-lockfile
bun run check
bun run test
bun run build
bun run test:e2e
```

## Historical implementation

Superseded desktop layouts and shell commands are available in Git history. They are not current build or installation instructions. Current source ownership is in AGENTS.md; the executable migration document records the remaining production integration gaps.
