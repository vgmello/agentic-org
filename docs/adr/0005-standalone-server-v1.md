# ADR-0005 · Standalone server, HQ web application and named instances

Status: **product direction accepted by the owner** 2026-11-09;
**implementation proposal for review**. Owner: ada.
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
2. **One bootstrap command downloads one release executable.** That executable
   owns setup, automation and lifecycle management, and can unpack trusted server
   and CLI helpers from its payload. Preserve per-office child processes and
   credential boundaries; one downloaded artifact does not require one process.
3. **Instances are explicit.** No name selects `default`; `--name` selects a
   stable existing instance or creates that named instance. Repeated install/setup
   reuses saved state and starts at most one service. Updates, resets and deletion
   are explicit. Data, endpoints, service names and credentials are instance-scoped.
4. **Roles belong to offices.** The same release serves HQ and satellite roles.
   An instance may host several offices. HQ remains the organization authority and
   the browser entry point. Satellites execute locally, connect outward, enroll
   once and resume authenticated synchronization on later starts.
5. **The server owns filesystem layout.** Agent identity is stable; setup chooses
   local data roots and the server provisions homes/configuration. Browser path
   display is read-only. There is no v1 desktop import, folder picker or workspace
   relocation flow. Binaries and persistent data have separate lifecycles.
6. **Human, service and agent identities remain distinct.** Browser sessions
   replace signed-desktop privileges. Office certificates do not confer human
   administration. Local agent IPC may remain; HTTP is the user interface's single
   application transport, not a mandate to replace all internal connections.
7. **Delete superseded code directly.** No dual desktop/web implementation, legacy
   IPC adapter or old-app data importer is required. Retain domain logic, useful
   tests and security invariants. Installers/tests never erase user data merely
   because this is an undeployed product.

## Component names and organization (owner amendment 2026-11-10)

Rename the retained browser application from `apps/desktop/` to `apps/org-ui/`
and use `org-ui` as its frontend package name. Delete the Tauri-only shell under
the existing removal inventory; retain the actual browser application and licensed
UI within its renamed product directory.

Rename `apps/org-office-server/` to `apps/org-server/`, its Cargo package and
worker executable to `org-server`, and affected Rust library imports to
`org_server`. Update workspace/lockfiles, test helpers, build/release/signing
references, host helper resolution, and ownership documentation with the source
rename. No legacy desktop naming alias is required for this undeployed product.

The setup/release executable remains provisionally `agentic-org`. The host,
private worker launch, per-office processes, key custody, and HQ/satellite roles
retain their boundaries. Names do not merge these processes or rename all
office-related domain concepts.

Use domain modules containing vertical slices inside meaningful package
boundaries, as specified in [project structure](../design/project-structure.md)
and the language-agnostic [architecture rules](../architecture-rules.md).
The layout and renames are implementation requirements; this PR changes docs only.
Baseline-source references retain their actual pre-rename paths.

## Review clarifications (2026-11-10)

- The planned instance-scoped `org-gateway` process terminates browser HTTP/TLS
  separately from the key-custody host and per-office workers. The gateway holds no
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
- Use Cargo package `org-setup` for the new `agentic-org` CLI and retire the old
  same-named Tauri workspace member before building/staging that binary.

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
