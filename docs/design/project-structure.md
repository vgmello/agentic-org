# v1 project structure

How the code is organized: application names, domain modules containing vertical
slices, and where shared code lives. Read the language-agnostic
[architecture rules](../architecture-rules.md) first. Product scope and process
boundaries are in [ADR-0005](../adr/0005-standalone-server-v1.md) and the
[standalone design](standalone-server.md); records and storage are in the
[data contract](organization-data/README.md). This layout is code organization: it
does not create a database, public API or executable for each table.

## 1. Names and process boundaries

| Source / Cargo package | Executable / output | Responsibility |
| --- | --- | --- |
| `apps/agentic-org` / `agentic-org` | `agentic-org` | The person's CLI and the `serve` composition |
| `apps/agentic-org-office` / `agentic-org-office` | `agentic-org-office` | One HQ or satellite office |
| `apps/agentic-org-web` / `agentic-org-web` | `agentic-org-web` | Browser HTTP, assets and APIs |
| `apps/agent-os` / `agent-os` | `agent-os` | The agent's CLI |
| `crates/org-host` / `org-host` | Library only | Host lifecycle and key custody |
| `apps/agentic-org-ui` / `agentic-org-ui` | Static assets | Browser application |

Source directories, package names and executable names match. Reusable `org-*`
libraries keep domain and contract names. Signing identifiers and persisted
identities do not follow file names; file names never define trust. Which entry
points work today is in [executables and entry points](executable-migration.md).

## 2. Domain modules containing vertical slices

The [browser BFF design](rust-browser-bff-and-static-sveltekit.md) sets the browser
API boundary and reusable module boundaries.

`application -> domain module -> feature/use case`

Crates give package and visibility boundaries. Modules group a cohesive capability
inside a crate. Features implement concrete use cases. One feature does not imply
one crate, service, database or a mandatory stack of handler, repository and
interface files.

