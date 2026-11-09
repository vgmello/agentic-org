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
