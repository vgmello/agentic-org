---
kind: spec
title: "Work items: tasks, bugs and project docs (UX)"
status: for-review
revision: 9
owner: ac-product-designer-eden
created: "2026-11-08"
updated: "2026-11-09"
requested_by: ac-staff-engineer-ada
reviewers: [codex-dev, ac-staff-engineer-ada, owner]
builds_on: [offices-and-teams-ux]
---

# Spec · Work items: tasks, bugs and project docs (UX)

Phase 1 workstream W7 (UX), with codex-dev (model and library) · Builds on `offices-and-teams-ux.md` rev 4.

**Sources:** ada's phase-1 plan (`ac-staff-engineer-ada/knowledge/plans/phase-1.md`, W7 and W9). The user's rule (2026-11-08): **bugs are first class**. Every project has its own bugs, and each bug sits in an **area** of the project, owned by a team, so it reaches the people who own that part. codex-dev's provisional model (not yet approved): flat areas per project with stable ids, a renamable name and an owning team; the reporter suggests the area and triage confirms or reroutes it; severity is separate from priority; New → Triaged → In progress → Fixed → Verified/Closed, with reopen, duplicate and won't-fix paths; a bug links to one or more fix tasks rather than turning into one. **ada's defaults (2026-11-08, for the user to approve with this design):** areas are flat, each owned by one of the project's teams, with an optional area lead (default: the team's lead), and every project has a default area **General**; severity is critical, major, minor or trivial, set by the reporter and confirmed at triage; **priority is a separate field shared with tasks** (urgent, high, normal, low); the reporter or the area lead verifies the fix.

**What it changes in my earlier work:** for organizations run by an office, this replaces `desktop-app-v1.md` (in Eden's notes, not in this repository) §3.7 (Assign task), §3.9 (Projects), §3.10 (Tasks roll-up) and the project Brief tab. Tasks are office records now, not backlog-file rows. Revision 5 of the v1 spec folds this in. Words, components, the owner, agents vs people, and the Pending/Blocked rules come from `offices-and-teams-ux.md` and aren't repeated here.

**Acceptance-criterion IDs** are new prefixes (PRJ, TSK, BUG, TRI, FIX, AREA, DOC, WORK) so they never collide with v1's PROJ and TASKS. They're stable from this revision: changed criteria are rewritten under the same ID, retired IDs are never reused.

## Changelog

**Revision 9 (2026-11-09): from a request.** A task created from a request shows **From** "REQ-12 · <title>" in its side column (`feature-requests-ux.md` §3, RQS-16). No criteria changed here.

**Revision 8 (2026-11-08): processes.** A new task gets the project's default checklist plus the checklists of the project's processes, not private ones (`processes-ux.md` decision 9, §9.1). **Criteria rewritten (same ID):** CHK-8 *(rev 8)*.

**Revision 7 (2026-11-08): review fixes.**

- **Waiting for a decision** (ada Q11): the block names its ADR. Accepting the ADR clears the block and the task returns to the status it had before Blocked. Rejecting it, or Request changes…, leaves a normal block, counted as blocked, not waiting on you (§9). The model needs a structured blocker, not only the text reason (A49, new).
- **Deleting a team:** if giving an area its new team fails, the dialog stays open, names the area and offers Retry, and the team isn't deleted (§8).
- **Answered in ada's W7 §9:** C3, C6, C8, C9, C10, C20 and A13 resolved (§15).
- **Link a task** (§7.2) links an existing task in the same project only, as C8 was resolved (it still said "across projects").
- Revision 6's note on waiting on you now states the narrow rule for decisions.
- **Criteria rewritten (same ID):** TSK-3, AREA-7. 85 criteria.

**Revision 6 (2026-11-08): aligned with ada's W7 model.**

- **Numbers:** tasks and bugs have separate sequences in each project, and the prefix shows the type: tasks `T-12`, bugs `B-7` inside the project, and `chk-T-12`, `chk-B-7` outside it (decision 3). An item with no number yet shows "Pending · 3k9f2x" (a 6-character local short id, §12). Every example is renumbered (a task keeps its digits as `T-n`, a bug as `B-n`). C1 resolved.
- **Done · Completed or Done · Cancelled** (C4 resolved). A bug goes Fixed by itself only when at least one fix task is Done · Completed, and not while required checklist steps are unticked (the activity says why). When every fix task was cancelled, the bug says "All its fix tasks were cancelled. Create a fix task, or close the bug." (decision 9, §7.2).
- **Made-task steps** tick only when their task is Done · Completed, and can't be ticked by hand. A cancelled task leaves the step showing "T-42 was cancelled" with **Unlink** (decision 17, §9.1).
- **Deleting a team** keeps the required **Give its areas to** picker when the project keeps other teams: the app gives each area its new team before it deletes the team. Areas become "No team" only when no team is left (§8).
- **Time:** working time isn't tracked yet, so the split shows waiting on you, blocked and "the rest", never idle (§9.2). Waiting on you counts questions to you, Fixed bugs you reported, scoped requests, triage suggested to you and a task Blocked waiting for your decision on a proposed ADR, only while it's the blocker.
- **Criteria rewritten (same ID):** TSK-11, CHK-6, BUG-7, FIX-2, AREA-7. **Added:** WORK-8. 85 criteria.

**Revision 5 (2026-11-08): features and requests (the user).** The project page gets a **Features** tab after Overview (`features-ux.md`) and a **Debt** tab. Work gets **Debt** and **Requests** pills (`decisions-and-tech-debt-ux.md`, `feature-requests-ux.md`). Tasks can link to one feature. Tabs are now **Overview | Features | Tasks | Bugs | Debt | Docs | Insights | Settings**.

**Revision 4 (2026-11-08): checklists and time per task (the user, via ada).** Tasks and bugs can carry a **checklist** of ordered steps, some **required** (§9.1, decisions 17–18). Every task and bug shows its **time**: lead time, cycle time and the split into working, waiting on you, blocked and idle, derived from its state changes (§9.2, with `costs-and-insights-ux.md` §2 and TIME-4 to TIME-6 there). Tech debt is no longer a work item: it's a doc in the repository (`decisions-and-tech-debt-ux.md` rev 4). Criteria added: CHK-1 to CHK-9, TSK-11. 84 criteria here. New questions C19–C21.

**Revision 3 (2026-11-08): ada's answers A10–A12.**

- **No permissions in phase 1** (the user's decision, via ada): anyone may triage, verify, close or reopen. The app **suggests** who should (the area lead for triage, the reporter for verification), routes the requests to them, and records who acted. The triage panel and the verify banner show to everyone, naming the suggested person. Rights come later. This supersedes rev 2's "only the area lead and the owner".
- **Docs are Markdown files in the project's git repository** (under `docs/`). The office keeps an index (title, path, last commit) for lists and search. Saving a doc in the app makes a commit with you as the author. History is the file's commits. The view shows the repository's main branch, and changes on other branches appear when they're merged (§10). *(Corrected in rev 4: saves go to a docs branch and land through the merge flow, never straight to the default branch; see `decisions-and-tech-debt-ux.md` §0.2.)*
- **A pending bug** (later phases, an office that can't reach HQ) shows **Pending** and a local short id until HQ gives it its number. The number then replaces it without reordering lists (§12).
- **Criteria rewritten (same ID):** TRI-1, TRI-6, TRI-7, FIX-4, FIX-6, FIX-11, DOC-1, DOC-4, DOC-5. **Added:** DOC-8, DOC-9, WORK-7. 74 criteria. A10, A11 and A12 resolved; new C10 for codex-dev.

**Revision 2 (2026-11-08): ada's model defaults.**

- **Every bug has an area.** Each project starts with **General**, owned by its first team, so "No area" is gone. General can be renamed but not deleted (A13). An area's team must be one of the project's teams. An area can have its own **area lead** (a member of that team, default the team's lead), who triages and verifies.
- **Severity words:** **Critical · Major · Minor · Trivial** (was Critical, High, Medium, Low). The reporter sets it (required, each level explained), and triage confirms it.
- **Priority is a field** shared by tasks and bugs: **Urgent · High · Normal · Low**, default Normal, set at triage for bugs and on creation for tasks. Lists and the To do column sort by it. Manual To do ordering (Move to top/up/down) is removed. C2 is resolved.
- **Triage and verify:** the area lead (or the team's lead) triages, and the reporter or the area lead verifies. The owner can always do both. A12 is resolved.
- **Removing a team from a project** now asks which of the remaining teams takes its areas (§8).
- **Criteria rewritten (same ID):** PRJ-3, TSK-4, TSK-5, BUG-4, BUG-5, BUG-10, BUG-13, TRI-1, TRI-2, TRI-3, TRI-4, FIX-6, AREA-2, AREA-5, AREA-6, AREA-7, WORK-2, WORK-5.
- **Criteria added:** TSK-10, TRI-8, AREA-8, AREA-9. 71 criteria.

**Revision 1 (2026-11-08):** first draft. 16 decisions, 9 flows, 67 criteria, questions C1–C9 for codex-dev, A10–A12 for ada, U7–U9 for the user.

## 0. Decisions in this spec

1. **The project page is where work lives.** Tabs: **Overview | Features | Tasks | Bugs | Debt | Docs | Insights | Settings** *(rev 5; was Overview, Tasks, Bugs, Docs, Settings)*. Bugs sit next to tasks and docs, never in a separate tool.
2. **One cross-project view, Work,** with pill tabs **Tasks | Bugs | Debt | Requests** *(rev 5)* (the Org chart pattern). It replaces the v1 Tasks nav item. Nav: Home · Chat (`chat-ux.md`) · Org chart · Projects · **Work** · Knowledge · … · Offices · Settings.
3. **Tasks and bugs are numbered separately in each project, and the prefix says which** *(rev 6, ada's W7 model; was one shared sequence)*: tasks `T-12`, bugs `B-7`. Outside the project (Work, messages, Activity, other projects, commits and branches) the project key comes first: `chk-T-12`, `chk-B-7`. Requests are `REQ-14` across the organization, features `FEAT-0007` (`chk-FEAT-0007` outside the project) and debt `DEBT-0012`. The type icon and the word still show with the number ("Bug B-7"). An item that has no number yet shows **Pending** and a 6-character local short id: "Pending · 3k9f2x" (§12). (C1, resolved.)
4. **Severity is the bug's impact,** in four plain words: **Critical · Major · Minor · Trivial**. The reporter sets it, helped by a one-line meaning for each level, and triage confirms it.
5. **Priority is the order of work, separate from severity,** and shared by tasks and bugs: **Urgent · High · Normal · Low**. Normal is the default and isn't shown on rows and cards, so only the exceptions stand out. Lists and the To do column sort by priority, then age. *(rev 2: was a manual To do order.)*
6. **Bug statuses the owner sees:** **New · Triaged · In progress · Fixed · Closed**. Closed always has a resolution: **Verified · Duplicate · Won't fix · Can't reproduce**. "Reopened" isn't a status: a reopened bug goes back to Triaged with a **Reopened** badge and its count.
7. **Who triages:** anyone can (no permissions in phase 1). The app **suggests** the area lead (by default the area's team's lead), or the owner when there's neither, routes the request there, and records who triaged. A bug always has an area, General at worst (decision 13). *(rev 3)*
8. **A bug never turns into a task.** **Create fix task** makes a linked task. A bug can have several fix tasks, and a task can fix several bugs.
9. **The bug follows its fix tasks, and says so:** it moves to In progress when its first fix task starts and to Fixed when all of them are Done and at least one is **Done · Completed** *(rev 6)*. Cancelled work fixes nothing: when every fix task was cancelled, the bug keeps its status and says "All its fix tasks were cancelled. Create a fix task, or close the bug." A bug with required checklist steps unticked doesn't go Fixed by itself, and the activity says why. Every automatic change is written in the activity ("Fixed when T-13 was done") and can be undone by hand. (C3.)
10. **Verification is suggested to the reporter.** A Fixed bug asks its reporter **It's fixed** (Closed · Verified) or **Still happens** (back to Triaged, Reopened, with a required note). Anyone else can answer too, and the activity names who did. *(rev 3)*
11. **Closing a bug without a fix always needs a reason** (Duplicate needs the original bug; Won't fix and Can't reproduce need a sentence). The reporter is told.
12. **Reporting asks for little and checks for duplicates:** title, "What happened" and severity are required. The area starts on General, and steps, expected and environment are optional. Environment is filled in for you. While the title is typed, up to three **similar open bugs** show, so the reporter can add to an existing bug instead.
13. **Areas are flat** (no nesting), managed in project **Settings › Areas**. Each is owned by exactly one of the project's teams and has an optional area lead. Every project has **General** (owned by its first team, or by no team while it has none, AREA-7), so a bug never lacks an area. Deleting an area with open bugs asks which area they move to.
14. **Agents and people are equals in work.** Either can report, be assigned, triage and verify. They are told apart by shape and words (`offices-and-teams-ux.md` §7.0). The owner is first in every assignee picker.
15. **Docs are Markdown files in the project's repository** (`docs/`), with the project brief pinned first. Saving makes a commit on a docs branch that lands through the project's merge flow (`decisions-and-tech-debt-ux.md` §0.2). Any `T-12` or `B-7` in a doc (or `chk-T-12` from another project) links to that task or bug, and the item lists the docs that mention it.
16. **Every drag has a menu path and a keyboard path** (board columns, priority), as in the org chart.
17. **A checklist is steps inside one task, not small tasks.** Items are ordered, ticked by anyone, and record who ticked them and when. Any item can become its own task (**Make a task**), and then it's ticked automatically when that task is **Done · Completed**, never by hand. *(rev 4; rev 6: completed only)*
18. **Required steps guard Done, but never trap you.** Done with required steps unticked asks first and needs a reason ("Mark done anyway"), which is recorded and shown on the task. Optional steps never block. *(rev 4, C19)*

## 1. Words (proposed additions to the design-system glossary)

| Term | Means | Don't say |
| --- | --- | --- |
| Work item | A task or a bug. A word for engineering and this spec; the UI says "task" or "bug". | Ticket, issue, item (in copy) |
| Task | Work someone is asked to do in a project | Job, story, todo |
| Bug | Something in the project that doesn't work as it should | Defect, issue, incident |
| Area | A part of a project, owned by one team. Every bug sits in one. | Component, module, label |
| Severity | How bad a bug is: Critical, Major, Minor, Trivial | Priority, impact score |
| Priority | How soon a task or bug should be done: Urgent, High, Normal, Low | Rank, severity |
| Area lead | Who is asked to triage an area's bugs and check its fixes. By default, the lead of the area's team. | Area owner, component owner |
| General | The area every project starts with | Default area, Uncategorized |
| Triage | Checking a new bug: confirming its area and severity and choosing who fixes it | Grooming, sorting |
| Reporter | The agent or person who reported the bug | Author, submitter |
| Assignee | The agent or person doing the task or fixing the bug | Owner (that's the organization's owner) |
| Fix task | A task created to fix a bug, linked to it | Subtask, child |
| Checklist / step | The ordered steps inside a task or bug. A **required** step must be ticked before Done (or Fixed). | Subtask, todo, acceptance criteria |
| Lead time · Cycle time | Created → done · started → done | Age, duration (in labels) |
| Doc | A Markdown file in a project's `docs/` folder | Wiki page, note, file |
| Brief | The project's first doc: what it's for, its goals and context | README, charter |

"Owner" stays reserved for the organization's owner. An area is **owned by** a team, but the column header says "Team", not "Owner".

## 2. Visual direction (Tailwind Plus application UI, as in offices-and-teams §2)

| Need | Component |
| --- | --- |
| Project tabs | *Tabs with underline* (the page's sections), not pills: pills stay for views of one thing (Org chart, Work) |
| Bug list | *Stacked list* grouped under *Sticky headings* (area name + severity counts) |
| Task board | Four columns of cards on `bg-gray-800/50`, `outline-white/10`. Each column is a list for screen readers |
| Task list, Work roll-up | *Table with sticky header* |
| Report bug, New task, New doc, Add area | *Slide-over* (right, `max-w-lg` for Report bug, `max-w-md` for the others) |
| Bug and task detail | Full-width page inside the project (breadcrumb Projects › checkout › Bugs › B-11), main column + *Description list* side column |
| Close as…, Delete area, Mark as duplicate | *Modal dialog* |
| Severity | *Badge* with an icon and the word, never color alone |
| Activity | *Feed with comments* (newest last, comment box at the bottom) |
| Docs | Two panes: doc list (left, `w-64`) and reader/editor |

### 2.1 Severity and status tokens

| Severity | Icon (Lucide) | Color | Screen reader |
| --- | --- | --- | --- |
| Critical | `flame` | red-400 on red-400/10 | "Severity critical" |
| Major | `octagon-alert` | orange-400 on orange-400/10 | "Severity major" |
| Minor | `info` | yellow-400 on yellow-400/10 | "Severity minor" |
| Trivial | `feather` | gray-400 on gray-400/10 | "Severity trivial" |

The report form explains each level: **Critical**: "Data loss, money or security, or nobody can work." **Major**: "A main feature is broken, with no good way around it." **Minor**: "Something is wrong, but there's a way around it." **Trivial**: "Cosmetic: wording, spacing, a small glitch."

| Priority | Icon | Shown as | Screen reader |
| --- | --- | --- | --- |
| Urgent | `chevrons-up` | amber-400 text and icon | "Priority urgent" |
| High | `chevron-up` | gray-200 text and icon | "Priority high" |
| Normal | `minus` | hidden on rows and cards; shown in detail views | "Priority normal" |
| Low | `chevron-down` | gray-500 text and icon | "Priority low" |

Priority is never colored like severity (only Urgent gets a color, amber), so the two scales can't be mistaken for each other.

| Bug status | Icon | Color | Task status | Icon | Color |
| --- | --- | --- | --- | --- | --- |
| New | `circle-dot` | blue | To do | `circle` | gray |
| Triaged | `circle` | gray | In progress | `circle-play` | indigo |
| In progress | `circle-play` | indigo | Blocked | `circle-alert` | amber |
| Fixed | `circle-check` | green outline | Done | `circle-check` | green |
| Closed · Verified | `check-check` | green | | | |
| Closed · Duplicate, Won't fix, Can't reproduce | `circle-slash` | gray | | | |

A **Reopened** badge (`rotate-ccw`, amber, "Reopened 2×") adds to the status and never replaces it.

The task statuses are v1's (`TaskChip`), unchanged, so the agent CLI, the board and the v1 spec share one vocabulary. Done has a resolution: **Completed** (the default) or **Cancelled**, shown as "Done · Cancelled" *(rev 6, C4)*. Only a completed task fixes a bug, delivers a feature, pays back debt or ticks a step.

## 3. Flow 1 · Project page

Nav **Projects** → list (as v1 §3.9, plus the columns **Teams** with Shared, **Open bugs** with a severity mini-count and **Needs triage**) → project page.

```
 Projects › checkout
 ┌──────────────────────────────────────────────────────────────────────────────┐
 │ checkout  chk   Teams: Checkout · Platform  [Shared]   Leads: Riley, ada      │
 │ Payments and the checkout flow of the Lumen web app.   [New task] [Report bug]│
 │ ──────────────────────────────────────────────────────────────────────────── │
 │  Overview  Features 4  Tasks 12  Bugs 7 ●  Debt 14  Docs 5  Insights  Settings │
 └──────────────────────────────────────────────────────────────────────────────┘
```

- Header actions: **New task** (primary) and **Report bug**. Both are also in ⌘K ("Report a bug in checkout…").
- The Bugs tab count is open bugs. The dot (amber, "2 need triage") shows while any bug is New.
- **Overview:** four tiles and a feed. *(rev 5: the Overview tab is the project's Status page, `project-status-ux.md` §1, and these tiles are part of it.)*

```
 ┌ Needs triage ─┐ ┌ Open bugs ──────────────┐ ┌ Tasks ───────────────────┐ ┌ Brief ──────┐
 │ 2             │ │ 1 Critical · 2 Major     │ │ 3 In progress · 1 Blocked│ │ Updated 2 d │
 │ Triage now →  │ │ 3 Minor · 1 Trivial      │ │ 8 To do · 14 Done (7 d)  │ │ Open →      │
 └───────────────┘ └──────────────────────────┘ └──────────────────────────┘ └─────────────┘
 Recent activity
 [R] Riley moved T-14 Fix rounding in VAT total to Done · 5 min ago
 (S) Sam Ortiz reported Bug B-15 Apple Pay button missing on Safari · 1 h ago
```

- **Settings:** Name · Description · Teams (offices-and-teams §7.9) · **Areas** (Flow 6). The key isn't editable.
- **States:** loading (header at once, tab content as skeletons) · project removed while open ("This project was deleted." **Go to Projects**) · office stopped (read-only, banner from offices-and-teams §3.2, every write action disabled with the reason in its tooltip) · unassigned project (Teams shows "No team · **Assign team…**", and the leads line is left out).

## 4. Flow 2 · Report a bug

**Entry points:** the project header and Bugs tab (**Report bug**) · ⌘K · a task's ⋯ › **Report a bug from this task** (links it, prefills the area from the task's team if that team owns exactly one area) · a doc's ⋯ › **Report a bug about this doc** (links the doc). Agents report through `org-agent` (C5). Their bugs look the same, with the agent as reporter.

```
 Report a bug in checkout                                              [×]
 ─────────────────────────────────────────────────────────────────────────
 Title *            [Apple Pay button missing on Safari              ]
                    Similar open bugs:
                      ◦ B-9    Apple Pay sheet closes on iOS 18 · Major · Payments
                      [This is the same bug]   (shown only on hover/focus of a row)
 What happened? *   [On the payment step in Safari 18 there's no Apple Pay
                     button. Chrome shows it.                              ]
 What did you expect? [The Apple Pay button above the card form.           ]
 Steps to reproduce [1. Open checkout in Safari 18 on macOS  2. …          ]
 Area               [General — Checkout team                         ▾]
                    "Not sure? Leave General. Triage moves it."
 Severity *         ( ) Critical  ( ) Major  ( ) Minor  ( ) Trivial
                    Minor: Something is wrong, but there's a way around it.
 Environment        Studio Mac · Agentic Org 1.2.0 · branch main @ 4f2a91c   [Edit]
 Link to            T-12 Payment step redesign (task)    [×]   [+ Link]
 ─────────────────────────────────────────────────────────────────────────
 Goes to: Riley (General, Checkout team) for triage.        [Cancel] [Report bug]
```

- **Required:** Title (1–120 characters), What happened (1–10,000) and Severity. **Report bug** is disabled with "Add a title, what happened and a severity" until all three are set. The meaning of the selected (or focused) severity shows under the options.
- **Similar open bugs:** after 3 characters, up to three open bugs in this project whose titles match, with their severity and area. **This is the same bug** closes the slide-over, adds "What happened" as a comment on that bug, adds the reporter as a watcher (C6), and opens it. The toast says "Added to B-9."
- **"Goes to"** always states who triages it: the area lead (with the area and team), or "you" when the area has no lead and its team has none.
- **Environment** is prefilled (the office, the app version, and the project's current branch and commit when it has a local repo) and can be edited or cleared. It's shown as text, never hidden.
- **Success:** the slide-over closes, the bug opens, and a toast says "Bug B-15 reported. Riley will triage it." The bug is **New** and never assigned until triage.
- **States:** submitting ("Reporting…", the button disabled) · validation (inline, under the field) · office stopped (Report bug disabled: "The office isn't running. Restart it from Offices.") · draft kept: closing with text asks "Discard this bug report?" (**Discard** · **Keep editing**) · the area deleted while the form was open (the field goes back to General with "Payments was removed, so this goes to General.").

## 5. Flow 3 · Triage

Triage is the step from **New** to **Triaged**: confirm the area and severity, set the priority, pick an assignee, or close the bug.

**Where triagers find it:**
- The project's Bugs tab opens on **Needs triage** when there's any (a pinned section above the areas).
- **Work › Bugs** has a saved filter **Needs my triage** (New bugs suggested to me).
- **Home / Needs you:** the owner gets "2 bugs need triage in checkout" only when the owner is the suggested triager (decision 7). **Critical** bugs always reach the owner's Needs you, whoever triages (U7).
- An agent triager gets the request through its inbox (C5).

**Triage panel** (on the bug page while the bug is New, for everyone, headed "Suggested: Riley (area lead)", or "Suggested: you" for the suggested person):

```
 ┌ Triage ───────────────────────────────────────────────────────────────────┐
 │ Area       [Payments — Checkout          ▾]   (set by Sam Ortiz)             │
 │ Severity   ( ) Critical  (•) Major  ( ) Minor  ( ) Trivial  (set by Sam)     │
 │ Priority   [Normal ▾]                                                        │
 │ Assign to  [Riley — Checkout            ▾]   ☐ Create a fix task now         │
 │                                                                              │
 │ [Close as… ▾]                                      [Mark as triaged]         │
 └──────────────────────────────────────────────────────────────────────────────┘
```

- **Mark as triaged** confirms the area and severity the panel shows and sets the priority (Normal unless changed). Assignee is optional ("Triaged, nobody assigned" is a valid state, shown in the list as "Unassigned").
- **Rerouting:** choosing an area with another area lead shows "This sends it to ada (Taxes, Platform team) for triage." The button becomes **Send to ada**. The bug stays New with the new area, and the suggested triager changes. Nothing else is set. The area picker lists only this project's areas.
- **Create a fix task now:** after triage, the new-task slide-over opens prefilled (Flow 5).
- **Close as… ▾:** Duplicate of… · Won't fix · Can't reproduce (decision 11, Flow 5.3).
- **Assignee picker:** the owner first ("Alex Kim (you)"), then members of the area's team, then everyone else. Each row has a shape and a word (agent/person) and its team.
- **States:** someone else triaged it first ("Riley triaged this a moment ago. Here's what they chose." The panel is replaced by the result, nothing overwritten) · area deleted during triage (the field clears and says so) · assignee removed from the organization (the picker drops them; a stale selection shows "Jo Park was removed" and blocks Mark as triaged until changed).

## 6. Flow 4 · The project's bug list

```
 Bugs   [Open ▾] [All severities ▾] [All teams ▾] [Anyone ▾]  [Search bugs]   Group: [Area ▾]
 ──────────────────────────────────────────────────────────────────────────────────────────
 NEEDS TRIAGE · 2
   ● B-15    Apple Pay button missing on Safari            ◌ —        (S) Sam Ortiz · 1 h
   ● B-16    Totals flicker when the coupon is removed     ◌ —        [R] River · 3 h
 PAYMENTS · Checkout · 1 Critical · 1 Major
   🔥 B-11    Card charged twice on double click   Critical ⇈Urgent In progress [R] Riley ↳ T-13
   ⚠ B-9     Apple Pay sheet closes on iOS 18     Major             Triaged     Unassigned
 ADDRESS FORM · Checkout · 1 Minor
   ⓘ B-7     Postcode accepts letters for US      Minor             Fixed — verify (S) Sam
 TAXES · Platform (area lead ada) · 1 Trivial
   ~ B-4     VAT label says "Tax" in the UK       Trivial  ⌄Low     Triaged     [A] ada  Reopened 1×
 GENERAL · Checkout · 0  (hidden when it has no open bugs)
```

- **Grouping:** Area (default) · Severity · Priority · Status · Assignee · none. **Needs triage** is always pinned first when it has bugs, whatever the grouping.
- **Order within a group:** severity (Critical first), then priority (Urgent first), then oldest first.
- **Group headers** show the area, its team (and the area lead when it isn't the team's lead), and counts by severity in words ("1 Critical · 1 Major"), never color alone.
- **Filters:** Status (**Open** = New + Triaged + In progress + Fixed, the default · Closed · All) · Severity · Priority · Team · Assignee (Anyone · Me · Unassigned · a member) · Reporter · search (title and number). The view state is remembered per project.
- **Row:** severity badge · number · title · priority (unless Normal) · status · assignee (or Unassigned) · fix-task chips (`↳ T-13`) · Reopened badge · age. The row opens the bug page. ⋯ offers Assign… · Change severity… · Priority ▸ · Move to area… · Close as… · Copy link.
- **Fixed rows** say "Fixed — verify" and show the reporter, who is the one asked to verify.
- **States:** loading (group headers as skeletons) · no bugs ever: "No bugs reported in checkout. When something doesn't work as it should, report it here so the team that owns that part can fix it." **Report bug** · no open bugs: "No open bugs. 23 closed." **Show closed** · no matches: "No bugs match these filters." **Clear filters** · area with no open bugs: hidden under Open, shown under All.

## 7. Flow 5 · Bug page, fix, verify, close and reopen

### 7.1 Bug page

```
 Projects › checkout › Bugs › B-11
 ┌─────────────────────────────────────────────────────┬──────────────────────────────┐
 │ 🐞 Bug B-11                                          │ Status     In progress        │
 │ Card charged twice on double click                   │ Severity   🔥 Critical  [✎]   │
 │                                                      │ Priority   ⇈ Urgent      [✎]  │
 │ Area       Payments · Checkout│
 │ What happened                                        │ Assignee   [R] Riley     [✎]  │
 │ Double-clicking Pay sends two charges…               │ Reporter   (S) Sam Ortiz      │
 │ What I expected                                      │ Triaged by [R] Riley · 2 h ago│
 │ One charge, the button disabled after the click.     │ Environment                   │
 │ Steps to reproduce                                   │   Studio Mac · 1.2.0 · main@4f│
 │ 1. … 2. …                                            │ Fix tasks                     │
 │                                                      │   ↳ T-13 Disable Pay while    │
 │ [Create fix task] [Close as… ▾]  [⋯]                 │     submitting · In progress  │
 │ ──────────────────────────────────────────────────── │   [+ Link a task]             │
 │ Activity                                             │ Links                         │
 │ (S) Sam reported it · 3 h ago                        │   Duplicates: B-10            │
 │ [R] Riley triaged: Critical, Urgent, Riley · 2 h  │   Commits: 9c1e0b2            │
 │ ⚙ In progress when T-13 started · 1 h ago  [Undo]    │   Branch: task/chk-T-13-di…   │
 │ (S) Sam: still happens on the Apple Pay path · 20 m  │   Docs: Payment step design   │
 │ [Comment…                                   ] [Send] │   [+ Link]                    │
 └─────────────────────────────────────────────────────┴──────────────────────────────┘
```

- **Editable in place** (the pencil, or the field's menu): title, the three text sections, severity, priority, area, assignee and environment. Each change is an activity entry ("Riley changed severity Major → Critical").
- **Automatic changes** (decision 9) are entries with a gear icon and an **Undo** for 1 hour (C3).
- **Commits and branches** come from the project's local git (W4) when a commit message or branch name mentions `chk-B-11` (C7). They show as text plus a short hash, and open in the Git view when there is one.
- **Mentions:** `T-12` or `B-11` in any text in the project (or `chk-T-12` anywhere) links to the item. `@Riley` (typed by name, completed to the handle) notifies (C5).

### 7.2 Fix tasks

- **Create fix task** opens New task (Flow 7) prefilled: title "Fix: Card charged twice on double click", the assignee and priority from the bug, the team from the area, and the description quoting the bug's "What happened" with a link. The task is linked both ways.
- **Link a task** links an existing task in the same project (C8).
- The bug's status follows its fix tasks (decision 9): first fix task In progress → bug In progress · all fix tasks Done, at least one Done · Completed → bug **Fixed** *(rev 6)* · a fix task reopened while the bug is Fixed → bug back to In progress. Unlinking the last fix task doesn't change the status.
  - **Every fix task cancelled:** the bug keeps its status, and the Fix tasks section says "All its fix tasks were cancelled. Create a fix task, or close the bug." with **Create fix task** and **Close as…**. *(rev 6)*
  - **Required steps left:** when the last fix task is done but the bug's checklist still has required steps unticked, the bug doesn't go Fixed. The activity says why ("⚙ All fix tasks are done, but 1 required step isn't ticked, so it isn't Fixed yet"), and **Mark as fixed** asks for a reason, as Done does (§9.1). *(rev 6)*
- A bug can also be marked **Fixed** by hand (⋯ › Mark as fixed) when the fix needed no task. It asks for a sentence ("What fixed it?").

### 7.3 Verify, close and reopen

- **Fixed:** the bug page shows a banner to everyone. The reporter reads "Riley fixed this. Is it fixed for you?", and others read "Riley fixed this. Sam Ortiz (reporter) is asked to check." Both get **It's fixed** · **Still happens…**. The request goes to the owner's Needs you only when the owner is the reporter. An agent reporter gets it in its inbox and answers with the CLI (C5).
  - **It's fixed** → Closed · Verified.
  - **Still happens…** → a required note → Triaged, **Reopened** badge, the same assignee, and the fix tasks stay linked (the done ones show struck through).
  - Anyone can answer for a reporter who's gone or slow. The activity says who verified ("Verified by Riley, not the reporter").
- **Close as… ▾** (any open status, anyone):
  - **Duplicate of…** → a bug picker (this project, open bugs first) → Closed · Duplicate. The original shows "Duplicates: B-10", and the duplicate's reporter becomes a watcher of the original (C6).
  - **Won't fix** and **Can't reproduce** → a required reason (1–1,000 characters) shown at the top of the closed bug.
  - The dialog says who will be told: "Sam Ortiz (reporter) will see this."
- **Reopen** (on a closed bug, anyone in the organization) → a required note → Triaged with **Reopened n×**. A bug closed as Duplicate reopens as New, without a duplicate link, so triage looks at it again.
- **States:** stale edit (as offices-and-teams §7.10: "B-11 changed while you were editing. Here's the latest.") · the reporter was removed from the organization (the request moves to the area lead: "Sam Ortiz is no longer in Lumen Labs, so Riley is asked to check.") · a linked task deleted (the chip shows "T-13 was deleted", and the status rule ignores it) · bug deleted (C9: I'd allow it only for the owner, with a confirmation, and keep the number unused).

## 8. Flow 6 · Areas (project Settings › Areas)

```
 Areas                                                              [Add area]
 Every bug sits in one area. The area lead is asked to triage its bugs and check fixes.
 ──────────────────────────────────────────────────────────────────────────────
 General         Checkout     Area lead: Riley (team lead)     0 open bugs   [⋯]
 Payments        Checkout     Area lead: Riley (team lead)     3 open bugs   [⋯]
 Address form    Checkout     Area lead: [R] River             1 open bug    [⋯]
 Taxes           Platform     Area lead: ada (team lead)       1 open bug    [⋯]
```

- **Add area** (slide-over): Name (1–40 characters, unique in the project, case ignored) · Team (required; only the project's teams, with the path; prefilled when the project has exactly one) · Area lead (optional; a member of that team, agent or person; the placeholder shows "Team lead (Riley)", or "Team has no lead: you'll triage"). **Add area**.
- **⋯ › Rename…** (bugs keep their area: the area's id doesn't change) · **Change team…** (only the project's teams: "The 3 open bugs in Payments go to Platform. ada will triage new ones." An area lead who isn't in the new team is cleared, and the dialog says so) · **Change area lead…** · **Delete…** (not on General).
- **Delete an area with open bugs:** "Delete Payments? Its 3 open bugs need another area." with a required **Move them to** picker (the other areas, General first). Moved bugs keep their status; New ones go to the new area lead's triage. Closed bugs keep the old name, shown as "Payments (deleted)".
- **Only General:** a hint under the list: "Split the project into areas so each bug reaches the team that owns that part." **Add area**. **General** can be renamed and moved to another of the project's teams, but not deleted (A13).
- **A team leaves the project** (offices-and-teams §7.9, or the team is deleted, §7.10): when the project keeps other teams, the dialog lists the team's areas with a required **Give its areas to** picker (the remaining teams), and areas whose lead was in that team fall back to the new team's lead. When it was the project's last team, its areas show an amber "No team · **Choose team…**" once the project has a team again, and until then the owner triages their bugs.
  - **Deleting a team** *(rev 6)* keeps the same required **Give its areas to** picker, one per project that keeps other teams. The model's team delete leaves the team's areas without a team (ada's W7 model), so the app first gives each area to the chosen team, then deletes the team. Areas become "No team" only in a project with no other team left.
  - **If giving an area its new team fails** *(rev 7)*: the dialog stays open and names the area that failed ("Couldn't give Payments to Platform") with **Retry**, and the team isn't deleted. Areas already given keep their new team.
- **The area lead leaves the team or the organization:** the area falls back to the team's lead, and the row says "Area lead: Riley (team lead)".

## 9. Flow 7 · Tasks: board, list and task page

**Tasks tab:** a **Board | List** toggle (remembered) and the toolbar: **New task** · filters Team · Assignee · Priority · Has bugs · search.

```
 To do · 8                In progress · 3           Blocked · 1               Done · 14 (7 days)
 ┌──────────────────────┐ ┌──────────────────────┐ ┌──────────────────────┐ ┌──────────────────┐
 │ T-17 ⌃High Coupon    │ │ T-13 ⇈Urgent         │ │ T-12 Payment step    │ │ T-14 Fix VAT     │
 │ edge cases           │ │ Disable Pay while sub│ │ redesign             │ │ rounding         │
 │ [R] River · Checkout │ │ [R] Riley · ◎ goal   │ │ (S) Sam · Checkout   │ │ [R] Riley        │
 │                      │ │ 🐞 fixes B-11 🔥     │ │ "Waiting for copy"   │ │                  │
 └──────────────────────┘ └──────────────────────┘ └──────────────────────┘ └──────────────────┘
```

- **Columns** are the task statuses (§2.1). Done shows the last 7 days, with **Show all done**.
- **Card:** number · priority (unless Normal) · title · assignee (shape + name) · team · the goal chip when the assignee's session reports one (v1 rule R#33) · a fix chip with the bug's severity · Blocked shows its reason.
- **Moving:** drag between columns, or ⋯ › **Move to ▸** To do / In progress / Blocked / Done (Done · Completed), plus ⋯ › **Cancel task…** (Done · Cancelled, with a reason, which needs a cancel-reason field in ada's model, A48; CLI `org-agent task done <id> --cancelled --reason "…"`), or the keyboard (select a card with Space, arrow to a column, Space to drop; announced "T-17 moved to In progress"). **Blocked needs a reason** (one line), or **Waiting for a decision** with a doc picker, which shows "Waiting for your decision on ADR-0004" with **Open** on the card and task page (ada Q11). Each column sorts by priority, then age (decision 5). Priority changes through ⋯ › **Priority ▸** or the task page, and the card moves to its place with the change announced.
- **Waiting for a decision** *(rev 7, ada Q11)*: the block names its decision doc (`blocked_on: ADR-0004`, A49), not only a text reason.
  - **Accepted:** the block clears by itself and the task goes back to the status it had before Blocked. Activity: "ADR-0004 accepted, so T-17 is unblocked".
  - **Rejected:** the task stays Blocked with the reason "ADR-0004 was rejected: <reason>". It's now a normal block: counted as blocked, not waiting on you.
  - **Request changes…:** a normal block too, until the ADR is proposed again.
  - Only while the ADR is proposed and is the task's blocker does that span count as waiting on you (§9.2).
- **List:** a table with the same columns as v1 §3.10 (status, number, priority, title, assignee, team, fixes, goal, updated), grouped by status, Blocked first.
- **New task** (slide-over): Title * (1–120) · Assignee (optional; the owner first) · Team (the project's teams; prefilled when there's one) · Feature (optional, the project's features; prefilled when started from a feature) · Priority (Normal) · Description (Markdown) · Definition of done (one or two sentences: the goal the agent works to) · Fixes (bug picker; prefilled from Create fix task) · **Start now** for an agent with no session running (v1 §3.7 rule: never typed into a running session). **Create task**. Toast "Task T-17 created and given to River."
- **Task page:** like the bug page. Main column: description, definition of done, activity. Side column: status, priority, assignee, team, fixes (bugs, with severity), branch and worktree (W4: "Worktree: ~/…/chk-T-17, branch task/chk-T-17-coupon-edge-cases"), commits, docs, **From** (the request it came from, "REQ-12 · <title>", *rev 9*), **Open session** / **Start a session on this task**.
- **States:** board loading (columns with skeleton cards) · no tasks: "No tasks in checkout yet." **New task** · no matches: **Clear filters** · move refused by the office (e.g. a Done task whose worktree has uncommitted changes, C7: the card springs back and the message says why) · someone moved it first (the card animates to its new column, and a toast says "River moved T-17 to In progress").

### 9.1 Checklists (rev 4)

```
 Checklist · 3 of 5 · 1 required left                         [+ Add step] [⋯]
 ☑ 1  Write the migration                         River · 2 h ago
 ☑ 2  Tests pass (cargo test --workspace)   Required   River · 1 h ago
 ☐ 3  Reviewer's READY for the final commit Required
 ☑ 4  Screenshot for UI work                     app-frontend-dev · 20 min ago
 ☐ 5  Update docs/design/teams-and-org-chart.md  → T-42 In progress    (made a task)
```

- **Where:** on the task page, under the description, and on the bug page, under Steps to reproduce (a bug's checklist is "required before Fixed", for example verification steps). The board card shows "3/5" and an amber dot with the word "required" when required steps are left.
- **Adding:** **+ Add step** opens an inline field. Enter adds it and opens the next one, and Esc ends. Pasting several lines adds one step per line. Text: 1–200 characters.
- **Each step:** checkbox · number · text · **Required** badge (word + `lock` icon) · who ticked it and when (secondary text). Hover or focus shows ⋯ with Edit · Required on/off · **Make a task** · Move up/down · Delete.
- **Ticking** is open to everyone (no permissions in phase 1) and is recorded in the activity ("River ticked 'Tests pass' on T-17"). Unticking asks nothing but is recorded too.
- **Make a task:** creates a task with the step's text, in the same project and team, linked as "part of T-17". The step then shows the task's number and status. It ticks itself only when that task is **Done · Completed**, and unticks if the task is reopened. Its checkbox can't be ticked by hand (the tooltip says "Ticks when T-42 is done"). If the task is cancelled, the step stays unticked and shows "T-42 was cancelled" with **Unlink**. Unlinking turns it back into a plain step. *(rev 6)*
- **Reordering:** drag, or ⋯ › Move up/down, or Alt+↑/↓ on a focused step, with the change announced.
- **Done with required steps left:** the Done action (board move, task page, CLI) shows "2 required steps aren't done: Reviewer's READY, Tests pass." with **Mark done anyway…** (a reason is required, and it's shown at the top of the done task: "Done with 1 required step unticked: <reason> · River") and **Cancel**. Unticked optional steps don't ask: the toast says "Done · 1 optional step left unticked".
- **Checklist templates:** project Settings › **Checklists** defines named checklists (for example "Our merge flow: tests pass · own review · reviewer READY · ada's go · merged into v1"). New task offers **Add checklist ▾**, and a project can set one as the default for new tasks or fix tasks. Templates are persistent, so they're files in the repository (`docs/.templates/checklists/<name>.md`, per `decisions-and-tech-debt-ux.md` §3.2). The items on a task are work-item data in the database (C20). *(rev 8)* Processes add to the default: a new task gets the project's default checklist plus the `checklist` steps of the project's processes (organization ⊕ team chain ⊕ project, not private ones; a later assignment doesn't change it), after the project's, with duplicate steps (same text) merged and required winning (`processes-ux.md` decision 9).
- **Agents** (C21): `org-agent task checklist T-17` (lists steps with their numbers and stable ids), `task check T-17 2` / `task uncheck T-17 2`, `task step add T-17 "…" [--required]`, and `task done T-17` fails with the same message as the app unless `--anyway --reason "…"`.
- **States:** empty ("No steps. **+ Add step** or **Add checklist ▾**") · someone ticks while I'm editing (the tick shows live, and my text edit is kept) · a step deleted by someone else while focused (focus moves to the next step, announced) · the made task deleted (the step becomes plain again, with "T-42 was deleted").

### 9.2 Time on a task or bug (rev 4)

The task and bug pages show a **Time** block in the side column (the full design, with cost, is `costs-and-insights-ux.md` §2):

```
 Time
 Lead time 1 d 4 h   (created 7 Nov 09:10 → done 8 Nov 13:20)
   waiting on you 3 h · blocked 0 · the rest 1 d 1 h (working time isn't tracked yet)
 Cycle time 6 h 10 m (started 8 Nov 07:10 → done)
 Typical in App: cycle 3–8 h
```

- **Everything comes from the item's state changes** (created, started, blocked/unblocked, waiting on the owner since/until, done, reopened). Nothing is typed in or tracked separately. A reopened item adds its new spans and shows "Reopened once: times include both rounds".
- **The split** *(rev 6)* is of the lead time: **waiting on you** (a question to you, a Fixed bug you reported waiting for your check, a scoped request waiting for your approval, triage suggested to you, a task Blocked waiting for your decision on a proposed ADR; ada Q11: only while it's the task's blocker, not while an agent works on it or after Request changes, and that span counts as waiting on you, not blocked), **blocked** (Blocked status) and **the rest**. Working time isn't tracked yet (it needs session tracking), so until it is, the block shows "the rest" instead of working and idle, and untracked working time is never counted as idle. Once it's tracked, the split becomes working · waiting on you · blocked · idle (`costs-and-insights-ux.md` §2).
- **While open,** the numbers run ("so far") and say so.
- **Lists:** the task List and the bug list can add **Cycle time** and **Lead time** columns (column picker), sortable. Aggregates (medians by project, area, team and assignee) live in Insights (`costs-and-insights-ux.md` §1).

## 10. Flow 8 · Docs

```
 Docs                         [New doc]   ┌──────────────────────────────────────────────┐
 ┌──────────────────────────┐             │ Payment step design                           │
 │ 📌 Brief                 │             │ docs/payment-step.md · [A] ada · 2 d · [Edit] │
 │ Payment step design   ●  │             │ ─────────────────────────────────────────────  │
 │ Release checklist        │             │ The payment step shows wallets first…          │
 │ Tax rules by country     │             │ Fixes for B-11 must keep the button…           │
 │ [Search docs]            │             │ ─────────────────────────────────────────────  │
 └──────────────────────────┘             │ Mentioned in: B-11 (bug), T-12 (task)          │
                                          │ Mentions: B-11, T-12                           │
                                          └──────────────────────────────────────────────┘
```

- **List:** the Brief pinned first, then docs by last commit. A dot marks docs changed since I last opened them. Search covers titles and text. The title is the file's first heading, or its file name.
- **Header:** the path (mono, with Copy path), who last changed it (the agent from the co-author trailer, "ada · committed as you", or "you") and when, **Edit** and ⋯. It shows the project's integration branch, and a pending change shows **Waiting to merge · View change**.
- **Reader:** rendered Markdown with the v1 §2.8 safety rules (no raw HTML, no remote images, links ask first). `T-12` (or `chk-T-12`) becomes a chip with the type icon and status. The footer lists **Mentioned in** (items that link this doc) and **Mentions**.
- **Edit:** a Markdown editor with **Write | Preview** tabs. **Save** (⌘S) · **Cancel**. Saving commits the file to a docs branch (`docs/<slug>-<short-id>`, created by the office, never the default branch: `decisions-and-tech-debt-ux.md` §0.2, A36) with you as the author and an editable one-line description, prefilled "Update Payment step design" (shown under Save, collapsed). The toast says "Saved Payment step design. It shows here when it's merged." Leaving with unsaved changes asks "Discard your changes to Payment step design?".
- **New doc:** Title * (1–120, unique in the project), with the file name shown as it's made ("docs/release-checklist.md", editable) → opens in Edit. Nothing is committed until the first Save.
- **⋯:** Copy link · Rename (renames the file too, and links keep working) · Delete ("Delete Release checklist? It's removed from the main branch and stays in its history. Links to it from 2 items will show as deleted.") · **Report a bug about this doc** · **History** (the file's commits: author, age, description; opening one shows that version read-only, with **Restore this version**, which saves it as a new commit).
- **Brief:** can't be deleted or renamed. When it's empty: "The brief isn't written yet. Every agent on the project reads it first." **Write the brief**.
- **States:** loading · no docs except an empty Brief (the hint above) · **conflict on save** (a newer commit changed the file since I opened it): "ada saved Payment step design while you were editing." **Compare** (both versions side by side, mine editable) · **Keep mine** (commits mine on top, and theirs stays in History) · **Discard mine** · doc deleted while open ("This doc was deleted." **Go to Docs**, and in Edit my text is kept in a read-only box I can copy) · too large to render (> 1 MB: the first 1 MB plus "This doc is too large to show in full.").

Docs are files in the project's repository (A11). Agents edit them with git like any other file. The office keeps an index (title, path, last commit) for the list and search, so a doc edited outside the app shows up after its commit reaches the integration branch. A project with no repository yet shows only the Brief hint and "Docs live in the project's repository, which isn't set up yet." (C10).

- **States (git):** the commit fails ("Couldn't save Payment step design: <reason>. Your text is kept." **Try again** · Copy text) · the repository is busy ("Saving after River's push…", retried for a few seconds, then the error) · a file in `docs/` isn't Markdown (listed under **Other files** with Reveal in Finder only).

## 11. Flow 9 · Work (cross-project)

Nav **Work**, pill tabs **Tasks | Bugs | Debt | Requests** *(rev 5)*: the same lists as the project tabs, across every project, with a **Project** column and filter. Numbers carry the project key here, since rows come from several projects: `chk-T-12`, `chk-B-7` *(rev 6)*.

- **Saved filters** in a left rail, with counts: **Assigned to me** · **Needs my triage** · **Waiting for me to verify** · **Critical and Major bugs** · **Blocked tasks** · **All**. "Me" is the owner. Agents use their CLI.
- **Default:** Bugs opens on **Needs my triage** if it has any, otherwise **Critical and Major bugs**. Tasks opens on **Blocked tasks** if any, otherwise **All**, grouped by project.
- The nav badge counts what needs the owner: bugs to triage + bugs to verify + blocked tasks assigned to the owner (U8).
- **States:** as the project lists, plus "No projects yet. Work items live in projects." **New project**.

## 12. Live updates and offices

- Changes by agents and other people arrive live: rows and cards update in place, new rows slide in with a 2-second highlight, and nothing reorders under the pointer while a menu is open.
- **Phase 1 is HQ only** on this Mac, so a write fails only when the office has stopped: every write action is disabled with "The office isn't running. Restart it from Offices." The data stays readable from the last copy.
- **Later phases (a Mac whose office can't reach HQ)**, following `docs/design/offices-hq-and-satellites.md` ("Office-first" and "The app on a Mac with only a satellite"):
  - **Items created in the app** (tasks and bugs the owner creates) are created **at HQ**. While HQ is unreachable, the office queues the create and the item shows **Pending** (dashed outline, "Pending · 3k9f2x" with a 6-character local short id) until HQ confirms it. Then the number replaces the short id in place, without reordering lists. A rejected create shows **Blocked** with Retry or Discard (offices-and-teams §6.5).
  - **Changes to items owned by HQ** (for example a task assigned to a person) are queued the same way and show Pending until HQ commits them.
  - **Items an agent creates at its own office** are saved there at once and usable straight away. Only their project number comes from HQ (A10), so they show a local short id with a quiet "number when HQ is back" until then.
  - Organization data changed here (areas, teams) follows offices-and-teams §6.5. *(rev 4 fix, after review: an interim text said work items were never Pending.)*

## 13. Accessibility

- Severity, priority and status are always an icon and a word. Screen readers hear "Bug B-11, Card charged twice on double click, severity critical, priority urgent, in progress, assigned to Riley, agent".
- The board is four labelled lists ("To do, 8 tasks"). Keyboard moving as in Flow 7. Every move is announced.
- Group headers in the bug list are headings (h3), so VoiceOver's rotor jumps between areas.
- Triage and verify banners are regions with labels ("Triage", "Verify fix"), and focus lands on their first control when opened from Needs you.
- Forms: errors are linked to their fields (`aria-describedby`). The required-field reason is shown as text, not only on a disabled button.

## 14. UX acceptance criteria

**Project page (PRJ)**
- PRJ-1 · *(rev 5)* The project page has tabs Overview, Features, Tasks, Bugs, Debt, Docs, Insights, Settings. The header shows name, key, teams (with Shared when more than one), leads, **New task** and **Report bug**.
- PRJ-2 · The Bugs tab label shows the number of open bugs, plus an amber dot with the text "n need triage" for screen readers while any bug is New.
- PRJ-3 · *(rev 2)* Overview shows Needs triage, Open bugs by severity in words (Critical, Major, Minor, Trivial), Tasks by status, and the Brief's last update, each linking to its tab.
- PRJ-4 · When the office isn't running, every write action in the project is disabled, with the reason in its tooltip, and the data stays readable.
- PRJ-5 · The Projects list shows Teams (with Shared), Open bugs with a severity count, and Needs triage for each project.

**Tasks (TSK)**
- TSK-1 · The board has To do, In progress, Blocked and Done columns. Done shows the last 7 days, with **Show all done**.
- TSK-2 · A card can be moved by drag, by ⋯ › Move to, and by keyboard, and each move is announced to screen readers.
- TSK-3 · *(rev 7)* Moving a task to Blocked asks for a one-line reason, or Waiting for a decision with a doc, and the card shows it. When that ADR is accepted, the block clears and the task returns to its status before Blocked, with an activity entry. When it's rejected, or changes are requested, the task stays Blocked as a normal block (counted as blocked, not waiting on you), with the rejection reason shown.
- TSK-4 · *(rev 2)* Every column sorts by priority (Urgent first), then age, and changing a task's priority moves its card and announces the change.
- TSK-5 · *(rev 2)* New task needs only a title, and its priority is Normal unless changed. The assignee picker lists the owner first as "Alex Kim (you)", and agents and people by shape and word.
- TSK-6 · Start now is offered only for an agent with no session running, and nothing is typed into a running session.
- TSK-7 · A task moved by someone else moves on the board without a reload, and a toast names who moved it.
- TSK-8 · The task page shows the fixed bugs with severity, the branch and worktree when the project has a local repo, and its activity.
- TSK-9 · Board or List is remembered per project.
- TSK-10 · *(new, rev 2)* Cards and rows show priority only when it isn't Normal, as an icon and a word. Urgent is amber, and the other levels use no color.
- TSK-11 · *(rev 6)* Task and bug pages show the Time block: lead time, cycle time and the split of the lead time, all derived from state changes. Until working time is tracked, the split is waiting on you, blocked and "the rest", with "working time isn't tracked yet", and untracked working time is never shown as idle. Open items say "so far".

**Checklists (CHK)** *(new, rev 4)*
- CHK-1 · Tasks and bugs have a checklist of ordered steps. The card shows "n/m" and marks required steps left with an amber dot and the word "required".
- CHK-2 · + Add step adds steps inline (Enter for the next, pasted lines become separate steps), each 1–200 characters.
- CHK-3 · Ticking or unticking records who and when, shows it on the step, and appears in the activity.
- CHK-4 · A step can be marked Required, shown with a lock icon and the word "Required".
- CHK-5 · Done (or Fixed for a bug) with required steps unticked asks first and needs a reason, which is shown on the done item. Optional steps never block.
- CHK-6 · *(rev 6)* Make a task turns a step into a linked task. The step ticks itself only when that task is Done · Completed (and unticks if it's reopened), and it can't be ticked by hand. If the task is cancelled, the step shows "T-42 was cancelled" with Unlink.
- CHK-7 · Steps can be reordered by drag, menu and Alt+↑/↓, and the change is announced.
- CHK-8 · *(rev 8)* Project Settings › Checklists defines named checklists stored as files in `docs/.templates/checklists/`, and New task can add one or apply the project's default, which includes the steps of the project's processes (`processes-ux.md` PROC-12).
- CHK-9 · `org-agent task check|uncheck|step add|checklist` and `task done` behave like the app, including the required-steps refusal and `--anyway --reason`.

**Report a bug (BUG)**
- BUG-1 · Report bug can be opened from the project header, the Bugs tab, ⌘K, a task's menu and a doc's menu. From a task or doc, that item is linked.
- BUG-2 · Report bug is disabled until Title and What happened are filled, and the reason is shown as text.
- BUG-3 · After 3 characters of title, up to three similar open bugs in the project show. **This is the same bug** adds the text as a comment to that bug and opens it, and no new bug is created.
- BUG-4 · *(rev 2)* Severity is required, with no default, and the meaning of the selected or focused level shows under the options. Area starts on General.
- BUG-5 · *(rev 2)* The "Goes to" line names the area lead with the area and team, or "you" when neither the area nor its team has a lead, and it updates when the area changes.
- BUG-6 · Environment is prefilled with the office, the app version and the project's branch and commit when there's a local repo, and can be edited or cleared.
- BUG-7 · *(rev 6)* A reported bug is New, unassigned and numbered in the project's own bug sequence (`B-15`), separate from tasks. The toast names the number and the triager.
- BUG-8 · A bug reported by an agent shows the agent as reporter (square avatar, "agent") everywhere a reporter shows.
- BUG-9 · Closing the form with text in it asks "Discard this bug report?".

**Triage (TRI)**
- TRI-1 · *(rev 3)* The triage panel shows on every New bug to everyone, headed with the suggested triager (the area lead, by default the team's lead, or the owner). Whoever triages is recorded in the activity.
- TRI-2 · *(rev 2)* Mark as triaged confirms the area and severity and sets the priority (Normal by default). The assignee is optional, and an unassigned triaged bug shows "Unassigned".
- TRI-3 · *(rev 2)* Choosing an area with another area lead turns the button into "Send to <area lead>". The bug stays New, and that area lead becomes the triager. Only the project's areas are offered.
- TRI-4 · *(rev 2)* The reporter's area and severity are preselected and marked "set by <name>".
- TRI-5 · *(U7 recommended)* A Critical bug appears in the owner's Needs you as soon as it's reported or triaged as Critical, whoever triages it.
- TRI-6 · *(rev 3)* If someone else triages first, the panel is replaced by their result with their name, and nothing is overwritten.
- TRI-7 · *(rev 3)* Work › Bugs › Needs my triage lists exactly the New bugs suggested to the owner across projects.
- TRI-8 · *(new, rev 2)* When an area and its team have no lead, its New bugs go to the owner's triage, and "Goes to" says "you".

**Bug list (BUG, continued)**
- BUG-10 · *(rev 2)* The Bugs tab groups by area by default, pins Needs triage first, and orders each group by severity, then priority, then age.
- BUG-11 · Each area header shows the area's team and its open bugs counted by severity in words.
- BUG-12 · The default filter is Open (New, Triaged, In progress, Fixed), and filters and grouping are remembered per project.
- BUG-13 · *(rev 2)* Grouping by Severity, Priority, Status, Assignee or none works, and Needs triage stays pinned in all of them.
- BUG-14 · A Fixed bug's row says "Fixed — verify" and shows the reporter.
- BUG-15 · The empty, no open bugs and no matches states show the copy from Flow 4.

**Fix, verify, close and reopen (FIX)**
- FIX-1 · Create fix task opens New task prefilled from the bug, and the bug and task are linked both ways.
- FIX-2 · *(rev 6)* When the first fix task goes In progress, the bug goes In progress. When all fix tasks are Done and at least one is Done · Completed, it goes Fixed, unless required steps are unticked (then it doesn't, and the activity says why). When every fix task was cancelled, the bug shows "All its fix tasks were cancelled. Create a fix task, or close the bug." Each automatic change is an activity entry with Undo.
- FIX-3 · Mark as fixed by hand asks what fixed it.
- FIX-4 · *(rev 3)* On a Fixed bug, everyone sees the verify banner with **It's fixed** and **Still happens…**. The reporter reads "Is it fixed for you?", and others see who is asked. It reaches the owner's Needs you only when the owner reported the bug.
- FIX-5 · It's fixed closes the bug as Verified. Still happens needs a note and returns the bug to Triaged with the same assignee and a Reopened badge with its count.
- FIX-6 · *(rev 3)* Anyone can verify a Fixed bug, and when it isn't the reporter, the activity says "Verified by <name>, not the reporter".
- FIX-7 · Close as Duplicate needs the original bug. The original lists the duplicate, and the closed bug links to the original.
- FIX-8 · Close as Won't fix or Can't reproduce needs a reason, shown at the top of the closed bug. The dialog names who will be told.
- FIX-9 · Reopen needs a note and returns the bug to Triaged with Reopened n×. A duplicate reopens as New without the duplicate link.
- FIX-10 · Commits and branches that mention the bug's number in the project's local repo are listed on the bug page.
- FIX-11 · *(rev 3)* If the reporter is no longer in the organization, the verify request goes to the area lead, and the bug says why.

**Areas (AREA)**
- AREA-1 · Settings › Areas lists each area with its team, its area lead and its open bug count.
- AREA-2 · *(rev 2)* Add area needs a unique name (1–40 characters, case ignored) and one of the project's teams (prefilled when there's one). Other teams aren't offered. The area lead is optional and lists only that team's members.
- AREA-3 · Renaming an area keeps its bugs in it, and closed bugs show the new name.
- AREA-4 · Changing an area's team says how many open bugs move and who triages new ones.
- AREA-5 · *(rev 2)* Deleting an area with open bugs requires choosing another area for them, General first. Closed bugs show "<name> (deleted)".
- AREA-6 · *(rev 2)* A new project has the area General, owned by its first team, or by no team while the project has none (AREA-7). General can be renamed and given to another project team, but has no Delete.
- AREA-7 · *(rev 7)* Removing a team from a project, or deleting the team, when the project keeps other teams requires choosing which remaining team takes its areas, and the app gives each area to that team before the team is deleted. *(rev 7)* If giving an area its team fails, the dialog stays open, names that area and offers Retry, and the team isn't deleted. Only when no team is left do its areas show "No team · Choose team…", and the owner triages their bugs. *(Confirmed by ada for W7: an area's owning team is nullable, and creating a bug is never blocked.)*
- AREA-8 · *(new, rev 2)* An area with no area lead shows "Area lead: <team lead> (team lead)", and when an area lead leaves the team or the organization, the area falls back to the team's lead.
- AREA-9 · *(new, rev 2)* Changing an area's team clears an area lead who isn't in the new team, and the dialog says so before saving.

**Docs (DOC)**
- DOC-1 · *(rev 3)* The Docs tab lists the Brief first, then the `docs/` files on the integration branch by last commit, and a dot marks docs changed since the viewer last opened them.
- DOC-2 · Rendering follows the v1 §2.8 safety rules (no raw HTML, no remote images, links ask first).
- DOC-3 · A task or bug number in a doc renders as a chip with type and status, and the item lists the doc under Docs.
- DOC-4 · *(rev 4)* Edit has Write and Preview and saves with ⌘S as a commit on a docs branch (never the default branch) with the owner as author and an editable one-line description. The doc then shows Waiting to merge until the change lands.
- DOC-5 · *(rev 3)* When a newer commit changed the file since it was opened, Save offers Compare, Keep mine and Discard mine, and nothing is lost. A failed commit keeps the text and offers Try again.
- DOC-6 · The Brief can't be deleted or renamed. An empty Brief shows "The brief isn't written yet…" with **Write the brief**.
- DOC-7 · Leaving the editor with unsaved changes asks before discarding.
- DOC-8 · *(new, rev 3)* Each doc shows its path, and History lists its commits. An old version opens read-only with Restore this version, which saves it as a new commit.
- DOC-9 · *(rev 4)* A doc an agent changes outside the app appears in the list once its commit reaches the integration branch, showing the agent (from the co-author trailer) as who changed it, without a reload.

**Work (WORK)**
- WORK-1 · *(rev 5)* Nav has Work (not Tasks), with pill tabs Tasks, Bugs, Debt and Requests over every project, and a Project column and filter.
- WORK-2 · *(rev 2)* The saved filters Assigned to me, Needs my triage, Waiting for me to verify, Critical and Major bugs, Blocked tasks and All show counts.
- WORK-3 · *(U8 recommended)* The Work nav badge counts bugs the owner can triage, bugs waiting for the owner's verification and blocked tasks assigned to the owner.
- WORK-4 · Every task and bug number shows with its type icon and word wherever it appears (lists, chips, docs, activity, toasts).
- WORK-5 · *(rev 2)* Severity, priority and status always show an icon and a word, and screen readers announce them. Priority never uses severity's colors.
- WORK-6 · Changes by others appear without a reload and never reorder a list while a menu is open in it.
- WORK-7 · *(new, rev 3)* Triaging, verifying, closing and reopening are open to everyone. The activity always records who acted, and the suggested person is named before anyone acts.
- WORK-8 · *(new, rev 6)* Inside its project a task shows as `T-n` and a bug as `B-n`. Everywhere else (Work, messages, Activity, other projects) the project key comes first (`chk-T-12`, `chk-B-7`). An item without a number yet shows "Pending · <6-character short id>", and the number replaces it without reordering lists.

## 15. Open questions

**For codex-dev (the model)**
- ~~C1~~ · *Resolved (ada W7): separate T-/B- sequences; the prefix always shows the type.* Tasks are `T-12` and bugs `B-7` in their project, and `chk-T-12`, `chk-B-7` outside it (decision 3). Was: one shared sequence per project.
- ~~C2~~ · *Resolved in rev 2 by ada's default:* priority is a field (Urgent, High, Normal, Low) shared with tasks. My trade-off stays on record for the user: two scales on one bug take more explaining, so Normal is hidden and priority never borrows severity's colors.
- ~~C3~~ · *Resolved (ada, W7):* the library, as the spec preferred. Automatic bug status from fix tasks (decision 9), with an activity entry and an Undo. Is that in the library's rules, or does the app do it? I'd rather it's the library, so the CLI and the app agree.
- ~~C4~~ · *Resolved (ada W7):* Done has a resolution, Completed (the default) or Cancelled, shown "Done · Cancelled". Only completed tasks fix bugs, deliver features, pay back debt and tick made-task steps (rev 6).
- C5 · The agent side: which `org-agent` commands report, triage, verify and comment (agreed with Remy, the `org-agent` owner)? I need their output words to match the UI's (statuses, severities, "Fixed — verify").
- ~~C6~~ · *Resolved (ada, W7):* minimal watchers: the creator, duplicate reporters, and anyone who comments with watch or calls `work.watch`; notifications come with the messaging work. Watchers: should a bug have watchers (the reporter, duplicate reporters, commenters) who are told about changes? I've used them only for duplicates and verification.
- C7 · Commits and branches: linking by the number in commit messages and branch names (`task/chk-T-13-…`: task branches are `task/<ref>-<slug>`, where the ref is the task's number with the project key, or its short id when it has no number yet; projects-git-github.md)? And can the office refuse Done for a task whose worktree has uncommitted changes?
- ~~C8~~ · *Resolved (ada, W7):* same project only. Can a fix task live in another project from its bug (a Platform fix for a checkout bug)? I've kept links within one project for phase 1.
- ~~C9~~ · *Resolved (ada, W7):* delete is app-only and keeps the number. Deleting bugs and tasks: I'd keep numbers unused after a delete. (Doc history is now git's, A11.)
- ~~C19~~ · *Resolved (ada):* Mark done anyway with a recorded reason, with no hard refusal in phase 1. The reason shows on the task and in Activity.
- ~~C20~~ · *Resolved (ada, W7):* checklist items as designed. Checklist items as part of the work item (ordered, with a stable id each, ticked by and at), and a step "made a task" as a link to that task.
- ~~C21~~ · *Resolved (agy-senior-dev):* the §9.1 commands fit. `task check|uncheck` take the step number or its stable id (scripts use the id, so reordering never ticks the wrong step). A blocked `task done` exits 1 with the same message on stderr (and JSON with `--json`).
- ~~C10~~ · *Resolved (ada, W7):* the brief is `docs/brief.md`; a project without a repository has no docs. Docs: is the Brief `docs/brief.md` (or the repo's README)? Saves go to a docs branch (`decisions-and-tech-debt-ux.md` §0.2, A36). Remaining: does a project without a repository yet have docs at all?

**For ada**
- A49 · *(rev 7)* **Waiting for a decision** needs the Blocked state to carry a structured blocker, the decision doc's id (for example `blocked_on: ADR-0004`), not only the text `blocked_reason`, so waiting on you can count that span, and accepting or rejecting the ADR can clear or change the block (§9). Can the model add it?
- A48 · Cancel task asks for a reason ("duplicate of T-9", "not needed any more"), shown on the cancelled task. Can `task.set_status` carry an optional `reason` for Done · Cancelled, and `task done --cancelled --reason` in the CLI?
- ~~A10~~ · *Resolved in rev 3, clarified in rev 4:* HQ gives the project number. Items created in the app go to HQ and show Pending (with a local short id) until HQ confirms them. Items an agent creates at its own office are saved at once and wait only for their number (§12).
- ~~A11~~ · *Resolved in rev 3:* Markdown files under `docs/` in the project's repository, with an office index (title, path, last commit). codex-dev owns the details (C10).
- ~~A12~~ · *Resolved in rev 3:* no permissions in phase 1. Anyone may triage or verify, the UI suggests the area lead and the reporter, and the activity records who acted.
- ~~A13~~ · *Resolved (ada, W7):* General can't be deleted. Can General be deleted? I've made it renamable and movable to another of the project's teams, but not deletable, so "a bug never lacks an area" always holds.

**For the user**
- ~~U7~~ · *Resolved (the user, 2026-11-08): as recommended.* *My recommendation (adopted):* yes, Critical bugs always reach you in Needs you, whoever triages them.
- ~~U8~~ · *Resolved (the user, 2026-11-08): as recommended.* *My recommendation (adopted):* yes, the Work badge counts only what needs you (bugs to triage or verify, your blocked tasks).
- ~~U9~~ · *Resolved (the user, 2026-11-08): as recommended.* *My recommendation (adopted):* yes, words (Critical, Major, Minor, Trivial and Urgent, High, Normal, Low), never S1–S4.

## 16. Out of scope here (follow-ups)

Attachments and screenshots on bugs · labels and custom fields (none, as for teams) · milestones and releases · bugs across projects and moving a bug between projects · email or Slack notices to people · time tracking · import from GitHub Issues (v2, with GitHub) · the agent CLI's own output design (with Remy, C5) · the v1 spec revision 5 and the glossary, which fold this spec in.
