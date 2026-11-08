---
kind: spec
title: "Features: a project's capabilities, defined in the repository (UX)"
status: for-review
revision: 2
owner: ac-product-designer-eden
created: "2026-11-08"
updated: "2026-11-08"
requested_by: owner
reviewers: [ac-staff-engineer-ada, owner]
builds_on: [feature-requests-ux, work-items-ux, decisions-and-tech-debt-ux]
---

# Spec · Features: a project's capabilities, defined in the repository (UX)

Extends `feature-requests-ux.md` (requests), `work-items-ux.md` (project page, tasks), `decisions-and-tech-debt-ux.md` (repo docs, templates) and `retrospectives-ux.md` (retros run on delivered features).

## The request (the user, 2026-11-08)

> Features should be a first-class part of a project, not only tasks. A project can have only tasks, and that's fine. But when I define a feature, its **definition lives in the repository**. The **feature request** is transient, in the database. Once it's scoped, it **materializes as a feature**, and the feature is **broken down into tasks**. So a feature is an object in the system too.

**Revision 2 (2026-11-08): aligned with ada's W7 model.** A feature's status lives in the office. The tool rewrites the doc's `status`, `tasks` and `delivered` from it on each status commit, so they're never edited by hand (decision 5, §1). Delivered needs at least one task **Done · Completed**: when every task was cancelled, the feature says "All its tasks were cancelled. Add tasks or retire it." A Delivered feature goes back to In progress when a linked task is reopened or an open task is linked. Task numbers follow `work-items-ux.md` rev 6 (`T-88`, `ac-T-88` outside the project). **Criteria rewritten (same ID):** FTR-5. **Added:** FTR-9.

## 0. Decisions

1. **Three objects, three lifetimes:**

   | Object | Lives in | Lifetime | Example |
   | --- | --- | --- | --- |
   | Feature request (`REQ-14`) | office database | transient: until it's approved, declined or merged | "Let me capture requests with a shortcut…" |
   | **Feature** (`FEAT-0007`) | **repository**, `docs/features/0007-<slug>.md` | **persistent**: the definition of what the product does | "Quick capture of requests" |
   | Task (`T-88`, `ac-T-88` outside the project) | office database | transient: done and archived | "Register the global shortcut" |

2. **A feature is the product's description of one capability,** in Markdown with front matter: what it does and for whom, its scope and what's out of scope, its acceptance criteria, its status, and links to the request(s) it came from, its specs, designs and decisions, and its tasks.
3. **Scoping a request materializes a feature.** The scoping agent's outcome (`feature-requests-ux.md` decision 7) is a **feature doc** (status Proposed) plus its draft tasks. **Approve** sets the feature to **Planned** and creates the tasks linked to it. A request can also extend an existing feature instead ("Adds to FEAT-0003").
4. **Tasks don't need a feature.** Small work, fixes and chores stay plain tasks. A task can be linked to one feature at most. Bugs can name the feature they affect.
5. **Feature statuses:** **Proposed · Planned · In progress · Delivered · Retired.** The status lives in the office *(rev 2)*. In progress follows its first task starting, and Delivered follows all its tasks being Done with at least one **Done · Completed** (with Undo, like bugs and debt). When every task was cancelled, it doesn't go Delivered and says "All its tasks were cancelled. Add tasks or retire it." A Delivered feature goes back to In progress when a linked task is reopened or an open task is linked to it. Retired is set by hand with a reason ("replaced by FEAT-0012"). The tool writes each status change into the doc (its `status`, `tasks` and `delivered`) as a commit on a docs branch (`decisions-and-tech-debt-ux.md` §0.2), so those fields are never edited by hand.
6. **"Delivered" is what retros listen for** (`retrospectives-ux.md` decision 1, A31): a feature reaching Delivered triggers the after-a-feature retro. The trigger is the office recording Delivered (the last task done, at least one completed), not the merge of the doc's status commit.
7. **Features are the project's map.** The project page gets a **Features** tab, the first after Overview, showing what the product does and what's coming, with progress from their tasks.

## 1. The feature file

```markdown
---
kind: feature
id: FEAT-0007
title: Quick capture of requests
status: in-progress        # proposed | planned | in-progress | delivered | retired; written by the tool from the office
owner: ac-product-designer-eden
area: Desktop app
requests: [REQ-14]
specs: [feature-requests-ux]
decisions: []
tasks: [T-88, T-89, T-90, T-91, T-92, T-93]   # written by the tool from the office (feature_id on tasks); never edit by hand
delivered: null                  # { on, by } when all tasks are done, at least one completed; written by the tool
retired: null                    # { on, reason, replaced_by }
---

## What it does
## Who it's for
## In scope
## Out of scope
## Acceptance criteria
## Notes
```

