---
kind: spec
title: "Typed tasks, bugs and project docs (UX)"
status: current
owner: lead
created: "2026-11-08"
updated: "2026-11-17"
builds_on: [offices-and-teams-ux]
---

# Spec · Typed tasks, bugs and project docs (UX)

[Organization-data 02](../design/organization-data/02-project-structure.md), [03](../design/organization-data/03-features-and-work.md), [05](../design/organization-data/05-operational-data.md), [06](../design/organization-data/06-identity-and-numbering.md), [08](../design/organization-data/08-record-contracts.md) and [12](../design/organization-data/12-task-assignment-and-history.md) own the fields, sources and assignment and history semantics. The [task ports](../design/tasks-projects-docs.md#32-the-ports-org-work-defines) describe the domain interface. This spec owns what people see.

A task's status is one of `todo`, `in_progress`, `blocked`, `cancelled` and `done`, shown as To do, In progress, Blocked, Cancelled and Done. Workflows specific to one task type (a bug's fix-and-verify cycle, a request's scoping) are [not in v1](#16-not-in-v1); their UI copy is kept there so that v1 does not contradict it.

## 0. Decisions in this spec

1. **Task is the entity.** Bug and Request are types with dedicated views and fields; every type uses T-. An optional project does not determine whether something is a task.
2. **Work spans projects and teams.** Tasks | Bugs | Requests views include projectless team work. Type filters avoid accidental double counting in combined totals.
3. **Task references are complete on their own.** Bug T-A42 reads the same inside Reservations P-8 and outside it, and a projectless request is simply Request T-A43; the creating office mints the reference. Features use F-6 (P-8/F-6 outside the project). There is one task reference form for every type.
4. **Every task has one accountable team.** Project tasks use the project's owner team; team tasks choose one. An assigned team and an individual are optional independent fields. Assignment changes neither accountability nor numbering.
5. **Severity and priority differ.** Severity is optional bug-impact metadata, explained in plain words. Priority is Urgent/High/Normal/Low, default Normal and shared across types. A task may also carry an effort, a whole number of points, shown beside priority and empty when not estimated.
6. **Reporter is immutable.** Show original admitted creator separately from current assignment, latest updater and recorded fix/closure actor. Missing historical provenance stays unknown.
7. **Bug presentation is first class.** Reporting, triage and reproduction, duplicate and closure evidence are kept in task details. Nothing but a title, a description and valid accountability is needed to capture a bug.
8. **Knowledge lives in organization Git.** Projects have docs even with no code repository. A feature's definition is its feature.md; supporting docs require explicit project and optional same-project feature/release/deployment scope.
9. **History is durable.** Every accepted task revision, including shared metadata, has an immutable event; accepted snapshot/history/receipt must recover together. Work assignment does not transfer write authority to an office.
10. **Private notes are a separate required capability.** Author-only CRUD, separate durable storage, no administrator exception, no shared count or existence metadata. Existing note comments remain shared.
11. **No implied execution engine.** Assignment does not start a session or grant access. Handoff preserves worktrees. Budget controls, provider routing and execution placement have their own contracts.
12. **Accessible alternatives are required.** Every drag has menu/keyboard paths; statuses/severity/priority always have words; conflicts preserve unsaved content.

## 1. Words (proposed additions to the design-system glossary)

| Term | Meaning |
| --- | --- |
| Task | One authored work entity, project-scoped or projectless |
| Bug / Request | Task types, shown with a type word/icon and the same T- identity |
| Accountable team | Required owning team, `team_id`; for project work, the project's `owner_team_id` |
| Assigned team | Optional current team doing the work, `assigned_team_id` |
| Assignee | Optional current person or agent, `assignee_id`; independent of assigned team |
| Reporter / originator | Original admitted creator, retained through reassignment and closure |
| Severity | Bug impact: Critical, Major, Minor, Trivial; can be unset |
| Priority | Order of work: Urgent, High, Normal, Low |
| Fix task | Task with authored fixes link to a bug-typed task; inverse fixed_by is derived |
| Shared comment | Visible task discussion, including old comments called note |
| Private note | Author-only task note in the separate private store |
| Supporting doc | Stable-ID organization Git knowledge with required project scope |
| Feature definition | feature.md describes current behavior/rules/limitations, not a supporting-doc duplicate |
| Pending number | A durably saved feature awaiting its number from HQ; not an unsaved draft. A task never shows it |

Use **Accountable team**, **Assigned team** and **Assignee** together where needed; do not call all three Owner. The authenticated organization owner remains a separate security designation. An employee representation of the owner needs a valid home team like other employees.

## 2. Visual direction (Tailwind Plus application UI, as in offices-and-teams §2)

| Need | Component |
| --- | --- |
| Project tabs | *Tabs with underline* (the page's sections), not pills: pills stay for views of one thing (Org chart, Work) |
| Bug list | *Stacked list* grouped under *Sticky headings* (assigned team + severity counts) |
| Task board | Four columns of cards on `bg-gray-800/50`, `outline-white/10`. Each column is a list for screen readers |
| Task list, Work roll-up | *Table with sticky header* |
| Report bug, New task, New doc, Assignment | *Slide-over* (right, `max-w-lg` for Report bug, `max-w-md` for the others) |
| Bug and task detail | Full-width page inside the project (breadcrumb Projects › checkout › Bugs › T-A42), main column + *Description list* side column |
| Close as…, Mark as duplicate | *Modal dialog* |
| Severity | Separate meta text with an icon and word, never color alone |
| Activity | *Feed with comments* (newest last, comment box at the bottom) |
| Docs | Two panes: doc list (left, `w-64`) and reader/editor |

### 2.1 Severity and status tokens

Follow the [shared shell standards](app-shell-ux.md#6a-shared-visual-and-interaction-rules):
one work-status pill per row, with severity and priority as separate meta text.
Use Heroicons as the screen is restyled, not a mixed icon family.

| Severity | Meta presentation | Screen reader |
| --- | --- | --- |
| Critical | red-300 word with exclamation-triangle | "Severity critical" |
| Major | neutral word with exclamation-circle | "Severity major" |
| Minor | neutral word with minus-circle | "Severity minor" |
| Trivial | neutral word with information-circle | "Severity trivial" |
| Not set | "Severity not set" with question-mark-circle | "Severity not set" |

The report form explains each level: Critical means data loss, money/security or
nobody can work; Major a main feature broken with no good workaround; Minor a
problem with a workaround; Trivial cosmetic wording/spacing. Severity is not a
colored badge competing with status. Priority is written as Urgent/High/Normal/
Low, each displayed value with a neutral Heroicon (Urgent chevron-double-up,
High arrow-up, Normal minus, Low arrow-down). Normal may be omitted on rows/cards;
when shown it retains its word and icon. Priority never uses amber merely for urgency;
amber means the work needs the owner. Keep lower-priority text contrast readable.

| Work state | Pill tone |
| --- | --- |
| To do | gray |
| In progress | indigo |
| Blocked on someone other than the owner | red |
| Waiting on the owner | one amber Needs you signal instead of the ordinary pill |
| Done | green |
| Cancelled | neutral outline |

A Reopened count is a neutral textual fact with a rotate icon, not a second
amber status. Done and Cancelled are distinct; neither automatically changes
another record.

## 3. Flow 1 · Project page

Projects → Reservations **P-8**, with a Projects back link. Header: **Accountable team: Payments**; **Contributors: Platform**; real done/total counts; New task and Report bug. The tabs are **Overview | Work | Team | Activity | Conversations**, Overview by default. Work filters shared typed tasks by type. Backlog and Releases join after Work when they are built; Insights is last when it is built. Features, Docs and Settings appear when their own experiences are served; their specs define them.

Overview shows the living Brief, real open/blocked counts and the five latest activity entries. Work shows the project's typed tasks with one status signal; Team shows accountable/contributing teams and then member rows, with only supported authorized association edits. Activity uses full references: “Ana assigned T-A42 to Platform · Ana.” Conversations shows permitted project-linked threads. Empty feeds say **No activity yet** or **No conversations yet**. Projectless work appears in Work/team backlogs rather than an artificial project.

Optional lead and due date show only when backed by explicit data; contributors never imply co-leads. No health or planning date is inferred or invented.

Settings includes project name/slug, accountable team, contributors and repositories. Each repository association shows its own required integration branch. Sharing a repository does not share a branch setting or create duplicate office caches. A project always has a team; a teamless project is never accepted. Archived project records remain readable. Loading uses skeletons; missing/tombstoned records explain their state and offer navigation. Disconnection keeps last known data readable and disables unavailable actions with visible reasons.

## 4. Flow 2 · Report a bug

Report bug opens from project, Work, command palette, task or supporting doc. In project context the required accountable team is shown from project ownership. Outside a project, choose **Project (optional)** and **Accountable team (required)**; selecting a project derives its team. No Area/General requirement appears.

Fields: title (1–120 characters), What happened, optional severity with all four explanations and **Not set**, optional steps/expected result/environment, priority default Normal, optional assigned team and assignee. Store selected reproduction values in `details.what_happened`, `expected`, `steps` and `environment`, distinct from description; confirmation/reopen/closure/duplicate/affected-feature/suggested-employee evidence uses document 03's `task_details` mapping. Tombstones keep those fields. A title and description plus valid accountability are sufficient to capture. Reporter comes from authentication and is not editable.

When a configured copilot drafts the report, each filled field has a visible
and accessibly named **Drafted** marker until that field is edited or explicitly
kept. A severity suggestion says **Suggested by the copilot** or remains a
nonselected hint; it never silently becomes a deliberate severity choice.
Editing one field clears only its marker. A new attempt or Dismiss clears that
panel's stale error while preserving typed text, Undo draft and the ordinary
manual Report bug confirmation. See the existing [copilot contract](../design/smart-features.md#2-bugfrom_sentence-implemented-library-entry-version-2).

After three title characters show up to three similar open bug-typed tasks in the selected scope. “This is the same bug” adds the report as an explicitly shared comment on the chosen existing record; it preserves that record's reporter and creates no second task. Clearly label suggestions from other projects or teams with where they belong. No match does not block reporting.

Environment capture is visible/editable: office/app version and per-repository branch/commit only when actually known, never guessed from a single project cache. Attachments remain hidden until their storage is designed. Closing dirty capture asks before discarding. Submit reports success only after durable acceptance; an accepted bug always has its reference at once, so it says “Bug T-A42 saved”, whereas an unsubmitted draft says **Unsaved**. 

## 5. Flow 3 · Triage

Triage confirms available impact evidence, priority and the desired assignment pair. Show original reported values and author/time distinctly from current values. The accountable team stays visible and read-only here. Suggest active same-organization teams and employees; do not restrict individuals by assigned team/home team or contributors.

Assignment controls offer **No assigned team** and **Unassigned individual** independently. Team-only, individual-only, both and neither are valid. Clearing an individual retains a selected team queue; clearing a team never infers a new one from the individual. Save sends the entire pair plus expected task revision. If someone else changed the task, show the current pair, actor/time and the user's desired pair, then allow an explicit retry at the new revision. Never overwrite silently or accept half of the pair.

Who may triage is an authorization decision of the task service; no field on a record grants it. The triage panel identifies a suggested responsible team or person and records who actually acts.

## 6. Flow 4 · The project's bug list

Rows: type/reference · title · severity (including Not set) · priority exception · status · accountable team · assigned team · assignee · reporter · age. Default grouping is **Assigned team**, with Unassigned team explicit, and sorting severity then priority then age. Alternatives: severity, priority, status, accountable team, assignee or none. No mandatory area group.

Filter by status, severity, assigned or accountable team, assignee, reporter and project or no project in Work. Remember filters and show counts consistently. Empty: “No bugs yet. Report one when something doesn't work.” No open bugs: “No open bugs.” No matches: “No bugs match these filters. Clear filters.” Loading uses skeletons; failed loading offers Retry without discarding the previous list. Keep row identity stable and do not reorder beneath an open menu.

## 7. Flow 5 · Bug page, fix, verify, close and reopen

### 7.1 Bug page

Main column: description and report detail from task details (reproduction/expected/steps/environment, confirmation/reopen/closure/duplicate/affected-feature evidence); linked fix tasks; shared comments; immutable task activity. Side column: reference/type, accountable team, optional project/feature/milestone, assigned team/assignee, reporter/time, latest update and per-repository branch/worktree/integrated-commit evidence. Resolution events name their actor/time; do not label the current assignee as “fixed by” without actual evidence. Helpers/reviewers appear through usage attribution, not an inferred owner field.

Activity shows assignment old/new pair, status/resolution and metadata changes, actors/times and causes when known. Unknown future events render as “Unknown change” and remain retained. Closed/archived tasks retain history and stable references.

**Shared discussion** and **Private notes** are distinct controls. The former note comment is labeled Shared comment. The private pane contains only the current author's notes, supports multiple notes/edit/delete with revision conflict recovery, and never shows anyone else's count, author or existence. An assignee/team lead/administrator gains no access. Reassignment preserves original private ownership. Publishing explicitly previews a new shared comment and asks the author to confirm its audience; the private original stays private. Private-note failure cannot fall back to a shared save. Private retention, authorized recovery and deletion follow document 05, including backups; no note bodies/IDs/author association enter shared history/Git.

### 7.2 Fix tasks

Create fix task opens capture with the bug context, a valid accountable team and optional project. A task's authored fixes link points at the bug stable ID and the bug's fixed_by view is derived, not a second authored edge. Task dependencies can cross projects within an organization and reject cycles. Linked progress is visible evidence and never changes the bug's or a feature's status by itself.

### 7.3 Verify, close and reopen

A bug is closed like any task, as Done or Cancelled, with its resolution and reason kept in task details: Duplicate needs a resolvable original and no self-link (`details.duplicate_of_task_id`); Won't fix and Can't reproduce need a reason; reopening needs a note. Status events keep actors, times and reasons. Task deletion keeps tombstone identity, history and the reference; no reference is reused. The fix-and-verify cycle is not in v1.

## 8. Areas (withdrawn)

Withdrawn. There are no areas; bugs are grouped by team.

## 9. Flow 7 · Tasks: board, list and task page

Board and list use one task projection with optional type filter, scope selector and persistent view preference. The familiar To do | In progress | Blocked | Done presentation applies to task lifecycle records; unsupported type statuses stay visible in an explicit status view rather than being coerced. Done defaults to the last seven days with Show all done. Cards show type/reference/title, assignment, priority exceptions and checklist count. Menu and keyboard Move to accompany drag, with live-region announcements. A status change never triggers another record's transition.

New task asks for a title and valid accountable scope. In selected project/team context only title needs typing. Projectless capture needs a team; project selection derives project ownership. Feature and milestone pickers offer only the chosen project's records and are unavailable when projectless. Assignment uses the pair flow in §5. Every new accepted task is admitted-actor reported, starts durable history at revision 1 and receives UUIDv7 identity.

A Blocked record shows an explicit reason; a link to a decision does not prove a waiting state. Assignment never starts execution, and nothing types into a running session. Preserve worktrees on handoff; placement changes need safe migration/fencing/process reconciliation.

### 9.1 Checklists

Checklist steps are shared data: ordered stable ID, text, required/done, optional linked task, check actor/time and revision. Inline Add step supports Enter for the next line and pasted lines; each field has clear validation. Drag/menu/Alt+↑↓ reorder accessibly. Show checked actor/time and keep check provenance in task activity. Concurrent edits preserve unsaved text and surface the current step revision rather than pretending a save succeeded.

The required flag and the linked task are data only; they enact no completion gate.

### 9.2 Time on a task or bug

Time views use the [shared duration definitions](costs-and-insights-ux.md#shared-duration-definitions) and identify endpoints and evidence coverage. Started is the first evidenced In progress entry; a verified direct To do → Done records start/completion together and has zero cycle time. Reopen preserves first Started. Lead/cycle end at last completion or now while open, exclude intervening Done/Cancelled spans and say “so far” while open. For example, start day 0, complete day 2, reopen day 9 and complete day 10 gives three days of cycle time.

Sum status intervals from actual recorded transitions; “Picked up by” requires an evidenced In progress transition after that assignment. Current assignment never proves historical work. A history baseline leaves earlier start/time unavailable, never zero. Waiting comes from saved spans independently of status; overlaps count once. Closing work cannot silently discard an unresolved question/span. Use the shared duration formatter and explicit units even for zero.

Before complete session coverage, show “Lead time 28 h · waiting on you 3 h · blocked 0 min · the rest 25 h (working time isn't tracked yet)”. Never call untracked time idle. Token and cost totals come from the durable usage ledger with historical attribution; null or incomplete data stays explicit.

### 9.3 Output

The task page collects permitted deliverables in **Output**: existing document links and supported repository/branch/commit/file evidence, with a label and link only when the target has a valid resolver. Unresolved references retain their identity with an unavailable explanation. Show a recorded shared completion explanation when present; otherwise say **No outcome recorded**. Do not derive output from private notes, protected chat or the assignee's activity.

Done is an attributed completion claim. Reviewed, merged, validated, included in a release and deployed are separate facts and appear only with their corresponding evidence. This view adds no outcome field, URL or path target kind, or completion gate.

## 10. Flow 8 · Docs

The initial project navigation follows §3. This reader/editor experience appears
when its knowledge capability is served; it does not insert an unsupported Docs
tab into the project skeleton.

Two panes: Brief pinned first, then project/feature/release/deployment documents with title, kind, scope, status and last update. Header shows canonical organization Git path, explicit accountable employee when current, latest update and publication state. New doc chooses kind and required project, then at most one same-project feature/release/deployment. Default scope follows context, remains visible and is validated server-side. Current docs require an active owner; draft/deprecated docs can be unassigned.

Example: `projects/<project-id>/features/<feature-id>/designs/<document-id>.md`, title **Prevent overlapping table bookings**, project Reservations P-8, feature F-6. The definition itself is `features/<feature-id>/feature.md` with Purpose/How it works/Rules/Known limitations/Non-goals; it is not duplicated in the supporting list as a second authored record. Cross-feature architecture is a project document linked by relates_to rather than copied to several features. Release changelog describes product changes; deployment changelog describes technical delivery.

Reader sanitizes Markdown: no raw HTML or automatic remote images, deliberate external navigation. Task references render chips with type/status, resolving stable IDs. A task reference resolves on its own; a release reference resolves in its project and a milestone in its project and release. Global lookup of a release or milestone reference lists each permitted match with its project (and release) instead of selecting the first; a chat thread without scope never guesses. An ambiguous reference stays visibly unresolved. Mention indexes are derived, not canonical relationship authority.

Write/Preview, Save (⌘S), Cancel and unsaved-navigation prompts remain. Saving first durably persists a validated stable-ID record and history, then separately reports publication; Git publication is never claimed before it is acknowledged. Writes use expected-revision checks, safe paths and Git hardening, durable intents and never write into another agent's worktree. An agent co-author trailer is a claim from Git, not authenticated provenance.

Compare/Keep mine/Discard mine resolves a newer revision without losing either text. Keep mine is an explicit new save at the latest revision, not silent overwrite. Failed saves retain text with Retry/Copy text. Deleted-while-editing retains copyable draft and navigates back. History opens an old version read-only; restore creates a new revision and preserves stable identity, not a rollback of immutable facts. Scope changes move the single canonical file and retain ID. Brief remains at project brief.md, pinned and without a destructive rename action. Large files show an explicit truncation/size state; browser UI uses Copy path rather than native Finder assumptions.

Projects need **no code repository** to read or write organization Git knowledge. A local-only code registration does not relax the organization's remote Git requirement. Loading, empty, no-match and error states apply; an empty brief offers Write the brief.

## 11. Flow 9 · Work (cross-project)

Work spans all project and team scopes with Tasks | Bugs | Requests. Columns include Project (or **No project**), Accountable team, Assigned team, Assignee and reference. Saved filters distinguish **Owned by my team**, **Assigned to my team**, **Assigned to me**, **Reported by me**, Blocked and All. “My” derives from the admitted user/employee context, never a security owner flag. Project and No project filters combine with teams/type/status/severity/priority.

Queue example: “Bug T-A42 · Prevent overlapping reservations · Accountable Payments · Assigned Platform · Ana”; “Request T-B17 · Review onboarding · Accountable People · Unassigned”. The two were created in different offices, which is what the letter shows. An individual's home-team move does not rewrite their recorded task's assigned team.

The **Show** menu offers Needs me, Mine, By project and By assignee, retaining
its choice in `?view=`. Default to Needs me when supported owner-waiting work
exists, otherwise Mine; “my” still requires the admitted mapping above. By
assignee includes people and agents. Requested-by is never inferred from reporter. List and Board use the same supported state data.

Needs me is work actually waiting on the owner through an available source:
owner blocks, review waits, linked hand-offs, open linked asks and reached task
budget holds. Each row links its corresponding open Inbox/source item; a hand-off
without a task stays in Chat/Inbox. A budget-caused block uses its Budget reached
item, while a pre-existing independent owner block retains its own item. Empty
copy is **Nothing needs you in Work**. These are read views; they start no workflow.

Remember the selected Tasks/Bugs/Requests tab separately for each authorized organization, in addition to board/list preference and filters. Reopening Work restores that tab even when its result is empty. An unavailable remembered tab falls back to a supported Work view without switching organization or clearing other view preferences.

The Needs you badge counts owner questions and work blocked on the owner. Empty “No work yet” offers New task and New request with valid team capture even when there are no projects.

## 12. Live updates and offices

Keep stable row/card identity, live actor-attributed updates and a two-second highlight; hold reordering while a menu is open. A disconnected/read-only last projection is visibly stale. An attempted shared mutation remains pending/conflicted until its accepting writer resolves it; show no optimistic final status as accepted. Lost responses resolve through the same operation identity, never a duplicate create.

Any office may create and durably save tasks and features, including while disconnected. A task has its final reference at once. A feature works by stable ID and shows Pending number until HQ registers it. HQ creates projects, teams, releases and milestones; an office never allocates HQ's numbers and keeps only its own task sequence. A browser has no promised background outbox or offline durable store. Its draft stays visibly unsaved in the open page until an available service acknowledges it. **Saved locally**, **Pending number**, **Pending publication** and **Unsaved** describe different states and must not be conflated.

Accepted task snapshot/events/receipt recover together before success; hourly Git publication alone is insufficient. Private notes, protected chat, usage, budget history and authority have separate durable sources. Rebuilding the office database does not authorize a new agent process or delete worktrees.

## 13. Accessibility

Severity/priority/status always have icon and word. Example screen-reader row: “Bug T-A42, Prevent overlapping reservations, severity major, priority high, accountable Payments, assigned team Platform, assignee Ana, person.” Board columns are labeled lists; group headings support heading navigation. Dialogs trap/restore focus, field errors use aria-describedby and visible text, and action failure explanations are keyboard accessible. The triage region is labeled. Privacy controls name the audience in plain words; author-private content never enters shared live-region announcements.

## 14. UX acceptance criteria

Criteria marked "Not in v1" belong to the type-specific workflows of §16.

**Project page (PRJ)**

- PRJ-1 · The project page uses Overview, Work, Team, Activity and Conversations with Projects back navigation. Work filters typed tasks. Backlog, Releases and Insights appear when built; tabs for experiences that are not served are never shown.
- PRJ-2 · Overview shows the living Brief, real open/blocked counts and the five latest activity entries. Team separates accountable/contributing teams from member rows; Conversations exposes only authorized linked threads. Empty states and optional lead/health/due fields are honest.
- PRJ-3 · The header shows one required accountable team and optional contributors. Settings keeps repository integration branches on each project/repository association; a teamless project is never accepted.
- PRJ-4 · When the accepting service is unavailable, writes have an accessible reason and last known data stays readable with a stale/read-only label.
- PRJ-5 · Projects list accountable team separately from contributors, and bug severity counts including unset.

**Tasks (TSK)**

- TSK-1 · The board has To do, In progress, Blocked and Done columns. Done shows the last 7 days, with **Show all done**.
- TSK-2 · A card can be moved by drag, by ⋯ › Move to, and by keyboard, and each move is announced to screen readers.
- TSK-3 · Blocked shows a reason.
- TSK-4 · Every column sorts by priority (Urgent first), then age, and changing a task's priority moves its card and announces the change.
- TSK-5 · In a valid project/team context New task needs only a title and defaults priority Normal. Outside context accountable team is required, project optional. Assignment pickers distinguish active people/agents; an authenticated owner is assignable only through a valid employee identity.
- TSK-6 · Assigning a task never starts work and nothing types into a running session. Running an agent on a task is an explicit Run (`agent-run-modes-ux.md`).
- TSK-7 · A task moved by someone else moves on the board without a reload, and a toast names who moved it.
- TSK-8 · The page shows linked bug tasks, immutable activity and per-repository branch/worktree evidence where known, independent of code repository existence.
- TSK-9 · Board or List is remembered per project.
- TSK-10 · Cards and rows show priority only when it isn't Normal, as an icon and a word. Priority is neutral; amber is reserved for a separate Needs you state, never urgency alone.
- TSK-11 · Task/bug Time follows the shared duration definitions: retain first Started across reopen, exclude evidenced Done/Cancelled spans, show open time as so far and mark unavailable earlier history. A baseline never supplies a fabricated start or zero. Status time and saved waits use their actual intervals, not current assignment; untracked time stays the rest, never idle.

**Checklists (CHK)**

- CHK-1 · Tasks and bugs have a checklist of ordered steps. The card shows "n/m" and marks required steps left with a neutral lock icon and the word "required"; the flag alone is not an owner blocker.
- CHK-2 · + Add step adds steps inline (Enter for the next, pasted lines become separate steps), each 1–200 characters.
- CHK-3 · Ticking or unticking records who and when, shows it on the step, and appears in the activity.
- CHK-4 · A step can be marked Required, shown with a lock icon and the word "Required".
- CHK-5 · Not in v1: a completion gate on required steps, with Mark done anyway.
- CHK-6 · Not in v1: promoting a step to a task, with automatic ticks.
- CHK-7 · Steps can be reordered by drag, menu and Alt+↑/↓, and the change is announced.
- CHK-8 · Not in v1: named checklist templates and steps derived from processes.
- CHK-9 · Not in v1: command-line completion gates and checklist promotion.

**Report a bug (BUG)**

- BUG-1 · Report bug can be opened from the project header, Work, ⌘K, a task's menu and a doc's menu. From a task or doc, that item is linked.
- BUG-2 · Report bug is disabled until Title and What happened are filled, and the reason is shown as text.
- BUG-3 · After 3 characters of title, up to three similar open bugs in the project show. **This is the same bug** adds the text as a comment to that bug and opens it, and no new bug is created.
- BUG-4 · Severity may remain Not set; Critical, Major, Minor and Trivial each have an explanation.
- BUG-5 · The capture shows required Accountable team, optional Assigned team/Assignee and valid project context; suggestions do not change accountability or grant permissions.
- BUG-6 · Environment shows only actual available office/app and per-repository branch/commit evidence and can be edited/cleared; report detail is stored in details/task_details.
- BUG-7 · Reporting creates type bug in the shared T- scope with required accountability, immutable reporter and explicit assignment pair. Confirmation distinguishes accepted from unsaved.
- BUG-8 · A bug reported by an agent shows that agent's immutable reporter identity, initial avatar and the word Agent; agent/person distinction never relies on avatar shape.
- BUG-9 · Closing the form with text in it asks "Discard this bug report?".

**Triage (TRI)**

- TRI-1 · Triage names its suggestion and the acting user; no permission follows from team or accountability.
- TRI-2 · Triage UX confirms available severity/priority and saves a complete desired assigned-team/individual pair atomically at the expected task revision; either or both may be null.
- TRI-3 · Changing assignment can route work to any active same-organization team/employee without changing accountable team/project or number. An individual need not belong to the assigned team.
- TRI-4 · Reported severity and detail keep reporter and time provenance separately from current triage values.
- TRI-5 · Not in v1: Critical bugs raised in the owner's Needs you.
- TRI-6 · A stale assignment revision conflicts and changes neither field; show current and desired pair with actor/time and an explicit retry, preserving the user draft.
- TRI-7 · Work filters distinguish accountable team, assigned team, individual and reporter.
- TRI-8 · Withdrawn: there are no areas, so there is no area-lead fallback. Accountable ownership never becomes null.

**Bug list (BUG, continued)**

- BUG-10 · Bugs group by assigned team including unassigned, sorting by severity, priority and age.
- BUG-11 · Group headings show assigned team and severity counts with words, including unset severity; accountable ownership remains visible separately.
- BUG-12 · Status filters keep recorded values and are remembered per scope.
- BUG-13 · Grouping by severity, priority, status, accountable or assigned team, assignee or none works and keeps row identity.
- BUG-14 · Not in v1: a Verify action on fixed bugs. The assignee is never relabelled as the historical fixer.
- BUG-15 · The empty, no open bugs and no matches states show the copy from Flow 4, with Retry on failures and explicit read-only stale state when disconnected.
- BUG-16 · Copilot field provenance is visible/accessibly named until edited/kept; suggested severity remains explicit. Retry/Dismiss clears only its panel's stale error, retaining input, Undo draft and manual submission.

**Fix, verify, close and reopen (FIX)**

- FIX-1 · Create fix task retains bug context and valid required accountability; authored fixes points at the stable bug-task ID and inverse fixed_by is derived.
- FIX-2 · Linked-task progress changes no other record. Not in v1: automatic propagation from fix tasks to the bug, and Undo.
- FIX-3 · Not in v1: Mark as fixed with an explanation.
- FIX-4 · Not in v1: verification by the original reporter.
- FIX-5 · Not in v1: Still happens with a reopen note.
- FIX-6 · Not in v1: an acting verifier shown separately from the reporter.
- FIX-7 · Closing as Duplicate requires a resolvable original, rejects a self-reference, and keeps navigation and evidence in `details.duplicate_of_task_id`, with same-organization target and cycle validation.
- FIX-8 · Closing as Won't fix or Can't reproduce requires a kept reason; no notification is claimed before it is delivered.
- FIX-9 · Reopening requires a note, actor and time and keeps the original history.
- FIX-10 · Known commits and branches resolve through the stored per-repository task identity; text that merely looks like a reference never creates a link.
- FIX-11 · The historical reporter stays visible even when inactive. Withdrawn: the area-lead verification fallback.

**Areas (AREA)**

- AREA-1 · Withdrawn: there are no areas.
- AREA-2 · Withdrawn: there are no areas.
- AREA-3 · Withdrawn: there are no areas.
- AREA-4 · Withdrawn: there are no areas.
- AREA-5 · Withdrawn: there are no areas.
- AREA-6 · Withdrawn: there are no areas.
- AREA-7 · Withdrawn: there are no areas.
- AREA-8 · Withdrawn: there are no areas.
- AREA-9 · Withdrawn: there are no areas.

**Docs (DOC)**

- DOC-1 · Docs pins project brief.md then supporting organization Git records by explicit scope/kind/status/update; features retain their distinct feature.md definitions and no duplicate supporting row.
- DOC-2 · Rendering sanitizes Markdown with no raw HTML, no automatic remote images and deliberate external-link navigation.
- DOC-3 · Task references become typed status chips only when unambiguous in scope. Mention indexes are derived.
- DOC-4 · Write/Preview and keyboard Save preserve draft text, stable identity, owner/status/scope and attribution, with durable save and Git publication shown separately. Git and worktree hardening applies.
- DOC-5 · A newer revision offers Compare/explicit Keep mine/Discard mine without loss; failure retains text and Retry/Copy. No silent overwrite or publication claim.
- DOC-6 · The project brief is pinned at projects/<project-id>/brief.md with an empty hint and Write the brief; no code repository is required for organization Git knowledge.
- DOC-7 · Leaving the editor with unsaved changes asks before discarding.
- DOC-8 · Each document exposes canonical path and retained revision/history; restoring creates a new revision with unchanged stable ID and honest attribution.
- DOC-9 · Validated external organization Git edits update the projection after publication; from-git co-author trailers remain claims distinct from admitted-actor provenance.

**Work (WORK)**

- WORK-1 · Work spans projects and projectless teams with type views Tasks, Bugs and Requests, task references, and project, no-project and team filters.
- WORK-2 · Saved filters distinguish Owned by my team, Assigned to my team, Assigned to me, Reported by me, Blocked and All.
- WORK-3 · The badge counts owner questions and work blocked on the owner; account ownership does not imply employee assignment.
- WORK-4 · Every task type displays its type word and icon and its T- reference.
- WORK-5 · Displayed severity, priority and status have a meaningful icon and word, announced to screen readers. Severity/priority remain separate meta text, with neutral priority icons and no amber except an actual owner blocker; Normal may be omitted on rows/cards. Each row has one work-status signal.
- WORK-6 · Changes by others appear without a reload and never reorder a list while a menu is open in it.
- WORK-7 · The admitted actor is recorded for every accepted change. Neither this UX nor assignment grants permissions.
- WORK-8 · All types use T- references that are complete on their own, such as T-A42; lookup by reference finds exactly one task or none, in any view. M-2 needs a release as well as a project. A feature's Pending number retains stable row identity and a copyable stable ID; replacing its label does not reorder rows.
- WORK-9 · Work remembers its type tab per authorized organization independently of filters and board/list preferences; an empty queue preserves it and an unavailable view has a safe supported fallback.
- WORK-10 · AC-IA18-1–3's Show views retain `?view=`, select the stated default and include both people and agents. Needs me and Inbox agree for supported sources, with no task row for an unlinked hand-off; unavailable source types are not inferred.

**Assignment, history and private boundaries**

- TSK-12 · Accountable team is required for every type; project tasks match project owner. Project/feature/milestone references stay same-project; projectless tasks have neither feature nor milestone.
- TSK-13 · All four assignment states are supported. Both fields and their old/new event commit with expected revision and durable receipt; stale/conflicting writes update neither. Home-team moves never silently rewrite assignments.
- TSK-14 · Reporter is admitted-creator-derived/immutable. Unknown history is marked unavailable, never invented from current state.
- TSK-15 · Accepted snapshot/events/receipt recover together; history survives task closure/archive/tombstone and unknown event tags. A feature's pending number, pending publication and unsaved drafts remain distinct.
- TSK-16 · Type changes preserve stable ID/number/metadata.
- DOC-10 · Each supporting doc requires project and at most one same-project feature/release/deployment; scope/path/kind agree, current requires an active accountable employee, and scope moves preserve ID without duplicate canonical copies.
- DOC-11 · Feature definitions use stable-ID feature.md paths, local_slug plus derived project prefix, explicit owners/status and behavior/limitation sections; task completion never silently rewrites feature status.
- TSK-17 · PN-01–06 in document 05 are required: author-only private CRUD/conflicts, no admin exception or shared existence metadata, durable recovery/closure continuity, unchanged privacy after assignment, explicit publish and documented retention/backup deletion. Shared note comments stay shared.
- TSK-18 · Output shows only permitted resolvable recorded deliverables and any shared completion explanation; missing evidence is explicit. Done alone never implies review, merge, validation, release or deployment. There are no arbitrary URL or path writes and no outcome gates.

## 15. Open questions

- Commit attribution and the handling of uncommitted work at task closure need their execution design. Handoff keeps worktrees; it is not closure.
- Watchers are stored data; how they are notified is not designed.

## 16. Not in v1

- **Type-specific workflows.** A bug's fix-and-verify cycle: Mark as fixed asks what fixed it; the original reporter verifies (Fixed or Still happens, with a note), the acting verifier shown separately; a Reopened count; Critical bugs raised in Needs you; and automatic propagation from fix tasks. A request's scoping lifecycle (`feature-requests-ux.md`).
- **Advanced checklists:** completion gates with Mark done anyway, promotion of a step to a task, named templates and steps derived from processes.
- **Undo**, automatic feature and decision propagation, scheduling and memory.
- **Attachments**, until attachment storage exists.
- **Following:** per-reader unseen tracking distinct from Inbox, with follow reasons, never revealing private notes.
- **Requested-by:** a verified link from a directly addressed message, distinct from the reporter.
- **Review** of a task by a named reviewer, with no self-approval.
- **Versioned shared brief and comments** with attributed change rationale.
- **Planning:** backlog rank, dates, close and reopen of plans, planner authorization and progress denominators. Milestone membership stays independent of release manifests.
