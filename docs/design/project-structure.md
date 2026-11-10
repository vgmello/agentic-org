# Proposed v1 project structure and architecture profile

Status: **owner-requested implementation proposal**, 2026-11-10. This document
extends PR #5 with application naming, domain modules containing vertical slices,
shared-code ownership, and a concrete reorganization sequence. It is documentation
only: no source directory, package, executable, or runtime behavior is changed here.
Source observations refer to `a4039a0c810ca222d574a44eba5f84b83d07e9a4`.

Read the language-agnostic [architecture rules](../architecture-rules.md) first.
[ADR-0005](../adr/0005-standalone-server-v1.md), the
[standalone design](standalone-server.md), and the
[removal inventory](standalone-server-removal-inventory.md) retain authority over
product scope, process/security boundaries, and deletion acceptance.

## 1. Names and process boundaries

| Inspected source | V1 target | Meaning |
| --- | --- | --- |
| `apps/desktop/` | `apps/org-ui/` | Retained Svelte 5/TypeScript browser application, built with SvelteKit 2's static adapter |
| Frontend package name | `org-ui` | UI build/package identity; product display name remains Agentic Org |
| `apps/org-office-server/` | `apps/org-server/` | Rust server application/composition |
| Cargo package and executable `org-office-server` | `org-server` | One office worker per process, including HQ or satellite role |
| Rust application-library import `org_office_server` | `org_server` | Update affected test harnesses and other authorized callers during implementation |
| New browser HTTP helper | `apps/org-gateway/`, package/binary `org-gateway` (planned) | One instance-scoped browser listener, no office-key custody |
| Setup command implementation | `apps/org-setup/`, package `org-setup`, binary `agentic-org` (planned) | Distinct package ID from the old Tauri shell; remove that workspace member before staging the new same-named binary |

The release/setup executable remains provisionally `agentic-org`, as described in
the standalone design. It is distinct from the renamed `org-server` worker.
`org-office-host`, `org-office`, `org-office-launch`, office identities, and the
HQ/satellite model retain their responsibilities. The server rename does not merge
the host, workers, and agent CLI into one privileged process.

Delete desktop-only Tauri code according to the inventory and move the retained
browser application to `apps/org-ui`. Keep licensed UI inside that application;
update AGENTS.md's path references with the source move without relaxing its
license restrictions. During this docs-only proposal, existing source links and
historical build examples still use their real `apps/desktop` and
`apps/org-office-server` paths. Their presence is not a target-layout exception.

## 2. Architecture to follow

Use **domain modules containing vertical slices**:

`application -> domain module -> feature/use case`

Crates establish useful package and visibility boundaries. Modules establish
cohesive capabilities within those crates. Features implement concrete use cases.
One feature does not imply one crate, service, database, or mandatory stack of
handler/repository/interface files.