| Capability | Rules it owns | Implementation home |
| --- | --- | --- |
| Organization | One home team per employee, recursive teams and leads, roles, accountable project ownership, repository registrations | Organization module in `org-office` |
| Work | Typed tasks, office-minted references, independent assignment, immutable reporter and history, conflicts; private notes in a separate author-only store | `org-work`, with office-owned implementations of its ports |
| Messaging | Conversations, channels, membership, delivery, asks and authorization | `org-mailbox`, with office storage and session implementations |
| Employees | Execution configuration, model profiles and connections, permits, turns and attempts, fenced placement, process cleanup, suspension on a required ask | Employee policy and state in `org-office`; runtime adapters behind `org-runtime` |
| Processes | Process and standard documents, scope, override lock and one-chain resolution | `org-process`, called by the office ([processes design](processes.md#7-where-the-code-lives)) |
| Instance lifecycle | Setup, catalog, service registration, release pins, key custody, accounts and sessions | `org-host`, composed by `agentic-org` |
| Federation | Enrollment, replication, reconnect, ownership generations | `org-sync`, `org-sync-proto`, `org-pki` |

Employee identity is organization data; an agent's office placement is a projection
of the durable placement authority, and a recreated runtime record is never
permission to launch. Workflows that span modules call small public capabilities.
Module dependencies are acyclic.

## 3. Rust layout

Feature file names illustrate cohesive responsibilities; size them to the real
complexity. Do not create empty packages.

```text
crates/
  org-host/                         # Host lifecycle, accounts, sessions, key custody
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
        launch_agent.rs
        pause_employee.rs
        suspend_on_ask.rs
      policy.rs                      # Health, recovery, budget rules
      model.rs
    processes/
      mod.rs                         # Office integration; pure rules in org-process
    projects/                        # The projects family, same shape as organization (B3)
    store/                           # The store seam and engines: the only place that knows the engine (ADR-0018)
      mod.rs                         # open_store: the one composition seam
      sqlite/                        # The SQLite engine: one module per record family
    office.rs                        # Office lifecycle and public facade

  org-work/src/
    features/
      create_task.rs
      set_assignment.rs
    model.rs
    ports.rs                         # Consumer-owned storage, read and clock contracts

  org-mailbox/src/
    features/
      join_channel.rs
      ask.rs
      open_dm.rs
    model.rs
    ports.rs

  org-store/                         # Engine-neutral store: journal, writer, files, rebuild, Engine and Projector
  org-proto/                         # Local request and event contracts
  org-office-launch/                 # Private host/worker launch contract (the verified rendezvous)
  org-client/                        # Local IPC client
  org-platform/                      # Every OS difference: platform contracts and their macOS, Linux and refusing adapters
  org-socket/                        # Portable Unix-socket I/O: ticked reads, descriptor passing
  org-runtime/                       # Vendor runtime contract and conformance support
  org-supervise/                     # OS process ownership and cleanup
  org-process/                       # Process and standard resolution
  org-docs/                          # Document and front-matter parsing and editing
  org-sync/                          # Replication
  org-sync-proto/                    # Replication contracts
  org-pki/                           # Recovery escrow and private-key wrapping
  org-release/                       # Release manifest verification
  org-copilot/                       # Bounded draft-only helper
  # Added when their first caller is built:
  org-git/                           # Local Git adapter
  org-runtime-claude/                # Vendor adapters behind org-runtime
  org-runtime-codex/
  org-runtime-agy/
  org-office-sessions/               # Agent sessions in the office (agent-sessions design)

apps/
  agentic-org/                       # CLI and serve composition
  agentic-org-office/                # Office: domain, IPC, runtime composition
  agentic-org-web/                   # Browser HTTP
  agent-os/                          # Agent CLI
  agentic-org-ui/                    # Static browser application
```

How the office stores data is the [office store design](office-store.md). `org-store`
holds the engine-neutral foundation: one acceptance journal per office as the commit
point, the single writer, the files under `durable/`, rebuild, and the `Engine` and
`Projector` traits. Each record family (organization, projects, work, messaging,
execution) has one shape: records and codec, a read port and a write port, a projector,
one file per operation and a testkit. The SQLite engine in `org-office/src/store/sqlite/`
implements the ports, one module per family. One journal entry keeps sequence
allocation, receipts and the outbox atomic; logical domain ownership does not need
separate databases, writers or public SQL repositories.

Dependencies point from adapters and composition towards policies and contracts:

```text
web process -> typed authorized host and office client operations
office -> public use-case APIs, runtime and sync adapters
office storage engine -> family ports and org-store's Engine
work and messaging -> org-store's change contract and their protocol vocabulary
session coordinator -> employee policy + runtime contracts
vendor adapters -> runtime contracts
```

One `agentic-org-web` per instance serves browser HTTP and TLS for every authorized
HQ organization on its listener. The host keeps office keys and catalog
provisioning; each `agentic-org-office` is one office. Web endpoint code is limited to
principal authorization, decoding, typed public operations and result mapping, and
never receives an office key. A small HTTP adapter crate is extracted when its
callers justify it. No runnable application depends on another application's
library.

### Libraries built but not yet reached

"Reached" means a person or agent can exercise it through a running executable.

| Library | Purpose | Decision | Work continues when |
| --- | --- | --- | --- |
| `org-process` | Parses process and standard documents and computes the effective set | Continue; simplify it to the one-chain rule first | The office builds an agent's instructions at launch (FC-110, FC-112) |
| `org-copilot` | Bounded draft-only helper that proposes text for a person to accept | Parked: keep it compiling and tested, add nothing | A vendor runtime adapter exists (FC-101) |
| `org-release` | Verifies a signed release manifest and its files | Continue | `agentic-org setup` and update (FC-010, FC-011) |
| `org-pki` | Seals and recovers the CA key with the recovery code | Continue; it also gains the office-secrets-key wrapping of ADR-0016 | HQ creation through setup (FC-016, FC-162) |
| `org-sync`, `org-sync-proto` | Replication between HQ and satellites | Parked until the store reads and writes the canonical files | After the storage move; then satellite enrollment |
| `org-runtime`, `org-supervise` | Vendor runtime contract; process start, stop and cleanup | Continue: the next foundation for running agents | The first vendor adapter and launch path (FC-101), with the isolation modes of ADR-0015 |
| `org-host` `hostdb` | Host accounts, sessions and office registry storage | Continue | Stored browser sessions and setup (FC-159, FC-010) |
| `org-platform` | Holds every operating-system difference: the platform contracts of ADR-0017 with macOS, Linux and refusing adapters ([Linux host design](linux-host.md)) | New in X1a; the only crate with platform conditionals | Track X, from X1a (the first day) |
| `org-store` | The office store's engine-neutral foundation ([office store design](office-store.md)) | New in B1a; depends on no family (on `org-platform` once X1b moves its flush there) | B1a |
| `org-spool` | File queue `agent-os` uses when no office is named | Deleted, with the route through it | With conversations and messages (implementation plan B6) |

## 4. Svelte and TypeScript layout

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
          assign-task/
        messaging/features/
          join-channel/
          answer-question/
          open-dm/
        employees/features/
          pause-employee/
          view-session/
        processes/features/
          edit-process/
          inspect-effective/
      platform/
        http/
          client.ts
          events.ts
      components/ui/                 # Reusable visual primitives and design tokens
  static/
  test-vectors/
  e2e/
  package.json
  svelte.config.js
```

A feature can hold its component, interaction state, API integration, validation
feedback and tests. Shared domain UI types and state sit beside that domain's
features. A simple route may stay self-contained.

The UI uses the static SvelteKit adapter, shadcn-svelte and Bits UI. The Rust
`agentic-org-web` serves assets, commands, queries, sign-in and live events; there is
no Node backend and no SvelteKit server action. Browser state is a view of server
authority, with loading, unavailable, error and freshness states. Shared HTTP and
event code holds no business policy. Tailwind Plus components stay inside
`apps/agentic-org-ui` and never become a published kit, template, package or crate.

## 5. Shared functionality: upload, validation and hashes

Use the narrowest meaningful owner. Avatar validation belongs to the avatar feature;
rules shared by one domain belong to its policy module. A genuinely reusable upload
capability may have a `files` owner with a public API. An illustration, not a v1
requirement:

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

Employee and project operations own their permissions; file operations own their
common invariants and storage. Consumers call public operations rather than
reassembling internal helpers. Client checks give feedback; Rust enforces policy on
what it receives.

Use an established hashing library such as `sha2`. File fingerprints, retry
deduplication hashes and message content hashes have different canonicalization
contracts and stay with their owners: `org-work::params_hash` and
`org-mailbox::content_hash` are not merged because both hash. A shared digest type
exists only for identical semantics. There is no generic shared, Common or Utils
module.

## 6. End-to-end slice: assign a task

1. The Svelte feature captures the new team and assignee and shows the submission
   and outcome state.
2. The web process authenticates the person and passes a typed operation with its
   operation ID and expected revision ([web API, Tasks](web-api-v1.md#tasks)).
3. The command reaches the office with the person's principal and a host grant.
4. `org-work` decides the change and the office store commits the task file, its
   history event and its idempotency receipt as one journal entry.
5. The browser sees the outcome on the response and the change on the work feed.

A lost response is resolved by lookup of the same operation ID. Closing the browser
never ends durable work.

## 7. Verification

Evaluate coupling at the package and the logical-domain level when new ports and
cross-domain workflows are wired. Done means consistent names, intentional public
APIs, no source cycles, authoritative server rules, licensed UI containment and no
production mock fallback. Run formatting, Clippy, Rust tests, frontend checks, tests
and build, and the relevant browser tests.

## What the code does today

The application renames and host extraction are done. The domain modules above are
not finished: `org-office` has `organization`, `employees`, `processes` and `rules`
modules that implement the older model with several memberships per member, and `store` holds the SQL for all of them,
with SQL in four files outside it (`organization/bootstrap.rs`,
`organization/admission.rs`, `office.rs`, `office/hq_creation.rs`). `org-store` does not
exist yet; B1b deletes today's `store` whole and the SQL outside it. `org-git` and the vendor runtime adapters do
not exist yet.
