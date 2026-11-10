---
kind: spec
title: "Standalone server: installation, instances and browser access (UX)"
status: for-review
revision: 2
owner: ac-product-designer-eden
created: "2026-11-09"
updated: "2026-11-10"
requested_by: owner
reviewers: [ac-staff-engineer-ada, owner, app-frontend-dev, server-dev, ac-software-engineer-river, ac-software-engineer-remy]
builds_on: [offices-and-teams-ux, agent-run-modes-ux, employee-supervision-ux, processes-ux, chat-ux]
---

# Spec · Standalone server: installation, instances and browser access

## 0. Scope and decisions

The owner changed the v1 direction on 2026-11-09: Agentic Org has never been
deployed. Build the new architecture directly and delete obsolete desktop paths.
There is no requirement to migrate the old app, import its data, preserve its IPC
API, or maintain two application transports. This does not authorize installers
or tests to erase existing user directories.

V1 is a standalone Rust service on macOS and Linux. HQ serves the static SvelteKit
application and its HTTP API. Satellites run their own offices and agents and
connect outward to HQ. Opening the website never installs or enrolls an office.
Tauri, the tray, native notifications, global shortcuts and an embedded desktop
window are v2 features. Any future desktop window uses the same web application
and HTTP API; it adds no second implementation of organization behavior.

The implementation proposal is [standalone-server.md](../design/standalone-server.md),
the decision is [ADR-0005](../adr/0005-standalone-server-v1.md), and the
[removal inventory](../design/standalone-server-removal-inventory.md) names the
existing code to delete, retain or replace. CLI spelling below is proposed,
not a claim that these commands exist on the inspected branch.

The owner's naming amendment uses `org-ui` / `apps/org-ui/` for the browser
application and `org-server` / `apps/org-server/` for the Rust worker app, package,
and binary. See [project structure](../design/project-structure.md) for the proposed
folder layout and reorganization guidelines. The setup/release command remains
provisionally `agentic-org`; office roles and the per-office process model remain.
These source renames are implementation acceptance, not changes performed by this PR.

## 1. Install once; select an instance

A small bootstrap command selects the platform release, downloads and verifies
one release executable, then hands off to that executable's setup command.
The executable contains the server payload, frontend assets and Agentic Org
helpers; setup may unpack them locally. Users do not build the application or
install a JavaScript server to run it.

| Invocation | New instance | Existing instance |
| --- | --- | --- |
| `agentic-org setup` | Configure `default`, then start it | Reuse `default`; start if stopped; otherwise show status and URL |
| `agentic-org setup --name lab` | Configure and start `lab` | Reuse `lab` with the same identity, configuration and data |
| Either, with `--non-interactive --config <file>` | Validate explicit inputs and perform the same setup | Reconcile the same inputs; incompatible changes fail with an actionable error |

The bootstrap forwards the instance selector and automation options. Running it
twice has the same instance semantics as running setup twice. It does not silently
upgrade, restart a running service, rotate credentials, change office roles or
enroll another satellite. Upgrades are explicit. Names are local selectors, not
office identities. A name resolves to an immutable instance ID and saved paths;
new IDs are generated once and persisted before provisioning.

Setup checks name validity, existing state, conflicting ports and data ownership.
Names cannot contain path separators or traversal. A second process using the
same data directory is refused before storage opens. An installation interrupted
halfway resumes its recorded operation; ambiguous state is reported for repair,
never silently replaced. Setup uses a lock too, so concurrent installations
cannot create competing identities.

## 2. Wizard and automation

For a new instance the wizard establishes, in order:

1. Instance name and data location, using platform defaults unless explicitly set.
2. **Create an organization (HQ)** or **Join an organization (satellite)** for its
   first office. An instance may later host more than one office. Each office's
   role is persisted independently.
3. HQ owner access, listening addresses and browser TLS configuration; or the
   satellite invitation, destination identity and enrollment confirmation.
4. Runtime selection, dependency checks and authentication readiness.
5. Service registration, start and a summary with separate readiness states.

Automation uses the same validation and state transitions. Missing mandatory
input causes a structured failure instead of waiting for stdin. Secrets enter
through a protected descriptor/file or a dedicated stdin option, not arguments
or ordinary configuration fields. Machine-readable output contains stable error
codes and status fields, never credentials. An unattended enrollment can remain
`awaiting_approval`; setup must distinguish that from an active satellite. Owner
approval is still required by the enrollment policy, even without a wizard.

