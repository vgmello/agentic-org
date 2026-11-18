---
kind: spec
title: "Features: a project's capabilities, defined in organization Git (UX)"
status: current
owner: lead
created: "2026-11-08"
updated: "2026-11-17"
requested_by: owner
builds_on: [feature-requests-ux, work-items-ux, decisions-and-tech-debt-ux]
---

# Spec · Features: a project's capabilities, defined in the repository (UX)

The data contract owns the records: [features and tasks](../design/organization-data/03-features-and-work.md), [project documents](../design/organization-data/02-project-structure.md), [numbering](../design/organization-data/06-identity-and-numbering.md) and [record contracts](../design/organization-data/08-record-contracts.md). This spec owns what a person sees and does.

## The request (the user, 2026-11-08)

> Features should be a first-class part of a project, not only tasks. A project can have only tasks, and that's fine. But when I define a feature, its definition lives in the repository. Once a request is scoped, it materializes as a feature, and the feature is broken down into tasks.

## 0. Decisions

1. **One typed task entity and a separate durable feature:**

   | Object | Canonical source | Identity and lifetime |
   | --- | --- | --- |
   | Request (`type: request`) | Project or team `work/tasks/<task-id>.md` in organization Git | Stable task ID; `T-` reference; keeps creator, comments and history after scoping |
   | Feature | `projects/<project-id>/features/<feature-id>/feature.md` in organization Git | Stable ID, project-scoped `F-` reference, durable capability definition |
   | Task (`type: task`, `bug`, or another type) | Same task storage and model as requests | `T-` reference; closure does not erase history or release attribution |

   Stable IDs are UUIDv7. A task reference such as `T-A42` is the same everywhere. Inside P-8 show F-6; outside it show P-8/F-6.
2. **A feature describes one capability:** purpose, intended users, current behavior, rules, known limitations, non-goals and acceptance criteria. Product requirements belong in this definition; supporting designs explain implementation, decisions explain choices and reports keep findings. "PRD" describes content, not another document kind or a duplicate feature record. Detailed designs and decisions are separate documents with a required project and same-project feature scope. Shared architecture stays project-wide.
3. **Scoping a request is a review followed by an approval** (`feature-requests-ux.md` decision 7). A scoping review may draft a feature and tasks or extend an existing feature. Approval saves each resulting record and keeps the originating request as a typed task; it does not turn a request ID into a feature ID or delete its evidence. There is no automatic scoping engine.
4. **Tasks do not need features.** A project task links to at most one same-project feature. Projectless team tasks cannot link to a feature. A bug may also store `details.affects_feature_id`, the same-project capability it affects, distinct from task `feature_id` (the feature implementing the fix). The feature page labels these relationships separately. Each project task keeps the project's accountable owner team; assignment is independent.
5. **Lifecycle:** `planned | in_progress | live | retired`, shown as **Planned · In progress · Live · Retired**. Planned may be unassigned; In progress and Live require an active `owner_employee_id`. `delivered_at`, `retired_reason` and optional same-project `replaced_by_feature_id` are durable facts in the feature file, with revision and actor attribution. The database projects these files; it is not a status authority. Task counts derive from task `feature_id`, not a stored feature `tasks` array.
6. **Status changes are explicit.** A person sets the feature's status. Reaching `live` does not schedule anything.
7. **Features are the project's map.** The Features view shows capabilities, current status, accountable employee and derived task progress. It is not a required tab in the initial project navigation. Projects with no features show their tasks normally.
8. **A project's process package may not use features.** Features are a record that a package uses or leaves alone, unlike releases and milestones, which every project has (owner, 2026-11-18). A project with no package, and one whose package lists `features` in its `uses`, has the Features tab. One whose package does not, such as the desired state package, shows no Features tab while it has no feature ([process packages](../design/process-packages.md#3a-features-and-a-second-package), PKG-15).

## 1. The feature file

