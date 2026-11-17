# Agentic Org

Agentic Org's selected v1 architecture is a standalone Rust server on macOS/Linux
with a static browser application served by HQ. Offices execute agents and serve
authorized organization data; teams and projects belong to the organization.
Tauri is deferred to v2.

Use the [feature checklist](docs/FEATURES.md) to see every capability's delivery
status, remaining work and link to its full specification. It distinguishes
completed foundations from partial UI work, planned integration and deferred scope.
The [feature area guide](docs/FEATURE-AREAS.md) defines the categories, their scope
and boundaries, and the implementation checks still needed in each area.

The [organization-data contract](docs/design/organization-data/README.md) is the
source of truth for selected relationships, fields, canonical storage, execution
and operational data. Read the [authority map](docs/design/documentation-authority.md)
for the corresponding design/UX guides and their implementation boundaries.

Employees have one home team; projects have an accountable owner team and may
share registered repositories. Tasks, bugs and requests share one typed task
model, with independent ownership and assignment. Portable definitions/work files
live in organization Git. Protected chat, private task notes, usage, budget and
recovery records have separate durable sources; neither a Git clone nor a fresh
office cache is a complete organization restore.

The selected data model is a design, not a deployed migration. The executable
status below identifies available entry points. Automatic scheduling/memory,
process execution, Areas, Undo and retrospectives remain unselected; their earlier
proposals do not become shipped capabilities because a related data record exists.

## Proposed v1 layout

The [organization data documentation](docs/design/organization-data/README.md) defines the selected organization/project/employee data model, including messaging, recovery and budget records. The [documentation authority map](docs/design/documentation-authority.md) distinguishes target contracts from current implementation and deferred engines. Its model-profile and agent-execution document is a linked implementation specification; application migrations are not applied by those SQL drafts.


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

agentic-org serve runs the existing host lifecycle and does not start the browser gateway. **HQ setup/service registration and the production web launcher are unavailable:** F3c (accepted and integrated at `67860707`) implements protected bootstrap/authentication in the host library, and F4 implements the web handoff and session backend for debug/test instance composition only. The production `agentic-org serve` launcher is U1-owned and not built, and signed release acceptance is pending. setup returns an error without creating state; agentic-org-web refuses to open a listener without a verified host handoff. Help/version commands are available. The existing test-only CSP harness remains a test example, not a production bypass. See [executable migration](docs/design/executable-migration.md) for migration scope and checks. This code change does not install services, update running agents or reset data.

Follow the [architecture rules](docs/architecture-rules.md) and
[proposed project structure](docs/design/project-structure.md) for domain/feature
organization, shared code ownership, and the source-to-target implementation plan.
The application/package naming migration and Tauri-shell removal are implemented. Remaining standalone deployment features are listed above. Browser areas without a gateway adapter show unavailable
in both ordinary development and release; test fakes require explicit test composition.
The historical desktop shell commands are retained in Git history, not as instructions for this tree.

Run the UI checks from `apps/agentic-org-ui`:

See [development tooling](DEVELOPMENT.md) for pinned runtimes, security checks,
linting, browser security suites, and Codex skills.

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