The default service runs as its configured user, not root. macOS uses a per-user
launchd service; Linux uses an appropriate systemd user service, with foreground
execution available when a user service manager is absent. Logout/reboot behavior
and credential availability are reported honestly. Running after logout on Linux
may require explicitly configured lingering; setup does not silently enable it.
Before-login macOS execution is outside this v1 default.

## 3. Files, identity and tools

The instance owns its configuration, office folders, encrypted databases, runtime
state and agent homes. Executables live separately from mutable data. Each agent
has a stable identity and server-provisioned configuration and workspaces. Local
paths are resolved on the office machine; no absolute path from another machine
becomes an agent's identity or a browser-selected storage root.

The web UI may display an authorized user's server paths read-only. There is no
folder picker, directory relocation setting, old-team scanner or automatic file
upload in v1. The server writes its own data and agents write only their permitted
workspaces. Instance credentials and another employee's private files are not
part of an agent's writable configuration. The [employee supervision requirements](employee-supervision-ux.md),
including bounded delegates and process-driven behavior, apply at the owning office;
moving access to HTTP does not turn those into browser responsibilities.

Setup installs `org-agent` from the same release. It locates or installs only the
selected supported harnesses, checks their versions and reports their sign-in
state. Git is a prerequisite for coding workflows. GitHub CLI is conditional on
the GitHub feature, which remains v2 in the existing project design. The old
`org-relay` alias is not required for a fresh install. Server readiness and runtime
readiness are distinct: the owner can use HQ to finish setup while agents remain
in **Needs you** with a specific reason.

## 4. HQ browser access and satellites

The browser authenticates to HQ and receives only authorized organization data.
No signed desktop identity or office certificate substitutes for a human session.
First setup must establish owner access before administration is available.
Remote browser access uses HTTPS; local access binds to loopback by default.
Satellite enrollment/replication uses its separately authenticated connection.

HQ displays its durable copies of organization, task and message data. Runtime
summaries include last-seen information. Live session inspection and control are
relayed to the owning office and depend on its availability. The UI never reports
an offline office's control action as completed. Durable operations distinguish
**Queued**, **Accepted** and **Completed** where those states apply; rejected and
unknown outcomes remain visible. A retry of the same operation cannot duplicate it.

On browser reconnect, reload a consistent snapshot or resume the event cursor.
During the gap show **Updates paused**. A stale cursor triggers an explicit refresh.
Only a durable server response warrants **Saved**. Unsaved browser text may remain
in the open tab, with an honest unsaved label; v1 provides no background browser
outbox or guarantee that closing the tab preserves drafts.

Satellite enrollment reuses the invitation, HQ identity verification, owner
approval and certificate model in the offices design. Restart uses the enrolled
identity and resumes streams. A satellite opens no public application listener.
The local agent CLI and local administration remain available on its machine.
HQ approvals work from any authorized browser, regardless of which machine runs
that browser. Neither remote browser access nor a same-user local connection
creates office trust automatically.

If HQ is unavailable, its web application is unavailable. Satellite-local work
continues within existing cached policy and runtime limits; organization commands
and cross-office delivery retain the existing outbox rules. Last-known copies are
not backups. Automatic HQ promotion and a satellite-hosted offline web UI are
outside this pivot.

## 5. Lifecycle, updates and removal

All instance operations accept `--name`, defaulting to `default`: `start`, `stop`,
`status`, `logs`, `doctor`, `upgrade` and `uninstall`. `serve` runs the selected
instance in the foreground. These are local control operations; starting a
stopped daemon cannot require an HTTP response from that daemon.

Status distinguishes **Installed**, **Service running**, **HQ ready**, **Awaiting
approval**, **Synchronizing**, **Office ready** and runtime authentication failures.
It reports the actual browser URL only for an HQ endpoint and the connected HQ
address for a satellite. Logs and diagnostics redact secrets.

Stopping an instance stops admission of new work, drains durable writes and stops
its children within defined deadlines, preserving data. It does not affect another
instance. Closing a browser does not stop anything. Upgrades target the selected
instance and switch a complete matching server/helper/assets release. Another
instance may continue using its existing version. Future upgrades of this new
format still require supported schema handling and recoverable failure; this is
distinct from migrating the undeployed desktop architecture.

