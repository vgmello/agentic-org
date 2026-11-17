# Proposed v1 project structure and architecture profile

Status: **owner-requested implementation proposal**, 2026-11-10. This document
extends PR #5 with application naming, domain modules containing vertical slices,
shared-code ownership, and a concrete reorganization sequence. The proposal changed
documentation only. The worker and UI renames and Tauri removal are now implemented;
the copilot module move has landed, and the remaining domain moves are pending.
Source observations refer to `a4039a0c810ca222d574a44eba5f84b83d07e9a4`.

Read the language-agnostic [architecture rules](../architecture-rules.md) first.
[ADR-0005](../adr/0005-standalone-server-v1.md), the
[standalone design](standalone-server.md), and the
[removal inventory](standalone-server-removal-inventory.md) retain authority over
product scope, process/security boundaries, and deletion acceptance.

**Executable migration implementation:** [executable-migration.md](executable-migration.md) records the aligned application directories, packages and binaries. Host behavior is the org-host library composed by agentic-org serve. F3c protected bootstrap/authentication is integrated at `67860707`; F4 implements the web handoff and session backend for debug/test instance composition. The production serve launcher (U1) and signed release remain pending.

## Selected data and source ownership

The [documentation authority map](documentation-authority.md) and [organization-data index](organization-data/README.md) own target entity/file relationships, including the messaging, recovery and budget contracts. The domain/vertical-slice layout here is code organization, not permission to infer a new database, public API or executable for every table. Earlier bug/request handler names can remain implementation details while target identities become typed tasks; public references follow document 06.

Repository registrations/caches are organization-wide; project/repository associations own integration branches. Runtime task worktrees follow document 04's employee hierarchy. Protected operational sources and private data are separate from code repository checkouts. These are target ownership rules; actual module/storage migrations remain pending.

## 1. Names and process boundaries

| Source / Cargo package | Executable / output | Responsibility |
| --- | --- | --- |
| apps/agentic-org / agentic-org | agentic-org | Human CLI and serve composition |
| apps/agentic-org-office / agentic-org-office | agentic-org-office | One HQ/satellite office worker |
| apps/agentic-org-web / agentic-org-web | agentic-org-web | Browser HTTP, assets and APIs |
| apps/agent-os / agent-os | agent-os | Agent-facing CLI |
| crates/org-host / org-host | Library only | Host lifecycle and key custody |
| apps/agentic-org-ui / agentic-org-ui | Static assets | Browser application |

Application source directories, package names and production executable names are aligned. Reusable org-* libraries retain domain/contract names. Existing signing identifiers, persisted data and wire fields remain stable; filenames do not define trust. HQ service registration/CLI setup and the production web launcher (U1) are unavailable as documented in executable-migration.md; F4's web handoff runs only through debug/test instance composition. The obsolete desktop/Tauri executable is removed; it must not overwrite the current agentic-org artifact.

## 2. Architecture to follow

The accepted [Rust browser BFF and static SvelteKit architecture](rust-browser-bff-and-static-sveltekit.md) records browser API priorities, reusable module boundaries, future integration adapters, and browser preloading.

Use **domain modules containing vertical slices**:

`application -> domain module -> feature/use case`

Crates establish useful package and visibility boundaries. Modules establish
cohesive capabilities within those crates. Features implement concrete use cases.
One feature does not imply one crate, service, database, or mandatory stack of
handler/repository/interface files.

