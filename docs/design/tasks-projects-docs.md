# Design: typed tasks, features, projects and project docs (W7)

Status: **selected target integration design, revision 3**, 2026-11-15. Owner: ada. The library remains **`org-work`**. This document reconciles W7 with [organization data](organization-data/README.md); it does not change application code, authorize a migration or claim that the target services exist. The [authority map](documentation-authority.md) separates target data from current runtime evidence.

## In one paragraph

There is one task entity with extensible `type`, required accountable team, optional project and independent current team/individual assignment. All task types share T- references, shared operational metadata and durable immutable task history. Projects require an accountable team and can associate many registered repositories, each with its own integration branch. Authored project/task/feature/knowledge records live in the remote organization Git repository, mirrored as durable ordinary files and indexed in shared SQLite. Private task notes have a separate durable author-only database. Runtime caches, execution placement and code worktrees are distinct from those sources. Existing code retains its old contracts until an explicitly reviewed adaptation; §4 preserves the earlier contract and security reference in a bounded section.

## Decisions

1. **One typed task.** `type: task | bug | request | ...` is classification, not a different entity/table/folder. Optional `work_type: build | run` is a separate classification; null means unclassified.
2. **Accountability and assignment differ.** Required task `team_id` equals `projects.owner_team_id` when project-scoped. Nullable `assigned_team_id` and `assignee_id` form one atomic current assignment pair. Neither is inferred from the other or from employee home membership.
3. **Identity is stable.** New records use UUIDv7. Preserve existing stable IDs exactly. Readable T- references are scoped to project or to the accountable team when projectless. Type changes and assignment never renumber tasks.
4. **Sources are durable.** Task files and published immutable task events are organization Git content. Acknowledged unpublished changes require a durable journal. SQLite is their indexed projection, not the sole recovery source.
5. **Features are definitions.** `projects/<project-id>/features/<feature-id>/feature.md` defines behavior, rules, limitations and non-goals. Its status and owner are authored fields, not values automatically calculated from task completion.
6. **Supporting documents have explicit scope.** Required project; zero or one same-project feature, release or deployment; stable IDs and common attribution. `feature.md` is not duplicated as a supporting-document row.
7. **Separate privacy.** Existing `note` comments remain shared. Private notes are author-only durable records with no implicit administrator exception, outside organization Git, shared history, shared search and shared feeds. Author-private search must enforce the same author boundary.
8. **Retain prior behavior as evidence.** Areas, Undo, advanced checklist engines, automatic feature/bug/request transitions and process execution remain unselected. Their earlier requirements are retained by reference and individually identified in the UX criteria. Selecting data does not deploy a workflow engine.

## 0. Where each thing lives

Paths below describe authored organization data, not this software repository's own `docs/` tree.

| Thing | Canonical source | Projection / boundary |
| --- | --- | --- |
| Project and repository associations | `projects/<project-id>/project.yaml` | Shared project, contributing-team and project/repository rows; HQ creates projects |
| Repository registration | Organization repository registration in document 01 | May be shared across projects; code remote or local-only |
| Project task, any type | `projects/<project-id>/work/tasks/<task-id>.md` | One task row with required owner team |
| Projectless task, any type | `teams/<team-id>/.../work/tasks/<task-id>.md` | Full ancestor chain; no duplicate project copy |
| Feature definition | `projects/<project-id>/features/<feature-id>/feature.md` | Feature row plus derived full slug |
| Supporting knowledge | Project/feature/release/deployment folders in document 02 | Indexed documents, with explicit scope and stable ID filenames |
| Shared comments/checklists/watchers/questions/waits | Task front matter | Shared operational projection; all types use `target_kind: task` |
| Task activity | `work-history/tasks/<task-id>/<revision>.json` | Immutable `task_events`; same identity on replay |
| Final readable allocations | `identity/counters/` and `identity/allocations/` | Authority journal also required before acknowledgment |
| Private task notes and receipts | Separate durable private database, document 05 | No shared reverse index, count or existence event |
| Usage / budgets / chat / receipts | Their protected durable sources in documents 10/15/16/17 | Git alone cannot restore them |
| Code cache / task worktree | `runtime/<office-id>/...` | Reconstructible only with recovery and process reconciliation; never authored organization content |

```text
<organization repository>/
├── projects/<project-id>/
│   ├── project.yaml
│   ├── brief.md
│   ├── features/<feature-id>/
│   │   ├── feature.md
│   │   ├── designs/<document-id>.md
│   │   ├── decisions/<document-id>.md
│   │   └── reports/<document-id>.md
│   ├── designs/<document-id>.md
│   ├── decisions/<document-id>.md
│   ├── reports/<document-id>.md
│   ├── processes/<document-id>.md
│   ├── standards/<document-id>.md
│   ├── releases/<release-id>/
│   │   ├── release.md
│   │   ├── changelog.md
│   │   ├── documents/<document-id>.md
│   │   ├── milestones/<milestone-id>.md
│   │   └── deployments/<deployment-id>/
│   │       ├── deployment.md
│   │       ├── changelog.md
│   │       └── documents/<document-id>.md
│   └── work/tasks/<task-id>.md
├── teams/<team-id>/.../work/tasks/<task-id>.md
├── work-history/tasks/<task-id>/<revision>.json
└── identity/{counters,allocations}/...
```

Release product changelogs and deployment technical changelogs differ. Tasks remain in `work/tasks/` when linked to a milestone; the milestone belongs to a release. Saving durable files and publishing Git are separate operations.

## 1. The model

### 1.1 Ids and references

Every foreign reference and filename uses the immutable stable ID. New IDs are UUIDv7; imported ULIDs and other accepted old IDs stay unchanged. `schema_version` identifies file format; `revision` identifies concurrency. Positive persisted `number` yields the derived readable key. Only a pending local task/feature or import staging can have a null number.

| Entity | Selected display | Scope |
| --- | --- | --- |
| Team | `TEAM-12` | Organization |
| Project | `P-8`, slug `reservations` | Organization |
| Task of every type | `T-42` | Project, or team when projectless |
| Feature | `F-6`, full slug `reservations/table-booking` | Number/local slug within project |
| Release | `R-3` | Project |
| Milestone | `M-2` | Release; full reference `P-8/R-3/M-2` |

Inside a selected scope use `T-42`; outside use `P-8/T-42` or `TEAM-12/T-42`. Type icons/words distinguish Bug T-42 and Request T-43. Pending records show **Pending number** plus a copyable stable ID; any short display must resolve unambiguously and is not a final reference. Never strip B-/REQ- prefixes to reuse their old number. An approved import must resolve namespace collisions and record explicit historical-reference mappings.

### 1.2 The core (every kind)

| Field | Selected meaning |
| --- | --- |
| `id`, `schema_version`, `revision`, `number` | Stable identity, format, concurrency and scoped allocation |
| `team_id` | Required accountable team; equals project owner for project tasks |
| `project_id` | Nullable; null means team-scoped work |
| `type` | Nonempty extensible identifier; task, bug, request are initial types |
| `work_type` | Nullable `build` / `run`, separate from type |
| `title`, body | Required title; Markdown body projects as `body_markdown` |
| `status`, `priority`, `severity`, `resolution`, `blocked_reason` | Lifecycle/triage data, not implicit transition-engine approval; priority defaults normal, severity optional |
| `assigned_team_id`, `assignee_id` | Independent nullable team/individual responsibility; replace atomically |
| `feature_id`, `milestone_id` | Each optional and from the same project; both null for team tasks |
| `created_at`, `created_by_kind`, `created_by_id` | Immutable admitted creator/reporter/originator and original time |
| `updated_at`, `updated_by_kind`, `updated_by_id` | Latest accepted update, not reporter or historical fixer |
| `repository_ids`, `branches`, `integrated_commits` | Zero or many organization repositories and per-repository work evidence |
| `checklist`, `comments`, `links`, `operations` | Shared nested metadata with stable child IDs |
| `details` | Optional structured task/bug/request payload projected into task_details with the same task identity/revision; absent map has no row, empty map has documented defaults |

Actor kinds are employee, owner, system or unknown; employee actors have an employee ID, other public actor kinds have null employee ID. New accepted records supply actual times/actors; unknown historical provenance stays unknown. The authenticated owner is a security principal, not an employee flag. If represented as an employee, owner/HR still needs a valid home team.

### 1.3 Task

A task may have no project and no repository. A director can assign a team task to another director without inventing a project. Project task repository targets must belong to its project's repository associations; projectless tasks can target organization registrations directly. Features and milestones are optional independent planning relationships, not canonical task folders.

Example: Payments owns P-8 and T-42. Platform is the assigned team; Ana is the individual assignee. Payments' owned backlog queries `team_id`, Platform's queue queries `assigned_team_id`, and Ana's queue queries `assignee_id`. No assignment changes the task's scope, number, canonical file or reporter.

A newly authored project task uses stable UUIDv7 IDs. This illustrative subset assumes the referenced project/team/employee/repository already exist; complete accepted records also include the common attribution and shared arrays required by documents 03/05/08.

```yaml
schema_version: 1
id: 019a8541-1800-7000-8000-000000000042
number: 42
revision: 3
type: bug
work_type: build
team_id: 019a8541-1800-7000-8000-000000000020
project_id: 019a8541-1800-7000-8000-000000000008
assigned_team_id: 019a8541-1800-7000-8000-000000000021
assignee_id: 019a8541-1800-7000-8000-000000000030
title: Prevent overlapping reservations
status: todo
priority: high
severity: major
feature_id: null
milestone_id: null
resolution: null
blocked_reason: null
repository_ids: [019a8541-1800-7000-8000-000000000002]
branches: {}
integrated_commits: {}
created_at: "2026-11-15T10:00:00Z"
created_by_kind: employee
created_by_id: 019a8541-1800-7000-8000-000000000031
updated_at: "2026-11-15T10:05:00Z"
updated_by_kind: employee
updated_by_id: 019a8541-1800-7000-8000-000000000032
```

Save at `projects/019a8541-1800-7000-8000-000000000008/work/tasks/019a8541-1800-7000-8000-000000000042.md`; the body explains overlap rejection and concurrency evidence. This is P-8/T-42; creator 031 remains reporter although updater 032 assigned Platform/Ana. A projectless request uses `type: request`, null project/feature/milestone and a valid owning team, with its canonical file under the full team ancestor chain. It does not copy this project's number into the team's scope.

### 1.4 Bug

A bug is a task with `type: bug`, optional severity and selected structured `details` in its task front matter. The UX offers severity explanations and can suggest confirmation at triage without requiring an area. Document 03 maps `what_happened`, `expected`, `steps`, `environment`, `severity_confirmed`, `reopen_count`, `closed_reason`, `duplicate_of_task_id`, `affects_feature_id`, `suggested_triager_id` and `suggested_verifier_id` into `task_details`. Reproduction text remains distinct from description; affected feature remains distinct from the implementing feature and must match the bug's project. Duplicate references resolve retained same-organization task identities, with target type/cycles validated. Suggested employees are neither assignments nor permissions. Preserve those selected fields atomically with the task/event, including final details in tombstone record metadata. Only unsupported unknown variants or still-unmapped legacy values remain in controlled staging.

