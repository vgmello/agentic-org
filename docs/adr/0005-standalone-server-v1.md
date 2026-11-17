# ADR-0005 · Standalone server, HQ web application and named instances

Status: **product direction accepted by the owner** 2026-11-09;
**implementation proposal for review**. Owner: lead.
Amends ADR-0001's desktop access/packaging, ADR-0002's platform and instance key
scope, and ADR-0003's app-bound human authorization. It does not supersede their
domain ownership, durability or employee permission rules.

## Context

Agentic Org has never been deployed. The owner wants a service installable from
one command on macOS or Linux, operating as HQ or satellite offices, with the
application served by HQ over HTTP. Tauri is deferred to v2. There is no installed
desktop population, old-data migration requirement or compatibility obligation to
the current Tauri command bridge.

The existing office model already separates HQ authority, local work and outbound
satellite synchronization. The current code has local host/office processes and a
static frontend, but its browser path uses mocks. HTTP serving and remote sync
still require implementation.

## Decisions

1. **V1 ships the server and browser application.** HQ serves static SvelteKit
   assets, authenticated HTTP operations and live events. Opening a browser does
   not create an office. Tauri, tray management and native capture are v2.
2. **One bootstrap command downloads agentic-org and invokes setup.** Initial setup creates HQ only, installs verified components, and registers/starts the service automatically. Satellite onboarding is a separate HQ-initiated UI/command flow. Online downloads and offline bundles use the same release verifier. Preserve per-office child processes and
   credential boundaries; one downloaded artifact does not require one process.
3. **Instances are explicit.** No name selects `default`; `--name` selects a
   stable existing instance or creates that named instance. Repeated install/setup
   reuses saved state and starts at most one service. Updates, resets and deletion
   are explicit. Data, endpoints, service names and credentials are instance-scoped.
4. **Roles belong to offices.** The same release serves HQ and satellite roles.
   An instance may host several offices. HQ remains the organization authority and
   the browser entry point. Satellites execute locally, connect outward, enroll
   once and resume authenticated synchronization on later starts.
5. **The server owns filesystem layout and source lifetimes.** Setup selects
   local roots, registers the required remote organization repository and state
   branch, and prepares durable local state. Offline setup validates local
   configuration only and leaves remote access/publication readiness unverified;
   it performs no external fetch or credential probe. Agent identity has a required home team; runnable
   agents additionally need separate execution configuration and validated local
   connection bindings. Runtime caches/worktrees use the office/team/employee/task
   layout in organization-data document 04. Protected chat, accounting, private
   notes, budgets and authority journals are not disposable caches or shared Git
   content. Browser paths are read-only; there is no folder picker or arbitrary
   relocation flow. Cache deletion requires recoverable sources and process
   reconciliation. Uninstall preserves data by default and never removes a peer
   instance's credentials or only acknowledged source.
6. **Human, service and agent identities remain distinct.** Browser sessions
   replace signed-desktop privileges. Office certificates do not confer human
   administration. Local agent IPC may remain; HTTP is the user interface's single
   application transport, not a mandate to replace all internal connections.
7. **Delete superseded code directly.** No dual desktop/web implementation, legacy
   IPC adapter or old-app data importer is required. Retain domain logic, useful
   tests and security invariants. Installers/tests never erase user data merely
   because this is an undeployed product. Existing live Phase A coordination and
   retained stable IDs still require explicit preservation; the selected data model
   is not a blanket authorization to reset them.

Selected entity, storage and recovery contracts are owned by
[organization-data](../design/organization-data/README.md), with precedence and
implementation boundaries in the [authority map](../design/documentation-authority.md).
The [creation flow](../design/create-organization.md) must provision required
organization/repository fields and valid home teams before admitting employees;
a built-in HR agent is not exempt from the employee constraints.

## Component names and organization (owner amendment 2026-11-10)

Rename the retained browser application from `apps/desktop/` to `apps/agentic-org-ui/`
and use `agentic-org-ui` as its frontend package name. Delete the Tauri-only shell under
the existing removal inventory; retain the actual browser application and licensed
UI within its renamed product directory.

The worker source is apps/agentic-org-office with Cargo package/executable agentic-org-office and Rust library agentic_org_office. Source/package/executable renames and host extraction have landed; workspace/lockfiles, test helpers, helper resolution and ownership references follow these names. No legacy desktop naming alias is required for this undeployed product.