| Capability | Rules and ownership | Proposed implementation home |
| --- | --- | --- |
| Organization | Single home-team membership, recursive teams/leads, roles, accountable project ownership and repository registrations | Organization module inside `org-office` |
| Work | Typed tasks, independent assignment, immutable reporter/history, scoped numbering and conflict handling; private notes use a distinct author-only store | Existing `org-work`, with office-owned implementations of its ports; migration and private-note service remain pending |
| Messaging | Conversations, channels, membership, delivery and authorization rules | Existing `org-mailbox`, with office storage/session implementations |
| Employees | Execution configuration, ordered model profiles/connections, permits, turns/attempts, fenced placement and verified process cleanup; employment and pause/hold states remain separate | Employee policy/state inside `org-office`; runtime adapters and supervision integrate documents 04/11/14/16/17 |
| Processes | Selected process/standard definitions, scope metadata and override flags; inherited resolution/execution remains deferred for the target | Existing `org-process`/`org-docs` provide earlier implementation evidence; office integration and target disposition follow [processes.md](processes.md#7-where-the-code-lives) |
| Instance lifecycle | Setup, catalogs, service registration, release pins, key custody | Host/platform implementation composed by applications |
| Federation | Enrollment, replication, reconnect, ownership generations | Proposed sync/PKI components under their existing design ownership |

Employee identity/profile is canonical organization data; current office placement
is a projection of the durable placement-generation authority. A recreated runtime
record does not become a second membership source or permission to launch. Workflows spanning modules
call small public capabilities. Keep intermodule source dependencies acyclic.

## 3. Proposed Rust layout

This is a target sketch. Feature filenames illustrate cohesive responsibilities;
adjust their granularity to actual implementation complexity. Existing exports and
test behavior need an explicit disposition before files move.

```text
crates/
  org-host/                         # Shared host lifecycle and key custody
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
        request_checkpoint.rs        # Proposed memory use case; not selected by data model
      policy.rs                      # Health, recovery, budget rules
      model.rs
    processes/
      mod.rs
      features/
        resolve_effective.rs         # Deferred target engine, not selected structural data
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
  org-socket/                        # Shared Unix-socket reads and liveness checks (libc only)
  org-runtime/                       # Harness contracts and conformance support
  org-copilot/                       # Existing bounded draft-only capability
  org-spool/                         # Explicitly retained offline CLI behavior

  # Present crates; existence does not establish selected-target integration:
  org-process/                      # Earlier process rules/resolution; target engine deferred
  org-docs/                         # Document/front-matter support
  org-supervise/                    # OS process ownership and cleanup
  org-sync/                         # Replication library; target source adaptation pending
  org-sync-proto/                   # Versioned sync contracts
  # Planned components, absent from this revision:
  org-git/                          # Local Git adapter
  org-office-sessions/              # Office session coordination
  org-runtime-claude/               # Harness adapters behind org-runtime
  org-runtime-codex/
  org-runtime-agy/
  org-pki/
  # Deferred v2 integration, not a standalone-v1 dependency:
  org-github/

apps/
  agentic-org-office/                        # Per-office worker: domain/IPC/runtime composition
  agentic-org-web/                       # HTTP library/executable; debug/test handoff (F4); production launcher pending (U1)
  agentic-org/                         # CLI/serve; protected setup remains unavailable
  # Host lifecycle is a library at crates/org-host, not a separate app.
  agent-os/                         # Small employee CLI
  agentic-org-ui/                            # Static browser application; see below
```

The sketch distinguishes present libraries from proposed packages. Present code
and its tests describe the implemented contract, not completion of the selected
organization-data schema, runtime routing or replication-source migration. Reassess
physical extraction during implementation rather than creating empty packages.
The agentic-org application composes org-host. Its setup command and serve process share library behavior; there is no additional host/updater daemon.

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

The standalone design chooses one `agentic-org-web` process per instance for browser
HTTP/TLS, routing all authorized HQ organizations on the same listener. The host
keeps office keys/catalog provisioning; each `agentic-org-office` remains one office worker.
Create organization requires instance-admin authority, not just organization
ownership. Keep gateway endpoint code focused on principal authorization, decoding,
typed public operations and result mapping; never return an office key to it. A small HTTP adapter crate remains an option
when its dependency boundary or actual callers justify extraction. Do not depend on
one runnable application's library from another to assemble the service.

## 4. Proposed Svelte/TypeScript layout

```text
apps/agentic-org-ui/
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

Retain the static SvelteKit adapter: the instance's Rust `agentic-org-web` serves assets, commands,
queries, authentication, and live events. Implement browser forms against that API;
this profile does not introduce a Node backend or SvelteKit server actions. Browser
state is a view of server authority, with loading/unavailable/error/freshness states.
Keep shared HTTP/event code free of unrelated business policy.

Tailwind Plus components remain inside `apps/agentic-org-ui`, the renamed product UI home.
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

apps/agentic-org-ui/src/lib/
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
| 2. Application naming | Rename retained browser sources to `apps/agentic-org-ui`; rename server app/package/binary to `apps/agentic-org-office` / `agentic-org-office`; use distinct `agentic-org` Cargo ID and remove Tauri workspace membership before building/staging its `agentic-org` binary | Update Cargo workspace/lockfile, Rust imports, test helper paths/env references, frontend package/lockfile, build/release scripts, helper resolution/signing checks and documentation together |
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