### 1.5 Request

A request is a task with `type: request`, project-scoped or projectless with required accountable team. Routing uses the same assignment pair. Its selected `details` retain `request_kind`, `request_text` (old WorkItem.text), `route_reason`, `routing_source`, optional `routed_by_employee_id`, `request_outcome`, `merged_into_task_id` and `decline_reason`. Old routed_by maps to person/rule/copilot source, never to an employee FK; independently known routing actor may populate the optional employee ID.

`details.request_outcome` projects to `request_outcome_json`: optional `feature: {type: new | extends, feature_id}`, `docs[]: {project_id, doc_key}`, `draft_tasks[]` with required draft_id/title and optional description/assignee_id/team_id/priority, and estimate with effort/cost_usd_cents_min/cost_usd_cents_max. Validate every nested reference/tag, nonnegative estimate bounds and min≤max. Draft IDs are proposals, not accepted tasks; an estimate is not a budget. These selected data structures preserve outcome evidence without implementing approval, automatic wake or feature/task creation. Public transition/approval handlers and migration execution still require explicit implementation decisions.

### 1.6 Checklist

Shared task front matter stores ordered stable-ID steps with text, required/done flags, optional linked task, check actor/time and revision. The array supplies position. Plain checklist editing and history are selected data. Task acceptance text and completion exceptions also remain selected evidence in `details.definition_of_done`, `details.branch`, `details.shortcut` and `details.done_anyway`: shortcut retains none/debt variant IDs without selecting a debt engine; done_anyway retains reason and step IDs without selecting the gate. Promotion, automatic linked-task ticks, default templates, required-step completion gates and process-derived steps remain earlier requirements awaiting selection.

### 1.7 Links

Authored `links[]` contains stable link ID, kind, target kind and stable target ID. `depends_on` rejects self-links/cycles and can cross projects in the same organization; inverse `blocks` is derived. `fixes` targets a bug-typed task; inverse `fixed_by` is derived. `relates_to` does not imply ownership. Task `feature_id` derives `implements`; document scope derives `documents`. Do not author both directions. A dependency on a tombstone needs explicit disposition before accepting new active work, never automatic satisfaction. Old `from_request` and other W7-only link kinds require explicit migration treatment rather than unsupported target fields.

### 1.8 Feature record (office data, P3)

The heading is retained for old links; **the selected source is organization Git**, not office-owned feature state. A feature requires project and project-unique `local_slug`; the full slug is derived from project slug. It has stable ID, number, revision, title, body, optional work classification and owner, attribution, lifecycle and retained delivery/retirement metadata. Planned features may lack an owner; in-progress/live features require an active accountable employee. Definition sections are Purpose, How it works, Rules, Known limitations and Non-goals. Detailed designs/decisions/reports are separate supporting records. Task progress does not silently change feature status.

### 1.9 Project and area (organization data)

Projects require `owner_team_id`, number and organization-unique slug; optional `contributing_team_ids` grant no implied authorization. Each `repositories[]` association requires `repository_id` and `integration_branch`. One registration may serve many projects with different branches; an office has one cache per repository, not a cache per project. Organization Git must be remote; code registrations may be local-only. Areas are unselected and have no target table or mandatory General record.

### 1.10 State machines

The selected draft stores status/resolution data. It does not select every v1 transition engine. Preserve task To do/In progress/Blocked/Done and old bug New/Triaged/In progress/Fixed/Closed, request New/Routed/Scoping/Scoped/Approved/Declined/Merged, and terminal resolutions as import evidence. A future lifecycle decision must define supported transitions, verification, cancellation, unresolved-question disposition and authorization before those controls ship. No automatic Fixed, Delivered, Paid, required-step gate, request approval or Undo is inferred from data fields. Existing implemented handlers remain current-contract behavior, described by source and §4, until adaptation.

## 2. The rules

### 2.1 Validation

Validate at the accepting backend boundary: organization/scope IDs, required accountable team, project-owner equality, active assignment recipients, same-project feature/milestone/document scopes, repository eligibility, path/field consistency, cycles and duplicate stable identities. New assignment is limited to active teams/employees within the organization; an individual's home team need not equal the assigned team or any project team. Preserve text validation, safe Markdown rendering and explicit size errors. Existing W7 numerical wire limits remain compatibility evidence in §4; they are not newly enacted target service limits.

### 2.2 Who may do what

Keep authentication and command authorization separate from project ownership, lead/subtree management and work assignment. Derive reporter/update actors from the admitted connection; reject forged actors. Preserve current bound-agent versus app/person/owner restrictions, app-only authorization checks, credential custody and required cryptographic identities. Do not expand old shared-work read access to private notes. Target mutation/authorization APIs are implementation work, not a new permission grant from this design.

### 2.3 Ids, idempotency and conflicts

An assignment mutation supplies **both desired fields including explicit nulls** plus expected task revision; accept both fields and an `assignment_changed` event together or change neither. A stale revision conflicts even when the caller changes only one half. Retrying the same operation returns its durable result; a different request cannot reuse that operation identity. The exact public API is pending. The current `task.assign`/`expected_version` and old canonical typed-JSON hash are retained as current-contract evidence, never silently redefined to UUIDv7 or target hashing.

The service acknowledges only after task snapshot, event(s) and idempotency result are one recoverable operation. Concurrent writes to shared tasks wait for or conflict at the single accepting writer. Raw replicated files do not create write authority. A new task begins with `created` revision 1; unknown future change tags remain readable as unknown and retained.

### 2.4 Automatic changes and Undo

Task content/metadata changes, including comments/checklists/watchers/questions/waits, append immutable task events. If separately authorized automation changes data, record system attribution and cause. Automatic status propagation, one-hour Undo, cascades and advanced checklist execution are unselected; retain their old designs in Git history rather than treating them as current acceptance requirements. Handoff is not closure and does not delete task worktrees.

### 2.5 Cascades from organization changes

Changing project ownership updates the owning `team_id` of affected project tasks in the same recoverable operation and records history; it leaves assignments unchanged. An employee home-team move does not rewrite current task assignment. Inactive employees/archived teams cannot receive new active assignments; preserve historical identity and transfer required active record ownership before deactivation. Do not port old area cascades or silently clear historical assignments. Exact disposition of existing open assignments during deactivation remains lifecycle implementation work.

### 2.6 Numbering

HQ creates teams/projects/releases/milestones. Any office may durably create tasks/features, including disconnected, immediately usable by stable ID with `number: null`. The shared organization authority serializes number/slug registration across offices; replicas never increment their own counters. Allocate entity and durable retained allocation together before acknowledgment; never use `MAX(number)+1`. A retry returns the same allocation. Retain allocations/tombstones after closure/deletion. Scope moves preserve stable identity and historical allocations; they need a separate explicit reallocation contract. Assignment/type changes do not move scope.

### 2.7 Ownership, versions and the stream (office-first)

The heading is retained; office placement no longer defines task ownership. Each accepted revision is archived at `work-history/tasks/<task-id>/<revision>.json`, with stable identity `(organization_id, task_id, revision)` and `task_kind: task` for every type. Events retain actor, time, optional operation, old/new values, source metadata and payload digest. Document 05 defines its exact versioned hash format; do not substitute the old WorkEvent/command codecs. Published files plus unpublished durable journal/receipts rebuild the same history. Assignment is not writer handover. Safe fencing/replication remain implementation work.

### 2.8 Deleting

Retain immutable task history, allocated numbers and stable reference tombstones after closure/archive/deletion. Tombstones retain record provenance, terminal state, last assignment pair and revision. Deletion does not reuse a number or silently satisfy dependencies. Private-note deletion uses its separate retention/backup policy and must not be blocked by immutable shared history. Existing records absent from selected draft stay in staging until disposition is approved.

### 2.9 Projects and docs without a repository

A project need not associate a code repository to have authored knowledge: its brief, features and supporting documents live in **organization Git**. Task work can also lack code. The old claim that no code repository means no docs is retired.

## 3. Storage: the `org-work` library and its storage trait

### 3.1 Where the code lives

Current source names are `apps/agent-os`, `apps/agentic-org`, `apps/agentic-org-office`, `crates/org-office`, `crates/org-work` and `crates/org-proto`. Apps compose domain libraries; `org-work` owns rules/ports and `org-office` implements storage. Keep SQL/Git/network adapters outside domain policy and avoid source cycles. This documentation adds no dependency or module. The older `org-docs` split is a proposal, not an installed component.

### 3.2 The ports `org-work` defines

The checked current ports are in [`crates/org-work/src/ports.rs`](../../crates/org-work/src/ports.rs); the current stored model is in [`model.rs`](../../crates/org-work/src/model.rs), task handlers in [`tasks.rs`](../../crates/org-work/src/tasks.rs), and wire types in [`org-proto/src/work.rs`](../../crates/org-proto/src/work.rs). They retain per-kind `NumberScope`, old `StoredItem` and `WorkEvent` semantics. Do not replace actual trait signatures with an invented target service. Required adaptation boundaries:

| Current contract / evidence | Target integration requirement |
| --- | --- |
| `StoredItem.created_by`, `assignee_id`; `task.assign` records individual `Assigned { from, to }` | Preserve reporter and prior individual facts; add nullable team assignment and atomic expected-revision pair |
| Old optional `team_id`, required project for tasks/bugs | Require accountable team for all types; allow projectless tasks and enforce project-owner equality |
| Old Task/Bug/Request identity and per-kind number scopes | One typed task identity and scoped T- allocation; explicit collision/reference mapping |
| ULID command/entity validators and typed canonical command hashing | Keep old codec when reading/retrying old commands; separately version new UUIDv7 creation/target persistence |
| `work_events` immutable store and current-state transaction | Preserve immutable ordered events; materialize target files/journal and receipts recoverably |
| Per-field `expected_version` conflict checks | Preserve old behavior for old methods; new assignment pair requires exact expected task revision |
| `CommentKind::Note`, `work.comment`, shared activity | Shared comments stay shared; private notes require separate store/API/access controls |
| `work.snapshot`/`work.activity`/`work.changed` wire references | Target read adapters map stable IDs, scopes and revisions without presenting old shapes as target files |
| `doc.*` / code-repository docs design in §4.9 | Implement organization Git scope/path validation separately, preserving all security and crash guarantees |

### 3.3 Tables (a sketch for server-dev, `office.db`)