**Owner amendment (2026-11-15).** Executables and matching application directories/packages are agentic-org (human CLI and service host), agentic-org-web (browser server), agentic-org-office (office worker), and agent-os (agent CLI). Host behavior lives in crates/org-host. Reusable domain/contract libraries retain their identities. agentic-org serve composes the existing host lifecycle/key-custody behavior through a library; no extra host/updater/agent-supervisor daemon is required. The OS service manager restarts the host. Each office contains its agent supervisor, router and session runner; the web process and agent CLI retain restricted identities and never gain raw office keys.

agentic-org setup is the single initial HQ setup command, including first administrator/organization and automatic service registration/start. There is no initial satellite selection or setup join flag. Satellites are added later from HQ's UI or an HQ-issued command. Offline HQ setup uses setup --bundle <path> --offline. A repeated setup safely resumes/reuses existing state; it does not convert office roles or upgrade implicitly. Exact setup/release behavior is defined in the standalone design and installation UX spec. The names and serve/agent CLI entry points are implemented; HQ setup/service registration, downloads/updates and production web handoff remain requirements. See [executable migration](../design/executable-migration.md).

Use domain modules containing vertical slices inside meaningful package
boundaries, as specified in [project structure](../design/project-structure.md)
and the language-agnostic [architecture rules](../architecture-rules.md).
Remaining domain-layout changes are implementation requirements. Explicit baseline-source references retain their actual pre-rename paths.

## Review clarifications (2026-11-10)

- The instance-scoped `agentic-org-web` process (production handoff pending) terminates browser HTTP/TLS
  separately from the key-custody host and per-office workers. The web process holds no
  office DB key. Create organization requires instance-admin authority; owning an
  organization alone is insufficient.
- Browser authority requires a session cookie plus an independently generated,
  origin-scoped proof on every protected operation/stream/refresh; cookies alone
  are insufficient because browsers do not isolate them by port. Browser tabs
  share one authenticated streaming fetch or use bounded short polling.
- First standalone local `org-proto` uses API_VERSION 2 for breaking retirements
  and changed authority. Retained versions remain additive-only. HTTP `/api/v1`
  and launch/sync versions are independent. `home`/`clear_home` mutations retire;
  managed launch paths come only from instance/office/member identity.
- Pause durably commits at HQ while a disconnected office's stop remains pending.
  New goal changes to a known-offline office are rejected before admission; an
  already-forwarded uncertain outcome remains Pending confirmation by operation ID.
- Existing live Phase A messaging has a conditional, verified cutover; no old
  desktop application import is required. HR approval still precedes provisioning.
- V1 creation includes CA escrow/recovery-code provisioning and protected delivery.
  REC-1 explicitly remains an open production-release decision for the reviewed
  manual HQ-loss recovery/fencing procedure; automatic promotion remains deferred.
- Use Cargo package `agentic-org` at `apps/agentic-org` for the CLI. The old
  same-named Tauri workspace member is removed; stage the current CLI artifact.

The [review decisions](../design/standalone-v1-review-decisions.md) map every review
item to its clarification and acceptance evidence. These are documentation changes;
their required security/platform/runtime checks have not run.

## Consequences

Standalone service registration and production key custody need implementation on
both operating systems. Existing bundle-specific code cannot simply be bypassed.
Remote enrollment/sync enters the v1 release scope even though previous phases
deferred it. HQ unavailability makes its UI unavailable while satellite-local work
can continue. Global shortcuts/native notifications wait for v2; request capture
inside the browser remains a v1 application capability.

Named instances share installed release payloads safely, with a per-instance
version pin. Future updates of the new architecture need schema/version handling;
the lack of a desktop migration does not eliminate that responsibility. Repository
transport for cross-office coding remains a separate dependency; do not silently
pull the deferred GitHub integration into this pivot.

The [UX requirements](../specs/server-installation-and-access-ux.md),
[implementation design](../design/standalone-server.md) and
[deletion inventory](../design/standalone-server-removal-inventory.md) define the
scope and evidence. Detailed security/platform choices remain review gates;
this ADR records no completed implementation or merge approval.
