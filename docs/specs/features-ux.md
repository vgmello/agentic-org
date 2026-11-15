---
kind: spec
title: "Features: a project's capabilities, defined in organization Git (UX)"
status: for-review
revision: 3
owner: ac-product-designer-eden
created: "2026-11-08"
updated: "2026-11-15"
requested_by: owner
reviewers: [ac-staff-engineer-ada, owner]
builds_on: [feature-requests-ux, work-items-ux, decisions-and-tech-debt-ux]
---

# Spec · Features: a project's capabilities, defined in the repository (UX)

Revision 3 reconciles the body and criteria with [features/tasks](../design/organization-data/03-features-and-work.md), [project documents](../design/organization-data/02-project-structure.md), [numbering](../design/organization-data/06-identity-and-numbering.md) and [record contracts](../design/organization-data/08-record-contracts.md). These are selected target contracts, not implemented commands or a migration. The [authority map](../design/documentation-authority.md) distinguishes current runtime evidence. Automatic lifecycle transitions, Undo and retrospectives remain deferred individually below.

## The request (the user, 2026-11-08)

> Features should be a first-class part of a project, not only tasks. A project can have only tasks, and that's fine. But when I define a feature, its definition lives in the repository. Once a request is scoped, it materializes as a feature, and the feature is broken down into tasks.

## 0. Decisions

1. **One typed task entity and a separate durable feature:**

   | Object | Canonical source | Identity and lifetime |
   | --- | --- | --- |
   | Request (`type: request`) | Project or team `work/tasks/<task-id>.md` in organization Git | Stable task ID; T- reference; retain creator, comments and history after scoping |
   | Feature | `projects/<project-id>/features/<feature-id>/feature.md` in organization Git | Stable ID, project-scoped F- reference, durable capability definition |
   | Task (`type: task`, `bug`, or another type) | Same task storage and model as requests | T- reference; closure does not erase retained history or release attribution |

   New stable IDs use UUIDv7. Existing IDs and imported references remain unchanged; old REQ-/FEAT-/B- values need explicit mappings rather than number reuse. Inside P-8 show T-42 or F-6; outside it show P-8/T-42 or P-8/F-6. Projectless tasks use TEAM-12/T-42.
2. **A feature describes one capability:** purpose, current behavior, rules, known limitations and non-goals. Acceptance criteria and supporting links can develop that explanation. Detailed designs and decisions are separate documents with required project and same-project feature scope. Shared architecture stays project-wide.
3. **Request scoping is a proposed workflow.** A scoping review may draft a feature and tasks or extend an existing feature. Approval must persist each resulting record and preserve the originating request as a typed task; it does not convert a request ID into a feature ID or delete its evidence. No automatic scoping engine is selected.
4. **Tasks do not need features.** A project task links to at most one same-project feature. Projectless team tasks cannot link to a feature. Bug/request records remain typed tasks. A bug may additionally store `details.affects_feature_id`, a same-project affected capability distinct from task `feature_id` (the feature implementing the fix). The feature page labels these relationships separately; they do not create new entities or counters. Each project task retains the project's accountable owner team; assignment is independent.
5. **Selected lifecycle:** `planned | in_progress | live | retired`. Show **Planned · In progress · Live · Retired**. Planned may be unassigned; In progress and Live require an active `owner_employee_id`. `delivered_at`, `retired_reason` and optional same-project `replaced_by_feature_id` are durable facts in the feature file, with revision and actor attribution. SQLite projects these files; it is not an independent status authority. Task completion counts are derived from task `feature_id`, not a stored feature `tasks` array.
6. **Deferred automatic lifecycle and retro trigger.** Earlier requirements proposed advancing after the first task starts, marking delivery after all tasks close with at least one completed, and returning to In progress on reopen. These remain proposed workflow rules requiring a separate decision, including cancellation handling. Selected status `live` does not itself schedule a retrospective. Undo is also deferred.
7. **Features are the project's map.** The project page gets a Features tab showing capabilities, current status, accountable employee and derived task progress. Projects with no features still show their tasks normally.

## 1. The feature file

At `projects/<project-id>/features/<feature-id>/feature.md` (illustrative existing stable IDs; use UUIDv7 for new records):

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

The display reference is F-6; a project's slug `agentic-org` derives public feature slug `agentic-org/quick-capture`. Persist only `local_slug` and `project_id`. Renaming project/feature slugs preserves stable ID, reference and filename. Numbers remain null until authoritative registration; a pending feature remains usable by ID. Duplicate local slugs in one project need explicit registration conflict resolution. Supporting designs use `features/<feature-id>/designs/<document-id>.md`; decisions and reports use their corresponding folders. The definition projects into `features`, not a duplicate `documents` row.