The authoritative draft DDL is [schema.sql](organization-data/schema.sql), not the earlier office-only W7 sketch. Core projections are tasks/task_details/task_repositories, features/feature_slugs, projects/project_repositories/project_contributing_teams, documents, shared operations, links/entity_relationships, record_refs/tombstones, task_events and reference counters/allocations. Private-note DDL is a separate [private-notes-schema.sql](organization-data/private-notes-schema.sql) with trusted identity/task mirrors and durable command receipts. Never combine it with the shared projection. Its author tuple comes from authentication; a public owner designation is not a private author ID.

### 3.4 Verified current codec and access evidence

At the checked base commit, [`hash.rs`](../../crates/org-work/src/hash.rs) computes the command receipt digest as **SHA-256(method UTF-8 + NUL + bound member ID UTF-8 + NUL + canonical typed-params UTF-8)**. Params are decoded to their actual Rust type and re-encoded before hashing; unknown ignored input fields cannot change the request fingerprint. Canonical JSON recursively sorts object keys by their UTF-8 bytes, preserves array order, emits compact separators and uses the existing serde_json scalar encoding. Its pinned test vectors are in [`proto-fingerprints.json`](../../crates/org-proto/test-vectors/v2/proto-fingerprints.json). This is a compatibility/security contract; replacing old entity generation with UUIDv7 does not change old command IDs or digests.

[`engine.rs`](../../crates/org-work/src/engine.rs) first enforces app-only and admitted-caller checks. An app caller may act only as a valid person/owner; another connection only as an active/paused agent, never owner/person. Then it looks up the change receipt before age/current-state validation: same ID/hash replays, changed params refuse. The old command ID remains a ULID, maximum age 29 days, future tolerance five minutes and receipt retention 30 days from its encoded time. Existing field-aspect conflicts retain their method semantics; target assignment-pair exact-revision validation is a separate adaptation. Keep canonical unknown-value decoding, request/frame bounds and stored failure behavior as implemented rather than silently recalculating old receipts. The earlier permissive binding residual risk remains in ADR-0003; this document makes no security-identity or key-namespace changes.

Target task-event hashing is **different**: document 05 hashes UTF-8 event JSON with sorted keys, compact separators, no ASCII escaping, explicit optional nulls, `{}` source metadata and checksum omitted. A replay compares actual canonical payload hash before returning an identical event; same identity/different payload conflicts. Validate contiguous revisions from a known baseline and final snapshot/tombstone agreement. Import keeps the entire original WorkEvent including auto/cause/org_position under `source_metadata.legacy_event`, retains its original change payload, actor/time/version and maps bug/request entity kind to task. Historical individual-only assignment events never infer past team assignments from today's home membership. Preserve unselected workflow evidence in staging. When only a snapshot survives, create an explicit history_baseline at its current revision and identify earlier history as unavailable.

### 3.5 Private-note storage and recoverable publication

The selected private service is a **single accepting HQ service**, with authoritative SQLCipher database at `<data-root>/<organization-id>/durable/private/task-notes.db`, service-private WAL/backups and existing host key custody. Other offices call as the bound author; no shared replication/Git path exists. This data/control contract is selected; the service/API/access controls/UI remain unimplemented. If HQ is unavailable, private edits cannot be acknowledged. Recovery needs fencing before another host accepts writes.

Reads, lists, author-private search, edits and deletion scope by organization **and authenticated author tuple**, plus task/note where applicable. Employee authors use stable employee ID; owner authors use stable authenticated subject, never the literal role name. Trusted mirrors are installed from admitted identities, not caller claims. Unauthorized/missing direct IDs have the same not-found response. Creation also checks task access. Existing notes remain author-readable after task archive/deletion, subject to authentication; assignment/leadership/admin roles never grant access.

Successful create/edit/delete and author-scoped operation/digest receipt commit in one private transaction. Check receipt before current revision: retries return original accepted ID/revision even after later edits; changed payload conflicts. Use stable note ID, exact expected revision and immutable task/author identity. No shared transaction is claimed across databases. Explicit Publish persists a private pending receipt with selected revision and fresh shared comment ID, blocks note edits/deletion, then uses that comment ID as shared idempotency key. An unknown response remains pending for query/retry; only known completion or definitive rejection releases the guard. Shared history receives the intentionally published comment/normal actor/time, never private note IDs or provenance. Publishing still requires ordinary task-post authorization and leaves the original private.

Deletion clears body and leaves private identity/revision/time tombstone; there is no body version archive or resurrection of a deleted ID. Task deletion never cascades private content away. Encrypted backups and authorized key recovery stay separate; restore must reconcile later deletions before serving reads and retain tombstones for the backup window. Missing deletion evidence/key blocks recovery instead of exposing an older deleted body. Define/enforce a maximum private-backup retention period before enabling the feature. Logical deletion does not promise immediate physical erasure from WAL/media/backups. These details implement PN-01–06's required boundary; the schema alone does not prove privacy or recovery.

## 4. The contract: `org_proto::work`

**Pre-migration W7 reference, 2026-11-08, retained for codec and security review.** This section reproduces the earlier wire/design reference, including proposed later phases; it is not a new target contract or an implementation inventory. The checked source at `c6e520a36c4dc7c738c68915fac5e7d5d1cbe804` is authoritative for actual signatures and availability. In particular old ULIDs/change IDs, B-/REQ-/FEAT- references, per-kind enums, optional task ownership, office ownership, areas, code-repository docs, automatic transitions and numbering must not be used as selected-target examples. See §3 for the required mapping. Do not change old codecs or required security identity namespaces through this reconciliation.

The `org-docs` ports, docs branch/intent algorithms and later-phase doc calls below are retained design requirements, not evidence that those services are installed. Their path and scope assumptions require a new organization-repository adapter before use against the target. References to §1/§2 describe the historical W7 concepts where they conflict with the active sections above; obsolete detailed plans are available in the [base document](https://github.com/vgmello/agentic-org/blob/c6e520a36c4dc7c738c68915fac5e7d5d1cbe804/docs/design/tasks-projects-docs.md).

A new module, `crates/org-proto/src/work.rs`, re-exported like `org`. Organization commands for projects and areas extend `org_proto::org` (§4.8). `API_VERSION` stays 1: everything is additive.

### 4.1 Conventions
- Calls go to the client's **own office** socket (ADR-0003 §2.1), on a bound connection (§2.2).
- Every write's params start with **`change_id: String`** (a ULID, §2.3), and its result is **`WorkAccepted`**:
```rust
pub struct WorkAccepted {
    pub status: WorkStatus,                 // committed | queued (phase 2) | unknown
    /// Set when committed: the client feed position of this change.
    pub position: Option<WorkPosition>,
    /// Every item and feature this call changed, whole, after the change: the item itself, items
    /// it created (request.approve, checklist.make_task) and automatic changes. Empty when queued.
    pub items: Vec<WorkItemSummary>,
    pub features: Vec<Feature>,
    /// Doc commits the call made before answering (feature.create).
    pub docs: Vec<DocWritten>,
}
pub struct DocWritten { pub project_id: String, pub path: String, pub branch: String, pub commit: String }
pub enum WorkStatus { Committed, Queued, #[serde(other)] Unknown }
/// A position in this office's work feed. `incarnation` changes if the office's work data is
/// restored or re-created; a client that sees another incarnation takes a fresh snapshot.
pub struct WorkPosition { pub incarnation: u64, pub seq: u64 }   // Copy, so SubscribeParams stays Copy
```
- Item references in params: `item_id` where the app writes, `item` (an id, a qualified number or a short id, §1.1) where an agent may type it; the CLI resolves short refs with `work.get` first and then writes by id.
- Times are RFC 3339 UTC with milliseconds (`2026-11-08T02:10:00.000Z`).
- **Size.** Every request and response is one frame (`MAX_FRAME_BYTES`, 1 MiB, measured on the JSON line; ADR-0003 §4.1).
  - A write whose encoded params exceed 768 KiB is refused with `too_large` (e.g. a `request.approve` with 50 long tasks).
  - Paged reads treat `limit` as a maximum and stop early so the encoded response stays under 768 KiB, returning `next_cursor` (`work.snapshot`, `work.list`, `work.activity`, `doc.list`, `doc.history`).
  - `work.get` returns at most 200 backlinks and 200 watchers, with `backlinks_more` / `watchers_more` flags; `work.list` (by link target) reaches the rest.
  - The limits keep one item's `work.get` under 768 KiB and one event under 256 KiB (`Created` carries no fields: the item's state comes with the change as an `ItemUpserted`; the largest event is a `request.scope` with its outcome, ≤ 128 KiB).
  - A `work.changed` event is at most 512 KiB: a call whose changes don't fit (an offboarding cascade over many items, `request.approve`) is sent as several `work.changed` events with the same `change_id`, the same position and `part` / `parts` numbers. A client advances its saved position only after the last part; resuming from a position re-sends every part of the calls after it.
- Enums decode unknown values as `Unknown` (as `org.rs` does); writers never send `Unknown`.

