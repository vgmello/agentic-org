> **Historical source revision.** Imported from Eden’s notes for completeness. The [current product specification](../../../features-ux.md) already existed in `v1` and remains the implementation reference. Do not replace it with this earlier revision.

# Spec · Features: a project's capabilities, defined in the repository (UX)

Owner: Eden · Status: **revision 3, for ada and the user** · 2026-11-09 · Extends `feature-requests-ux.md` (requests), `work-items-ux.md` (project page, tasks), `decisions-and-tech-debt-ux.md` (repo docs, templates) and `retrospectives-ux.md` (retros run on delivered features).

## The request (the user, 2026-11-08)

> Features should be a first-class part of a project, not only tasks. A project can have only tasks, and that's fine. But when I define a feature, its **definition lives in the repository**. The **feature request** is transient, in the database. Once it's scoped, it **materializes as a feature**, and the feature is **broken down into tasks**. So a feature is an object in the system too.

**Revision 3 (2026-11-09): the feature page, its actions and its states, for the app build.** Adds the feature page's layout (§2.2), what each action does and who may use it, matching ada's W7 contract (§2.3: **Plan** and retiring with **Close them** are the owner's, everything else is anyone's), and every state the tab and the page can be in (§2.4): no repository, a doc not merged yet, a status not written to the doc yet, a doc changed or removed outside the tool, a feature doc the office doesn't track yet (**Track it**), the office stopped, a failed save and a newer change. The tab hides Retired features until the filter asks for them. **Added:** FTR-10 to FTR-14. **Question:** A78.

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

### 2.1 The tab

```
 Features   [New feature]   [Proposed ▾ Planned ▾ In progress ▾ Delivered ▾ Retired ▾]   [Search]
 IN PROGRESS · 2
   FEAT-0007  Quick capture of requests     Desktop app   ▓▓▓▓░░ 4 of 6 tasks   from REQ-14
   FEAT-0006  Agent run modes               Office        ▓░░░░░ 1 of 7 tasks
 PLANNED · 1
   FEAT-0008  Retrospectives                Office        0 of 9 tasks
 PROPOSED · 1
   FEAT-0009  Costs per project             Office        scoped by Eden · waiting for you   [Review]
 DELIVERED · 5  (collapsed)
```

