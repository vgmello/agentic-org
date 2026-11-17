# Design: typed tasks, features, projects and project documents

How tasks, features, projects and their documents behave. The records are owned by
the data contract: [projects](organization-data/02-project-structure.md),
[tasks and features](organization-data/03-features-and-work.md),
[operational data](organization-data/05-operational-data.md),
[identity and numbering](organization-data/06-identity-and-numbering.md),
[record contracts](organization-data/08-record-contracts.md) and
[assignment and history](organization-data/12-task-assignment-and-history.md). The
screens are in the [work items UX](../specs/work-items-ux.md) and the
[feature requests UX](../specs/feature-requests-ux.md). The library is `org-work`.

## In one paragraph

There is one task entity with an extensible `type`, a required accountable team, an
optional project and an independent current team and individual assignment. Every
task type has a `T-` reference minted by the office that creates it, and a durable,
immutable history. A project has an accountable team and may use several registered
repositories, each with its own integration branch. Project, task, feature and
knowledge records live in the remote organization repository, mirrored as durable
files and indexed in a projection. Private task notes have a separate durable,
author-only store. Runtime caches, agent placement and code worktrees are separate
from all of these.

## Decisions

1. **One typed task.** `type: task | bug | request | ...` is a classification, not a
   different entity, table or folder. Optional `work_type: build | run` is a separate
   classification; null means unclassified.
2. **Accountability and assignment differ.** The required `team_id` equals the
   project's `owner_team_id` for a project task. Nullable `assigned_team_id` and
   `assignee_id` form one atomic assignment pair. Neither is inferred from the other or
   from an employee's home team.
3. **Identity is stable.** IDs are UUIDv7. A task reference is minted by the creating
   office and is unique in the organization on its own, for example `T-B37`
   ([ADR-0019](../adr/0019-office-coded-task-numbers.md)). Type changes and assignment
   never renumber a task.
4. **Sources are durable.** Task files and published task events are organization Git
   content; acknowledged unpublished changes are in a durable journal. The database is
   their projection.
5. **Features are definitions.** `projects/<project-id>/features/<feature-id>/feature.md`
   defines behaviour, rules, limitations and non-goals. Its status and owner are
   authored fields, never calculated from task completion.
6. **Supporting documents have explicit scope:** a required project and at most one
   feature, release or deployment of the same project. `feature.md` is not duplicated
   as a supporting document.
7. **Privacy is separate.** Shared comments stay shared. Private notes are author-only
   records with no administrator exception, outside organization Git, shared history,
   shared search and shared feeds.
8. **No workflow engines in v1.** Areas, Undo, advanced checklist engines, automatic
   feature, bug or request transitions and process execution are not part of v1.

## 0. Where each thing lives

Paths are inside the organization repository, not this software repository.

| Thing | Canonical source | Projection and boundary |
| --- | --- | --- |
| Project and repository associations | `projects/<project-id>/project.yaml` | Project, contributing-team and project-repository rows; HQ creates projects |
| Repository registration | Organization repository registration (document 01) | Shared across projects; a code repository may be remote or local-only |
| Project task, any type | `projects/<project-id>/work/tasks/<task-id>.md` | One task row with the owner team |
| Projectless task, any type | `teams/<team-id>/.../work/tasks/<task-id>.md` | Under the full team path; no copy in a project |
| Feature definition | `projects/<project-id>/features/<feature-id>/feature.md` | Feature row and derived full slug |
| Supporting knowledge | Project, feature, release or deployment folders (document 02) | Indexed documents with explicit scope and stable-ID file names |
| Comments, checklists, watchers, questions, waits | Task front matter | Shared operational projection |
| Task history | `work-history/tasks/<task-id>/<revision>.json` | Immutable `task_events` |
| HQ-allocated numbers | `identity/counters/` and `identity/allocations/` | The authority journal is also required before acknowledgement |
| Private task notes | Separate durable private database (document 05) | No shared index, count or existence event |
| Usage, budgets, chat, receipts | Their protected sources (documents 10, 15, 16, 17) | Git alone cannot restore them |
| Code cache and task worktrees | `runtime/<office-id>/...` | Never authored organization content |

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

Release product changelogs and deployment technical changelogs are different
documents. A task linked to a milestone stays in `work/tasks/`. Saving durable files
and publishing to Git are separate operations.

## 1. The model

### 1.1 IDs and references