### 4.2 Types
```rust
pub enum ItemKind { Task, Bug, Request, Unknown }
pub enum ItemStatus {                       // valid values per kind: §1.10
    Todo, InProgress, Blocked, Done,        // task
    New, Triaged, Fixed, Closed,            // bug (+ InProgress)
    Routed, Scoping, Scoped, Approved, Declined, Merged, // request (+ New)
    Unknown,
}
pub enum Resolution { Completed, Cancelled, Verified, Duplicate, WontFix, CantReproduce, Unknown }
pub enum Priority { Urgent, High, Normal, Low, Unknown }
pub enum Severity { Critical, Major, Minor, Trivial, Unknown }
pub enum RequestKind { Feature, Change, Technical, Question, Unknown }
pub enum RoutedBy { Person, Rule, Copilot, Unknown }
pub enum LinkKind { Fixes, PartOf, FromRequest, ReportedFrom, RelatesTo, PaysBack, Unknown }
pub enum LinkTarget { Item { item_id: String }, Doc { project_id: String, doc_key: String }, Unknown } // tagged "type"
pub enum FeatureStatus { Proposed, Planned, InProgress, Delivered, Retired, Unknown }

pub struct ChecklistCounts { pub total: u32, pub done: u32, pub required_left: u32 }

/// What lists, boards and live updates need. No long text.
pub struct WorkItemSummary {
    pub id: String, pub kind: ItemKind,
    pub project_id: Option<String>, pub number: Option<u64>, pub short_id: String,
    pub title: String, pub status: ItemStatus, pub resolution: Option<Resolution>,
    pub priority: Option<Priority>,                 // tasks and bugs
    pub assignee_id: Option<String>, pub created_by: String,
    pub created_at: String, pub updated_at: String,
    pub team_id: Option<String>,                    // tasks
    pub feature_id: Option<String>,                 // tasks (P3)
    pub blocked_reason: Option<String>,             // tasks
    pub area_id: Option<String>,                    // bugs
    pub severity: Option<Severity>, pub severity_confirmed: bool, // bugs
    pub reopen_count: u32,                          // bugs
    pub duplicate_of: Option<String>,               // bugs
    pub request_kind: Option<RequestKind>,          // requests
    pub fixes: Vec<String>,                         // task: the bugs it fixes
    pub fixed_by: Vec<String>,                      // bug: its fix tasks
    pub checklist: ChecklistCounts,
    pub done_anyway: bool,
    pub owner_office_id: String, pub generation: u64, pub version: u64,
    pub deleted: bool,
}

/// One item in full (work.get).
pub struct WorkItem {
    pub summary: WorkItemSummary,
    pub backlinks_more: bool, pub watchers_more: bool,   // more than 200 (§4.1)
    pub description: Option<String>, pub definition_of_done: Option<String>, // task
    pub branch: Option<String>, pub done_anyway_detail: Option<DoneAnyway>,   // task
    pub shortcut: Option<Shortcut>,                                           // task, P4
    pub what_happened: Option<String>, pub expected: Option<String>,
    pub steps: Option<String>, pub environment: Option<String>,
    pub closed_reason: Option<String>, pub affects_feature_id: Option<String>, // bug
    pub text: Option<String>, pub route_reason: Option<String>,
    pub routed_by: Option<RoutedBy>, pub outcome: Option<RequestOutcome>,
    pub merged_into: Option<String>, pub decline_reason: Option<String>,      // request
    pub checklist: Vec<ChecklistStep>,
    pub links: Vec<Link>, pub backlinks: Vec<Link>,
    pub watchers: Vec<String>,
    pub time: TimeFigures,                                                    // §6
    pub suggested_triager: Option<String>, pub suggested_verifier: Option<String>, // bug, derived
}
pub struct DoneAnyway { pub reason: String, pub steps: Vec<String> }
pub enum Shortcut { None, Debt { ids: Vec<String> }, Unknown }   // tagged "type": {"type": "debt", "ids": ["DEBT-0012"]}
pub struct ChecklistStep {
    pub id: String, pub text: String, pub required: bool, pub done: bool,
    pub task_id: Option<String>,
    pub checked_by: Option<String>, pub checked_at: Option<String>, // from the latest check event
    pub version: u64,
}
pub struct Link { pub id: String, pub from_item_id: String, pub kind: LinkKind, pub to: LinkTarget,
                  pub created_by: String, pub created_at: String, pub target_deleted: bool }
/// §1.5's `Outcome`, named `RequestOutcome` in Rust; the wire is the same.
pub struct RequestOutcome { pub feature: Option<FeatureOutcome>, pub docs: Vec<DocRef>,
                     pub draft_tasks: Vec<DraftTask>, pub estimate: Option<Estimate> }
pub enum FeatureOutcome { New { feature_id: String }, Extends { feature_id: String }, Unknown } // tagged "type"
pub struct DocRef { pub project_id: String, pub doc_key: String }
pub struct DraftTask { pub draft_id: String, pub title: String, pub description: Option<String>,
                       pub assignee_id: Option<String>, pub team_id: Option<String>,
                       pub priority: Option<Priority> }
pub struct Estimate { pub effort: Option<String>, pub cost_usd_cents_min: Option<u64>, pub cost_usd_cents_max: Option<u64> }
pub struct Feature { pub id: String, pub project_id: String, pub number: u32, pub path: String,
                     pub title: String, pub status: FeatureStatus, pub delivered_at: Option<String>,
                     pub retired_reason: Option<String>, pub replaced_by: Option<String>,
                     pub tasks_total: u32, pub tasks_done: u32, pub version: u64 }

/// One event: a committed change to one item or feature (the activity feed).
pub struct WorkEvent {
    pub target_id: String, pub version: u64, pub at: String,
    pub actor_id: Option<String>,           // None: automatic or a system cascade
    pub auto: bool,
    pub cause: Option<EventRef>,            // the triggering or undone event
    pub org_position: Position,
    pub change: WorkEventKind,
}
pub struct EventRef { pub target_id: String, pub version: u64 }
/// Tagged "type"; unknown kinds decode as Unknown (show "changed").
pub enum WorkEventKind {
    Created,
    FieldsChanged { fields: Vec<FieldChange> },         // { field, from?, to? } as JSON values
    StatusChanged { from: ItemStatus, to: ItemStatus, resolution: Option<Resolution>, reason: Option<String>,
                    anyway: Option<DoneAnyway>,     // Done or Fixed with required steps left
                    duplicate_of: Option<String> }, // (amended 2026-11-08: P2) a bug closed as a duplicate: the
                                                    // original's id; absent otherwise. `reason` is free text, never an id (events from before have the id there).
                                                    // The reopen of a duplicate is `Reopened`; its item state has it cleared.
    Assigned { from: Option<String>, to: Option<String> },
    Triaged { area_id: String, severity: Severity, priority: Priority, assignee_id: Option<String> },
    Verified { outcome: VerifyOutcome, by_reporter: bool, note: Option<String> },
    Reopened { note: Option<String> },
    Routed { from: ItemStatus, to: ItemStatus, scoper_from: Option<String>, scoper_to: Option<String>,
             reason: Option<String>, routed_by: Option<RoutedBy> },  // (amended 2026-11-09: request events)
                                                    // request.route (unroute: scoper_to None, to: new) and request.accept; aspects: status, assignee_id (the scoper), route_reason, routed_by (§4.7)
    Linked { link: Link }, Unlinked { link_id: String },
    Comment { comment_id: String, body: String, comment_kind: CommentKind,
              asks: Option<String>, answers: Option<String>,
              watch: bool },                      // the commenter also started watching (work.comment { watch })
    StepsAdded { steps: Vec<ChecklistStep>, after_step_id: Option<String> }, StepEdited { step_id: String, text: Option<String>, required: Option<bool> },
    StepChecked { step_id: String, done: bool }, StepMoved { step_id: String, after_step_id: Option<String> },
    StepRemoved { step_id: String }, StepMadeTask { step_id: String, task_id: String },
    StepTaskUnlinked { step_id: String, task_id: String },  // checklist.unlink_task, or its task deleted (an event of
                                                            // the work.delete call on the step's item, same actor): a plain step again
    WaitingStarted { on_member_id: String, reason: WaitReason },   // office bookkeeping (§2.3, §6)
    WaitingEnded { on_member_id: String, reason: WaitReason },
    WorkingStarted { session_id: String }, WorkingStopped { session_id: String },  // §6
    Watched { member_id: String, watching: bool },
    DebtStatusChanged { doc_key: String, to: String },   // P4, on the triggering task
    Undone { event_version: u64 },
    Deleted,
    HandedOver { from_office_id: String, to_office_id: String }, Reclaimed { to_office_id: String }, // phase 2
    // A number is not an event: HQ's ItemNumbered arrives as WorkChange::ItemNumbered (§2.6).
    Unknown,
}
pub enum VerifyOutcome { Fixed, StillHappens, Unknown }
pub enum CommentKind { Note, Question, Answer, Unknown }
/// Why an item waits on the owner (§6). Tagged "type"; a span is keyed by (on_member_id, reason).
pub enum WaitReason {
    Question { comment_id: String },        // work.comment { question, asks: owner }
    Message { message_id: String },         // msg.ask / msg.send to the owner with meta.work_item
    Triage,                                 // a New bug whose suggested triager is the owner
    Decision { doc_key: String },           // a linked or related ADR in `proposed`
    Unknown,
}

pub struct TimeFigures {                   // §6; all durations in milliseconds
    pub lead_ms: u64, pub cycle_ms: Option<u64>,
    pub waiting_on_owner_ms: u64, pub blocked_ms: u64,
    pub rest_ms: u64,                      // lead − waiting − blocked: working and idle together
    /// Both Some only when working time was tracked for the item's whole lead time (§6);
    /// then working_ms + idle_ms = rest_ms. None: "working time isn't tracked yet".
    pub working_ms: Option<u64>, pub idle_ms: Option<u64>,
    pub open: bool,                        // "so far"
    pub rounds: u32,                       // 1 + reopens
    pub as_of: String,
}
```
Rust details (derives, `#[serde(default)]` and `skip_serializing_if` on every optional field, `#[non_exhaustive]`, the custom `Deserialize` that never fails on unknown enum values) follow `org.rs`. The data-carrying enums `LinkTarget`, `Shortcut`, `FeatureOutcome` (and `WaitReason`) are tagged `type` with an `Unknown` variant (`#[serde(other)]`), so an unknown `type` decodes as `Unknown` instead of failing the message. **The record structs carry every phase's fields from P1** (`WorkItemSummary`, `WorkItem`, `Feature`, the call params), so later phases add calls and event kinds, not fields (the plan items of §7.1 are the exception: each adds its fields additively, with `#[serde(default)]`, in its phase); a P1 server refuses a later-phase field in a write's params with `invalid_request` (e.g. `task.create { feature_id }` or `{ fixes }`, `task.set_status { shortcut }`) until its phase lands. The P1 contract on `ada/proto-work-p1` (`495ccff`) predates rev 2's §6: its `TimeFigures` (`idle_ms: u64`, no `rest_ms`) and `WaitingStarted` / `WaitingEnded` (`reference`, no `reason`) change to the shapes above before it merges. Limits are public constants: `WORK_TITLE_MAX_CHARS = 120`, `WORK_TEXT_MAX_CHARS = 10_000`, `WORK_DESCRIPTION_MAX_CHARS = 20_000`, `WORK_REQUEST_TEXT_MAX_CHARS = 20_000`, `WORK_REASON_MAX_CHARS = 1_000`, `WORK_BLOCKED_REASON_MAX_CHARS = 200`, `WORK_DOD_MAX_CHARS = 1_000`, `WORK_ENVIRONMENT_MAX_CHARS = 2_000`, `WORK_STEP_TEXT_MAX_CHARS = 200`, `WORK_MAX_STEPS = 100`, `WORK_MAX_LINKS = 100`, `WORK_MAX_DRAFT_TASKS = 50`, `WORK_PAGE_MAX = 500`, `AREA_NAME_MAX_CHARS = 40`, `PROJECT_KEY_MAX_CHARS = 10`.