## 2. Flow · The Features tab

```text
 Features   [New feature]   [Planned ▾ In progress ▾ Live ▾ Retired ▾]   [Search]
 IN PROGRESS · 2
   F-6 Quick capture of requests     Owner Eden   4 of 6 tasks completed
   F-7 Agent run modes               Owner River  1 of 7 tasks completed
 PLANNED · 1
   F-8 Reporting                    Unassigned    0 of 9 tasks completed
 LIVE · 5 (collapsed)
```

- **Row:** reference, title, status, owner, progress in words and a bar, and origin links resolved through task relationships. Completed, cancelled and open counts remain distinguishable; all-cancelled does not imply delivery.
- **Feature page:** metadata card, definition body, supporting designs/decisions/reports, tasks grouped by current state, affected bugs queried by `details.affects_feature_id`, separately from bug-typed implementation tasks linked by `feature_id`, cost/time where authorized, and Git revision history. A Retros section is deferred with the retrospective engine.
- **New feature:** opens the definition editor. **Break into tasks…** is a proposal: drafts are editable before creation. Each created task has required accountable team, same project/feature and independent nullable team/individual assignments. Assignment writes update the pair atomically with expected revision and retain activity history and immutable reporter.
- **Overview tile:** “Features: 2 in progress · 1 planned · 5 live”. Cost totals come from captured usage attribution; missing usage is shown as incomplete and never zero. Amounts obey owner/project-manager scope.
- **States:** loading skeletons; malformed metadata names the field and keeps the body readable; unresolved origin references show retained identity; archived tasks resolve through history/tombstones. Retiring needs a reason and preserves task links. Closing or unlinking open tasks is a separate explicit action with its own authorization/history; retirement does not silently rewrite them. Optional replacement must be another feature in the same project with no replacement cycle. Preserve `legacy_definition_path` only as import provenance, never a filesystem instruction.

A feature may appear in several explicit release manifests: release feature_ids/task_ids select same-project records, independently of task milestone membership. Milestones belong to exactly one same-project release and use release-scoped M- references; release references are project-scoped R-. A Live feature or completed task does not prove deployment/availability. Release product changelogs explain included and gated capabilities; deployment technical changelogs retain each attempt's commit/configuration changes, validation and rollback. Flags keep release environment/target intent separate from confirmed deployment snapshots. No deployment executor or flag evaluator is selected by this UI.

## 3. UX acceptance criteria (FTR)

Criterion IDs use FTR- and are independent of feature F- references.

- FTR-1 · A feature definition uses the stable-ID organization-Git path and fields in §1, with project-scoped F- numbering and derived project-prefixed slug.
- FTR-2 · **Deferred scoping workflow:** scoping proposes a feature and draft tasks; approval creates explicit linked records without deleting or reidentifying the request.
- FTR-3 · A request outcome can propose a new/existing same-project feature through `details.request_outcome.feature: {type: new | extends, feature_id}`; preserve draft-task IDs, document references and estimates without claiming those drafts were created. Task `feature_id` remains implementation membership, distinct from outcome provenance.
- FTR-4 · Tasks may omit feature membership; a project task links to at most one same-project feature and projectless tasks have none.
- FTR-5 · Selected lifecycle facts are explicit file data. **Deferred automation/Undo:** first-start, all-completed, all-cancelled and reopen transitions require a separately selected workflow and cannot override file authority.
- FTR-6 · The Features tab groups selected statuses and shows derived task progress; feature pages show tasks, supporting knowledge, authorized cost/time and history. **Deferred retrospective subsection** appears only with that engine.
- FTR-7 · **Deferred retrospective trigger:** recording delivery does not currently initiate an after-feature retrospective.
- FTR-8 · Retiring requires `retired_reason`, preserves links/history and validates an optional same-project replacement; open-task changes are separate explicit actions.
- FTR-9 · File status, delivery and retirement facts survive projection rebuilds; task counts derive from canonical task membership. The earlier office-owned status/tasks rewrite is obsolete and must not overwrite valid file edits.

## 4. Open questions

- A34 · Superseded: task membership is durable task data projected into SQL; no database-only feature membership authority or regenerated feature `tasks` array.
- A35 · Deferred: automatic delivery/reopen rules, cancellation handling and Undo require explicit workflow selection. Retrospective scheduling is independently deferred.