- **Row:** id · title · area · progress (tasks done of total, in words and a bar; cancelled tasks don't count toward either) · origin (request) · the next action.
- **Groups** in this order: In progress · Planned · Proposed · Delivered (collapsed). **Retired** is hidden until the status filter includes it *(rev 3)*; then it's a collapsed group at the end. Search matches the id, title and area.
- **The next action** on a row: a Proposed feature from a request shows **Review** (it opens the request's review, `feature-requests-ux.md`); a Proposed feature made with New feature shows **Plan** to the owner; a feature with every task cancelled shows "All its tasks were cancelled"; otherwise nothing.
- **New feature** (without a request): Title * (1–120) and Area (optional), then **Create**. The office writes the doc from the template on a docs branch (`decisions-and-tech-debt-ux.md` §0.2) and the feature page opens in **Proposed**, with the definition open in the editor. **Break into tasks…** drafts tasks (with the copilot when on, smart features #7), each editable, then **Create tasks**.
- **Overview tile:** "Features: 2 in progress · 1 planned · 5 delivered".

### 2.2 The feature page *(rev 3)*

```
 ← Features   FEAT-0007 · Quick capture of requests                     [Edit definition] [⋯]
 ┌──────────────────────────────────────────────────────┬──────────────────────────────┐
 │ ● In progress · 4 of 6 tasks ▓▓▓▓░░                   │ Owner    Eden                │
 │                                                      │ Area     Desktop app         │
 │ What it does                                         │ From     REQ-14              │
 │ Captures a request from anywhere with a shortcut…    │ Specs    feature-requests-ux │
 │ Who it's for · In scope · Out of scope ·             │ Decisions —                  │
 │ Acceptance criteria · Notes                          │ Doc      docs/features/0007- │
 │                                                      │          quick-capture….md   │
 │ Tasks                          [Break into tasks…]   │ Cost to date  $41 · 6 h 10 m │
 │  To do 1        In progress 1       Done 4           │ Retros   —                   │
 │  T-93 Settings  T-92 Shortcut key   T-88 ✓ T-89 ✓ …  │                              │
 │  [+ New task] [+ Link a task]                         │                              │
 │ Bugs naming it                                        │                              │
 │  B-21 Shortcut ignored in full screen · Triaged      │                              │
 │ ──────────────────────────────────────────────────── │                              │
 │ History                                              │                              │
 │ ⚙ In progress when T-88 started · 2 d ago  [Undo]     │                              │
 │ (V) Vitor planned it · 3 d ago                        │                              │
 │ Doc: "status: in-progress" committed · 2 d ago        │                              │
 └──────────────────────────────────────────────────────┴──────────────────────────────┘
```

- **The header card** (right) comes from the record and the doc's front matter: owner, area, requests, specs, decisions, the doc's path (opens it in the Docs tab), **Cost to date** (`costs-and-insights-ux.md`, rolled up from its tasks) and the retros that covered it.
- **The body** is the doc's sections, rendered. **Edit definition** opens them in the editor (§2.3).
- **Tasks** is a small board (To do · In progress · Done). Done tasks show a tick, and Done · Cancelled ones are struck through. Each task opens its page.
- **Bugs naming it** lists bugs whose "Affects feature" is this one, with their status.
- **History** mixes status changes (automatic ones with the gear icon and **Undo** for 1 hour, as bugs, `work-items-ux.md` §7.1), who planned or retired it and why, tasks linked and unlinked, and the doc's commits.

### 2.3 Actions *(rev 3)*

| Action | Where | Who | What happens |
| --- | --- | --- | --- |
| **Plan** | a Proposed feature made with New feature (one from a request is planned by approving the request) | the owner | Planned. "FEAT-0009 is planned. Its tasks can start." |
| **Edit definition** | the page | anyone | Opens the doc's sections in the editor; **Save** commits them on the doc's open docs branch, or a new one (`decisions-and-tech-debt-ux.md` §0.2). The title and area change here too. `status`, `tasks` and `delivered` aren't editable (decision 5). |
| **Break into tasks…** | the Tasks section | anyone | Drafts tasks (§2.1); **Create tasks** creates them linked to the feature. |
| **New task** | the Tasks section | anyone | New task with Feature prefilled (`work-items-ux.md` Flow 7). |
| **Link a task** | the Tasks section | anyone | A picker of the project's tasks. A task already in another feature says "T-12 is part of FEAT-0003. Move it here?" with **Move** (a task has one feature, FTR-4). |
| **Unlink** | a task's menu on the board | anyone | The task becomes a plain task; the status follows the rules in decision 5 (unlinking the last open task can deliver the feature). |
| **Undo** | an automatic change in History, for 1 hour | anyone | Restores the status before it, as a history entry ("Vitor undid Delivered"). |
| **Retire…** | ⋯ | anyone; **Close them** is the owner's | A reason * (one line) and **Replaced by** (optional, another feature). With open tasks: "Its 3 open tasks stay as plain tasks." and, for the owner only, the choice **Close them** (they become Done · Cancelled). **Retire**. |
| **Open the doc** | ⋯ | anyone | The doc in the Docs tab, with its branches and commits. |

Agents do the same through `org-agent feature …` and `doc edit`, except **Plan** and **Close them**, which only the owner can do (ada's W7 contract, §2.2).

### 2.4 States *(rev 3, extends rev 2's list)*

| Case | Behavior |
| --- | --- |
| Loading | Skeleton rows (tab) or skeleton card and sections (page). |
| No features yet | "Features describe what the product does, and their definitions live in the repository. A project can have only tasks." with **New feature**, and "Features also come from scoping a request." |
| The project has no repository | "Features live in the project's repository. Connect one in Settings to add features." with **Open Settings**; New feature is off, saying why. |
| A new feature's doc isn't merged yet | The page shows the doc from its branch, with "Not merged yet · on docs/features-9-…" and **Open the branch**. The feature counts as normal everywhere else. |
| The status isn't written to the doc yet (the doc write failed and is retrying) | A quiet line under the status: "Status not written to the doc yet. Retrying." The office's status is the true one, so nothing else changes. |
| The doc's `status`, `tasks` or `delivered` were edited by hand | "The doc says <value>; the office says <value>. The next status change rewrites the doc." The office's value shows. |
| The doc was removed or renamed outside the tool | "FEAT-0007's doc isn't in the repository at docs/features/0007-quick-capture….md anymore." with **Find it…** (pick the moved file) and **Retire…**. Tasks and history stay. |
| A feature doc the office doesn't track (written with git) | A group "Not tracked yet · 1" at the end of the tab, each row with **Track it** (the office reads its number, title and status, `feature.adopt`). |
| Every task cancelled | "All its tasks were cancelled. Add tasks or retire it." with **New task** and **Retire…** (decision 5). |
| All tasks done, at least one completed | Delivered, with Undo for 1 hour, and the owner is told. |
| A task reopened, or an open task linked, on a Delivered feature | Back to In progress, noted in history. |
| A task linked to the feature is deleted | Removed from `tasks`, noted in history. |
| Retired | A banner: "Retired · <reason>" and "Replaced by FEAT-0012" when set. Edit definition, Break into tasks, New task and Link a task are off; Open the doc stays. A request can't extend it. |
| The office is stopped or can't be reached | The tab and page show the last values read-only, with "The office isn't running, so features can't be changed now." and every action off. |
| Can't save (the office refused, or the repository is busy) | "Couldn't save FEAT-0007: <reason>." with **Try again**; what was typed stays. |
| A newer change (someone else saved first) | The page reloads the feature and keeps what was typed in the editor, with "FEAT-0007 changed while you were editing. Your text is kept; check it, then save." |

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
- FTR-10 · *(new, rev 3)* The Features tab groups In progress, Planned, Proposed and Delivered (collapsed), with Retired hidden until the status filter includes it; each row shows id, title, area, progress (cancelled tasks counted in neither part), origin and its next action (Review, Plan or "All its tasks were cancelled").
- FTR-11 · *(new, rev 3)* The feature page shows the header card (owner, area, requests, specs, decisions, doc path, cost to date, retros), the doc's sections, a Tasks board, the bugs naming it and a history in which automatic changes have Undo for 1 hour.
- FTR-12 · *(new, rev 3)* Plan is shown only to the owner, on Proposed features made with New feature. Edit definition saves on a docs branch and can't change `status`, `tasks` or `delivered`. Link a task asks before moving a task from another feature. Retire… needs a reason, and only the owner sees Close them.
- FTR-13 · *(new, rev 3)* New feature asks for a title (and optionally an area), creates a Proposed feature whose doc is on a docs branch, and opens its page with the definition in the editor. It's off, saying why, when the project has no repository.
- FTR-14 · *(new, rev 3)* The tab and page cover loading, no features, no repository, a doc not merged yet, a status not written to the doc yet, front matter edited by hand, a doc removed or renamed outside the tool (Find it…, Retire…), a feature doc not tracked yet (Track it), a retired feature (actions off), the office stopped or unreachable (read-only), a failed save, and a newer change (reload, keep the typed text).

## 4. Open questions

**For ada and codex-dev**
- ~~A34~~ · *Resolved (ada):* the definition is a repo doc. The task ↔ feature link (`feature_id` on tasks) is database data and the source of truth. The doc's `tasks` list is regenerated from the database on each status commit, never edited by hand.
- A78 · *(rev 3)* Three things the page needs from the W7 contract: (1) the index flags a feature record whose doc is no longer at its path on the integration branch (removed or renamed outside the tool), and **Find it…** points the record at the new path; (2) the tab lists feature docs that have no record yet, for **Track it** (`feature.adopt`); (3) the record's title and area follow the doc when a save changes them.
- ~~A35~~ · *Resolved (ada):* Delivered is automatic when the last task is done, with Undo, and the owner is notified. Plan row W17: codex-dev (model), Remy (CLI, the messaging and `org-agent` owner since agy ran out of credits), app-frontend-dev (UI).