At `projects/<project-id>/features/<feature-id>/feature.md` (illustrative stable IDs):

```markdown
---
schema_version: 1
id: existing-feature-id
number: 6
revision: 1
project_id: existing-project-id
local_slug: quick-capture
title: Quick capture of requests
status: in_progress
owner_employee_id: existing-employee-id
work_type: build
created_at: "2026-11-15T10:00:00Z"
created_by_kind: employee
created_by_id: existing-employee-id
updated_at: "2026-11-15T10:00:00Z"
updated_by_kind: employee
updated_by_id: existing-employee-id
delivered_at: null
retired_reason: null
replaced_by_feature_id: null
links: []
---

## Purpose
## How it works
## Rules
## Known limitations
## Non-goals
```

The display reference is F-6; a project with slug `agentic-org` derives the public feature slug `agentic-org/quick-capture`. Only `local_slug` and `project_id` are stored. Renaming a project or feature slug keeps the stable ID, reference and filename. A feature created while HQ is unreachable has a null number until HQ registers it and stays usable by ID, shown as "Pending number". Duplicate local slugs in one project need explicit conflict resolution at registration. Supporting designs use `features/<feature-id>/designs/<document-id>.md`; decisions and reports use their own folders. The definition projects into `features`, not into a duplicate `documents` row.

## 2. Flow · The Features tab