### 4.3 Reads
| Call | Params | Result | Errors | Phase |
| --- | --- | --- | --- | --- |
| `work.snapshot` | `project_id?`, `closed_since?` (closed/done items whose last terminal event is at or after it), `all_closed?: bool`, `cursor?`, `limit?` (≤ 500, default 500) | `{ position: WorkPosition, items: Vec<WorkItemSummary>, features: Vec<Feature>, next_cursor? }` | `not_authorized`, `not_found` (project), `invalid_request` | P1 (features P3) |
| `work.get` | `item` | `{ item: WorkItem }` | `not_authorized`, `not_found`, `invalid_request` (ambiguous short id) | P1 |
| `work.activity` | `target_id`, `after_version?`, `limit?` (≤ 500) | `{ events: Vec<WorkEvent>, next_after_version? }` | `not_authorized`, `not_found` | P1 |
| `work.list` | `project_id?`, `kinds?`, `statuses?`, `assignee?` (id, handle or `none`), `created_by?`, `area_id?`, `feature_id?`, `linked_to?` (an item id: the items with a link to it, i.e. its backlinks, including those past `work.get`'s 200), `link_kind?` (with `linked_to`: only links of this kind), `query?` (words in the title, ≤ 200 chars), `include_closed?: bool`, `cursor?`, `limit?` (≤ 500) | `{ items: Vec<WorkItemSummary>, next_cursor? }` | `not_authorized`, `not_found`, `invalid_request` | P1 (the CLI, "similar open bugs") |
| `feature.list` | `project_id?`, `status?` | `{ features: Vec<Feature> }` | `not_authorized`, `not_found` | P3 (the CLI resolves `FEAT-n` through it) |
| `work.times` | `item_ids` (≤ 500) | `{ times: Vec<{ item_id, time: TimeFigures }> }` | `not_authorized`, `invalid_request` | P1 (list columns, TIME-5) |

**Snapshot then subscribe:** `work.snapshot` pages are read independently; each page carries the position it was read at. The client subscribes from the **first** page's position (§4.6) and applies a change only if it is newer (`(generation, version)`) than the one it holds, so changes made between pages converge. Deleted items are left out of the snapshot; their deletion arrives as a change.

### 4.4 Tasks
| Call | Params (after `change_id`) | Errors (besides the common `not_authorized`, `expired`, `clock_skew`) | Phase |
| --- | --- | --- | --- |
| `task.create` | `item_id`, `project_id`, `title`, `description?`, `definition_of_done?`, `assignee_id?`, `team_id?`, `priority?`, `feature_id?` (P3), `fixes?: Vec<{ link_id, bug_id }>`, `checklist?: Vec<NewStep>` | `not_found` (project, bug, feature), `invalid_request` (limits; archived project; team not the project's; a bug in another project), `unknown_recipient`, `already_exists` (an id) | P1 (`fixes` P2) |
| `task.update` | `item_id`, `expected_version`, `title?`, `description?`, `clear_description?`, `definition_of_done?`, `clear_definition_of_done?`, `priority?`, `team_id?`, `clear_team?`, `feature_id?`, `clear_feature?` (P3) | `not_found`, `invalid_request` (nothing to change; a field with its `clear_`; limits), `conflict` | P1 |
| `task.assign` | `item_id`, `expected_version`, `assignee_id?` (absent: unassign) | `not_found`, `unknown_recipient`, `conflict` | P1 |
| `task.set_status` | `item_id`, `expected_version`, `status`, `blocked_reason?` (exactly when `blocked`), `resolution?` (only with `done`), `anyway_reason?` (only with `done`), `shortcut?: Shortcut` (only with `done`; P4) | `not_found`, `invalid_request` (wrong state; a reason missing or misplaced), `conflict`, `checklist_incomplete` (required steps left and no `anyway_reason`; the message names the steps) | P1 |
| `work.set_current` | `item?` (absent: clear) | `not_found`, `invalid_request` (not a task; the caller isn't an agent; the caller isn't the task's assignee) | P1 |

`NewStep { step_id, text, required }`. `work.set_current` sets the bound agent's current task (C14) in `member_status`; it isn't an item change and takes no `change_id`; its result is `Empty`. A Done task (any resolution) can stay current until replaced or cleared. Reassignment, deletion and offboarding do not publish presence in C2a: `who` checks validity; C2b will refresh live views.

### 4.5 Bugs (P2)
| Call | Params (after `change_id`) | Errors |
| --- | --- | --- |
| `bug.report` | `item_id`, `project_id`, `title`, `what_happened`, `severity`, `area_id?`, `expected?`, `steps?`, `environment?`, `from?: { link_id, to: LinkTarget }`, `affects_feature_id?` (P3), `checklist?` | `not_found`, `invalid_request` (limits; archived project; area not in the project or deleted), `already_exists` |
| `bug.update` | `item_id`, `expected_version`, `title?`, `what_happened?`, `expected?`, `clear_expected?`, `steps?`, `clear_steps?`, `environment?`, `clear_environment?`, `severity?`, `priority?`, `area_id?`, `affects_feature_id?`, `clear_affects_feature?` | `not_found`, `invalid_request`, `conflict` |
| `bug.triage` | `item_id`, `expected_version`, `area_id`, `severity`, `priority`, `assignee_id?` | `not_found`, `invalid_request` (wrong state; area), `unknown_recipient`, `conflict` |
| `bug.assign` | `item_id`, `expected_version`, `assignee_id?` | `not_found`, `invalid_request` (the bug is closed; amended 2026-11-08: P2), `unknown_recipient`, `conflict` |
| `bug.mark_fixed` | `item_id`, `expected_version`, `note`, `anyway_reason?` | `not_found`, `invalid_request`, `conflict`, `checklist_incomplete` |
| `bug.verify` | `item_id`, `expected_version`, `outcome: fixed \| still_happens`, `note?` (required for `still_happens`) | `not_found`, `invalid_request` (not `fixed`; note missing), `conflict` |
| `bug.close` | `item_id`, `expected_version`, one of `duplicate_of` or `{ resolution: wont_fix \| cant_reproduce, reason }` | `not_found` (the original), `invalid_request` (wrong state; the original in another project, itself, or closed as a duplicate; reason missing), `conflict` |
| `bug.reopen` | `item_id`, `expected_version`, `note` | `not_found`, `invalid_request` (not closed; note missing), `conflict` |

### 4.6 Shared item calls
| Call | Params (after `change_id`) | Errors | Phase |
| --- | --- | --- | --- |
| `work.comment` | `item_id`, `comment_id`, `body` (1–10,000), `comment_kind?` (`note` default), `asks?` (member: a `question` addressed to them), `answers?` (a question's `comment_id`), `watch?: bool` | `not_found` (item, the question), `invalid_request` (`asks` without `question`; `answers` without `answer`), `unknown_recipient`, `already_exists` | P1 |
| `work.watch` | `item_id`, `watching: bool` | `not_found` | P1 |
| `work.link` | `link_id`, `from_item_id`, `kind` (`fixes`, `relates_to`, `pays_back`), `to` | `not_found`, `invalid_request` (the kind's rule, §1.7; too many links; linking an item to itself), `already_exists` (the id, or the same link) | P1 (`fixes` P2, `pays_back` P4) |
| `work.unlink` | `link_id` | `not_found`, `invalid_request` (`from_request` and `part_of` links can't be removed this way: `checklist.unlink_task` does `part_of`) | P1 |
| `work.undo` | `target_id`, `event_version` | `not_found`, `invalid_request` (not automatic; older than 1 hour; already undone), `conflict` | P1 |
| `work.delete` | `item_id`, `expected_version` | `not_authorized` (app only), `not_found`, `conflict` | P1 |

A `question` with `asks` = the owner starts a **waiting on the owner** span on the item (§6), and its `answer` ends it. `answers` must name a question on the same item. Likewise `work.link` of a `relates_to` link to an ADR in `proposed` starts a decision span, and `work.unlink` ends it unless the ADR's `related` still names the item (§6). These spans are bookkeeping events in the call's transaction (§2.3).

### 4.7 Checklists, requests and features
**Checklists** (P1; tasks and bugs):
| Call | Params (after `change_id`) | Errors |
| --- | --- | --- |
| `checklist.add` | `item_id`, `steps: Vec<NewStep>` (1–100), `after_step_id?` (absent: at the end) | `not_found`, `invalid_request` (a request; over 100 steps; limits), `already_exists` |
| `checklist.edit` | `step_id`, `expected_version` (the step's), `text?`, `required?` | `not_found`, `invalid_request`, `conflict` |
| `checklist.check` | `step_id`, `done: bool` | `not_found`, `invalid_request` (a made-task step: it follows its task) |
| `checklist.move` | `step_id`, `after_step_id?` (absent: to the top) | `not_found`, `invalid_request` (another item's step) |
| `checklist.remove` | `step_id`, `expected_version` | `not_found`, `conflict` |
| `checklist.make_task` | `step_id`, `task_id`, `link_id`, `title?` (default: the step's text) | `not_found`, `invalid_request` (already made a task; the item isn't in a project), `already_exists` |
| `checklist.unlink_task` | `step_id` | `not_found`, `invalid_request` (no task) |

`checklist.make_task` creates a task in the same project (and the same `team_id` when the item is a task), with a `part_of` link to the item; the step then shows the task. `checklist.check` records an event only when the state changes, so it's idempotent. A step's version goes up on edit, check, move and make-task.

**Requests** (P3):
| Call | Params (after `change_id`) | Errors |
| --- | --- | --- |
| `request.create` | `item_id`, `text`, `title?` (default: the text's first line, cut to 120), `request_kind?`, `project_id?`, `scoper_id?`, `route_reason?`, `routed_by?` | `not_found`, `invalid_request`, `unknown_recipient`, `already_exists` |
| `request.update` | `item_id`, `expected_version`, `title?`, `request_kind?`, `project_id?`, `clear_project?` | `not_found`, `invalid_request` (terminal state), `conflict` |
| `request.route` | `item_id`, `expected_version`, `scoper_id?` (absent: unrouted), `route_reason?`, `routed_by?` | `not_found`, `invalid_request` (wrong state), `unknown_recipient`, `conflict` |
| `request.accept` | `item_id`, `expected_version` | `not_found`, `invalid_request`, `conflict` |
| `request.scope` | `item_id`, `expected_version`, `outcome: RequestOutcome` | `not_found` (feature, doc's project), `invalid_request` (wrong state; a draft over limits; more than 50 drafts), `conflict` |
| `request.ask_changes` | `item_id`, `expected_version`, `comment_id` (an existing comment on this request, made by `work.comment` first), `note` | `not_found` (also: no such comment on this request), `invalid_request`, `conflict` |
| `request.approve` (**app only**) | `item_id`, `expected_version`, `tasks: Vec<ApprovedTask>` (1–50) | `not_authorized`, `not_found`, `invalid_request` (wrong state; no project; a task over limits), `unknown_recipient`, `conflict`, `already_exists` |
| `request.decline` | `item_id`, `expected_version`, `reason` | `not_found`, `invalid_request`, `conflict` |
| `request.merge` | `item_id`, `expected_version`, `into` (a request id), `comment_id` | `not_found`, `invalid_request` (itself; `into` not open), `conflict` |

Each request call records one event on the request (§2.3) (amended 2026-11-09: request events):
- `request.create`: `Created`; the initial routing is part of the created item's state.
- `request.route` and `request.accept`: `Routed { from, to, scoper_from?, scoper_to?, reason?, routed_by? }`. A route or re-route has `to: routed`; an unroute has no `scoper_to` and `to: new`; an accept has `scoper_to` = the caller and `to: scoping`.
- `request.update`: `FieldsChanged`.
- `request.ask_changes`: `StatusChanged { from: scoped, to: scoping, reason: note }`, and nothing else. The comment it names was already recorded by its own `work.comment` call (its own `Comment` event); `ask_changes` doesn't record it again.
- `request.scope`, `request.approve`, `request.decline`, `request.merge`: `StatusChanged`, with `resolution` / `reason` where relevant.
- For aspects (§2.3), `Routed` counts as writing `status`, `assignee_id` (the scoper), `route_reason` and `routed_by`, even when `from == to` or the scoper is unchanged; it's never a no-op. For cycle time (§6), a request's round starts at its first move into `scoping`, normally an accept's `Routed`.

`ApprovedTask { item_id, link_id, title, description?, definition_of_done?, assignee_id?, team_id?, priority?, checklist? }`: the drafts as the owner edited them (RQS-9). `request.approve` creates them all or none, each with a `from_request` link and, when the outcome names a feature, its `feature_id`; the feature becomes `planned`.

**Features** (P3; records at HQ):
| Call | Params (after `change_id`) | Errors |
| --- | --- | --- |
| `feature.create` | `feature_id`, `project_id`, `title`, `body?` (the sections, Markdown), `fields?` (front matter beyond the generated ones) | `not_found`, `invalid_request` (archived; limits; the template refuses a field), `unavailable` (no repository; repository busy), `already_exists` |
| `feature.set_status` | `feature_id`, `expected_version`, `status: planned \| retired`, `reason?` (required for `retired`), `replaced_by?`, `close_open_tasks?: bool` | `not_authorized` (`planned` and `close_open_tasks` are app only), `not_found`, `invalid_request`, `conflict` |
| `feature.adopt` | `feature_id`, `project_id`, `path` | `not_found` (no such doc on the integration branch), `invalid_request` (not a feature doc), `already_exists` (the path, or its FEAT number, already has a record) |

`feature.create` is synchronous and follows §4.9's two-step intent: the first step reserves the FEAT number and the branch `docs/features-<n>-<short id>`; the office builds the commit of `docs/features/<n>-<slug>.md` from the template; the second step records it and moves the branch; then the record is committed as `proposed` and the answer carries the commit in `docs`. A retry with the same `change_id` reuses the reserved number and the built commit and never writes twice. `feature.adopt` makes a record for a feature doc written outside the tool (with git), reading its number, title and status. Linking a task to a feature is `task.update { feature_id }` (FTR-4: at most one).

### 4.8 Organization commands: projects and areas (`org_proto::org`)
Same style as T1: params start with `command_id`, the result is `CommandAccepted`, events are `org.changed`, **all app-only**, HQ's error order (`not_a_member` and `owner_protected`, which `org.rs`'s list doesn't place, come after `invalid_name` and before `conflict`). The full project record replaces `ProjectSummary` additively: `ProjectSummary` gains the fields of §1.9 with `#[serde(default)]` (on the wire, old readers ignore them) and is renamed `Project`, with `pub type ProjectSummary = Project;`. New fields break in-repo struct literals (server-dev's code) since the struct isn't `#[non_exhaustive]`; that's acceptable while version 1 is unreleased (`lib.rs`), and `Project` is marked `#[non_exhaustive]` from now on. `Project.created_at` is optional on read (`Option`, `#[serde(default)]`): records from before it existed have none; HQ always sets it on new projects. `OrgSnapshot` gains `areas: Vec<Area>` (`#[serde(default)]`). `OrgChange` gains `ProjectUpserted { project }` and `AreaUpserted { area }` (a deleted area is an upsert of its tombstone).

| Command | Params (after `command_id`) | Errors | Phase |
| --- | --- | --- | --- |
| `project.create` | `project_id`, `name`, `key?` (absent: HQ derives one, §below), `description?`, `teams?: Vec<{ project_team_id, team_id }>`, `general_area_id`, `general_area_name?` (default "General"), `repo?: RepoSpec`, `integration_branch?` | `not_found` (a team), `invalid_request` (key format or reserved; limits; a repeated team; repo spec; an `integration_branch` with a reserved prefix, amended 2026-11-08: proto follow-ups, `processes.md` "Project rule"), `invalid_name` (name, area name), `already_exists` (an id; the key; the name, ignoring case) | P1 |
| `project.update` | `project_id`, `expected_version`, `name?`, `description?`, `clear_description?`, `integration_branch?`, `clear_integration_branch?` | `not_found`, `invalid_request` (also an `integration_branch` with a reserved prefix, amended 2026-11-08: proto follow-ups), `invalid_name`, `conflict`, `already_exists` | P1 |
| `project.archive` | `project_id`, `expected_version`, `archived: bool` | `not_found`, `conflict` | P1 |
| `project.add_team` (exists) | unchanged | unchanged | — |
| `project.remove_team` (extended) | `project_team_id`, `expected_version`, `areas_to_team_id?` (§2.5) | adds `invalid_request` (missing or not a remaining team) | P1 |
| `area.create` | `area_id`, `project_id`, `name`, `team_id?` (required when the project has teams), `lead_member_id?` | `not_found`, `invalid_request` (team not the project's; team missing; offboarded lead), `invalid_name`, `not_a_member`, `owner_protected`, `already_exists` | P1 |
| `area.rename` | `area_id`, `expected_version`, `name` | `not_found`, `invalid_request` (deleted), `invalid_name`, `conflict`, `already_exists` | P1 |
| `area.set_team` | `area_id`, `expected_version`, `team_id?` (absent only when the project has no team) | `not_found`, `invalid_request`, `conflict` | P1 |
| `area.set_lead` | `area_id`, `expected_version`, `lead_member_id?` | `not_found`, `invalid_request` (offboarded), `not_a_member`, `owner_protected`, `conflict` | P1 |
| `area.delete` | `area_id`, `expected_version`, `move_open_to_area_id` | `not_found`, `invalid_request` (General; already deleted; the target is itself, deleted or in another project), `conflict` | P2 |

- `RepoSpec` (phase 1): `{ "new_local": {} }`: HQ creates a bare repository with an initial commit on `main` under HQ's workspace (`<workspace>/repos/<project_id>.git`) **before** the command, recording the intent first and reusing the folder on a retry, as the GitHub design does for repository creation; or `{ "existing_local": { "path": "<absolute path>" } }`: an existing repository on HQ's Mac, checked before the command (exists, is a git repository, has the default branch). G1 adds `new_github` and `existing_github` additively. HQ resolves the spec into the `ProjectRepo` the event carries; the client never sends a `ProjectRepo`.
- `project.create` makes General (`general_area_id`, `general: true`), owned by the first team in `teams` or by none, and the project-team links, in one `org.changed`.
- **Key derivation** when `key` is absent: the name's ASCII letters and digits, lowercased, first 3 (prefixed with `p` if that doesn't start with a letter, and padded with `x` to 2); if taken or reserved, a digit 2, 3, … is appended. The event carries the key; the app shows it before creating.
- `name` follows the organization-name rules and is unique among projects ignoring case. `description` ≤ 1,000 characters.

### 4.9 Docs (P3 for features, P4 for the rest)
Every doc write is a commit on a branch, never on the default or integration branch. The office:
- validates the path: relative, under `docs/`, ending in `.md`, ≤ 256 bytes; each component matches `[A-Za-z0-9._-]+`, doesn't start with `-`, isn't `.` or `..`, and isn't `.git` ignoring case; components starting with `.` are allowed only for the known folders `.templates` and `.guidance`. In the target tree (`ls-tree`), no entry along the path may be a symlink (mode 120000) or a submodule (160000), and a new path may not differ only in case from an existing one (macOS checkouts are case-insensitive). Content ≤ 512 KiB through the tool (bigger files are edited with git and still indexed). Branch names are checked with `git check-ref-format`. Every git command goes through `org-git`'s hardening (projects design);
- commits with the **owner's** name and address as author and adds the co-author trailer itself: for an agent `Co-authored-by: <agent name> <messaging_name>@agents.agentic-org.invalid`; for a person editing in the app `Co-authored-by: <name> <contact, if it is an email, else handle@people.agentic-org.invalid>`; none for the owner's own edits (`.invalid` is a reserved domain, so these addresses never deliver mail). `message` is one line and may not contain a trailer; in phase 1 the owner's commit identity is an organization setting (open question 4);
- takes `expected_blob` (the blob id the writer started from, or none for a new file) and refuses with `conflict` when the file on the target branch differs (DOC-5's Compare / Keep mine / Discard mine is the app's, on top of that);
- writes **only on branches it made**: `docs/<slug>-<short id>`, created from the integration branch (A36). It commits them in **`mirror.git`** (office-only; agents never write there) and then, as retried steps after the commit:
  - **pushes** each office-made `docs/…` branch (listed in `doc_branches`) from the mirror to the project's remote, like a task branch: never the default branch, never a force push (an extension of the projects design's push rule, A36). The mirror's fetch refspec never force-updates or prunes `refs/heads/docs/*`;
  - **syncs** it into `work.git` with the existing hardened fetch and a **non-force** refspec, so agents read it. The call answers success with `synced: false` until this is done. If the branch is checked out in an agent's worktree or an agent committed on it, the fetch fails rather than moving or dropping anything; the office marks the branch **diverged**, reports it, and its later writes for that doc go on a new branch. It never writes into an agent's worktree or on a `task/…` branch, so nothing an agent controls (untracked files, symlinks, a rebase in progress, its index) is in the way. **Task branches without the office:** for a doc on its own `task/…` branch (C18: debt travels with its code), the agent's CLI calls `doc.render`, which validates against the template, reserves the number and returns the path and content; the CLI, running as the agent, writes that file into the agent's current worktree, and the agent commits it with git like any other file. The office crosses no boundary into the worktree, and the index shows the doc from that branch's overlay. `doc.save { branch? }` names an unmerged branch the office made for that doc (`doc_branches`); absent, the office uses the doc's open office-made create, edit or rename branch if there is one (never its status branch), else a new one. So an agent filling a new feature doc keeps writing on the branch `feature.create` made;
- answers retries without trusting anything in the repository, with a **two-step intent** in `office.db`, keyed by `change_id` (or the effect id). **Step 1** records the reserved doc number (if any), the branch and the fixed author, committer and dates. The office then builds the commit objects from it, deterministically (`GIT_AUTHOR_*` and `GIT_COMMITTER_*` from the intent, never from config). **Step 2** records the base commit and the **new commit's SHA**. Only then does it `update-ref <branch> <sha> <base>`; then it records the result in `work_change_results` and the commit in `doc_commits`. A retry finds the result. While the first attempt is still running, a retry gets `unavailable`. After a crash, the office finishes every open intent **at start, before serving**: an intent without a SHA is rebuilt from step 1 (same number, same dates); with a SHA, if it is the branch tip or an ancestor of it, the write happened; if the tip is still the base, it runs `update-ref` again; otherwise it records `conflict`. Then it re-runs the push and the sync below. No message text is ever used to find a commit;
- **reads only in `mirror.git`**: task branches are copied into a private namespace there (`refs/office/task/*`) by the projects design's hardened fetch from `work.git`, run in `mirror.git`, so `work.git`'s config never applies to a read;
- runs **every** git command, reads included (`doc.history`, index rebuilds, the overlay of `task/…` branches), through one hardened `org-git` entry point: `GIT_CONFIG_GLOBAL=/dev/null`, `GIT_CONFIG_NOSYSTEM=1`, `-c core.hooksPath=/dev/null -c core.fsmonitor=false -c log.showSignature=false -c log.mailmap=false`, `--no-mailmap`, `--no-ext-diff`, `--no-textconv`, fully qualified `refs/heads/…` and `--end-of-options` before refs and paths;
- builds commits with **plumbing, never porcelain**: `hash-object -w --no-filters`, a temporary index (`GIT_INDEX_FILE`) read from the branch tip, `update-index --cacheinfo`, `write-tree`, `commit-tree`, and `update-ref <branch> <new> <old>` (compare-and-swap), with the hardening plus `-c core.hooksPath=/dev/null -c core.fsmonitor=false -c commit.gpgsign=false -c core.attributesFile=/dev/null`. So no filter, hook, fsmonitor or signing program from any repository config ever runs, and no token is ever present on a commit (projects design). Contention on a lock file is `unavailable`;
- reuses **one status branch per doc** for status changes (a feature's Planned → In progress → Delivered, an Undo, a debt's Planned → Paid), each commit on top of the last (`expected_blob` from that branch), so they never conflict with each other. While the branch that created or renamed the doc is an unmerged **office-made** branch (e.g. `docs/features-<n>-<short id>` right after scoping, or a `doc.rename` branch), status commits go on top of it; once the doc is on the integration branch, they go on a status branch `docs/<id>-status-<short id>` (the doc's id, lowercased, and the short id of the change that made the branch), made from the integration branch and reused while it is unmerged and not diverged; after it merges or diverges, the next status change makes a new one. If the doc exists only on an agent's unmerged `task/…` branch (a debt doc, C18), the office never writes there: the status write stays pending, shown as "Planned once merged" (or "Paid once merged"), until the doc reaches the integration branch ("status not written to the doc yet" is only for repeated write failures, §3.2);
- leaves **merging** to the project's merge flow, which `projects-git-github.md` leaves for later. Until it's designed, a person merges docs branches into the integration branch (for us: the review, the go, the merge into `v1`). That is a dependency of P3 (open question 10). Whatever the flow, it merges `docs/…` branches with real merges (no squash or rebase), so the office's commit SHAs in `doc_commits` survive and office-set values don't read "from git";
- serializes git work per repository (one operation at a time) and answers `unavailable` ("repository busy") after a short retry.

The index (`doc_index`) is rebuilt from the integration branch after each fetch or merge, plus an overlay for each open `docs/…` and `task/…` branch the office knows ("Waiting to merge"). Doc numbers (ADR, FEAT, DEBT, LIM, EXC, EVAL) are the highest number of that kind in the index (integration branch and overlays) or in `doc_numbers`, plus one, not counting the v0 ADRs (`docs/adr/v0/`, their own series, §7.1), reserved in `doc_numbers` in the same transaction as the request; a clash from outside the tool is shown, not fixed (DEC-7), and the clash check also leaves the v0 series out. ADR numbering next to the v0 ADRs is a P4 plan item (§7.1).

**Stable doc ids** (agreed with Eden, §9). Every doc has an `id` in its front matter, unique in its project and never changed:
- **Numbered kinds** (ADR, FEAT, DEBT, LIM, EXC, EVAL) use their number as their id (`ADR-0004`). For a numbered doc without front matter (our `docs/adr/0001-…md` today starts with `# ADR-0001 · …` and a Status line), the index derives the id from the number in its file name at read time (the heading as fallback; a mismatch is a problem, DEC-7), so links to it are by id before any save, and its first save writes that id. Its status is read only from front matter, so our ADRs are migrated to front matter by the P4 import (§7.1). A numbered doc under `docs/adr/v0/` belongs to the v0 series and its id is `ADR-v0-<n>` (§7.1). The one change to such an id is DEC-7's **Renumber** of a number used twice (`doc.rename { renumber: true }`), offered once both docs are on the integration branch (before that the app shows the clash with "Renumber after it merges"; v0 docs are never renumbered): it reserves the next number, writes the new `id`, records the old one in the doc's `former_ids` and renames the file. It does **not** rewrite references to the old id, since some of them meant the other doc: the app lists them so the user moves each one. A live id always wins over a `former_ids` alias, so the alias only resolves once no doc holds the old id. **Other kinds** (spec, design, plan, review, the brief, guidance files, a project's own kinds) get `id: doc-<ULID, lowercase>`. Templates (`docs/.templates/`, checklist templates included) are keyed by their kind or name and never get an id, since their front matter is a schema.
- **Assigned at creation** by the office: `doc.new`, `feature.create` and `doc.render` write it. A doc that has none (written with git, or from before ids, such as our specs and designs today) gets one on its **first save** through the tool: `doc.save`, `doc.set_status` and `doc.rename` add it in the commit they make. The id is reserved per (project, path) in `doc_ids` and recorded in the intent's step 1, like a doc number, so every branch that first saves the same doc writes the same id and their merges agree. A reservation is retired when an index rebuild finds its id on the integration branch at that path (or the doc's path there was renamed or deleted through the office). Before using a reservation, the office checks the **integration branch** (never the overlays): if a doc at another path there already carries that id, the file now at this path is another doc and gets a new id; and if the file at the path was deleted or replaced on the integration branch since the reservation's base commit, it is a new doc and gets a new id too; otherwise every branch reuses the reservation. `doc.save` refuses removing or changing an `id` (`invalid_request`).
- **Links are by id**: item links (`LinkTarget::Doc`, §1.7), relations in front matter (`refines`, `supersedes`, `related`, …) and the app's pickers use the id. The index resolves an id to the doc's current path, so a rename never breaks one. Two files with the same id (a copy made with git) are both shown with a problem and links resolve to the older one (DEC-7: shown, not fixed).
- **Path links are rewritten by `doc.rename`**: the rename always goes on a new docs branch made from the integration tip, and the doc must be there unchanged: if it has any unmerged office-made branch (its create branch, unmerged edits, or a status branch, a feature's included), or exists only on an agent's unmerged `task/…` branch, `doc.rename` is `conflict` ("a change to it is waiting to merge"). In the same commit it rewrites relative Markdown links and front-matter values that point at the old path in every other doc **as they are in that tip** (so nothing is reverted; links inside code blocks are left alone), except docs that have an unmerged office-made branch of their own: those are skipped and listed in the result (`skipped`), and their links resolve through `former_paths`, so the merges never conflict. It also appends the old path to the doc's own `former_paths` front matter, so the alias lives in the repository and the index rebuilds it (`doc_aliases`) once the rename merges. Links the rewrite can't reach (docs on other open branches, links added after the rename branch was made) are resolved by the index and the reader through `former_paths`; a path that still holds a live doc always wins over an alias. While a rename is unmerged, `doc.save` of the doc goes on the rename branch, and a `path` there resolves through `former_paths`. Anything left unresolved (for example, a doc without an id renamed with plain git) is reported as a problem.
- **Features are found by id**: the feature record's `path` is refreshed from the index, and status commits and `DocWrite` effects look the doc up by its `FEAT-n` id, so a renamed feature doc keeps its status writes. While two docs hold the same id (a clash before Renumber), they use the record's own `path` instead, and the refresh from the index is skipped. A Renumber of a feature doc also updates the `number` of the record whose `path` is that doc (no record changes if that copy has none) (a record change, with its event, in the call's record step), so the record keeps pointing at its own doc.

| Call | Params | Result | Errors |
| --- | --- | --- | --- |
| `doc.list` | `project_id`, `kind?`, `status?`, `include_pending?: bool`, `cursor?`, `limit?` (≤ 500) | `{ indexed_commit, docs: Vec<DocEntry>, next_cursor? }` | `not_found`, `unavailable` |
| `doc.get` | `project_id`, `path` or `doc_key`, `branch?` (checked with `git check-ref-format`), `commit?` (40 or 64 hex), `offset?` (bytes) | `{ path, content (cut on a character boundary so its JSON encoding is ≤ 512 KiB), blob, commit, total_bytes, next_offset?, binary: bool, built_in: bool }` (`built_in`: a template path with no file, answered with the organization's default; `blob` and `commit` are empty) (a file that isn't valid UTF-8 has no `content` and `binary: true`; it's listed under Other files) | `not_found`, `invalid_request`, `unavailable` |
| `doc.history` | `project_id`, `path`, `cursor?` | `{ commits: Vec<DocCommit>, next_cursor? }` | `not_found`, `unavailable` |
| `doc.new` | `change_id`, `project_id`, `kind`, `title`, `fields?`, `body?` | `{ doc_key, path, branch, commit }` | `not_found` (project, kind), `invalid_request` (template validation), `unavailable` |
| `doc.save` | `change_id`, `project_id`, `path`, `content` (its JSON encoding ≤ 512 KiB), `expected_blob?`, `message?` (one line, ≤ 200), `branch?` (an unmerged office-made `docs/…` branch for this doc) | `{ path, branch, commit, blob, pushed: bool, synced: bool }` | `not_found`, `invalid_request` (path, size, front matter), `conflict`, `unavailable` |
| `doc.set_status` | `change_id`, `project_id`, `doc_key`, `status`, `reason?`, `fields?` | `{ path, branch, commit }` | `not_found`, `invalid_request` (not an allowed value), `conflict`, `unavailable` |
| `doc.rename` | `change_id`, `project_id`, `path`, `new_path?` (required unless `renumber`), `renumber?: bool` (DEC-7: the next number, a new `id` and file name), `expected_blob` | `{ doc_key, path, branch, commit, rewritten: Vec<path>, skipped: Vec<path> }` (the other docs whose links it rewrote, or left for `former_paths`) | `not_found`, `invalid_request` (the brief; `renumber` on an unnumbered kind or under `docs/adr/v0/`), `conflict` (also: a change to it is waiting to merge), `already_exists`, `unavailable` |
| `doc.delete` | `change_id`, `project_id`, `path`, `expected_blob` | `{ branch, commit }` | `not_found`, `invalid_request` (the brief), `conflict`, `unavailable` |
| `doc.render` | `change_id`, `project_id`, `kind`, `title`, `fields?`, `body?` | `{ doc_key, path, content, author (the owner's commit identity), commit_message (one line plus the agent's `Co-authored-by` trailer) }` (number reserved; nothing committed; content's JSON ≤ 512 KiB) | `not_found`, `invalid_request` (template validation), `unavailable` |
| `doc.schema` | `project_id`, `kind` | `{ kind, folder, fields: Vec<FieldSchema>, sections: Vec<SectionSchema> }` | `not_found` |
| `doc.validate` | `project_id`, `path` or `content`, `kind?` | `{ problems: Vec<{ field?, problem }> }` | `not_found`, `invalid_request` |

`DocEntry { path, kind, doc_key (the `id`; the path only while the doc has none), title, status?, fields (the front matter, only when its JSON is ≤ 4 KiB; else empty with `fields_truncated: true`, and `doc.get` has it all), last_commit: DocCommit, pending: Vec<{ branch, commit }>, problems: Vec<String> }`, `DocCommit { sha, at, author_name, agent_member_id?, trailer_coauthor?, message }`: `agent_member_id` is set only for commits the office made itself (it records their SHAs); for other commits the `Co-authored-by` trailer is reported as `trailer_coauthor` and shown as **from git**, since anyone with git can write one. Fields that record who decided or delivered (`decided`, `delivered`, `accepted`, `approved`, `taken.by`) are written only by the office (`doc.set_status`, the status effects, `doc.render` from the bound member); `doc.save` refuses a change to them (`invalid_request`). A value set by an office commit recorded in `doc_commits` is shown plainly; a value set by a commit made with git (a `doc.render` file included) is shown as **from git**, like a trailer: a quiet note beside the value, never a warning, because `debt add` on a task branch is the normal path (agreed with Eden, §9). Templates are docs (`docs/.templates/<kind>.md`), changed with `doc.save` on a docs branch and landed through the merge flow like any doc, by the app (TPL-4) or by an agent (`org-agent doc template show|edit`, §5); `doc.save` on a template path validates it as a template. The organization's built-in defaults live in `org-docs`, and `doc.get` of a template path that has no file returns the built-in default with `built_in: true`. The brief is `docs/brief.md` (C10): it can't be renamed or deleted. Front matter is a YAML subset (maps, sequences, strings, numbers, booleans, null; no anchors, aliases, tags or multi-document streams) with YAML 1.2 core-schema meanings (`yes`/`no` are strings), parsed with limits on depth (16) and size (64 KiB). The parser is a maintained crate at an exact pin (not the unmaintained `serde_yaml`), chosen by the `org-docs` owner in P3 and reviewed by me. The office validates every write against the kind's template; reads never fail on invalid front matter (DEC-7).

### 4.10 Events and the stream shapes
- **`work.changed`** (to subscribers): `{ position: WorkPosition, change_id?: String, part: u32, parts: u32, changes: Vec<WorkChange> }`, one per committed call (automatic changes included; split into parts when over 512 KiB, §4.1), where `WorkChange` (tagged `type`, unknown → `Unknown`: take a fresh snapshot) is `ItemUpserted { item: WorkItemSummary }`, `FeatureUpserted { feature }`, `EventAdded { event: WorkEvent }` (for live activity) and `ItemNumbered { item_id, number }` (phase 2).
- **`work.rejected`** (phase 2): `{ position, change_id, error: ApiError }`: a queued create or change request that the owner or HQ refused, so the app shows it as blocked.
- **`subscribe`** gains `from_work_position?: WorkPosition`: the office sends the work changes after it, then live ones. It answers `snapshot_required` if it can't (another incarnation, a position beyond its feed, or one older than the feed's 30-day retention). The error doesn't say which stream: a client that gave both positions takes both snapshots again.
- **The office-to-office shapes** (phase 2, in `org-sync-proto`, owned by ada; `org-work` keeps them as internal types until then): `WorkChange::ItemVersion { origin, org_position, item (whole), generation, version, events }`, `ChangeRequest { request_id, change_id, origin, org_position, target_id, expected_version, call (method + params) }`, `ChangeResult { request_id, change_id, result }`, `Handover { item_id, from_office, to_office, generation, version, change_results }`, `ItemNumbered { item_id, scope, number }` (HQ only), `FeatureVersion { feature (whole), version }`. Phase 1 writes only the reference rows (§2.7); the payloads are built at stream time.

### 4.11 Error codes
Reused: `invalid_request`, `invalid_name`, `not_found`, `not_authorized`, `conflict`, `already_exists`, `unknown_recipient`, `not_a_member`, `owner_protected`, `expired`, `clock_skew`, `snapshot_required`, `unavailable`, `storage_error`. Shared with ADR-0003 (added by whichever contract lands first, same names): `not_bound` (an unbound connection) and `too_large` (a write over the size limit). **One code new to this design: `checklist_incomplete`** (Done or Fixed with required steps left and no reason). It gets its own code because the app and the CLI branch on it ("Mark done anyway…", CHK-5, CHK-9), and `invalid_request` would hide it among input errors. `not_owner` (offices design) is added in phase 2 with change requests.


## 5. The `org-agent` commands (Remy)

The heading preserves old links. Current executable naming is **`agent-os`**; installed live Phase A `org-agent` instructions remain a separately controlled transport cutover, not a rename operation. See [executable migration](executable-migration.md) for available entry points. The earlier command inventory is in the [base W7 design](https://github.com/vgmello/agentic-org/blob/c6e520a36c4dc7c738c68915fac5e7d5d1cbe804/docs/design/tasks-projects-docs.md#5-the-org-agent-commands-remy). Actual source remains authoritative for support. No new target `assign-team`, private-note or organization-repository commands are invented here.

Target UX/CLI adaptation must resolve stable IDs and selected scoped references; distinguish type from reference prefix; show accountable versus assigned team and individual; send atomic assignment pairs with expected revision; retain pending-number records; reuse operation identity on retry; show conflict/unknown outcome without claiming acceptance; and expose private notes only through an authenticated separate author-only capability. Preserve bound-actor checks, app-only restrictions and accepted exit/error contracts for existing methods.

## 6. Time figures: how they're derived

Task history and saved waiting spans permit durable duration calculations, but data selection does not select the old automatic triage/decision waiting engine or analytics retention. Lead/cycle views must identify lifecycle endpoints and incomplete imported history. A `history_baseline` starts evidence at import; it does not reconstruct missing prior rounds. Count overlapping saved waiting spans once. Never infer a historical fixer from current assignment or infer active work from wall-clock status.

Retain the UX distinction between working time and untracked time: until complete session evidence exists, show **the rest (working time isn't tracked yet)**, never label it idle. Example: lead 1 d 4 h, waiting 3 h, blocked 0, rest 1 d 1 h. Working/idle splits need complete evidence for their reported interval. Usage/cost attribution comes from document 10's durable ledger including helpers/reviewers and historical snapshots, not counters in the task file or joins to current ownership. Budget records/controls in document 17 remain distinct; no runtime budget evaluator is claimed.

## 7. Phases and owners

Documentation selects the data target; implementation/migration phases need explicit planning. Domain rules remain with the `org-work` owner, storage with `org-office`, contracts with their defining crates and UX with the spec owner. Required work includes source-preserving import, durable task/history/receipt writes, organization authority registration, target read/command adapters, organization Git knowledge editing, author-private notes and lifecycle decisions. None is marked shipped by this reconciliation.

### 7.1 Gaps from Eden's review, as plan items

Preserve useful requirements without automatically selecting their engines: accessible triage/verification, explicit reasons and duplicate evidence, checklist provenance, safely attributed doc editing, live conflict handling and honest incomplete time figures. Areas, Undo, checklist promotion/templates/gates, process-derived steps, automatic fix/feature/debt propagation, retrospective analytics and scheduling remain individually deferred in the UX specs. Their earlier detailed proposals remain in Git history, not current plan phases.

## 8. Tests that prove the rules

These are implementation acceptance obligations, not tests run by this documentation change:

- All task types require accountable team; project tasks match project owner; projectless tasks use the full team path. Any valid assignment pair (neither, team-only, individual-only, both) is accepted only with active same-organization recipients.
- A stale assignment revision changes neither field nor history. A successful pair replacement atomically records old/new pair, actor/time, snapshot and receipt; retry returns the same result after response loss/crash.
- Reporter is immutable and admitted-actor derived. Deactivation, assignment, closure and import preserve original actors/times; absent history gets an explicit baseline, never fabricated events.
- Type changes retain stable identity, number, comments and links. Import preserves old IDs/events/codec metadata, detects namespace collisions and retains unmapped details/workflows in staging. No blind B-/REQ- renaming.
- Multi-office concurrent creations allocate distinct numbers in the right project/team scope; pending local tasks/features remain usable. Restore includes every acknowledged allocation and unpublished task/event/receipt journal.
- Task activity remains immutable across updates, replacement attempts, deletion, archive and projection rebuild. Unknown event tags survive. Shared metadata changes have matching task events.
- Documents require project and at most one same-project feature/release/deployment. Path and metadata agree; a feature definition is never also a supporting-document row. Features/current documents enforce active-owner requirements.
- Many repositories/project and many projects/repository keep association-specific integration branches and one office/repository cache. Projects without code repositories retain their organization Git docs.
- PN-01–06 in document 05 prove private CRUD/conflicts, author-only reads, no admin exception, no shared metadata leakage, assignment/recovery continuity, explicit publish and retention/deletion behavior. Shared `note` comments do not satisfy them.
- Preserve current security/codec regressions: actor binding, app-only checks, typed canonical request hashing, unknown enum retention and size bounds. Target Git adapters must reject traversal/symlinks/submodules, disable executable Git configuration on reads/writes, use expected blobs/ref CAS and durable intents, preserve commit attribution and never write across an agent-worktree boundary.
- Runtime handoff preserves task worktrees; new path sandbox denies and fencing/process cleanup are verified before launch. A cache rebuild or task reassignment never authorizes another process or implies closure.

## 9. Changes to Eden's specs and other designs

[work-items-ux.md](../specs/work-items-ux.md) and [feature-requests-ux.md](../specs/feature-requests-ux.md) now describe the selected task model throughout their active flows and criteria. Old separate number sequences, mandatory areas, office-only sources, teamless ownership, code-repository-only knowledge, mutable reporter assumptions and implicit team assignment are retired from active guidance. Existing criterion IDs remain stable; unselected transition engines are labeled individually. Related feature, project, team and operational specifications must use the owning organization-data contracts rather than this document's historical §4 shapes.

## 10. Open questions

- Target public mutation/wire contracts and migration sequencing remain implementation work. Choose explicit historical readable-reference mappings and preserve namespace collision evidence before publication.
- The lifecycle contract must select supported bug/request/type transitions, cancellation/verification, closure with unresolved questions/waits, duplicate resolution and existing-assignment disposition on deactivation.
- Areas, Undo, advanced checklist/process engines and automatic status propagation remain unselected. Do not reactivate them because the current code happens to implement an earlier rule.
- Organization Git editing/publication and safe target runtime sandbox adapters require designs preserving existing security/durability guarantees. Required private-note storage/access/UI remain unimplemented.