Every reference and file name uses the immutable stable ID. `schema_version` names
the file format and `revision` the concurrency revision.

| Entity | Display | Scope |
| --- | --- | --- |
| Team | `TEAM-12` | Organization, allocated by HQ |
| Project | `P-8`, slug `reservations` | Organization, allocated by HQ |
| Task of every type | `T-A42` | Minted by the creating office; unique in the organization |
| Feature | `F-6`, full slug `reservations/table-booking` | Number and local slug within the project, registered by HQ |
| Release | `R-3` | Project, allocated by HQ |
| Milestone | `M-2` | Release; full reference `P-8/R-3/M-2` |

A task reference is complete on its own in every view and never needs a project or
team in front. A type word or icon distinguishes Bug T-A42 from Request T-A43. A
feature HQ has not yet registered shows **Pending number** with a copyable stable ID;
a task never does. Resolving an HQ-numbered reference needs its scope: the project for
`F-` and `R-`, the project and release for `M-`. Page or conversation context supplies
only the scope it actually knows; a global lookup lists every permitted match with
its full reference, and too little scope asks for qualification. A mutation echoes
the resolved full reference.

### 1.2 The core (every type)

| Field | Meaning |
| --- | --- |
| `id`, `schema_version`, `revision`, `office_code`, `number` | Stable identity, format, concurrency and the office-minted reference |
| `team_id` | Required accountable team; the project's owner team for a project task |
| `project_id` | Nullable; null means team-scoped work |
| `type` | Extensible identifier; `task`, `bug` and `request` to start |
| `work_type` | Nullable `build` or `run` |
| `title`, body | Required title; the Markdown body projects as `body_markdown` |
| `status` | `todo`, `in_progress`, `blocked`, `cancelled` or `done`, the same for every type |
| `priority` | `urgent`, `high`, `normal` or `low`; default `normal` |
| `effort` | Optional whole number of points, zero or more |
| `severity`, `resolution`, `blocked_reason` | Optional triage data and the reason a task was closed |
| `assigned_team_id`, `assignee_id` | Independent, nullable, replaced together |
| `feature_id`, `milestone_id` | Each optional and from the same project; both null for team tasks |
| `created_at`, `created_by_kind`, `created_by_id` | Immutable creator, who is the reporter |
| `updated_at`, `updated_by_kind`, `updated_by_id` | The latest accepted update |
| `repository_ids`, `branches`, `integrated_commits` | Zero or more repositories and per-repository work evidence |
| `checklist`, `comments`, `links`, `operations` | Shared nested metadata with stable child IDs |
| `details` | Optional structured payload per type, projected into `task_details` with the same identity and revision |

Actor kinds are employee, owner, system or unknown; only an employee actor has an
employee ID. The owner is a security principal held by the host, not an employee
flag. Status, priority and effort change only when an authorized person or agent sets
them, and each change is recorded in history.

### 1.3 Task

A task may have no project and no repository. A director can give a team task to
another director without inventing a project. A project task's repositories must be
among its project's associations; a projectless task names organization registrations
directly. Features and milestones are optional planning relationships, not folders.

Example: Payments owns P-8 and T-A42. Platform is the assigned team and Ana the
assignee. Payments' backlog queries `team_id`, Platform's queue `assigned_team_id`, and
Ana's queue `assignee_id`. No assignment changes the task's scope, reference, file or
reporter.

