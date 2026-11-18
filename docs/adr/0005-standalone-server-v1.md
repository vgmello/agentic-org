# ADR-0005 · Standalone server, HQ web application and named instances

Status: accepted. Builds on [ADR-0001](0001-app-is-a-client-office-is-a-server.md)
and [ADR-0002](0002-storage-sqlite-sqlcipher-one-writer.md). Platforms are macOS
and Linux, including a container ([ADR-0017](0017-linux-and-container-hosting.md)).

## Context

The owner wants a service installable from one command, operating as HQ or
satellite offices, with the application served by HQ over HTTP. People use a
browser; there is no native application.

## Decisions

1. **V1 ships the server and the browser application.** HQ serves static SvelteKit
   assets, authenticated HTTP operations and live events. Opening a browser does
   not create an office.
2. **One bootstrap command downloads `agentic-org` and invokes setup.** Setup
   creates the instance only: it installs verified components, registers and starts
   the service, and prints a one-time registration link for the instance's
   administrator. It creates no account, organization or office; the administrator
   creates organizations in the browser, and each new organization's HQ office is
   created with it. Satellite onboarding is a separate HQ-initiated UI or command flow.
   Online downloads and offline bundles use the same release verifier. Each office
   keeps its own child process and credential boundary; one downloaded artifact
   does not mean one process.
3. **Instances are explicit.** No name selects `default`; `--name` selects a
   stable existing instance or creates that named instance. Repeated install or
   setup reuses saved state and starts at most one service. Updates, resets and
   deletion are explicit. Data, endpoints, service names and credentials are
   instance-scoped.
4. **Roles belong to offices.** The same release serves HQ and satellite roles.
   An instance may host several offices. HQ is the organization authority and the
   browser entry point. Satellites execute locally, connect outward, enroll once
   and resume authenticated synchronization on later starts.
5. **The server owns filesystem layout and source lifetimes.** Setup selects the
   data root and prepares durable local state. Creating an organization registers
   its required remote organization repository and state branch; on an offline
   instance it validates local configuration only and leaves remote access and
   publication readiness unverified, with no external fetch or credential probe. An agent has a
   required home team; a runnable agent also needs execution configuration and
   validated local connection bindings. Runtime caches and worktrees use the
   office/team/employee/task layout of organization-data document 04. Protected
   chat, accounting, private notes, budgets and authority journals are not
   disposable caches or shared Git content. Browser paths are read-only; there is
   no folder picker or arbitrary relocation flow. Cache deletion requires
   recoverable sources and process reconciliation. Uninstall preserves data by
   default and never removes another instance's credentials or only acknowledged
   source.
6. **Human, service and agent identities are distinct.** Browser sessions carry
   human authority. Office certificates do not confer human administration. Local
   agent IPC stays; HTTP is the user interface's only application transport, not
   a mandate to replace internal connections.
7. **Four executables.** `agentic-org` is the person's command line and, as
   `serve`, the host: it owns accounts, sessions and secrets and starts the
   offices; host behaviour lives in `crates/org-host`. `agentic-org-web` serves the
   browser. `agentic-org-office` is the office worker; each office contains its
   agent supervisor, router and session runner. `agent-os` is the agent's command
   line. No extra host, updater or agent-supervisor daemon exists; the OS service
   manager restarts the host. The browser application lives in
   `apps/agentic-org-ui`. The web process and the agent CLI never hold office keys.
8. **`agentic-org setup` is the single initial setup command,** including the
   administrator's registration link and service registration and start, and no
   organization. There is no satellite choice or join flag at setup. Satellites are added later from
   HQ's UI or an HQ-issued command. Offline setup uses
   `setup --bundle <path> --offline`. A repeated setup resumes or reuses existing
   state; it does not convert office roles or upgrade implicitly. The
   [standalone design](../design/standalone-server.md) and the
   [installation UX](../specs/server-installation-and-access-ux.md) define the
   exact behaviour. [Executable migration](../design/executable-migration.md)
   records which entry points exist today.
9. **Code is organized** as domain modules containing vertical slices, as specified
   in [project structure](../design/project-structure.md) and the
   [architecture rules](../architecture-rules.md).

Entity, storage and recovery contracts are owned by
[organization-data](../design/organization-data/README.md). The
[creation flow](../design/create-organization.md) provisions the required
organization and repository fields and a valid home team before admitting
employees; a built-in HR agent is not exempt from the employee constraints.

## Rules that follow

- `agentic-org-web` terminates browser HTTP and TLS separately from the host and
  the office workers, and holds no office key. Creating an organization requires
  instance-administrator authority; owning an organization is not enough.
- Browser authority requires a session cookie plus an independently generated,
  origin-scoped proof on every protected operation, stream and refresh; a cookie
  alone is not enough, because browsers do not isolate cookies by port. Browser
  tabs share one authenticated streaming fetch or use bounded short polling.
  Sessions are stored at the host ([ADR-0013](0013-stored-browser-sessions.md)).
- The local `org-proto` API, HTTP `/api/v1`, the launch contract and the sync
  contract are independent contracts that change in place until v1 ships. Managed
  launch paths come only from instance, office and member identity.
- Pause commits durably at HQ while a disconnected office's stop is pending; an
  already-forwarded uncertain outcome stays Pending confirmation by operation ID.
- A lead's approval precedes provisioning a hire ([ADR-0022](0022-a-lead-approves-a-hire.md)).
- Creation includes CA escrow, recovery-code provisioning and protected delivery
  ([ADR-0007](0007-hq-creation-escrow.md), [ADR-0011](0011-protected-recovery-delivery.md)).
  Manual recovery after losing HQ is REC-1 in the
  [installation UX](../specs/server-installation-and-access-ux.md).

## Consequences

Standalone service registration and the host's credential custody need an
implementation on each platform. Remote enrollment and sync are in the v1 scope.
When HQ is unavailable its UI is unavailable, while satellite-local work
continues. Named instances share installed release payloads, with a per-instance
version pin. Repository transport for cross-office coding is a separate
dependency.

The [UX requirements](../specs/server-installation-and-access-ux.md) and the
[implementation design](../design/standalone-server.md) define the scope and evidence.

## Not in v1

A native application, tray management, global shortcuts and native notifications.
Automatic HQ promotion.
