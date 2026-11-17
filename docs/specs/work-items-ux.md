---
kind: spec
title: "Typed tasks, bugs and project docs (UX)"
status: for-review
revision: 13
owner: lead
created: "2026-11-08"
updated: "2026-11-15"
requested_by: ac-staff-engineer-ada
reviewers: [codex-dev, ac-staff-engineer-ada, owner]
builds_on: [offices-and-teams-ux]
---

# Spec · Typed tasks, bugs and project docs (UX)

Selected target UX, revision 13, 2026-11-15. This is a documentation/data-model reconciliation, not a claim that these screens or mutation services have shipped. [Organization-data 02](../design/organization-data/02-project-structure.md), [03](../design/organization-data/03-features-and-work.md), [05](../design/organization-data/05-operational-data.md), [06](../design/organization-data/06-identity-and-numbering.md), [08](../design/organization-data/08-record-contracts.md) and [12](../design/organization-data/12-task-assignment-and-history.md) own the fields, sources and assignment/history semantics. See the [authority map](../design/documentation-authority.md) and [W7 current-contract mapping](../design/tasks-projects-docs.md#32-the-ports-org-work-defines).

Criterion IDs remain stable. **Deferred** criteria below preserve earlier UX requirements without selecting the corresponding engines. Retired wording is available in [revision 11](https://github.com/vgmello/agentic-org/blob/c6e520a36c4dc7c738c68915fac5e7d5d1cbe804/docs/specs/work-items-ux.md).

## Changelog

**Revision 13 (2026-11-15):** integrates project/Work navigation, task-view persistence, Output, reference lookup, event-time and copilot-field requirements; retains unselected Following/review/planning workflows as explicit follow-ups. Shared visual rules and existing criterion IDs remain authoritative.

**Revision 12 (2026-11-15):** rewrites the active model, examples, flows and criteria around one typed task, required accountability, independent assignment, scoped T- references, organization Git knowledge, durable task history and separate author-private notes. Mandatory areas, old separate counters, office-owned data and automatic transition/checklist/process engines no longer appear as active target requirements. Earlier revisions' automatic behavior and code-repository doc claims are historical; the linked revision preserves that decision history.

## 0. Decisions in this spec

1. **Task is the entity.** Bug and Request are types with dedicated views and fields; every type uses T-. An optional project does not determine whether something is a task.
2. **Work spans projects and teams.** Keep Tasks | Bugs | Requests views, with Debt linking to knowledge rather than inventing another task entity. Include projectless team work. Type filters avoid accidental double counting in combined totals.
3. **References have scope.** Inside Reservations P-8: Bug T-42. Outside: Bug P-8/T-42. A projectless request may be Request TEAM-12/T-42. Features use F-6 (P-8/F-6 outside project); no new B-/REQ-/FEAT- counters. Never rewrite an old readable reference without an explicit import mapping.
4. **Every task has one accountable team.** Project tasks use the project's owner team; team tasks choose one. An assigned team and an individual are optional independent fields. Assignment changes neither accountability nor numbering.
5. **Severity and priority differ.** Severity is optional bug-impact metadata, explained in plain words. Priority is Urgent/High/Normal/Low, default Normal and shared across types.
6. **Reporter is immutable.** Show original admitted creator separately from current assignment, latest updater and recorded fix/closure actor. Missing historical provenance stays unknown.
7. **Bug presentation remains first class.** Preserve reporting, triage, fix/verify/duplicate evidence and reason requirements, subject to explicit lifecycle selection. Areas, automatic bug following and Undo are deferred. No mandatory General or area picker blocks capture.
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
| Pending number | Durably saved local task/feature awaiting organization allocation; not an unsaved draft |

Use **Accountable team**, **Assigned team** and **Assignee** together where needed; do not call all three Owner. The authenticated organization owner remains a separate security designation. An employee representation of the owner needs a valid home team like other employees.

## 2. Visual direction (Tailwind Plus application UI, as in offices-and-teams §2)

| Need | Component |
| --- | --- |
| Project tabs | *Tabs with underline* (the page's sections), not pills: pills stay for views of one thing (Org chart, Work) |
| Bug list | *Stacked list* grouped under *Sticky headings* (assigned team + severity counts) |
| Task board | Four columns of cards on `bg-gray-800/50`, `outline-white/10`. Each column is a list for screen readers |
| Task list, Work roll-up | *Table with sticky header* |
| Report bug, New task, New doc, Assignment | *Slide-over* (right, `max-w-lg` for Report bug, `max-w-md` for the others) |
| Bug and task detail | Full-width page inside the project (breadcrumb Projects › checkout › Bugs › T-42), main column + *Description list* side column |
| Close as…, Delete area, Mark as duplicate | *Modal dialog* |
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
| To do / bug New | gray |
| In progress / bug Triaged or In progress | indigo |
| Blocked on someone other than the owner | red |
| Waiting on the owner | one amber Needs you signal instead of the ordinary pill |
| Done / bug Fixed | green |
| Closed or cancelled | neutral outline |

A Reopened count is a neutral textual fact with a rotate icon, not a second
amber status. A scoped request keeps its neutral Scoped status; an associated
approval appears in Inbox rather than adding a competing pill.

The four task columns retain familiar vocabulary. Bug-specific statuses and the
Reopened count remain presentation requirements for a separately selected
lifecycle adapter; styling never enacts those transitions. Completed and
Cancelled stay distinct resolution data. Neither automatically changes another
record.

## 3. Flow 1 · Project page

Projects → Reservations **P-8**, with a Projects back link. Header: **Accountable team: Payments**; **Contributors: Platform**; real done/total counts; New task and Report bug. The integrated IA target replaces separate Tasks/Bugs tabs with **Overview | Work | Team | Activity | Conversations**, Overview by default. Work filters shared typed tasks by type. Backlog and Releases join after Work when their services are available; Insights is last when its separately gated analytics service exists. Routines stays deferred. Features, Debt, Docs and Settings do not appear merely because this navigation skeleton exists; their owning capability specs define their separately supported experiences.

Overview shows the living Brief, real open/blocked counts and the five latest activity entries. Work shows the project's typed tasks with one status signal; Team shows accountable/contributing teams and then member rows, with only supported authorized association edits. Activity uses qualified references: “Ana assigned P-8/T-42 to Platform · Ana.” Conversations shows permitted project-linked threads. Empty feeds say **No activity yet** or **No conversations yet**. Projectless work appears in Work/team backlogs rather than an artificial project.

Optional lead, due date and health show only when backed by supported explicit data; contributors never imply co-leads. The source's manual health control in the header's More menu remains pending a selected health field/write contract; no inferred health or invented planning date is saved. Progress denominators and Flow follow the separately gated analytics definitions in costs-and-insights-ux.md/project-status-ux.md.

Settings includes project name/slug, accountable team, contributors and repositories. Each repository association shows its own required integration branch. Sharing a repository does not share a branch setting or create duplicate office caches. A project always has a team; invalid imported ownership is surfaced for resolution and cannot become a teamless accepted project. Archived project records remain readable. Loading uses skeletons; missing/tombstoned records explain their state and offer navigation. Disconnection keeps last known data readable and disables unavailable actions with visible reasons.

## 4. Flow 2 · Report a bug

Report bug opens from project, Work, command palette, task or supporting doc. In project context the required accountable team is shown from project ownership. Outside a project, choose **Project (optional)** and **Accountable team (required)**; selecting a project derives its team. No Area/General requirement appears.

Fields: title (1–120 characters), What happened, optional severity with all four explanations and **Not set**, optional steps/expected result/environment, priority default Normal, optional assigned team and assignee. Store selected reproduction values in `details.what_happened`, `expected`, `steps` and `environment`, distinct from description; confirmation/reopen/closure/duplicate/affected-feature/suggested-employee evidence uses document 03's `task_details` mapping. Preserve those fields on import and tombstones; only unknown unsupported variants stay staged. A title and description plus valid accountability are sufficient to capture. Reporter comes from authentication and is not editable.

When a configured copilot drafts the report, each filled field has a visible
and accessibly named **Drafted** marker until that field is edited or explicitly
kept. A severity suggestion says **Suggested by the copilot** or remains a
nonselected hint; it never silently becomes a deliberate severity choice.
Editing one field clears only its marker. A new attempt or Dismiss clears that
panel's stale error while preserving typed text, Undo draft and the ordinary
manual Report bug confirmation. See the existing [copilot contract](../design/smart-features.md#2-bugfrom_sentence-implemented-library-entry-version-2).

After three title characters show up to three similar open bug-typed tasks in the selected scope. “This is the same bug” adds the report as an explicitly shared comment on the chosen existing record; it preserves that record's reporter and creates no second task. Clearly label suggestions from other scopes with qualified references. No match does not block reporting.

Environment capture is visible/editable: office/app version and per-repository branch/commit only when actually known, never guessed from a single project cache. Attachments remain hidden until their storage is designed. Closing dirty capture asks before discarding. Submit reports success only after durable acceptance; local acceptance with no number says **Saved · Pending number**, whereas an unsubmitted draft says **Unsaved**. In place of an old B- toast use “Bug P-8/T-42 saved”.

## 5. Flow 3 · Triage

Triage confirms available impact evidence, priority and the desired assignment pair. Show original reported values and author/time distinctly from current values. The accountable team stays visible and read-only here. Suggest active same-organization teams and employees; do not restrict individuals by assigned team/home team or contributors.

Assignment controls offer **No assigned team** and **Unassigned individual** independently. Team-only, individual-only, both and neither are valid. Clearing an individual retains a selected team queue; clearing a team never infers a new one from the individual. Save sends the entire pair plus expected task revision. If someone else changed the task, show the current pair, actor/time and the user's desired pair, then allow an explicit retry at the new revision. Never overwrite silently or accept half of the pair.

Who may triage and exact lifecycle transitions remain an explicit service/authorization decision. Prior permissive phase-1 behavior is current-contract evidence, not a grant based on YAML ownership. The eventual triage panel identifies a suggested responsible team/person and records who actually acts. Critical bugs remain prominent in the owner's Needs you when that workflow is selected. Area-lead routing and automatic offboarding cascades are deferred.

## 6. Flow 4 · The project's bug list

Rows: type/reference · title · severity (including Not set) · priority exception · status · accountable team · assigned team · assignee · reporter · age. Default grouping is **Assigned team**, with Unassigned team explicit, and sorting severity then priority then age. Alternatives: severity, priority, status, accountable team, assignee or none. No mandatory area group.

Filter by status, severity, assigned/accountable team, assignee, reporter and project/no project in Work. Remember filters and show counts consistently. New/triage/verification groups are enabled only by the selected lifecycle adapter. Empty: “No bugs yet. Report one when something doesn't work.” No open bugs: “No open bugs.” No matches: “No bugs match these filters. Clear filters.” Loading uses skeletons; failed loading offers Retry without discarding the previous list. Keep row identity stable when a pending number arrives and do not reorder beneath an open menu.

## 7. Flow 5 · Bug page, fix, verify, close and reopen

### 7.1 Bug page

Main column: description and selected report detail from task details (reproduction/expected/steps/environment, confirmation/reopen/closure/duplicate/affected-feature evidence); linked fix tasks; shared comments; immutable task activity. Side column: reference/type, accountable team, optional project/feature/milestone, assigned team/assignee, reporter/time, latest update and per-repository branch/worktree/integrated-commit evidence. Resolution events name their actor/time; do not label the current assignee as “fixed by” without actual evidence. Helpers/reviewers appear through usage attribution, not an inferred owner field.

Activity shows assignment old/new pair, status/resolution and metadata changes, actors/times and causes when known. Unknown future events render as “Unknown change” and remain retained. Imports with only a snapshot show “Earlier activity unavailable” at a history baseline; do not manufacture a complete activity feed. Closed/archived tasks retain history and stable references.

**Shared discussion** and **Private notes** are distinct controls. The former note comment is labeled Shared comment. The private pane contains only the current author's notes, supports multiple notes/edit/delete with revision conflict recovery, and never shows anyone else's count, author or existence. An assignee/team lead/administrator gains no access. Reassignment preserves original private ownership. Publishing explicitly previews a new shared comment and asks the author to confirm its audience; the private original stays private. Private-note failure cannot fall back to a shared save. Private retention, authorized recovery and deletion follow document 05, including backups; no note bodies/IDs/author association enter shared history/Git.

### 7.2 Fix tasks

Create fix task opens capture with the bug context, a valid accountable team and optional project. A task's authored fixes link points at the bug stable ID and the bug's fixed_by view is derived, not a second authored edge. General task dependencies can cross projects within an organization and reject cycles; the exact fix-task workflow/scope choices remain lifecycle decisions. Linked progress is visible evidence and does not automatically change bug/feature/debt status. Earlier automatic start/fixed/reopen propagation, all-cancelled handling and Undo remain deferred (FIX-2).

### 7.3 Verify, close and reopen

Retained UX for a future selected bug lifecycle: Mark as fixed asks what fixed it; verification distinguishes reporter from acting verifier; Still happens and Reopen need a note; Won't fix/Can't reproduce need a reason; Duplicate needs a resolvable original and no self-link. Explicit status events preserve actors/times and reasons. Exact states, reopening counts, notifications, permissions and unresolved-question/wait disposition require a lifecycle contract before controls ship. No automatic area-lead fallback is selected. Task deletion retains tombstone identity/history and allocated references rather than reusing numbers.

## 8. Flow 6 · Areas (project Settings › Areas)

**Deferred.** The selected model has no area record and no mandatory General. Preserve the earlier proposal: flat project areas with unique names, optional lead, routing/verification suggestions, safe rename, explicit reassignment of open bugs before deletion, lead fallback and retryable team-removal cascades. AREA-1–9 retain those requirements individually as unselected. They require an explicit model/authorization/migration decision before this screen or mandatory routing can return.

## 9. Flow 7 · Tasks: board, list and task page

Board and list use one task projection with optional type filter, scope selector and persistent view preference. The familiar To do | In progress | Blocked | Done presentation applies to task lifecycle records; unsupported type statuses stay visible in an explicit status view rather than being coerced. Done defaults to the last seven days with Show all done. Cards show type/reference/title, assignment, priority exceptions and checklist count. Menu and keyboard Move to accompany drag, with live-region announcements. Status controls require actual supported lifecycle handlers; they do not trigger another record's transition.

New task asks for a title and valid accountable scope. In selected project/team context only title needs typing. Projectless capture needs a team; project selection derives project ownership. Feature and milestone pickers offer only the chosen project's records and are unavailable when projectless. Assignment uses the pair flow in §5. Every new accepted task is admitted-actor reported, starts durable history at revision 1 and receives ULID identity; an imported old ID remains exact.

A Blocked record shows an explicit reason. Structured decision blocking and automatic return to prior status are deferred; links to a decision do not prove a waiting state. “Start now” and current task/agent execution are separate services: never type into a running session or assume assignment starts execution. Preserve worktrees on handoff; placement changes need safe migration/fencing/process reconciliation.

### 9.1 Checklists (rev 4)

Plain steps are selected shared data: ordered stable ID, text, required/done, optional linked task, check actor/time and revision. Inline Add step supports Enter for the next line and pasted lines; each field has clear validation. Drag/menu/Alt+↑↓ reorder accessibly. Show checked actor/time and keep check provenance in task activity. Concurrent edits preserve unsaved text and surface the current step revision rather than pretending a save succeeded.

Required flag and linked task are data only. Earlier completion gates/“done anyway”, promotion with automatic ticks, named project templates/defaults, process-derived steps and their CLI automation remain individually deferred under CHK-5/6/8/9. Do not run inherited process/checklist engines because the structural documents exist.

### 9.2 Time on a task or bug (rev 4)

Time views use the [shared duration definitions](costs-and-insights-ux.md#shared-duration-definitions) and identify endpoints and evidence coverage. Started is the first evidenced In progress entry; a verified direct To do → Done records start/completion together and has zero cycle time. Reopen preserves first Started. Lead/cycle end at last completion or now while open, exclude intervening Done/Cancelled spans and say “so far” while open. For example, start day 0, complete day 2, reopen day 9 and complete day 10 gives three days of cycle time.

Sum status intervals from actual recorded transitions; “Picked up by” requires an evidenced In progress transition after that assignment. Current assignment never proves historical work. A history baseline leaves earlier start/time unavailable, never zero. Waiting comes from saved spans independently of status; overlaps count once. Closing work cannot silently discard an unresolved question/span. Use the shared duration formatter and explicit units even for zero.

Before complete session coverage, show “Lead time 28 h · waiting on you 3 h · blocked 0 min · the rest 25 h (working time isn't tracked yet)”. Never call untracked time idle. The detailed analytics/automatic-wait engine remains separate work. Token/cost totals come from the durable usage ledger, including helpers/reviewers and historic attribution; null/incomplete data remains explicit. Budgets and stop controls do not become implemented from displaying cost fields.

### 9.3 Output

The task page collects permitted deliverables in **Output**: existing document links and supported repository/branch/commit/file evidence, with a label and link only when the target has a valid resolver. Unresolved references retain their identity with an unavailable explanation. Show a recorded shared completion explanation when present; otherwise say **No outcome recorded**. Do not derive output from private notes, protected chat or the assignee's activity.

Done is an attributed completion claim. Reviewed, merged, validated, included in a release and deployed are separate facts and appear only with their corresponding evidence. This view adds no outcome field, URL/path target kind, link-mutation command or completion gate; those require explicit contracts.

## 10. Flow 8 · Docs

The initial project navigation follows §3. This reader/editor experience appears
when its knowledge capability is served; it does not insert an unsupported Docs
tab into the project skeleton.

Two panes: Brief pinned first, then project/feature/release/deployment documents with title, kind, scope, status and last update. Header shows canonical organization Git path, explicit accountable employee when current, latest update and publication state. New doc chooses kind and required project, then at most one same-project feature/release/deployment. Default scope follows context, remains visible and is validated server-side. Current docs require an active owner; draft/deprecated docs can be unassigned.

Example: `projects/<project-id>/features/<feature-id>/designs/<document-id>.md`, title **Prevent overlapping table bookings**, project Reservations P-8, feature F-6. The definition itself is `features/<feature-id>/feature.md` with Purpose/How it works/Rules/Known limitations/Non-goals; it is not duplicated in the supporting list as a second authored record. Cross-feature architecture is a project document linked by relates_to rather than copied to several features. Release changelog describes product changes; deployment changelog describes technical delivery.

Reader sanitizes Markdown: no raw HTML or automatic remote images, deliberate external navigation. Selected scoped task references render chips with type/status, resolving stable IDs. A local reference resolves only in its unambiguous owning scope: project/team for a task, project for a release and project plus release for a milestone. Global lookup lists each permitted match with its canonical qualified reference instead of selecting the first; a chat thread without scope never guesses. An ambiguous or historical reference stays visibly unresolved until mapped; do not parse old B-/REQ- into new T- numbers. Mention indexes are derived, not canonical relationship authority.

Write/Preview, Save (⌘S), Cancel and unsaved-navigation prompts remain. Saving first durably persists a validated stable-ID record/history as appropriate, then separately reports publication; never claim Git merge or shared publication before acknowledgment. Exact organization Git editing/branch APIs remain implementation work. Preserve the older code-repository doc adapter's expected-blob/ref-CAS, safe paths/Git hardening, durable intents and no cross-agent-worktree writes when adapting it. An agent co-author trailer is a from-git claim, not authenticated provenance.

Compare/Keep mine/Discard mine resolves a newer revision without losing either text. Keep mine is an explicit new save at the latest revision, not silent overwrite. Failed saves retain text with Retry/Copy text. Deleted-while-editing retains copyable draft and navigates back. History opens an old version read-only; restore creates a new revision and preserves stable identity, not a rollback of immutable facts. Scope changes move the single canonical file and retain ID. Brief remains at project brief.md, pinned and without a destructive rename action. Large files show an explicit truncation/size state; browser UI uses Copy path rather than native Finder assumptions.

Projects need **no code repository** to read or write organization Git knowledge. A local-only code registration does not relax the organization's remote Git requirement. Loading/empty/no-match/error states remain; an empty brief offers Write the brief. Exact publication and conflict contracts are pending adapter work, not `doc.*` support invented by this UX spec.

## 11. Flow 9 · Work (cross-project)

Work spans all project/team scopes with Tasks | Bugs | Requests and a knowledge/debt link. Columns include Project (or **No project**), Accountable team, Assigned team, Assignee and qualified reference. Saved filters distinguish **Owned by my team**, **Assigned to my team**, **Assigned to me**, **Reported by me**, Blocked and All. “My” derives from the admitted user/employee context, never a security owner flag. Project and No project filters combine with teams/type/status/severity/priority.

Queue example: “Bug P-8/T-42 · Prevent overlapping reservations · Accountable Payments · Assigned Platform · Ana”; “Request TEAM-12/T-42 · Review onboarding · Accountable People · Unassigned”. Same local number is valid across these scopes. An individual's home-team move does not rewrite their recorded task's assigned team.

The **Show** menu offers Needs me, Mine, By project and By assignee, retaining
its choice in `?view=`. Default to Needs me when supported owner-waiting work
exists, otherwise Mine; “my” still requires the admitted mapping above. By
assignee includes people and agents. Created by me, Asked by me and Followed
arrive only with their verified relationship/tracking services; requested-by is
never inferred from reporter. List and Board use the same supported state data.

Needs me is work actually waiting on the owner through an available source:
owner blocks, review waits, linked hand-offs, open linked asks and reached task
budget holds. Each row links its corresponding open Inbox/source item; a hand-off
without a task stays in Chat/Inbox. A budget-caused block uses its Budget reached
item, while a pre-existing independent owner block retains its own item. Empty
copy is **Nothing needs you in Work**. These are read-view requirements, not
selection of automatic review/wait/lifecycle engines.

Remember the selected Tasks/Bugs/Requests tab separately for each authorized organization, in addition to board/list preference and filters. Reopening Work restores that tab even when its result is empty. An unavailable remembered tab falls back to a supported Work view without switching organization or clearing other view preferences.

Needs you/triage/verification badges require their eventual lifecycle rule; preserve the intent to highlight owner questions and blocked work without inventing an automatic engine. Empty “No work yet” offers New task and New request with valid team capture even when there are no projects.

## 12. Live updates and offices

Keep stable row/card identity, live actor-attributed updates and a two-second highlight; hold reordering while a menu is open. A disconnected/read-only last projection is visibly stale. An attempted shared mutation remains pending/conflicted until its accepting writer resolves it; show no optimistic final status as accepted. Lost responses resolve through the same operation identity, never a duplicate create.

Any office may locally create/durably save tasks/features, including disconnected; those records work by stable ID and show Pending number until organization registration. HQ creates projects/teams/releases/milestones; office replicas cannot allocate independent counters. A browser has no promised background outbox or offline durable store. Its draft stays visibly unsaved in the open page until an available service acknowledges it. **Saved locally**, **Pending number**, **Pending publication** and **Unsaved** describe different states and must not be conflated.

Accepted task snapshot/events/receipt recover together before success; hourly Git publication alone is insufficient. Private notes, protected chat, usage, budget history and authority have separate durable sources. Rebuilding office SQLite does not authorize a new agent process or delete worktrees. Exact target mutation/replication/restore services remain unimplemented.

## 13. Accessibility

Severity/priority/status always have icon and word. Example screen-reader row: “Bug P-8/T-42, Prevent overlapping reservations, severity major, priority high, accountable Payments, assigned team Platform, assignee Ana, person.” Board columns are labeled lists; group headings support heading navigation. Dialogs trap/restore focus, field errors use aria-describedby and visible text, and action failure explanations are keyboard accessible. Triage/verification regions are labeled when their lifecycle is enabled. Privacy controls name the audience in plain words; author-private content never enters shared live-region announcements.

## 14. UX acceptance criteria

Active criteria define selected UX intent and needed implementation evidence; they do not assert deployed support. **Lifecycle-dependent** identifies retained UX whose transition/permission/notification contract must be selected before release. Deferred requirements are not launch gates for the selected data scope.

**Project page (PRJ)**

- PRJ-1 · The IA target uses Overview, Work, Team, Activity and Conversations with Projects back navigation. Work filters typed tasks. Backlog/Releases/Insights appear with their services; unsupported feature/debt/docs/settings/routine tabs are not fabricated. AC-IA17-1–4's source health editor remains pending its field/write contract.
- PRJ-2 · Overview shows the living Brief, real open/blocked counts and five latest qualified activity entries. Team separates accountable/contributing teams from member rows; Conversations exposes only authorized linked threads. Empty states and optional lead/health/due fields are honest.
- PRJ-3 · The header shows one required accountable team and optional contributors. Settings keeps repository integration branches on each project/repository association; invalid teamless imports require resolution.
- PRJ-4 · When the accepting service is unavailable, writes have an accessible reason and last known data stays readable with a stale/read-only label.
- PRJ-5 · Projects list accountable team separately from contributors, bug severity counts including unset, and lifecycle-supported triage counts.

**Tasks (TSK)**

- TSK-1 · The board has To do, In progress, Blocked and Done columns. Done shows the last 7 days, with **Show all done**.
- TSK-2 · A card can be moved by drag, by ⋯ › Move to, and by keyboard, and each move is announced to screen readers.
- TSK-3 · Blocked shows a reason. **Deferred:** structured decision blockers, automatic unblock/return to prior status and decision-derived waiting bookkeeping require their own lifecycle contract.
- TSK-4 · *(rev 2)* Every column sorts by priority (Urgent first), then age, and changing a task's priority moves its card and announces the change.
- TSK-5 · In a valid project/team context New task needs only a title and defaults priority Normal. Outside context accountable team is required, project optional. Assignment pickers distinguish active people/agents; an authenticated owner is assignable only through a valid employee identity.
- TSK-6 · **Execution-dependent:** Start now is offered only by a supported execution service with safe admission; it never types into a running session or starts work merely because assignment changed.
- TSK-7 · A task moved by someone else moves on the board without a reload, and a toast names who moved it.
- TSK-8 · The page shows linked bug tasks, immutable activity and per-repository branch/worktree evidence where known, independent of code repository existence.
- TSK-9 · Board or List is remembered per project.
- TSK-10 · *(new, rev 2)* Cards and rows show priority only when it isn't Normal, as an icon and a word. Priority is neutral; amber is reserved for a separate Needs you state, never urgency alone.
- TSK-11 · Task/bug Time follows the shared duration definitions: retain first Started across reopen, exclude evidenced Done/Cancelled spans, show open time as so far and mark unavailable earlier history. A baseline never supplies a fabricated start or zero. Status time and saved waits use their actual intervals, not current assignment; untracked time stays the rest, never idle.

**Checklists (CHK)** *(new, rev 4)*

- CHK-1 · Tasks and bugs have a checklist of ordered steps. The card shows "n/m" and marks required steps left with a neutral lock icon and the word "required"; the flag alone is not an owner blocker.
- CHK-2 · + Add step adds steps inline (Enter for the next, pasted lines become separate steps), each 1–200 characters.
- CHK-3 · Ticking or unticking records who and when, shows it on the step, and appears in the activity.
- CHK-4 · A step can be marked Required, shown with a lock icon and the word "Required".
- CHK-5 · **Deferred:** required-step completion gates and Mark done anyway with a recorded reason remain earlier workflow requirements; required flags alone do not enact the gate.
- CHK-6 · **Deferred:** Make a task promotion, completed-only automatic ticks/unticks, manual-tick prohibition and cancelled-task Unlink remain earlier advanced checklist requirements.
- CHK-7 · Steps can be reordered by drag, menu and Alt+↑/↓, and the change is announced.
- CHK-8 · **Deferred:** named templates/default checklists and process-derived task steps remain earlier requirements; no target docs/.templates/checklists source or process execution is selected.
- CHK-9 · **Deferred:** CLI completion gates, checklist promotion and done-anyway flags need an explicitly supported command adaptation. Existing CLI behavior is current-contract evidence, not target availability.

**Report a bug (BUG)**

- BUG-1 · Report bug can be opened from the project header, the Bugs tab, ⌘K, a task's menu and a doc's menu. From a task or doc, that item is linked.
- BUG-2 · Report bug is disabled until Title and What happened are filled, and the reason is shown as text.
- BUG-3 · After 3 characters of title, up to three similar open bugs in the project show. **This is the same bug** adds the text as a comment to that bug and opens it, and no new bug is created.
- BUG-4 · Severity may remain Not set; Critical/Major/Minor/Trivial each have an explanation. No area/General field is required.
- BUG-5 · The capture shows required Accountable team, optional Assigned team/Assignee and valid project context; suggestions do not change accountability or grant permissions.
- BUG-6 · Environment shows only actual available office/app and per-repository branch/commit evidence and can be edited/cleared; selected legacy report detail is preserved through details/task_details; unknown variants retain controlled import staging.
- BUG-7 · Reporting creates type bug in the shared T- scope with required accountability, immutable reporter and explicit assignment pair. Confirmation distinguishes accepted/number-pending from unsaved; no separate B- counter exists.
- BUG-8 · A bug reported by an agent shows that agent's immutable reporter identity, initial avatar and the word Agent; agent/person distinction never relies on avatar shape.
- BUG-9 · Closing the form with text in it asks "Discard this bug report?".

**Triage (TRI)**

- TRI-1 · **Lifecycle-dependent:** triage names its suggestion and acting user; no area-lead grant or implicit permission follows from team/accountability.
- TRI-2 · Triage UX confirms available severity/priority and saves a complete desired assigned-team/individual pair atomically at the expected task revision; either or both may be null.
- TRI-3 · Changing assignment can route work to any active same-organization team/employee without changing accountable team/project or number. An individual need not belong to the assigned team.
- TRI-4 · Reported severity/detail retain reporter/time provenance separately from current triage values; no mandatory reported area exists.
- TRI-5 · **Lifecycle-dependent:** Critical bugs remain prominent in the owner Needs you workflow; notification/triage rules need explicit implementation selection.
- TRI-6 · A stale assignment revision conflicts and changes neither field; show current and desired pair with actor/time and an explicit retry, preserving the user draft.
- TRI-7 · Work filters distinguish accountable team, assigned team, individual and reporter. **Deferred:** area-derived Needs my triage requires a selected routing contract.
- TRI-8 · **Deferred:** the earlier no-area-lead/no-team-lead fallback to owner triage is not selected. Required accountable ownership never becomes null.

**Bug list (BUG, continued)**

- BUG-10 · Bugs group by assigned team including unassigned, sorting severity/priority/age; no area is required. Lifecycle-specific triage grouping awaits its handler.
- BUG-11 · Group headings show assigned team and severity counts with words, including unset severity; accountable ownership remains visible separately.
- BUG-12 · Status filters preserve recorded values and are remembered per scope; a type-specific Open definition requires a supported lifecycle adapter.
- BUG-13 · Grouping by severity, priority, status, accountable/assigned team, assignee or none works and preserves row identity; no mandatory area group.
- BUG-14 · **Lifecycle-dependent:** a Fixed row can show verify action and immutable reporter; the assignee is not relabeled historical fixer.
- BUG-15 · The empty, no open bugs and no matches states show the copy from Flow 4, with Retry on failures and explicit read-only stale state when disconnected.
- BUG-16 · Copilot field provenance is visible/accessibly named until edited/kept; suggested severity remains explicit. Retry/Dismiss clears only its panel's stale error, retaining input, Undo draft and manual submission.

**Fix, verify, close and reopen (FIX)**

- FIX-1 · Create fix task retains bug context and valid required accountability; authored fixes points at the stable bug-task ID and inverse fixed_by is derived.
- FIX-2 · **Deferred:** automatic In progress/Fixed/reopen propagation, completed-only fix aggregation, required-step gate, all-cancelled handling and one-hour Undo remain earlier engine requirements, including the single cause-linked reopen count. Linked-task progress alone changes no other record.
- FIX-3 · **Lifecycle-dependent:** manual Mark as fixed records explanation, actor and time; current assignment does not prove who performed all work.
- FIX-4 · **Lifecycle-dependent:** verification names the original reporter and offers fixed/still happens with accessible audience/context; no automatic notification engine is implied.
- FIX-5 · **Lifecycle-dependent:** verification/Still happens retain actor/time and a required reopen note; exact state/count rules require an approved transition contract.
- FIX-6 · **Lifecycle-dependent:** show the acting verifier separately from the reporter and preserve immutable reporter provenance; authorization remains the service contract.
- FIX-7 · **Lifecycle-dependent:** Duplicate requires a resolvable original, rejects self-reference, and retains navigation/evidence; duplicate metadata uses details.duplicate_of_task_id and selected same-organization target/cycle validation.
- FIX-8 · **Lifecycle-dependent:** Won’t fix/Can’t reproduce require a retained reason and identify any actual notification audience; no delivery is claimed before acknowledgment.
- FIX-9 · **Lifecycle-dependent:** explicit reopen requires note, actor/time and retained original history; exact resolution/count/duplicate-edge disposition requires a transition decision.
- FIX-10 · **Adapter-dependent:** known commits/branches resolve via stored per-repository task identity and explicit reference mappings; textual old-prefix rewrites never create target links.
- FIX-11 · **Deferred:** reporter departure/area-lead verification fallback needs a selected routing model; historical reporter remains visible even inactive.

**Areas (AREA)**

- AREA-1 · **Deferred:** area settings/list with team, lead and open count remain an earlier unselected screen.
- AREA-2 · **Deferred:** unique flat area names, project-team constraint and optional direct-member lead require a selected area model.
- AREA-3 · **Deferred:** area rename must preserve associations and historical navigation if areas are selected.
- AREA-4 · **Deferred:** changing area team must preview affected open work and routing if selected.
- AREA-5 · **Deferred:** area deletion must explicitly rehome open bugs and retain deleted historical identity if selected.
- AREA-6 · **Deferred:** mandatory General, rename/move and delete prohibition are earlier requirements; no General is created by the selected model.
- AREA-7 · **Deferred:** area/team-removal transfer choices, failure/retry and safe cascade ordering are preserved earlier requirements; no teamless accepted project is allowed.
- AREA-8 · **Deferred:** area-lead departure and team-lead fallback require a selected routing/area contract.
- AREA-9 · **Deferred:** clearing an incompatible area lead on team change needs explicit preview and atomic application if selected.

**Docs (DOC)**

- DOC-1 · Docs pins project brief.md then supporting organization Git records by explicit scope/kind/status/update; features retain their distinct feature.md definitions and no duplicate supporting row.
- DOC-2 · Rendering sanitizes Markdown with no raw HTML, no automatic remote images and deliberate external-link navigation; existing v1 safety rules remain required.
- DOC-3 · Selected task references become typed status chips only when unambiguous in scope; historical references need explicit mapping. Mention indexes are derived.
- DOC-4 · Write/Preview and keyboard Save preserve draft text, stable identity, owner/status/scope and attribution, with durable save and Git publication shown separately. Exact organization Git editor/branch API remains implementation work; preserve hardened Git/worktree boundaries.
- DOC-5 · A newer revision offers Compare/explicit Keep mine/Discard mine without loss; failure retains text and Retry/Copy. No silent overwrite or publication claim.
- DOC-6 · The project brief is pinned at projects/<project-id>/brief.md with an empty hint and Write the brief; no code repository is required for organization Git knowledge.
- DOC-7 · Leaving the editor with unsaved changes asks before discarding.
- DOC-8 · Each document exposes canonical path and retained revision/history; restoring creates a new revision with unchanged stable ID and honest attribution.
- DOC-9 · Validated external organization Git edits update the projection after publication; from-git co-author trailers remain claims distinct from admitted-actor provenance.

**Work (WORK)**

- WORK-1 · Work spans projects and projectless teams with type views Tasks/Bugs/Requests plus knowledge/debt navigation, qualified references, project/no-project and team filters.
- WORK-2 · Saved filters distinguish Owned by my team, Assigned to my team, Assigned to me, Reported by me, Blocked and All; lifecycle-specific triage/verify filters require their selected rules.
- WORK-3 · **Lifecycle-dependent:** badge counts actionable owner questions/verification/blocked work using explicitly selected rules; account ownership does not imply employee assignment.
- WORK-4 · Every task type displays its type word/icon and T- reference in the selected scope; imported old references remain explicit historical mappings.
- WORK-5 · Displayed severity, priority and status have a meaningful icon and word, announced to screen readers. Severity/priority remain separate meta text, with neutral priority icons and no amber except an actual owner blocker; Normal may be omitted on rows/cards. Each row has one work-status signal.
- WORK-6 · Changes by others appear without a reload and never reorder a list while a menu is open in it.
- WORK-7 · The admitted service actor is recorded for all accepted changes. Current permissive phase-1 handlers remain implementation evidence; no target permissions are granted by this UX or assignment.
- WORK-8 · Within scope all types use T-n; outside use P-8/T-42 or TEAM-12/T-42. Global lookup lists all permitted scope-qualified matches; insufficient context never picks one. M-2 needs a release as well as a project. Pending number retains stable row identity and a copyable stable ID; replacing its label does not reorder rows.
- WORK-9 · Work remembers its type tab per authorized organization independently of filters and board/list preferences; an empty queue preserves it and an unavailable view has a safe supported fallback.
- WORK-10 · AC-IA18-1–3's Show views retain `?view=`, select the stated default and include both people and agents. Needs me and Inbox agree for supported sources, with no task row for an unlinked hand-off; unavailable source types are not inferred.

**Assignment, history and private boundaries (selected additions)**

- TSK-12 · Accountable team is required for every type; project tasks match project owner. Project/feature/milestone references stay same-project; projectless tasks have neither feature nor milestone.
- TSK-13 · All four assignment states are supported. Both fields and their old/new event commit with expected revision and durable receipt; stale/conflicting writes update neither. Home-team moves never silently rewrite assignments.
- TSK-14 · Reporter is admitted-creator-derived/immutable; old IDs, event order, times and actors survive import. Unknown history is marked unavailable, never invented from current state.
- TSK-15 · Accepted snapshot/events/receipt recover together; history survives task closure/archive/tombstone and unknown event tags. Pending numbering, pending publication and unsaved drafts remain distinct.
- TSK-16 · Type changes preserve stable ID/number/metadata. Import detects old namespace collisions and never blindly converts B-/REQ- numbers to T- numbers.
- DOC-10 · Each supporting doc requires project and at most one same-project feature/release/deployment; scope/path/kind agree, current requires an active accountable employee, and scope moves preserve ID without duplicate canonical copies.
- DOC-11 · Feature definitions use stable-ID feature.md paths, local_slug plus derived project prefix, explicit owners/status and behavior/limitation sections; task completion never silently rewrites feature status.
- TSK-17 · PN-01–06 in document 05 are required: author-only private CRUD/conflicts, no admin exception or shared existence metadata, durable recovery/closure continuity, unchanged privacy after assignment, explicit publish and documented retention/backup deletion. Shared note comments remain shared. Private runtime/store/access/UI are still unimplemented.
- TSK-18 · Output shows only permitted resolvable recorded deliverables and any shared completion explanation; missing evidence is explicit. Done alone never implies review, merge, validation, release or deployment. Arbitrary URL/path writes and outcome gates remain deferred.

## 15. Open questions

- C1/C10/A10/A11: resolved by selected data contracts: one scoped T- sequence, organization Git knowledge without a code repository, any-office local task/feature creation with organization registration, and HQ-only project/team/release/milestone creation. Earlier contrary answers are historical.
- C3/C19/C20/A49/A110: automatic fix/feature/decision/checklist behavior and Undo remain deferred; their older decisions do not select an engine. Plain checklist metadata and task history are selected.
- C5/C21: exact target CLI/wire adapters remain implementation work. Preserve actual old command/codec support; do not invent new task-assignment/private-note commands.
- C7: safe target worktree handling, commit attribution and uncommitted-work closure policy require execution/lifecycle decisions. Handoff preserves worktrees; it is not closure.
- C8: same-organization cross-project dependencies are selected. Exact bug fix/duplicate lifecycle constraints remain explicit decisions; no old same-project assumption overrides general dependency data.
- C4/A48: preserve resolution/reason data; target supported transitions and cancellation validation need lifecycle selection.
- C6/A12: watchers are selected data; notifications and target authorization remain separate implementation work. Shared note read access must never apply to private notes.
- A13/AREA-1–9: areas remain unselected. U7/U8/U9 retain owner attention and plain-language display intent without selecting automatic routing/analytics services.

## 16. Out of scope here (follow-ups)

Areas · advanced checklist/templates/promotion/gates · Undo · automatic bug/feature/debt propagation · process execution · scheduling/memory · retrospective analytics · attachments until storage design · GitHub Issues import. Releases/milestones, usage and budgets have selected data in their owning documents; their lifecycle/enforcement services are not implemented by this spec. Target wire/mutation/import/organization Git adapters and private-note runtime/access/UI still require implementation.

Retained task proposals still needing contracts:

- **Following / AC-TK1–2:** neutral task-level unseen tracking distinct from Inbox, manual/automatic follow reasons, actual owner waits surviving filters, and per-reader last-seen clearing from any task entry point. Exclude self-caused changes and all private-note existence/activity. Watcher rows alone do not implement cursors or source adapters; nested-child rollups also require a subtask model.
- **Source attribution / AC-TK3:** a future verified requested-by/message relationship remains distinct from immutable creator/reporter. Only a directly addressed/mentioned permitted message can establish it; source links retain their own access checks and the shared brief must be sufficient without exposing private content.
- **Review / AC-TK5:** same-task optional review, explicit reviewer/outcome and no self-approval; actor/defaulting/transition/Inbox rules require a lifecycle contract and never grant private-note access.
- **Shared brief/comment versions / AC-TK7:** retain attributed change rationale and failed drafts. Editing/deletion/redaction needs a retention decision reconciling immutable shared Git/events with secret-erasure expectations; do not promise erasure from exports/backups. Private notes follow PN-01–06 instead.
- **Planning / AC-PL2–7:** backlog rank/batch/no-op behavior, dates, close/reopen/open-item disposition, planner authorization and progress denominators remain pending. Current milestone membership is independent of explicit multi-release manifests; no exclusive plan field or automatic rollover is selected.