Uninstall unregisters only the selected service and preserves its data by default.
Data deletion is a separate explicit, instance-scoped operation; active children
must be stopped first. Shared releases stay while any instance references them.
An office removed from an organization follows the existing revocation/reclaim
rules, which are distinct from uninstalling software on a machine.

## 6. Acceptance criteria

- SRV-1 · A supported macOS or Linux host installs and runs HQ without Tauri or frontend build tools. Its browser loads the bundled application through the server.
- SRV-2 · A first setup without a name creates `default`; a repeated setup reuses its identity, data and port and starts at most one daemon.
- SRV-3 · A named setup creates or selects that name. Two instances have disjoint data, credentials, sockets, services and listening endpoints.
- SRV-4 · Concurrent setup/start operations and aliased paths cannot open the same instance storage twice; conflicting ports produce an actionable error without changing saved addresses.
- SRV-5 · Interactive and noninteractive setup reach the same configuration. Missing inputs fail predictably; pending approval is not reported as enrollment success; secrets do not appear in logs or command arguments.
- SRV-6 · Repeating setup cannot silently upgrade, re-enroll, rotate identity, reset configuration or restart a healthy daemon. Interrupted setup resumes without duplicate offices.
- SRV-7 · HQ setup establishes owner authentication. Unauthenticated or unauthorized requests, including requests from another website to loopback, cannot perform administration.
- SRV-8 · Any authorized browser can use HQ without installing software or becoming a satellite; quitting it leaves offices and agents running.
- SRV-9 · A satellite enrolls through a verified invitation and approval, reconnects using its stored identity and opens no public application listener.
- SRV-10 · Organization changes remain HQ-authoritative; locally owned work commits locally and synchronizes with deduplication and resumable positions.
- SRV-11 · HQ shows last-known data and freshness accurately. Live control of an unreachable satellite is unavailable or explicitly queued by that operation's contract, never falsely completed.
- SRV-12 · Browser reconnection recovers missed updates without duplicates or a snapshot/event gap; an expired cursor refreshes and a lost mutation response can be resolved by operation ID.
- SRV-13 · Instance and agent paths are server-managed and shown read-only where authorized. No browser path picker, desktop import or storage relocation flow ships in v1.
- SRV-14 · Setup installs a matching `org-agent`, detects required coding tools and selected harnesses, and distinguishes daemon readiness from agent authentication/readiness.
- SRV-15 · `start`, `stop`, `status`, `logs`, `doctor`, `upgrade` and `uninstall` operate on the selected instance. Service manager or credential-unlock limitations are surfaced explicitly.
- SRV-16 · Upgrading or uninstalling one instance cannot replace a running peer's helpers, remove a referenced release, delete peer data or sweep peer credentials.
- SRV-17 · Shutdown drains acknowledged writes and reaps owned descendants within documented deadlines. Restart reconciles recorded work; browser disconnect does not cancel durable work.
- SRV-18 · Production browser paths use the HTTP API, with an explicit unavailable state for missing features. Missing Tauri globals or API failure never select sample data.
- SRV-19 · The v1 build, runtime and acceptance suite require no Tauri packages, desktop command bridge, tray, native notification or global shortcut.
- SRV-20 · The implementation deletes superseded paths without preserving old desktop contracts or importing undeployed app data. Domain rules, storage durability, office identity and supervision invariants remain covered.
- SRV-21 · A fresh HQ and satellite complete the cross-platform walkthrough in [dogfood-run.md](dogfood-run.md), including disconnect/reconnect and a real agent. Mock-only success is insufficient.

- SRV-22 · The retained browser application's source/package is `apps/org-ui` / `org-ui`; the server application's source/Cargo package/worker executable is `apps/org-server` / `org-server`. Workspace, imports, tests, build, packaging and ownership references agree; the rename preserves the per-office worker and host credential boundaries.

## 7. Decisions still requiring implementation evidence

The server/installer must document the supported OS/architecture matrix, minimum
versions and Linux service prerequisites. Production key custody on headless Linux
and standalone signed macOS must pass the design's platform checks. TLS termination,
version negotiation and bounded command/event handling need concrete contracts.
These are engineering gates for release, not reasons to restore Tauri or an old-app
compatibility layer. Cross-office coding also needs a supported repository transport;
the pivot does not silently bring v2 GitHub integration into v1.