Navigation availability follows the [project IA](work-items-ux.md#3-flow-1--project-page).
The Features view requirements below apply when that view is served; they do not
add a tab to the initial project skeleton.

```text
 Features   [New feature]   [Planned ▾ In progress ▾ Live ▾ Retired ▾]   [Search]
 IN PROGRESS · 2
   F-6 Quick capture of requests     Owner Mira   4 of 6 tasks completed
   F-7 Agent run modes               Owner Tomas  1 of 7 tasks completed
 PLANNED · 1
   F-8 Reporting                    Unassigned    0 of 9 tasks completed
 LIVE · 5 (collapsed)
```

- **Row:** reference, title, status, owner, progress in words and a bar, and origin links resolved through task relationships. Completed, open and cancelled counts are distinct; cancelled tasks are excluded from the progress denominator and an empty eligible set never implies 100% delivery. Live is collapsed by default and Retired appears only when selected in the status filter. Search matches reference, title and slug.
- **Feature page:** metadata card, definition body, supporting designs, decisions and reports, tasks grouped by current state, affected bugs (queried by `details.affects_feature_id`) separately from bug-typed implementation tasks (linked by `feature_id`), cost and time where authorized, and Git revision history.
- **New feature:** requires a title and a valid project, creates a Planned record and opens its definition editor. No code repository is required; a feature with a pending number is usable by stable ID. The editor keeps valid authored fields rather than overwriting them from office-only state. **Break into tasks…** proposes drafts that are editable before creation. Each created task has its required accountable team, the same project and feature, and independent nullable team and individual assignments. Assignment writes update the pair atomically with the expected revision and keep activity history and the immutable reporter.
- **Overview tile:** "Features: 2 in progress · 1 planned · 5 live". Cost totals come from captured usage attribution; missing usage is shown as incomplete, never zero. Amounts follow owner and project-manager scope.
- **States:** loading skeletons; malformed metadata names the field and keeps the body readable; unresolved origin references show their stable identity; archived tasks resolve through history and tombstones. Retiring needs a reason and keeps task links. Closing or unlinking open tasks is a separate explicit action with its own authorization and history; retirement does not silently rewrite them. An optional replacement must be another feature in the same project with no replacement cycle.

**Membership actions:** New task prefills this project and feature. Link a task lists eligible same-project tasks and asks before moving one already attached to another feature, naming both. Link and unlink keep task identity, attribution and history; they do not change either feature's status. Retiring does not unlink or close tasks. A control is available only when its handler exists and the person is authorized.

**Read and edit states:** with no features, the view explains the capability map and offers New feature. A disconnected projection is visibly stale and read-only, with an explanation. A failed save keeps copyable typed text with Retry; a newer revision keeps the draft and presents a comparison before an explicit retry. A missing or invalid canonical file keeps the feature identity and task and history links, shows the validation or path problem and offers a supported repair, never a silent retirement or a guessed path. A valid edit made directly in Git is indexed from organization Git with identity preserved; an unrecognized file is reported as invalid, never turned into a feature. Locally accepted state, pending number and pending publication are shown separately; office state never overrides the canonical file.

A feature may appear in several explicit release manifests: a release's `feature_ids` and `task_ids` select same-project records, independently of task milestone membership. Milestones belong to exactly one same-project release and use release-scoped `M-` references; releases use project-scoped `R-`. A Live feature or completed task does not prove deployment or availability. Release product changelogs explain included and gated capabilities; deployment technical changelogs keep each attempt's commit and configuration changes, validation and rollback. Flags keep release environment and target intent separate from confirmed deployment snapshots. This UI selects no deployment executor or flag evaluator.

The feature page lists **Included in release** backlinks to every same-project manifest that selects it, with product changelog links when present. Each release links back to the one capability definition. A later extension or reopening keeps earlier release references. Membership alone does not prove it shipped; corrections use explicit revision history and deployment attempts keep their separate evidence.

## 3. UX acceptance criteria (FTR)

Criterion IDs use FTR- and are independent of feature F- references.

- FTR-1 · A feature definition uses the stable-ID organization-Git path and fields in §1, with project-scoped F- numbering and a derived project-prefixed slug.
- FTR-2 · Scoping proposes a feature and draft tasks; approval creates explicit linked records without deleting or re-identifying the request.
- FTR-3 · A request outcome can propose a new or existing same-project feature through `details.request_outcome.feature: {type: new | extends, feature_id}`; draft-task IDs, document references and estimates are kept without claiming the drafts were created. Task `feature_id` remains implementation membership, distinct from outcome provenance.
- FTR-4 · Tasks may omit feature membership; a project task links to at most one same-project feature and projectless tasks have none.
- FTR-5 · Lifecycle facts are explicit file data, changed by a person; task starts, completions, cancellations and reopenings never change a feature's status by themselves.
- FTR-6 · The Features view groups statuses and shows derived task progress; feature pages show tasks, supporting knowledge, all explicit release backlinks, authorized cost and time, and history. A feature included in R-1 and R-3 keeps both after extension; milestone membership alone adds neither.
- FTR-7 · Recording delivery starts no retrospective and schedules nothing.
- FTR-8 · Retiring requires `retired_reason`, keeps links and history and validates an optional same-project replacement; open-task changes are separate explicit actions.
- FTR-9 · File status, delivery and retirement facts survive projection rebuilds; task counts derive from canonical task membership. Office state never overwrites a valid file edit.
- FTR-10 · Rows show reference, title, status, owner, progress and permitted origins. Cancelled tasks are left out of the denominator; Live is collapsed, Retired is filter-selected and search covers reference, title and slug.
- FTR-11 · The page shows the canonical definition and path, owner, tasks, affected bugs, supporting knowledge, release backlinks and history. Unavailable or unauthorized cost and time are shown as such.
- FTR-12 · Membership controls confirm before moving a task between features, name both features and keep history. Link, unlink and retire never silently close tasks or change feature status.
- FTR-13 · New feature validates project and title, creates Planned at the stable-ID organization-Git path, opens the definition editor and shows pending number and pending publication distinctly. No code repository is required.
- FTR-14 · Loading, empty, retired, malformed or missing file, stale or read-only, failed save, pending publication and newer revision states keep identity, history and unsaved text. Valid direct-Git edits are authoritative; the office never silently overwrites them.

## 4. Not in v1

Automatic feature status changes from task activity, Undo, and retrospectives.