```yaml
schema_version: 1
id: 019a8541-1800-7000-8000-000000000042
office_code: A
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
effort: 3
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

The file is `projects/019a8541-1800-7000-8000-000000000008/work/tasks/019a8541-1800-7000-8000-000000000042.md`.
This is T-A42; creator 031 stays the reporter although updater 032 assigned Platform
and Ana. A projectless request uses `type: request`, a null project, feature and
milestone, and a valid owning team, with its file under the team's full path.

The task page's Output view collects permitted document links and repository, branch,
commit and file evidence, with unresolved targets shown as such. Done does not
establish review, merge, validation, release or deployment.

### 1.4 Bug

A bug is a task with `type: bug`, optional severity and structured `details`.
Document 03 maps `what_happened`, `expected`, `steps`, `environment`,
`severity_confirmed`, `reopen_count`, `closed_reason`, `duplicate_of_task_id`,
`affects_feature_id`, `suggested_triager_id` and `suggested_verifier_id` into
`task_details`. Reproduction text is distinct from the description; the affected
feature is distinct from the implementing feature and belongs to the same project. A
duplicate reference names a task of the same organization, with its type and cycles
validated. Suggested employees are neither assignments nor permissions.

### 1.5 Request

A request is a task with `type: request`, in a project or projectless with an
accountable team. Routing uses the same assignment pair. Its `details` hold
`request_kind`, `request_text`, `route_reason`, `routing_source`, optional
`routed_by_employee_id`, `request_outcome`, `merged_into_task_id` and `decline_reason`.

`request_outcome` holds an optional `feature: {type: new | extends, feature_id}`,
`docs[]: {project_id, doc_key}`, `draft_tasks[]` (each with a required `draft_id` and
`title` and an optional description, assignee, team and priority) and an estimate with
effort and a minimum and maximum cost in cents. Every nested reference is validated,
estimate bounds are nonnegative and min ≤ max. Draft tasks are proposals, not tasks,
and an estimate is not a budget.

### 1.6 Checklist

Task front matter stores ordered steps with a stable ID, text, required and done
flags, an optional linked task, who checked it and when, and a revision. Editing a
checklist and its history are part of v1. `details.definition_of_done`,
`details.branch`, `details.shortcut` and `details.done_anyway` keep acceptance text and
completion exceptions as data. Promotion of a step to a task, automatic ticks, default
templates, required-step gates and steps derived from processes are not part of v1.

### 1.7 Links

`links[]` holds a stable link ID, kind, target kind and target ID. `depends_on` refuses
self-links and cycles and may cross projects within the organization; its inverse
`blocks` is derived. `fixes` targets a bug; its inverse `fixed_by` is derived.
`relates_to` implies no ownership. A task's `feature_id` derives `implements`; a
document's scope derives `documents`. Only one direction is authored. A dependency on a
deleted task needs an explicit decision before new active work is accepted; it is
never satisfied automatically.

### 1.8 Feature

A feature has a project and a project-unique `local_slug`; its full slug is derived
from the project's slug. It has a stable ID, number, revision, title, body, optional
work classification and owner, attribution and lifecycle (`planned`, `in_progress`,
`live`, `retired`). A planned feature may have no owner; an in-progress or live one
needs an active accountable employee. Its sections are Purpose, How it works, Rules,
Known limitations and Non-goals. Designs, decisions and reports are separate supporting
documents. Task progress never changes a feature's status.

### 1.9 Project

A project has `owner_team_id`, a number and an organization-unique slug; optional
`contributing_team_ids` grant no authorization. Each `repositories[]` association has a
`repository_id` and an `integration_branch`. One registration may serve several
projects with different branches, and an office keeps one cache per repository. The
organization repository is remote; code repositories may be local-only.

### 1.10 Status

Status is the closed list of §1.2 for every type. `done` and `cancelled` finish a
task, and both count as done for milestone and release completion. Milestones and
releases store no status; done is derived from their tasks. Which transitions a type
allows, verification, and closing a task with open questions are open (§7.1). Nothing
changes status automatically.

## 2. The rules

### 2.1 Validation

The office validates at the accepting boundary: organization and scope IDs, the
required accountable team, equality with the project's owner, active assignment
targets, same-project feature, milestone and document scopes, repository
eligibility, path and field agreement, cycles and duplicate identities. A new
assignment names an active team or employee of the organization; an individual's home
team need not match the assigned team or any project team. Text is validated,
Markdown is rendered safely, and an oversized input gets an explicit size error.

### 2.2 Who may do what

Authentication and authorization are separate from project ownership, lead
management and assignment. Reporter and update actors come from the admitted
connection, and a forged actor is refused. A person's request carries the person's
principal and a host grant; an agent's carries its run's binding. Access to shared
work never extends to private notes.

### 2.3 IDs, idempotency and conflicts

An assignment change supplies **both fields, explicit nulls included**, and the
expected task revision. Both fields and an `assignment_changed` event are accepted
together or nothing changes. A stale revision conflicts even when only one half
changes. Retrying the same operation returns its durable result; another request
cannot reuse that operation ID.

The office acknowledges only once the task snapshot, its events and the idempotency
result are one recoverable operation, committed in the class
[ADR-0014](../adr/0014-acknowledgement-classes-and-agent-os-latency.md) assigns
(creation and comments committed, status and assignment durable). Concurrent writes
to a task wait for or conflict at its single accepting writer. A new task starts with
`created` at revision 1; unknown future change tags stay readable as unknown.

### 2.4 History

Every change to a task's content or metadata, including comments, checklists,
watchers, questions and waits, appends an immutable task event. An authorized
automation records system attribution and its cause. Handoff is not closure and never
deletes worktrees.

### 2.5 Changes to the organization

Changing a project's owner updates the accountable `team_id` of its tasks in the same
recoverable operation and records history, leaving assignments unchanged. An
employee's home-team move never rewrites task assignment. An inactive employee or
archived team cannot receive a new assignment; historical identity is kept, and
required active ownership is transferred before deactivation.

### 2.6 Numbering

HQ creates teams, projects, releases and milestones. Any office may create tasks and
features durably, including while disconnected. A task gets its final reference from
its creating office at once, under a durable high-water mark (ADR-0019). A feature is
usable by stable ID with `number: null` until HQ registers its number and slug; offices
never increment an HQ counter. An allocation and its entity are stored together before
acknowledgement, never as `MAX(number)+1`, and a retry returns the same allocation.
Allocations and tombstones are kept after closure or deletion.

### 2.7 History files

Each accepted revision is archived at `work-history/tasks/<task-id>/<revision>.json`
with the identity `(organization_id, task_id, revision)`. An event keeps actor, time,
optional operation, old and new values, source metadata and payload digest, in the
versioned hash format of document 05. Published files plus the unpublished journal and
receipts rebuild the same history.

### 2.8 Deleting

Task history, allocated numbers and reference tombstones are kept after closure,
archive or deletion. A tombstone keeps provenance, terminal state, last assignment
pair and revision. Deletion never reuses a number or satisfies a dependency. Deleting a
private note follows its own retention policy and is never blocked by shared history.

### 2.9 Projects without a repository

A project's brief, features and supporting documents live in organization Git, so a
project needs no code repository to have knowledge, and a task may involve no code.

## 3. Storage: `org-work` and its ports

### 3.1 Where the code lives

`org-work` owns the task rules and the storage ports; `org-office` implements storage;
`org-proto` holds the wire types. Apps compose them. SQL, Git and network adapters stay
outside domain policy.

### 3.2 The ports `org-work` defines

The ports are in [`crates/org-work/src/ports.rs`](../../crates/org-work/src/ports.rs),
the stored model in [`model.rs`](../../crates/org-work/src/model.rs), the task handlers
in [`tasks.rs`](../../crates/org-work/src/tasks.rs) and the wire types in
[`org-proto/src/work.rs`](../../crates/org-proto/src/work.rs). They implement an older
model and change as follows:

| Code today | Target |
| --- | --- |
| `StoredItem.created_by`, `assignee_id`; `task.assign` records `Assigned { from, to }` | Keep the immutable reporter; add the team assignment and the atomic pair at an expected revision |
| Optional `team_id`, a required project for tasks and bugs | A required accountable team for every type; projectless tasks; equality with the project owner |
| Separate task, bug and request identities with per-kind `NumberScope` | One typed task with an office-minted `T-` reference |
| ULID command and entity validators | UUIDv7 for every ID, command IDs included |
| Per-kind status lists | The one closed status list |
| `work_events` and the current-state transaction | Immutable ordered events, materialized to files, journal and receipts recoverably |
| Per-field `expected_version` checks | The assignment pair needs the exact expected revision |
| `CommentKind::Note` and shared activity | Shared comments stay shared; private notes have their own store and API |
| `work.snapshot`, `work.activity`, `work.changed` | Reads carry stable IDs, scopes and revisions |
| A bug's `area_id`, the undo and cascade modules | Deleted |

`selected_codec`, `selected_assignment` and `selected_history` are the parts already
written for the target model.

### 3.3 Tables

The draft DDL is [schema.sql](organization-data/schema.sql): tasks, `task_details`,
`task_repositories`, features and feature slugs, projects with their repositories and
contributing teams, documents, shared operations, links and relationships, record
references and tombstones, `task_events`, office codes and task counters, and
HQ-allocated counters. Private notes use the separate
[private-notes-schema.sql](organization-data/private-notes-schema.sql), never combined
with the shared projection. A note's author comes from authentication.

### 3.4 Request fingerprints and admission

A command receipt's digest is **SHA-256(method UTF-8 + NUL + bound actor ID UTF-8 +
NUL + canonical typed-params UTF-8)** ([`hash.rs`](../../crates/org-work/src/hash.rs)).
Params are decoded to their Rust type and re-encoded before hashing, so ignored
unknown fields cannot change the fingerprint. Canonical JSON sorts object keys by
UTF-8 bytes, keeps array order, uses compact separators and the `serde_json` scalar
encoding. The test vectors are in
[`proto-fingerprints.json`](../../crates/org-proto/test-vectors/v2/proto-fingerprints.json).

[`engine.rs`](../../crates/org-work/src/engine.rs) checks the caller first: a person's
request may act only as that person or the owner, an agent's only as an active or
paused agent. It then looks up the receipt before checking age and state: the same ID
and hash replays, changed params are refused. An operation ID is admitted for 29 days,
up to five minutes in the future, and its receipt kept 30 days from its encoded time.

Task-event hashing is different: document 05 hashes the UTF-8 event JSON with sorted
keys, compact separators, no ASCII escaping, explicit nulls, `{}` source metadata and
the checksum omitted. A replay compares the canonical payload hash before returning an
identical event; the same identity with a different payload conflicts. Revisions are
contiguous and the final snapshot or tombstone agrees.

### 3.5 Private notes

One accepting service at HQ owns private notes, in a plain SQLite database at
`<data-root>/<organization-id>/durable/private/task-notes.db`, protected by file
permissions ([ADR-0016](../adr/0016-no-application-level-data-encryption.md)). Other
offices call it as the bound author; nothing replicates it and it never enters Git. If
HQ is unavailable, private edits cannot be acknowledged. Another host accepts writes
only after fencing.

Reads, lists, author-private search, edits and deletion are scoped by organization
**and the authenticated author**, and by task and note where relevant. An employee
author is the stable employee ID; the owner is the stable authenticated subject.
Unauthorized and missing IDs get the same not-found response. Creating a note also
checks access to the task. A note stays readable by its author after the task is
archived or deleted; assignment, leadership and administrator roles never grant
access.

A create, edit or delete and its author-scoped receipt commit in one private
transaction. The receipt is checked before the revision, so a retry returns the
original result even after later edits; a changed payload conflicts. **Publish**
persists a private pending receipt with the selected revision and a fresh shared
comment ID, blocks edits and deletion of the note, then uses that comment ID as the
shared idempotency key. An unknown response stays pending for query or retry. Shared
history receives the published comment with the normal actor and time, never the
private note's ID. Publishing needs ordinary permission to comment, and the note stays
private.

Deletion clears the body and keeps a tombstone with the note's identity, revision and
time; a deleted ID is never resurrected and task deletion never cascades to notes.
Restoring a backup reconciles later deletions before serving reads and keeps tombstones
for the backup window; missing deletion evidence blocks recovery rather than exposing a
deleted body. A maximum backup retention is set before the feature is enabled, and
backups are encrypted at their destination. Logical deletion does not promise immediate
physical erasure. These rules implement PN-01 to PN-06 of document 05.

## 4. The contract

The wire contract for tasks is `org_proto::work`, changed in place to the model above
(implementation plan B4); the browser routes are in the
[web API](web-api-v1.md#tasks). Conventions that hold:

- Calls go to the caller's own office on a bound connection.
- Every write carries an operation ID and returns its outcome; a lost response is
  resolved by lookup or retry with the same ID.
- Times are RFC 3339 UTC with milliseconds.
- Every request and response is one frame of at most 1 MiB. A write whose params
  exceed 768 KiB is refused with `too_large`. Paged reads stop early to stay under
  768 KiB and return a cursor. A change event over 512 KiB is split into parts sharing
  the operation ID and position; a client advances its position only after the last
  part.
- Enums decode unknown values as `Unknown`; writers never send `Unknown`.
- `subscribe` resumes from a work position, or answers `snapshot_required` for another
  incarnation, a position beyond the feed or one older than its 30-day retention.
- `checklist_incomplete` is its own error code, so clients can offer "Mark done
  anyway…".

## 5. The `agent-os` commands

`agent-os` resolves stable IDs and full references, distinguishes a task's type from
its reference, shows the accountable team separately from the assigned team and
person, sends the assignment pair with its expected revision, reuses the operation ID
on retry, and reports a conflict or unknown outcome without claiming success. Commands
that need a project take it from the agent's context, or require it when there is none
(ADR-0019, "Default project for commands"). Lists default to one item per line, with
documented JSON output and actionable refusals that leak no inaccessible identity.
Private notes are reachable only through the separate author-only capability. Help
reflects only implemented commands.

## 6. Time figures

The [shared duration definitions](../specs/costs-and-insights-ux.md#shared-duration-definitions)
apply. With complete evidence, Started is the first In progress, kept across a reopen;
a direct To do → Done records both at once with zero cycle time. Lead time runs from
creation; cycle time from first Started to last completion (or now while open),
excluding the Done and Cancelled spans in between. Started day 0, Done day 2, reopened
day 9, Done day 10 gives three days of cycle time. Intervals come from actual events;
"picked up" needs an actual start after assignment.

Coverage and open intervals show "so far". Waiting comes from saved spans independently
of status, and overlaps count once. Until complete session evidence exists, the rest is
shown as **the rest (working time isn't tracked yet)**, never as idle. Usage and cost
come from document 10's ledger, never from counters in the task file or joins to the
current assignment.

## 7. Open design points

### 7.1 Lifecycle

Which transitions each type allows, verification and cancellation, closing a task
with open questions or waits, resolving duplicates, and what happens to open
assignments when an employee is deactivated are not yet decided.

### 7.2 Tracking, review, subtasks and planning

Each of these is open and is decided before it is built:

- **Following (AC-TK1–2):** per-reader durable last-seen positions and follow
  reasons; counts of changed tasks excluding the reader's own changes and all
  private-note activity; urgent waits stay visible through filters. Watchers alone
  implement none of this.
- **Requested-by (AC-TK3):** a requester relationship separate from the immutable
  reporter, verified against a message the requester actually sent or was addressed in,
  with protected links hidden from unauthorized readers.
- **Review (AC-TK4–6):** supported transitions, reviewers and defaults, no
  self-approval, outcome guards and reset rules. Review never grants access to private
  notes.
- **Comment history (AC-TK7):** who may edit or delete a shared comment, version
  readers, and redaction of secrets before any promise of erasure; immutable shared
  history is never rewritten silently.
- **Subtasks (AC-TK8):** a one-level parent model with ordinary child IDs and history,
  parent completion guards, closed-parent constraints and cascades that touch only the
  children they changed.
- **Planning (AC-PL2–7):** stable backlog rank, where new and returned items go,
  ordered batch moves, dates, missed-target display, close and reopen reasons,
  disposition of open items, who may plan, and honest progress. Every drag has a
  keyboard and menu equivalent. A task's milestone is independent of a release's
  explicit content list. Critical-path analysis, recurring schedules and task
  templates are not part of v1.

## 8. Tests that prove the rules

- Every type needs an accountable team; a project task matches its project's owner; a
  projectless task lives under the full team path. Each assignment pair (neither, team
  only, individual only, both) is accepted only with active targets of the
  organization.
- A stale assignment revision changes neither field nor history. A successful change
  records the old and new pair, actor, time, snapshot and receipt atomically; a retry
  returns the same result after a lost response or a crash.
- The reporter is immutable and comes from the admitted actor.
- A type change keeps identity, reference, comments and links.
- Two offices creating tasks concurrently, one of them disconnected, mint distinct
  references at once; a crash never reuses a number. Pending features stay usable.
  Restore includes every acknowledged allocation and every unpublished event and
  receipt.
- History stays immutable across updates, deletion, archive and projection rebuild,
  and unknown event tags survive.
- A document needs a project and at most one same-project feature, release or
  deployment; a feature definition is never also a supporting document.
- Several repositories per project and several projects per repository keep their own
  integration branches and one cache per office and repository.
- PN-01 to PN-06 prove private-note behaviour; shared comments do not satisfy them.
- Actor binding, typed canonical request hashing, unknown enum retention and size
  bounds hold. Git adapters refuse traversal, symlinks and submodules, disable
  executable Git configuration, use expected blobs and compare-and-swap refs, and never
  write across a worktree boundary.
- Handoff keeps task worktrees; a projection rebuild or reassignment never authorizes
  another process or implies closure.

## What the code does today

`crates/org-work` implements tasks, bugs and requests as separate kinds with
per-project counters, per-kind statuses, a bug's area, undo and cascades, against the
ULID identity; `crates/org-proto/src/work.rs` carries that wire contract. The
`selected_codec`, `selected_assignment` and `selected_history` modules and the office's
acceptance journal (`crates/org-office/src/store/acceptance`) implement the target
assignment and history. Private notes are not implemented. All of this is replaced by
increment B4 of the [implementation plan](../implementation-plan.md).