The template is editable like every kind (`decisions-and-tech-debt-ux.md` §3.2). `status`, `tasks` and `delivered` belong to the office *(rev 2)*: the tool rewrites them on each status commit, a hand edit is overwritten at the next one, and `org-agent doc validate` flags it.

## 2. Flow · The Features tab

```
 Features   [New feature]   [Proposed ▾ Planned ▾ In progress ▾ Delivered ▾]   [Search]
 IN PROGRESS · 2
   FEAT-0007  Quick capture of requests     Desktop app   ▓▓▓▓░░ 4 of 6 tasks   from REQ-14
   FEAT-0006  Agent run modes               Office        ▓░░░░░ 1 of 7 tasks
 PLANNED · 1
   FEAT-0008  Retrospectives                Office        0 of 9 tasks
 PROPOSED · 1
   FEAT-0009  Costs per project             Office        scoped by Eden · waiting for you   [Review]
 DELIVERED · 5  (collapsed)
```

- **Row:** id · title · area · progress (tasks done of total, in words and a bar) · origin (request) · the next action.
- **Feature page:** the header card from the front matter (status, owner, area, requests, specs, decisions) · the body sections · **Tasks** (a small board: To do / In progress / Done) · **Bugs** naming the feature · **Cost and time** to date (the costs spec, rolled up from its tasks) · **Retros** that covered it · history (its commits).
- **New feature** (without a request): opens the feature template in the editor. **Break into tasks…** drafts tasks (with the copilot when on, smart features #7), each editable, then **Create tasks**.
- **Overview tile:** "Features: 2 in progress · 1 planned · 5 delivered".
- **States:** a task linked to the feature is deleted (removed from `tasks`, noted in history) · all tasks done, at least one completed (Delivered, with Undo) · every task cancelled ("All its tasks were cancelled. Add tasks or retire it." with **New task** and **Retire…**) · a task reopened, or an open task linked, on a Delivered feature (back to In progress, noted in history) · a feature Retired with open tasks ("Retire FEAT-0006? Its 3 open tasks stay as plain tasks, or **Close them**").

## 3. UX acceptance criteria (FTR)

Criterion ids use **FTR-** so they never collide with feature records (`FEAT-n`) *(renamed in rev 1a, before any test used them)*.

- FTR-1 · A feature is a file `docs/features/<n>-<slug>.md` with front matter per the feature template, numbered FEAT-<n>.
- FTR-2 · Scoping a request produces a Proposed feature with draft tasks. Approve sets it to Planned and creates the tasks linked to it.
- FTR-3 · A request can extend an existing feature, which then lists the request in `requests`.
- FTR-4 · Tasks can exist without a feature, and a task links to at most one feature.
- FTR-5 · *(rev 2)* A feature moves to In progress when its first task starts and to Delivered when all its tasks are Done and at least one is Done · Completed, each written to the doc as a commit on a docs branch, with Undo. With every task cancelled it stays and shows "All its tasks were cancelled. Add tasks or retire it." A Delivered feature returns to In progress when a linked task is reopened or an open task is linked.
- FTR-6 · The project's Features tab groups features by status with task progress, and each feature page shows its tasks, bugs, cost and time, retros and history.
- FTR-7 · A feature reaching Delivered (recorded by the office when its last task is done, not when the status commit merges) triggers the after-a-feature retrospective.
- FTR-8 · Retiring a feature needs a reason, and its open tasks stay as plain tasks unless the owner closes them.
- FTR-9 · *(new, rev 2)* A feature's status lives in the office. The tool rewrites the doc's `status`, `tasks` and `delivered` from it on each status commit, and `doc validate` flags a hand edit to them.

## 4. Open questions

**For ada and codex-dev**
- ~~A34~~ · *Resolved (ada):* the definition is a repo doc. The task ↔ feature link (`feature_id` on tasks) is database data and the source of truth. The doc's `tasks` list is regenerated from the database on each status commit, never edited by hand.
- ~~A35~~ · *Resolved (ada):* Delivered is automatic when the last task is done, with Undo, and the owner is notified. Plan row W17: codex-dev (model), Remy (CLI, the messaging and `org-agent` owner since agy ran out of credits), app-frontend-dev (UI).