| Capability | Rules and ownership | Proposed implementation home |
| --- | --- | --- |
| Organization | Membership, teams, roles, projects, authoritative placement | Organization module inside `org-office` |
| Work | Task/bug transitions, checklists, numbering, idempotency, conflicts | Existing `org-work`, with office-owned implementations of its ports |
| Messaging | Conversations, channels, membership, delivery and authorization rules | Existing `org-mailbox`, with office storage/session implementations |
| Employees | Goals, desired state, health/recovery rules, internal-job budgets and checkpoints | Employee policy/state inside `org-office`; session coordination through the planned runtime integration |
| Processes | Approved definitions, scope resolution, revisions, instruction delivery | Planned pure `org-process` (using `org-docs` parsing); `org-office::processes` owns indexing, persistence, approvals and I/O integration, as specified in [processes.md](processes.md#7-where-the-code-lives) |
| Instance lifecycle | Setup, catalogs, service registration, release pins, key custody | Host/platform implementation composed by applications |
| Federation | Enrollment, replication, reconnect, ownership generations | Proposed sync/PKI components under their existing design ownership |

Organization membership/placement remains authoritative; an employee runtime record
does not become a second membership source of truth. Workflows spanning modules
call small public capabilities. Keep intermodule source dependencies acyclic.

## 3. Proposed Rust layout

This is a target sketch. Feature filenames illustrate cohesive responsibilities;
adjust their granularity to actual implementation complexity. Existing exports and
test behavior need an explicit disposition before files move.

```text
crates/
  org-office/src/
    organization/
      mod.rs                         # Intentional module API
      features/
        create_organization.rs
        hire_employee.rs
        place_employee.rs
      policy.rs                      # Shared organization invariants
    employees/
      mod.rs
      features/
        set_goal.rs
        pause_employee.rs
        request_checkpoint.rs
      policy.rs                      # Health, recovery, budget rules
      model.rs
    processes/
      mod.rs
      features/
        resolve_effective.rs
        publish_revision.rs
      policy.rs                      # Office integration; pure rules in org-process
    store/                           # Private SQLCipher/transaction adapters
    office.rs                        # Office lifecycle and public facade

  org-work/src/
    features/
      create_task.rs
      complete_task.rs
      report_bug.rs
    model.rs
    ports.rs                         # Consumer-owned storage/read/clock contracts

  org-mailbox/src/
    features/
      join_channel.rs
      ask_channel.rs
      open_dm.rs
    model.rs
    ports.rs

  org-proto/                         # Local request/event contracts
  org-office-launch/                 # Private host/worker launch contract
  org-client/                        # Local IPC client
  org-runtime/                       # Harness contracts and conformance support
  org-copilot/                       # Existing bounded draft-only capability
  org-spool/                         # Explicitly retained offline CLI behavior

  # Planned components, absent from the inspected workspace:
  org-process/                      # Pure process rules/resolution; see processes.md
  org-docs/                         # Pure document/front-matter support
  org-git/                          # Local Git adapter; needed v1 workflows
  org-office-sessions/               # Office session coordination
  org-supervise/                     # OS process ownership and cleanup
  org-runtime-claude/                # Harness adapters behind org-runtime
  org-runtime-codex/
  org-runtime-agy/
  org-sync/
  org-sync-proto/
  org-pki/
  # Deferred v2 integration, not a standalone-v1 dependency:
  org-github/

apps/
  org-server/                        # Per-office worker: domain/IPC/runtime composition
  org-gateway/                       # Planned instance browser HTTP/TLS helper
  org-setup/                         # Planned setup CLI; binary agentic-org adapters
  org-office-host/                   # Instance/worker supervision composition
  org-agent/                         # Small employee CLI
  org-ui/                            # Static browser application; see below
```

The planned crate names record the existing designs' proposed responsibilities;
they are not evidence of implemented runtime or replication behavior. Reassess
physical extraction during implementation rather than creating empty packages.
The release/setup command's code location follows its actual library callers and
packaging needs; naming it `agentic-org` does not make another daemon necessary.

`org-office` currently deliberately combines domain behavior and storage. Keep its
private store and single-writer invariant. Logical domain ownership does not require
separate databases, writers, or public SQL repositories. Its store adapters implement
consumer-owned contracts such as the work and messaging ports. Keep transactions,
deduplication, outbox writes, and sequence allocation atomic as their designs require.

Source dependencies point from adapters/composition toward policies and contracts:

```text
instance gateway -> typed authorized host/office client operations
per-office server -> public use-case APIs / selected runtime and sync adapters
office storage implementations -> work and messaging ports
work and messaging -> their required protocol vocabulary
session coordinator -> employee policy + runtime contracts
harness adapters -> runtime contracts
```

The standalone design chooses one `org-gateway` process per instance for browser
HTTP/TLS, routing all authorized HQ organizations on the same listener. The host
keeps office keys/catalog provisioning; each `org-server` remains one office worker.
Create organization requires instance-admin authority, not just organization
ownership. Keep gateway endpoint code focused on principal authorization, decoding,
typed public operations and result mapping; never return an office key to it. A small HTTP adapter crate remains an option
when its dependency boundary or actual callers justify extraction. Do not depend on
one runnable application's library from another to assemble the service.

## 4. Proposed Svelte/TypeScript layout

```text
apps/org-ui/
  src/
    routes/                          # SvelteKit navigation entry points
    lib/
      modules/
        organization/features/
          create-organization/
          hire-employee/
        work/features/
          create-task/
          report-bug/
        messaging/features/
          join-channel/
          ask-question/
          open-dm/
        employees/features/
          set-goal/
          pause-employee/
          inspect-run/
        processes/features/
          edit-process/
          inspect-effective/
      platform/
        http/
          client.ts
          events.ts
      components/ui/                 # Reusable visual primitives/design tokens
  static/
  test-vectors/
  e2e/
  package.json
  svelte.config.js
```

A feature can contain its component, interaction state, API integration, validation
feedback, and tests. Place shared domain UI types/state beside that domain's
features. A simple route may remain self-contained when extraction adds no value.

Retain the static SvelteKit adapter: the instance's Rust `org-gateway` serves assets, commands,
queries, authentication, and live events. Implement browser forms against that API;
this profile does not introduce a Node backend or SvelteKit server actions. Browser
state is a view of server authority, with loading/unavailable/error/freshness states.
Keep shared HTTP/event code free of unrelated business policy.

Tailwind Plus components remain inside `apps/org-ui`, the renamed product UI home.
They must not become a separately published kit, template, package, or crate.

## 5. Shared functionality: upload, validation, and hashes

Use the narrowest meaningful owner. Feature-specific avatar validation belongs to
the avatar feature; rules shared by one domain belong to its internal policy module.
A genuinely reusable upload capability can have a `files` owner with a public API.
The following is an **illustrative future example**, not an added v1 upload requirement:

```text
office implementation/
  files/
    mod.rs                           # Public file operations
    features/upload.rs
    policy.rs                        # Common file constraints
    content_digest.rs                # File identity semantics, if required
    ports.rs                         # Storage capability contract
    adapters/local_storage.rs
  employees/features/upload_avatar.rs
  projects/features/attach_document.rs

apps/org-ui/src/lib/
  components/ui/FilePicker.svelte
  components/ui/UploadProgress.svelte
  modules/files/upload/UploadPanel.svelte
  modules/files/api.ts
  modules/employees/features/upload-avatar/AvatarEditor.svelte
```

Employee/project operations own their business permissions and restrictions; file
operations enforce their common invariants and own storage integration. Consumers
call public operations rather than reconstructing a sequence of internal helpers.
Client checks provide feedback; Rust enforces policy on received content. Share
policy metadata, contracts, and test vectors across runtimes where useful.

Use an established hashing library, such as the workspace's `sha2`, for the basic
algorithm. File fingerprints, retry-deduplication hashes, and message-content hashes
have different canonicalization and compatibility contracts. Keep those semantics
with their owners. Existing `org-work::params_hash` and `org-mailbox::content_hash`
must not be merged merely because both use hashing. Extract a shared digest type or
module only for deliberately identical semantics; a separate crate needs a real
package-level purpose. Avoid a generic shared/Common/Utils dumping ground.

## 6. End-to-end slice example: set employee goal

1. The Svelte feature captures the goal and displays submission/outcome state.
2. HQ authenticates the human, authorizes the employee scope, and validates the
   operation ID and expected version through a typed HTTP operation.
3. The command reaches the owning office through an explicit, authorized contract.
4. The employee module validates and persists the update through office-owned storage.
5. Local session coordination delivers the current goal at a safe boundary under
   the single-main-session, takeover, and bounded reminder rules.
6. The browser observes the command outcome and runtime delivery/freshness separately.

A new goal change to an office known to be offline is rejected before admission,
with no goal write or queued update; keep the unsaved draft. A forwarded request
with unknown outcome uses Pending confirmation and the same operation ID for lookup/
retry, as [employee-supervision.md](employee-supervision.md#goal-command-admission-and-offline-behavior)
specifies. Acceptance does not prove completion or that a model read the goal. Browser disconnection does
not end durable work. Pause is a durable HQ status command even for an offline office, with runtime
stop pending until acknowledged, as the employee and session designs require; renaming/moving code does not transfer that authority.

## 7. Source-to-target implementation sequence

| Step | Source-to-target work | Verification |
| --- | --- | --- |
| 1. Contract/owner inventory | Map every moved public API, test fixture, transport caller, and baseline-only path; coordinate under AGENTS.md | Preserve documented behavior and acceptance IDs; identify unimplemented surfaces |
| 2. Application naming | Rename retained browser sources to `apps/org-ui`; rename server app/package/binary to `apps/org-server` / `org-server`; use distinct `org-setup` Cargo ID and remove Tauri workspace membership before building/staging its `agentic-org` binary | Update Cargo workspace/lockfile, Rust imports, test helper paths/env references, frontend package/lockfile, build/release scripts, helper resolution/signing checks and documentation together |
| 3. Domain organization | Move organization/employee/process behavior into cohesive modules; group work and messaging use cases in their existing crates | Preserve public compatibility where retained, visibility, transactions, invariants, and acyclic source direction |
| 4. Real feature integration | Implement each retained use case through its canonical Rust behavior, typed HTTP operation, Svelte feature, and meaningful tests | Replace production mock selection and Tauri callers in the same supported feature change; show explicit unavailable state for missing operations |
| 5. Desktop deletion and packaging | Apply the removal inventory; delete Tauri-only sources/dependencies and regenerate lockfiles | Preserve host key custody, private worker launch, local agent IPC, licensed UI location, bounded resources, and complete release payloads |
| 6. Runtime/federation acceptance | Implement the planned session/harness/sync capabilities according to their designs | Run platform, isolation, reconnect, durability, and cleanup evidence required by PR #5 |

Keep implementation commits buildable. Naming, domain extraction, HTTP integration,
and deletion may be grouped where callers require an atomic change. No old desktop
data migration or Tauri compatibility adapter is required. New standalone upgrades
still require their own schema/version compatibility handling.

Evaluate coupling at both workspace-package and logical-domain levels. For example,
the inspected normal internal package dependencies give `org-work` Ce=1,
`org-mailbox` Ce=1, and `org-office` Ce=2, excluding external/dev dependencies.
These are current package observations, not target counts or proof that internal
boundaries are correct. Reassess when wiring new ports and cross-domain workflows.

Completion requires consistent target names, intentional public APIs, no new source
cycles, authoritative server rules, licensed UI containment, no production mock
fallback, and preserved office/process/storage semantics. Run repository formatting,
Clippy, Rust tests, frontend checks/tests/build, and relevant browser tests for the
actual implementation. Validate docs/links and preserved acceptance IDs for this
documentation proposal; do not claim source moves or runtime checks here.
