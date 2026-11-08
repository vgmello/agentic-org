# Design: work items (tasks, bugs, requests, features), projects and project docs (W7)

Status: **proposed, rev 2 (Eden's review applied)** 2026-11-08. Owner: ada. It builds on [ADR-0001](../adr/0001-app-is-a-client-office-is-a-server.md), [ADR-0002](../adr/0002-storage-sqlite-sqlcipher-one-writer.md), [ADR-0003](../adr/0003-agent-messaging-office-first.md) (proposed), [`offices-hq-and-satellites.md`](offices-hq-and-satellites.md) ("Office-first: tasks and messages"), [`teams-and-org-chart.md`](teams-and-org-chart.md) and [`projects-git-github.md`](projects-git-github.md). It implements Eden's specs [`work-items-ux.md`](../specs/work-items-ux.md) rev 5, [`features-ux.md`](../specs/features-ux.md), [`feature-requests-ux.md`](../specs/feature-requests-ux.md), [`decisions-and-tech-debt-ux.md`](../specs/decisions-and-tech-debt-ux.md) rev 7 and [`costs-and-insights-ux.md`](../specs/costs-and-insights-ux.md) rev 3, and follows codex-dev's approved outline. Where it departs from a spec, §9 says so. **Rev 2** applies Eden's review of rev 1 against her specs: the departures she accepted are marked agreed in §9 and carry her UI wordings where this design mentions UI; time figures no longer fold untracked working time into idle (§6); waiting on the owner includes triage and decisions (§6); values set with git read "from git" (§4.9); every doc has a stable `id` (§4.9); `doc template show|edit` (§5); and the gaps she listed are plan items with a phase (§7.1). It sets the work-items library's crate name: **`org-work`**.

## In one paragraph
Tasks, bugs and feature requests are **work items**: one shared core (id, number, title, status, priority, assignee, events, checklist, links, comments) with per-kind fields and a state machine per kind. They are **office data** in `office.db`, office-first: the assignee's office owns each one, and every change is an immutable **event** that is also the activity feed and the only source of time figures. Phase 1 has one office (HQ), so HQ owns everything, but every change is already numbered for the per-origin stream. **Projects, areas and project-team links are organization data** in `org.db`: HQ commands, exactly like the teams contract (T1). **Features and every other persistent doc are Markdown files with YAML front matter in the project's repository**; the office keeps an index and a small feature record, and writes docs only as commits on branches that land through the merge flow. The library `org-work` holds the rules and defines a storage trait that `store.rs` implements; it has no SQL, no git and no network.

## Decisions
1. **One work-item core for task, bug and request.** Per-kind fields and state machines sit on it (§1).
2. **Data split.** Projects, areas and project-team links: organization data (HQ commands, `org.db`). Work items, checklists, links, comments, events, feature records and the docs index: office data (`office.db`). Persistent knowledge (features, ADRs, designs, debt, …): files in the project's repository (§0).
3. **Numbers.** Tasks and bugs have **separate per-project sequences**: `T-12`, `B-7`. Requests have one organization-wide sequence: `REQ-14`. HQ assigns every number. The UI always shows the prefix (`T-12`, never a bare `12`), and outside its project an item is written `<project key>-T-12` (e.g. `chk-T-12`). An item without a number yet shows **Pending · \<short id\>** (§2.6). Agreed with Eden (§9).
4. **Areas** are flat, have a stable id, one owning team (empty when the project has no team) and an optional area lead. Every project has **General**, which can be renamed and moved to another team but never deleted.
5. **Bugs:** severity `critical | major | minor | trivial` (required, confirmed at triage); priority `urgent | high | normal | low`, shared with tasks; a bug links to fix tasks and never turns into one; closure is verified by the reporter or the area lead (suggested; anyone may); won't fix, can't reproduce and duplicate need a reason or the original; a closed bug can be reopened.
6. **Checklists** on tasks and bugs: ordered steps with stable ids; who ticked and when come from events; required steps guard Done (or Fixed) unless "done anyway" with a recorded reason; a step can become a linked task.
7. **Time figures** (lead, cycle; waiting on the owner, blocked, and the rest) are derived only from the item's immutable events (§6). The rest is split into working and idle only once sessions report working time (W14/W15); untracked working time is never shown as idle.
8. **No permissions in phase 1:** any bound member may triage, verify, close or reopen. But **agents act only through their bound identity** (ADR-0003 §2.1): the wire never carries an actor, the office uses the connection's bound member. The app-only rule covers calls that change spend, identity or destroy history (§2.2).
9. **Features** are repository docs (`docs/features/<n>-<slug>.md`). `feature_id` on tasks is the database's source of truth for the task ↔ feature link; the doc's `tasks` list is regenerated on each status commit. A feature becomes In progress and Delivered automatically, with Undo.
10. **Docs** are Markdown + YAML front matter in the project repository (`docs/`, templates in `docs/.templates/<kind>.md`). Every doc has a stable `id` in its front matter, and links to docs are by id (§4.9). The office keeps an index (id, path, kind, title, last commit). Writes are commits on a branch only, never the default branch: the office commits on branches it makes (`docs/<slug>-<short id>`), and an agent can instead put a doc on its own `task/…` branch, where the CLI writes the rendered file into the agent's worktree and the agent commits it like any file. Owner as author and agent as co-author; everything lands through the merge flow.
11. **Automatic changes are the library's**, not the app's (bug follows its fix tasks, feature follows its tasks, a made-task step ticks itself). Each is its own event marked automatic, with an Undo for one hour (§2.4).
12. **Tasks** keep v1's statuses (To do, In progress, Blocked, Done). Done has a resolution: `completed` (default) or `cancelled`.
13. **Deleting** a work item is app-only and a tombstone: the record and its events stay, its number is never reused, links to it show "deleted", and the automatic rules ignore it.

---

## 0. Where each thing lives

| Thing | Where | Who changes it | How it changes |
| --- | --- | --- | --- |
| Project (key, name, repository, integration branch, archived) | `org.db` (HQ's, copied everywhere) | the app | HQ command `project.*` |
| Project-team link | `org.db` | the app | `project.add_team` / `project.remove_team` (T1, extended) |
| Area | `org.db` | the app | HQ command `area.*` |
| Task, bug, request; their checklist steps, links, comments and events | `office.db` of the **owning office** (HQ in phase 1) | anyone bound (app or agent), through the owner | work calls (§4) |
| Item numbers | assigned by HQ, stored with the item | HQ only | in the create (phase 1), or as a separate HQ record (phase 2, §2.6) |
| Feature record (status, delivered, task links) | `office.db` at HQ | anyone bound; status is mostly automatic | work calls (§4.7) |
| Feature definition | repository, `docs/features/<n>-<slug>.md` | through the office (and by hand with git) | branch commits (§4.9) |
| Other docs, templates, guidance | repository, `docs/…` | through the office or git | branch commits |
| Docs index (id, path, kind, title, status, last commit, mentions, former paths and ids) | `office.db`, a cache | the office | rebuilt from git; never a source of truth (former paths and ids come from each doc's `former_paths` and `former_ids` front matter, §4.9) |
| Session usage (W14) | `office.db` | the office | out of scope here; §6 says how working time uses it |

Rule (the user, decisions spec rev 4): **persistent in the repository, transitional in the database.** Nothing persistent is duplicated in the database except the index, which is rebuildable.

## 1. The model

### 1.1 Ids and references
- **`id`**: a ULID made by the client (as in T1). It never changes and is the only key the wire uses for writes.
- **`short_id`**: the last 6 characters of the id, lowercase (e.g. `3k9f2x`). For display ("Pending · 3k9f2x") and for typing. It resolves only when exactly one item the office holds matches; otherwise `invalid_request` ("ambiguous").
- **`number`**: per kind scope (§2.6). Display forms:

| Kind | In its project | Elsewhere (messages, other projects, commits, CLI) | Pattern |
| --- | --- | --- | --- |
| Task | `T-12` | `chk-T-12` | `\b([a-z][a-z0-9]{1,9})-T-([1-9][0-9]*)\b` |
| Bug | `B-7` | `chk-B-7` | `\b([a-z][a-z0-9]{1,9})-B-([1-9][0-9]*)\b` |
| Request | `REQ-14` | `REQ-14` | `\bREQ-([1-9][0-9]*)\b` |
| Feature | `FEAT-0007` | `chk-FEAT-0007` | the doc's number (§1.8) |

  The unqualified `T-12` / `B-7` is resolved against a project the caller names (CLI: `--project`, or the project of the agent's current task). `chk` is the project **key** (§1.9).
- **Item reference params** (`item` in §4) accept an id, a qualified number (`chk-T-12`, `REQ-14`), or a short id. Writes made by the app always send the id.

### 1.2 The core (every kind)
| Field | Type | Notes |
| --- | --- | --- |
| `id` | ULID | client-made |
| `kind` | `task \| bug \| request` | fixed at create |
| `project_id` | id, optional | required for tasks and bugs; optional for requests |
| `number` | u64, optional | empty until HQ assigns it (always set in phase 1) |
| `title` | text | 1–120 characters, one line |
| `status` | per kind (§1.10) | changes only through the state machine |
| `resolution` | per kind, optional | set exactly when the status is terminal (`done`, `closed`) |
| `priority` | `urgent \| high \| normal \| low` | tasks and bugs; default `normal` |
| `assignee_id` | member id, optional | tasks and bugs: who does it; requests: who scopes it |
| `created_by` | member id | the bound member that created it (the reporter of a bug, who raised a request) |
| `created_at`, `updated_at` | RFC 3339 UTC ms | the owning office's clock (§2.7) |
| `watchers` | member ids | the creator always; plus duplicate reporters and anyone who asks (C6, minimal) |
| `owner_office_id` | office id | the office that owns the item (HQ in phase 1) |
| `generation` | u64 | starts at 1; +1 on a handover or reclaim (phase 2) |
| `version` | u64 | starts at 1 (the create); +1 for each committed change; equals the last event's version |
| `deleted` | bool | a tombstone; deleted items are left out of reads unless asked |

### 1.3 Task
| Field | Type | Notes |
| --- | --- | --- |
| `description` | Markdown, optional | ≤ 20,000 characters |
| `definition_of_done` | text, optional | ≤ 1,000 characters ("the goal the agent works to") |
| `team_id` | team id, optional | one of the project's teams |
| `feature_id` | feature id, optional | at most one feature (P3) |
| `blocked_reason` | text | 1–200 characters, one line; set exactly when `status = blocked` |
| `done_anyway` | `{ reason, steps: [step ids] }`, optional | set when Done with required steps unticked; cleared on reopen |
| `branch` | text, optional | set by the office when it makes the task's worktree (`task/<ref>-<slug>`, §9); read-only to clients |
| `shortcut` | `none \| debt [DEBT ids]`, optional | the answer to "did you take a shortcut?" (P4, GUIDE-3) |

### 1.4 Bug
| Field | Type | Notes |
| --- | --- | --- |
| `area_id` | area id | required; defaults to the project's General |
| `severity` | `critical \| major \| minor \| trivial` | required at report; no default |
| `severity_confirmed` | bool | false until triage |
| `what_happened` | text | 1–10,000 characters |
| `expected`, `steps` | text, optional | ≤ 10,000 characters each |
| `environment` | text, optional | ≤ 2,000 characters; the client prefills it |
| `closed_reason` | text, optional | won't fix / can't reproduce reason, or "what fixed it" for Fixed by hand; 1–1,000 |
| `duplicate_of` | bug id, optional | set exactly when closed as duplicate |
| `reopen_count` | u32 | how many times it was reopened or sent back by "still happens" |
| `affects_feature_id` | feature id, optional | P3 |

Derived, never stored: the **suggested triager** (the area lead, else the area team's lead, else the owner) and the **suggested verifier** (the reporter if still an active member, else the suggested triager).

### 1.5 Request
| Field | Type | Notes |
| --- | --- | --- |
| `text` | text | the original words, 1–20,000 characters; never edited |
| `request_kind` | `feature \| change \| technical \| question`, optional | suggested, changeable |
| `route_reason` | text, optional | ≤ 500; why this scoper ("product designer: a new user-facing flow") |
| `routed_by` | `person \| rule \| copilot`, optional | where the suggestion came from |
| `outcome` | `Outcome` (`RequestOutcome` in Rust), optional | set when scoped (below) |
| `merged_into` | request id, optional | set exactly when `merged` |
| `decline_reason` | text, optional | 1–1,000; set exactly when `declined` |

`Outcome { feature: Option<FeatureOutcome>, docs: [DocRef], draft_tasks: [DraftTask], estimate: Option<Estimate> }`, where `FeatureOutcome = New { feature_id } | Extends { feature_id }`, `DraftTask { draft_id, title, description? (≤ 2,000), assignee_id?, team_id?, priority? }` (≤ 50; the encoded `Outcome` ≤ 128 KiB, `too_large` otherwise), `Estimate { effort?: text ≤ 200, cost_usd_cents_min?: u64, cost_usd_cents_max?: u64 }`.

### 1.6 Checklist
`ChecklistStep { id (client-made ULID), text (1–200, one line), required: bool, done: bool, task_id?: id, version }`, ordered. At most 100 steps per item. `checked_by` and `checked_at` in reads come from the latest check event. A step with `task_id` was made into a task (CHK-6): it ticks only when that task becomes Done · Completed, unticks when it's reopened, and can't be ticked by hand (`checklist.check` refuses it). When its task is Done · Cancelled the step stays unticked and shows "T-42 was cancelled" with **Unlink** (`checklist.unlink_task` makes it a plain step again). Agreed with Eden (§9). Tasks and bugs have checklists; requests don't.

### 1.7 Links
`Link { id (client-made), from_item_id, kind, to: Item { item_id } | Doc { project_id, doc_key }, created_by, created_at }`. The link belongs to its `from` item (its owner commits it). Reads return each item's links and its **backlinks** (links whose `to` is the item).

| `kind` | From → to | Made by | Rule |
| --- | --- | --- | --- |
| `fixes` | task → bug | `task.create { fixes }`, `work.link` | same project (C8: phase 1); drives the bug's status (§2.4) |
| `part_of` | task → task or bug | `checklist.make_task` | the new task is part of the step's item; the step names it |
| `from_request` | task → request | `request.approve` only | read-only afterwards |
| `reported_from` | bug → task or doc | `bug.report { from }` | "Report a bug from this task / about this doc" |
| `relates_to` | any item → item or doc | `work.link` | the "Why" link (ADR, design, evaluation) is `relates_to` a doc |
| `pays_back` | task → doc (a `DEBT-n`) | `work.link` | P4: drives the debt doc's Planned and Paid (§2.4) |

`doc_key` is the doc's front-matter `id` (`ADR-0004`, `DEBT-0012`, or `doc-…` for unnumbered kinds, §4.9). Only a link made to a doc that has no id yet (written with git and never saved through the tool) stores its path; it keeps resolving after the doc gets its id and after a `doc.rename` (§4.9). Duplicates use the bug field `duplicate_of`, not a link. At most 100 links per item.

### 1.8 Feature record (office data, P3)
`Feature { id (client-made ULID), project_id, number (FEAT-n, from the doc), path, title, status: proposed | planned | in_progress | delivered | retired, delivered_at?, retired_reason?, replaced_by?, version }`. HQ owns every feature record. The task ↔ feature link is `feature_id` on the task. The doc's front matter `status`, `tasks` and `delivered` are written by the office from this record on each status commit; editing them by hand is overwritten at the next status commit, and `doc validate` flags it. Agreed with Eden (§9).

### 1.9 Project and area (organization data)
- `Project { id, key, name, description?, repo?: ProjectRepo, integration_branch?, archived, created_at?, version }` (`created_at` is absent only on records from before it existed)
  - `key`: 2–10 characters, lowercase ASCII letters and digits, starting with a letter; unique in the organization; never changes. Reserved: `req`, `feat`, `adr`, `eval`, `debt`, `lim`, `exc`.
  - `repo`: `ProjectRepo { host: local | github, default_branch, path? (local: the bare repository on HQ's Mac), github?: { owner, name, repo_id, private } }`. Never a token. The local path is visible organization-wide (like an agent's `home`).
  - `integration_branch`: the branch the Docs tab reads (C13). Empty means the repository's default branch.
  - `archived`: no new items in it; everything else still works.
- `Area { id, project_id, name, team_id?, lead_member_id?, general: bool, deleted: bool, replaced_by?, version }`
  - `name`: 1–40 characters (organization-name rules), unique in its project among areas that aren't deleted, ignoring case.
  - `team_id`: one of the project's teams; empty only while the project has no team, or after its team left as the project's last one or was deleted (§2.5).
  - `lead_member_id`: a member of `team_id` (`not_a_member` otherwise), never the owner (`owner_protected`), never an offboarded agent (`invalid_request`).
  - `general`: exactly one per project, made by `project.create`; it can't be deleted.
  - A deleted area stays as a tombstone (`deleted: true`, `replaced_by`) so closed bugs keep showing "Payments (deleted)".

### 1.10 State machines
"Anyone" means any bound member, app or agent. Every transition is one event, with the actor and the office's time. A transition that the current status doesn't allow is `invalid_request` ("wrong state").

**Task** (`todo → in_progress ⇄ blocked → done`, reopen back to `todo` or `in_progress`):

| From | To | Call | Who | Guard and effect |
| --- | --- | --- | --- | --- |
| (none) | `todo` | `task.create` | anyone | number assigned; `fixes` links made |
| `todo`, `blocked` | `in_progress` | `task.set_status` | anyone | if the caller is the assignee agent, the office sets it as the agent's current task (§6) |
| `in_progress` | `todo` | `task.set_status` | anyone | a board move back (TSK-2); not a reopen: the round goes on and its cycle start stays the first start |
| `todo`, `in_progress` | `blocked` | `task.set_status { blocked_reason }` | anyone | reason required |
| `blocked` | `todo` | `task.set_status` | anyone | clears the reason |
| `todo`, `in_progress`, `blocked` | `done` | `task.set_status { resolution?, anyway_reason? }` | anyone | `resolution` defaults to `completed`. Required steps unticked: `checklist_incomplete` unless `anyway_reason` is given, which is recorded as `done_anyway`. `cancelled` skips the step guard. |
| `done` | `todo`, `in_progress` | `task.set_status` | anyone | a **reopen**: clears `resolution` and `done_anyway`; starts a new round (§6) |
| any | (deleted) | `work.delete` | app only | tombstone |

**Bug** (`new → triaged → in_progress → fixed → closed`, with `closed` from any open status, and reopen):

| From | To | Call | Who | Guard and effect |
| --- | --- | --- | --- | --- |
| (none) | `new` | `bug.report` | anyone | severity required; area defaults to General; unassigned; number assigned |
| `new` | `new` | `bug.update { area_id }` | anyone | **reroute** (TRI-3): the suggested triager changes; nothing else does |
| `new` | `triaged` | `bug.triage { area_id, severity, priority, assignee_id? }` | anyone | confirms area and severity (`severity_confirmed`), sets priority |
| `triaged` | `in_progress` | automatic | the library | the first fix task goes In progress (§2.4) |
| `triaged`, `in_progress` | `fixed` | automatic | the library | every fix task is Done and at least one completed, and no required step is left (§2.4) |
| `triaged`, `in_progress` | `fixed` | `bug.mark_fixed { note, anyway_reason? }` | anyone | "what fixed it" (1–1,000) into `closed_reason`; the step guard as for tasks |
| `fixed` | `in_progress` | automatic | the library | a fix task goes back from Done |
| `fixed` | `closed` (`verified`) | `bug.verify { outcome: fixed }` | anyone (suggested: the verifier) | the event records whether the actor was the reporter |
| `fixed` | `triaged` | `bug.verify { outcome: still_happens, note }` | anyone | note required; `reopen_count + 1`; assignee and fix links kept |
| `new`, `triaged`, `in_progress`, `fixed` | `closed` (`duplicate`) | `bug.close { duplicate_of }` | anyone | the original: same project, not this bug, not deleted, not itself closed as duplicate; the reporter becomes a watcher of the original |
| same | `closed` (`wont_fix`, `cant_reproduce`) | `bug.close { reason }` | anyone | reason 1–1,000 |
| `closed` | `triaged` | `bug.reopen { note }` | anyone | note required; `reopen_count + 1`; a new round |
| `closed` (`duplicate`) | `new` | `bug.reopen { note }` | anyone | `duplicate_of` cleared, so triage looks again |
| any | (deleted) | `work.delete` | app only | tombstone |

**Request** (`new → routed → scoping → scoped → approved`, or `declined` / `merged` from any open status). Routed is agreed with Eden (§9); for a request routed to Eden, the UI shows Routed as "Sent to Eden · not accepted yet" and Scoping as "Eden is scoping".

| From | To | Call | Who | Guard and effect |
| --- | --- | --- | --- | --- |
| (none) | `new` / `routed` | `request.create { scoper_id? }` | anyone | `routed` when a scoper is given; REQ number assigned |
| `new`, `routed`, `scoping` | `routed` | `request.route { scoper_id, reason? }` | anyone | (re)route; `scoper_id` must be a valid recipient (§2.1) |
| `routed`, `new` | `scoping` | `request.accept` | anyone | the **caller** becomes the scoper |
| `routed`, `scoping` | `new` | `request.route { scoper_id: none }` | anyone | back to "Not routed" |
| `routed`, `scoping` | `scoped` | `request.scope { outcome }` | anyone | outcome required |
| `scoped` | `scoping` | `request.ask_changes { note }` | anyone | note required (a comment) |
| `scoped` | `approved` | `request.approve { tasks }` | **app only** | the project must be set and not archived (a request without a project: the app's **Approve** asks for one and sends `request.update { project_id }` first); creates the tasks atomically (§2.4); the feature's doc gains `requests: [REQ-n]` and the request's text quoted under Notes (RQS-11, a doc-write effect) |
| `new`, `routed`, `scoping`, `scoped` | `declined` | `request.decline { reason }` | anyone | reason required |
| same | `merged` | `request.merge { into }` | anyone | `into`: another open request, not this one; a comment on `into` quotes this request's text (cut to 10,000 characters, with "continued in REQ-n" when cut) and its creator becomes a watcher |
| any | (deleted) | `work.delete` | app only | tombstone |

**Feature** (P3; office record, §1.8):

| From | To | Call | Who | Guard and effect |
| --- | --- | --- | --- | --- |
| (none) | `proposed` | `feature.create` | anyone | writes the doc from the template on a docs branch (§4.9) |
| `proposed` | `planned` | `request.approve` or `feature.set_status` | app only (approval) | |
| `planned` | `in_progress` | automatic | the library | a linked task goes In progress, or a task already In progress is linked (approval alone leaves it Planned, FTR-5) |
| `delivered` | `in_progress` | automatic | the library | a linked task is reopened, or an open task is linked (agreed with Eden, §9) |
| `planned`, `in_progress` | `delivered` | automatic | the library | every linked task is Done and at least one completed; the owner is told; retros listen (P5) |
| `proposed`, `planned`, `in_progress`, `delivered` | `retired` | `feature.set_status { reason, replaced_by?, close_open_tasks? }` | anyone | reason required; open tasks become plain tasks (their `feature_id` is cleared), or with `close_open_tasks` are Done · Cancelled (FTR-8); done tasks keep it |

Every feature status change also commits the doc's front matter on a docs branch (§4.9).

## 2. The rules

### 2.1 Validation
- **Text:** trimmed; one-line fields (titles, reasons, step text, blocked reason) have no control characters; multi-line fields allow `\n` and `\t` only. Limits are in §1. Out of limits: `invalid_request` naming the field.
- **References** are checked against the office's `org.db` copy **at the position it has applied** (never against queued org commands, offices design): the project exists and isn't archived (for every call that creates an item, `task.create`, `bug.report`, `checklist.make_task`, `request.approve`, and for `request.create` and `request.update` naming a project), a team is one of the project's teams, an area is in the item's project and not deleted, an assignee or scoper is a valid recipient (next point).
- **Recipients** (assignee, scoper, a question's addressee): an agent of the organization (active or paused, not offboarded), a person who has been added, or the owner. Anyone else: `unknown_recipient` (teams design). A paused agent may be assigned work; the app warns.
- **Enums** a build doesn't know decode as `Unknown`; writers never send `Unknown` (`invalid_request`).
- **Kinds:** a task call on a bug (or any kind mismatch) is `invalid_request`.

### 2.2 Who may do what
- Every work call needs a **bound connection** (ADR-0003 `msg.bind`): an agent bound as itself, or the app (with `app_token`) bound as the owner or a person. An unbound connection gets `not_bound` (ADR-0003 §4.1's code; whichever contract lands first adds it).
- **The actor is always the bound member.** No call carries `actor`, `reporter` or `created_by`. A connection acts only as the member it is bound as, and only an app connection can act as the owner or a person.
  - **Residual risk (ADR-0003 §2.1, M-A, accepted for v1):** until the office runner spawns agents with a session token, any local process can bind as any agent (`--as` / `ORG_AGENT`). So an agent named on a work item means "a process that bound as that agent". It is never a person or the owner.
- **No permissions in phase 1:** any bound member may make any call below except the app-only ones.
- **App-only** (refused on any other connection with `not_authorized`, checked by the receiving office before anything else):
  - `request.approve` and `feature.set_status { planned }`: they commit the organization to work, so they are the owner's approval (feature-requests U21);
  - `work.delete`: it destroys history;
  - every organization command in §4.8 (`project.*`, `area.*`), as T1 (teams design: every org command is app-only today).
  - `feature.set_status { retired, close_open_tasks: true }`: it cancels approved work (FTR-8: "unless the owner closes them"); retiring without closing is open to anyone.
  - Assigning or routing to an on-call agent can wake it, which costs money (RQS-8); wakes follow the run-modes design and its budgets, not this contract. Starting a session on a task ("Start now") is the agent-sessions design's call and is not here. Nothing else in this contract changes spend or identity.
- Reads are open to every bound member (no row visibility rules for work items in phase 1).

### 2.3 Ids, idempotency and conflicts
- **Every write carries `change_id`**, a client-made ULID. The owning office keeps `change_id → (params hash, result)` for 30 days after the ULID's time, failures included except `storage_error`, `unavailable`, `not_bound`, `too_large`, `expired` and `clock_skew` (never stored: a later retry may succeed).
  - The params hash is SHA-256 over the method name, the bound member id and the decoded, re-encoded typed params as canonical JSON (object keys sorted, no insignificant whitespace).
  - The app-only check comes first, then the lookup. A retry with the same `change_id` and hash gets the stored result without re-validating. The same `change_id` with another hash is `invalid_request` ("change id reused").
  - The lookup comes **before** the age checks. Then: dated more than 29 days ago, `expired`; more than 5 minutes ahead of the office's clock, `clock_skew`. The office keeps a monotonic high-water mark of its clock (offices design).
  - The table moves with the item at a handover (phase 2).
- **Client-made ids** (`item_id`, `step_id`, `link_id`, `comment_id`, `feature_id`, draft task ids): a valid ULID (`invalid_request` otherwise), not already used (`already_exists`).
- **`expected_version`** is required on every call that changes an existing item, except the commutative ones: `checklist.add`, `checklist.check`, `checklist.move`, `checklist.make_task`, `checklist.unlink_task`, `work.comment`, `work.watch`, `work.link` and `work.unlink` (`work.undo` has its own rule, §2.4). A value above the current version is `invalid_request`.
- **Conflicts are per field, not per item.** Each call touches a set of aspects: the fields it writes, plus `status` when it changes the status. It is refused with `conflict` only when an event after `expected_version` changed one of those aspects. So a comment or a tick never makes someone's title edit stale, but two people triaging the same bug do conflict (TRI-6).
  - **Checklist steps** have three aspects: content (text and required), `done`, and position. A step's `version` counts the events that touched it. `checklist.edit` and `checklist.remove` take the step's `expected_version` and conflict only if the step's **content** changed after it (or, for `remove`, it was made a task), so a tick or a move never makes a text edit stale (work-items §9.1: "my text edit is kept").
- **One call, one event per item.** A call records exactly one event, at one new version, on each item it changes, carrying every change it made to that item (`Triaged` carries area, severity, priority and assignee; Done anyway is one `StatusChanged` with its `anyway` detail; `checklist.add` is one `StepsAdded`). Automatic changes are separate events at the next versions, in the same transaction (§2.4), and so are the office's waiting bookkeeping events (`WaitingStarted` / `WaitingEnded`, §6): system events (`actor_id: None`, `auto: false`, `cause` = the triggering item event, if there is one) whose only aspect is `waiting`, which no call writes, so they never make a caller's `expected_version` stale and are never undone.
- **Error order** (the first that applies): `not_bound`, `not_authorized`, `expired`, `clock_skew`, `not_found`, `too_large`, `invalid_request` (the params alone: shape, limits, references), `invalid_name`, `unknown_recipient`, `not_a_member`, `owner_protected`, `conflict`, `invalid_request` (the current state doesn't allow it), `checklist_incomplete`, `already_exists`, then `unavailable` and `storage_error`. Stale beats wrong-state on purpose: "Riley triaged this a moment ago" is the useful answer. This departs from `org.rs`, where wrong state comes before `conflict`; the organization commands of §4.8 keep `org.rs`'s order (§9).

### 2.4 Automatic changes and Undo
The library makes these changes **in the same transaction** as the change that triggers them (phase 1). Each is its own event with `auto: true`, no actor, and a `cause` naming the triggering event. Each rule fires only on a triggering change (an edge), never on a scan, so an Undo is never re-applied by the same trigger.

| Rule | Trigger | Change |
| --- | --- | --- |
| Bug follows fix tasks (C3) | while the bug is `triaged`: a fix task (`fixes` → bug) goes `in_progress`, or an `in_progress` task is linked with `fixes`; or `bug.triage` commits while a fix task is `in_progress` | bug → `in_progress` |
| | a fix task goes `done`, or `bug.triage` (New → Triaged) commits, and now every non-deleted fix task is `done`, at least one `completed`, and the bug is `triaged` or `in_progress`. Never on "still happens" or a reopen: those return to Triaged on purpose | bug → `fixed`; if the bug has required steps unticked, **no change**, and an auto event records "all fix tasks done; required steps left" (agreed with Eden, §9). If every fix task is Done · Cancelled, **no change**: the bug says "All its fix tasks were cancelled. Create a fix task, or close the bug" |
| | a fix task goes back from `done` while the bug is `fixed` | bug → `in_progress` |
| | unlinking or deleting a fix task | no change |
| Step follows its task (CHK-6) | the step's task goes `done` (`completed`) / is reopened | step ticked / unticked (Done · Cancelled leaves it unticked: "T-42 was cancelled", §1.6) |
| Feature follows its tasks (P3) | while `planned`: a linked task goes `in_progress`, or an `in_progress` task is linked. While `delivered`: a linked task is reopened, or an open task is linked | feature → `in_progress` |
| | a linked task goes `done`, or a linked open task is unlinked or deleted, and now every linked non-deleted task is `done`, at least one `completed` | feature → `delivered`, `delivered_at`; the owner is told; retros listen (P5). If every linked task is Done · Cancelled, **no change**: the feature says "Add tasks or retire it" |
| Debt follows payback tasks (P4) | a `pays_back` link is made while the debt is `open` / a `pays_back` task goes `done` and now every non-deleted `pays_back` task of that debt is `done`, at least one `completed` | the debt doc → `planned` / `paid`. Debt has no record, so the automatic event (`DebtStatusChanged { doc_key, to }`) is recorded on the **triggering task**, with a `DocWrite` effect. The debt's current status is read from the docs index (`WorkTxn::doc_status`). Undo writes the earlier status back, and is `conflict` unless the doc's current status still equals the one the event set; if the event's `DocWrite` hasn't started yet (still pending, e.g. "Planned once merged"), Undo cancels that pending write instead; a write whose docs intent is recorded counts as carried out, and Undo is then another commit. If every `pays_back` task is Done · Cancelled, **no change**: the debt stays Planned. While the debt doc is only on an agent's unmerged `task/…` branch, the status can't be written yet (§4.9) and the debt shows "Planned once merged" (agreed with Eden, §9) |
| Request approval (part of the call, not automatic: the owner is the actor, `auto: false`, no Undo) | `request.approve` | creates each task (`from_request` link, `feature_id` when the outcome names a feature); a `proposed` feature → `planned` (a feature already `planned`, `in_progress` or `delivered` keeps its status, and the rules above apply to the new tasks; extending a `retired` feature is `invalid_request`) |
| Member leaves | `member.offboard` / `member.remove` applied (§2.5) | their open items unassigned; requests they scope go back to `new` |
| Area deleted | `area.delete` applied (§2.5) | open bugs move to the replacement area |
| Team leaves the project | `project.remove_team` / `team.delete` applied | tasks with that `team_id` lose it |

The last three rows are **cascades**: system events (`actor_id: None`, `auto: false`), never undone (the app offers reassignment instead). The Request approval row is part of its call (`auto: false`). The other rows are **automatic** (`auto: true`).

**Undo** (`work.undo { target_id, event_version }`): only for an automatic event, within 1 hour of it, and only if no later event changed the aspects it changed (`conflict` otherwise). It records a new event that restores the earlier values, with the caller as actor and `cause` = the undone event. Doc changes are undone by another commit.

In phase 2 an automatic change can target an item another office owns (a fix task at Office A, its bug at HQ). The library then returns it as a **change request** to that item's owner instead of applying it (§2.7); the owner applies the same rule. Phase 1 never has that case.

### 2.5 Cascades from organization changes
Work items live in `office.db` and organization data in `org.db`, which have separate writers (ADR-0002). So work cascades **follow** an organization event instead of sharing its transaction:
- After the office applies (HQ: commits) an organization event at position P, it runs the library's `on_org_applied(P, changes)`. Its work changes record P as their organization position, and the same work transaction stores P as the work side's **org cursor**.
- **The cascades reconcile from current state**, not from the event's details: `on_org_applied` finds open items assigned to someone who is no longer a valid recipient, requests scoped by one, watches held by one (they end), tasks whose `team_id` is no longer one of their project's teams, and open bugs in a deleted area (following `replaced_by` to a live area), and fixes each. The organization changes only narrow where to look (`OrgView::changes_after`); when they can't be replayed back to the cursor (the org log was trimmed), the office scans everything. So the cascades are idempotent, and a removed link that no longer says which project it was (`project_team_removed`) is still handled.
- On start, the office runs the reconciliation for every organization position after the stored cursor, so a crash between the two commits loses nothing.
- **Catch-up inside every write.** Every work write reads `OrgView` inside its own `transact` (never a value read earlier), and before its own rules it compares `OrgView::applied_position()` with the org cursor: if the organization is ahead, it first runs the pending cascades **in their own savepoint**, with their own feed row, and leaves their changes out of the caller's result (they reach clients as their own `work.changed`). If the catch-up fails, it rolls back only its savepoint and checks that the batch's transaction is still open (`is_autocommit()` false, ADR-0002 decision 4); if SQLite ended it, `f` returns `Err` and the writer re-runs the batch. Otherwise the call goes on, and the office retries the catch-up in the background with backoff: correctness doesn't depend on it, because the call's own checks read `OrgView` directly. Work writes are serialized on one writer, so for an `assign` racing a `member.offboard` either the assign commits first and the cascade (which queries current state) unassigns it, or the assign sees the offboarded member and is refused. The full scan (when the org log can't be replayed to the cursor) runs only in the background, never inside a client's request.

| Organization change | Work cascade (each a system event) |
| --- | --- |
| `member.offboard`, `member.remove` | their open tasks and bugs are unassigned (status kept); requests they scope go back to `new`; their watches end. Shown to the owner for reassignment (teams design). |
| `area.delete { move_open_to }` | the area's open bugs (status not `closed`) move to the replacement area; New ones go to its triager. Closed bugs keep the old area. |
| `project.remove_team`, `team.delete` | tasks of that project with that `team_id` lose it. |
| `project.archive` | none (new items refused). |
| any change to an area's lead or team, a team's lead, or membership (`area.set_lead`, `area.set_team`, `area.delete`, `team.set_lead`, the area-lead cascade below, `project.remove_team`, `team.delete`, `member.*`) | each New bug whose suggested triager became, or stopped being, the owner gets a `WaitingStarted` / `WaitingEnded { reason: triage }` (§6). Reconciled from current state like the rest, after the other cascades of the same organization event. |

Organization-side cascades, inside the organization event (server-dev's apply code in `org-office`, with T1's):
- **An area lead leaves the area's team** (`team.remove_member`, `member.move`, `member.offboard`, `member.remove`): the area's lead is cleared (it falls back to the team's lead, AREA-8); the area's version goes up.
- **`project.remove_team`** gains `areas_to_team_id?`. If the leaving team owns areas and the project keeps other teams, it is required and must be one of them (`invalid_request` otherwise); those areas move to it, and their leads who aren't in it are cleared. If it was the project's last team, it must be absent; the areas become team-less and their leads are cleared.
- **`team.delete`**: the areas its project links owned become team-less, leads cleared. The command has no picker, and the model stays so (agreed with Eden, §9). The **app's** delete dialog keeps Eden's **Give its areas to** picker for each project that keeps other teams, and when the user picks a successor team it issues `area.set_team` for each of those areas **before** `team.delete` (if one fails, the dialog stays open and names it, and the team isn't deleted; the areas already moved stay with the successor team, which is what the user chose anyway). Areas become team-less only when the project has no other team or the user skips the picker; they then show "No team · Choose team…", and the owner triages their bugs until a team is chosen.
- **`area.set_team`**: clears a lead who isn't in the new team (AREA-9).

### 2.6 Numbering
- Scopes: `(project, task)`, `(project, bug)`, `(organization, request)`. Numbers start at 1, have no gaps at assignment and are **never reused**, deleted items included (C9).
- **HQ assigns every number.** In phase 1 HQ owns every item, so the number is allocated in the create's transaction (`work_numbers` table).
- **Phase 2:** an item created at a satellite has no number until HQ receives its create. HQ then assigns the next number in that scope and streams an **`ItemNumbered { item_id, number }`** record. It is HQ's own record, not an item version: it never changes the item's version or generation, every office applies it whatever the owner, and a later version from the owner never clears it. Until then the item shows **Pending · \<short id\>** (A10). An item created in the app at a satellite is created at HQ (queued while HQ is unreachable), as the offices design says.
- Feature, ADR, debt and other doc numbers are doc numbers (§4.9), not work numbers.

### 2.7 Ownership, versions and the stream (office-first)
- **Owner:** the assignee's office. An agent's office is its `office_id`; a person or the owner is at HQ; an unassigned item stays with the office that created it. Created in the app: at HQ. Created by an agent: at its office (offices design, decision 10). Features: always HQ.
- **Phase 1:** one office, so the owner is always HQ. The library still computes the intended owner after every change, and returns a `Handover` effect when it differs from `owner_office_id`; phase 1 never produces one (tested with a two-office fake, §8).
- **Every committed call** is recorded as **one** row in the office's `work_origin_log`, with its origin `(office_id, incarnation, origin_seq)` (unique, ADR-0003's dedup key) and the organization position it was checked at, plus a child row with a **reference** `(target_id, version)` for **each** item and feature version the call committed (its own, the tasks `request.approve` creates, automatic changes, cascades), all in the same transaction. That table and its storage trait belong to `org-sync` (River; ADR-0003 §3.1, shared with messaging); until `org-sync` exists, `store.rs` writes the same row shape. The payload is built when it streams, so a tick never copies a whole item: an `ItemVersion` carries the item's **current** state and every event since the version last streamed, so intermediate versions coalesce. That is safe for work items, which offices apply only if newer (offices design); messages keep ADR-0003's own rules. Phase 1 writes the rows even though nothing streams yet, so phase 2 streams from them without a migration.
- **Newer** is `(generation, version)`, compared in that order, everywhere.
- **Phase 2 hooks** (built then, shapes fixed now, §4.10):
  - **Change requests:** a write at an office that doesn't own the item becomes a change request carrying the call, its `change_id` and `expected_version`; it answers `queued`. The owner applies it with the same rules and returns the result to the requester (`work.changed` on commit, `work.rejected` on refusal). The owner's `change_id` table answers retries.
  - **Handover:** only the current owner emits it; `generation + 1`; the item's change-id table travels with it; requests in flight are forwarded. Before a task handover the old office pushes the task branch (projects design).
  - **Reclaim:** an organization event (office removed, an explicit choice); `generation + 1` at HQ's latest version; HQ turns a stale owner's later versions into change requests (offices design).
  - **Automatic changes across owners** become change requests to the target's owner (§2.4).
- **Time:** an event's `at` is the owning office's clock, clamped so it is never earlier than the item's previous event (`at = max(now, previous at)`), so spans are never negative.

### 2.8 Deleting
`work.delete` (app-only) marks the item deleted and records an event. The number stays used; links to and from it remain and read as "deleted"; the automatic rules and the time figures ignore it; a made-task step whose task is deleted becomes a plain step again ("T-42 was deleted"). There is no undelete in phase 1.

### 2.9 Projects and docs without a repository
A project may have no repository (`repo` empty). Its tasks, bugs and areas work; worktrees and docs don't: doc calls return `unavailable` ("the project has no repository"), and the Docs tab shows the C10 hint. `project.update` can't add a repository in this contract; G1 adds `project.set_repo`.

---

## 3. Storage: the `org-work` library and its storage trait

### 3.1 Where the code lives
```
apps/org-agent ───────────▶ crates/org-client ─────────────────────────┐
apps/org-office-server ───▶ crates/org-office (store.rs: all SQL) ─┬──▶ crates/org-proto
        │                         │                                │
        │                         ├──▶ crates/org-work  ───────────┤   (rules, no SQL/git/network)
        │                         └──▶ crates/org-docs  ───────────┘   (P3/P4: front matter, templates, index)
        └──▶ crates/org-git (River) — implements org-docs' DocRepo port in the composition root
```
- **`org-work`** (new library; owner **TBD by ada**: codex-dev if it's back, otherwise River or server-dev): the work-item model, the state machines, validation, links, checklists, numbering requests, automatic rules and Undo, org cascades, ownership decisions, change-request and handover effects, and the time figures. It depends on `org-proto` (the wire types are its vocabulary), `serde`, `serde_json` and `sha2` (the params hash) (`Ce = 4`; `org-sync-proto` joins in phase 2). Expected `I ≈ 0.4` (a domain capability). It defines its ports; it implements none.
- **`org-docs`** (new library, P3/P4; owner **TBD by ada**): front matter parsing and writing, templates and schemas, validation, doc numbering, mention extraction, building the index from a tree. It defines a `DocRepo` port (read a file at a ref, list a tree, commit a set of path changes (write, rename, delete) on a branch with expected blobs, list branches). `Ce = 4` (`org-proto`, `serde`, `serde_json`, one YAML parser at an exact pin). It knows nothing about work items: the office composes the two (a feature status effect from `org-work` becomes a doc write through `org-docs`).
- **`org-office`** (server-dev) depends on both, implements their storage traits in `store.rs` and the `OrgView` read port, wires the calls, and runs the org cascades. **`org-office-server`** (the composition root) implements `DocRepo` with River's `org-git`, as the GitHub design does for its traits. No cycle: `org-work` and `org-docs` never depend on `org-office`, `org-git` or each other.
- Check against the change-time checklist: one new dependency each for `org-office` (to `org-work`, later `org-docs`), both stable-ward (`org-office` is the more volatile); no cross-domain dependency between work and docs; no new edge into `org-proto` except additive types.

### 3.2 The ports `org-work` defines
```rust
/// Read-only view of the organization (org.db at the office's applied position).
/// One `OrgView` is one org.db read transaction, opened by `transact` and held for its duration:
/// every check and the recorded org position come from the same point in time.
pub trait OrgView {
    fn applied_position(&self) -> Result<Position, StoreError>;
    /// The organization changes after `after` up to `applied_position()`, for the cascades'
    /// catch-up and restart (§2.5).
    fn changes_after(&self, after: Position, limit: u32) -> Result<Vec<(Position, Vec<OrgChange>)>, StoreError>;
    fn member(&self, id: &str) -> Result<Option<MemberView>, StoreError>;      // kind, owner, office_id, status
    fn resolve_member(&self, id_or_handle: &str) -> Result<Option<MemberView>, StoreError>;
    fn owner_id(&self) -> Result<String, StoreError>;
    fn project(&self, id: &str) -> Result<Option<ProjectView>, StoreError>;     // key, archived, repo present
    fn project_by_key(&self, key: &str) -> Result<Option<ProjectView>, StoreError>;
    fn project_teams(&self, project_id: &str) -> Result<Vec<String>, StoreError>;  // team ids
    fn area(&self, id: &str) -> Result<Option<AreaView>, StoreError>;           // incl. deleted, replaced_by
    fn general_area(&self, project_id: &str) -> Result<String, StoreError>;
    fn team_lead(&self, team_id: &str) -> Result<Option<String>, StoreError>;
}

/// The office's work storage. All SQL lives in store.rs (ADR-0002 decision 5).
pub trait WorkStore {
    /// Run `f` as one write request on the office's single writer, inside its own SAVEPOINT
    /// (ADR-0002 decision 4): reads and writes in `f` see one consistent state, and either all
    /// of `f`'s writes commit or none do. The result is reported after the batch's commit.
    /// `f` is `Fn`, not `FnOnce`: when SQLite ends a batch's transaction the writer re-runs every
    /// request in it (ADR-0002), each time with a fresh `OrgView`. So `f` has no effect outside
    /// its `WorkTxn` (effects go to the `work_effects` outbox).
    fn transact(
        &self,
        f: std::sync::Arc<dyn Fn(&dyn OrgView, &mut dyn WorkTxn) -> Result<TxnOutput, WorkError> + Send + Sync>,
    ) -> Result<TxnOutput, WorkError>;

    // Reads from the read pool (no writer involved).
    fn get(&self, id: &str) -> Result<Option<StoredItem>, StoreError>;
    fn page(&self, q: &ItemQuery) -> Result<ItemPage, StoreError>;          // work.snapshot, work.list
    fn events(&self, item_id: &str, after_version: u64, limit: u32) -> Result<Vec<WorkEvent>, StoreError>;
    fn feed_after(&self, pos: &WorkPosition, limit: u32) -> Result<FeedPage, StoreError>; // subscribe resume
    fn features(&self, project_id: Option<&str>) -> Result<Vec<Feature>, StoreError>;
}

/// What a write request may do, inside `transact`.
pub trait WorkTxn {
    fn get(&mut self, id: &str) -> Result<Option<StoredItem>, StoreError>;
    fn by_number(&mut self, scope: &NumberScope, number: u64) -> Result<Option<StoredItem>, StoreError>;
    fn by_short_id(&mut self, short: &str) -> Result<Vec<StoredItem>, StoreError>;
    fn linked_to(&mut self, target_id: &str, kind: LinkKind) -> Result<Vec<StoredItem>, StoreError>;
    fn feature(&mut self, id: &str) -> Result<Option<Feature>, StoreError>;
    fn feature_tasks(&mut self, feature_id: &str) -> Result<Vec<StoredItem>, StoreError>;
    /// The doc's newest status: from a status write the office recorded but hasn't carried out yet, else from its open office-made branch (its create, rename or status branch) when it has
    /// one, else from the integration branch, else (a debt doc only on an agent's task branch, C18) from that
    /// branch's overlay, so an office status commit counts before it merges; P4.
    fn doc_status(&mut self, project_id: &str, doc_key: &str) -> Result<Option<String>, StoreError>;
    /// The ADRs in `proposed` that the item links to (`relates_to`) or that name it in `related`; P4 (§6).
    fn proposed_decisions(&mut self, item_id: &str) -> Result<Vec<String>, StoreError>;
    /// The open items linked to a doc (`relates_to`) or named in its front-matter `related`; P4 (§6).
    fn items_for_doc(&mut self, project_id: &str, doc_key: &str) -> Result<Vec<StoredItem>, StoreError>;
    fn open_items(&mut self, filter: &CascadeFilter) -> Result<Vec<StoredItem>, StoreError>; // cascades
    fn events_since(&mut self, item_id: &str, after_version: u64) -> Result<Vec<WorkEvent>, StoreError>;
    fn change_result(&mut self, change_id: &str) -> Result<Option<StoredResult>, StoreError>;

    fn allocate_number(&mut self, scope: &NumberScope) -> Result<u64, StoreError>;
    /// Write the item's whole new state (core, kind fields, checklist, out-links) and append its
    /// event; the store checks `event.version == stored version + 1` and fails otherwise.
    fn put_item(&mut self, item: &StoredItem, event: &WorkEvent) -> Result<(), StoreError>;
    fn put_feature(&mut self, feature: &Feature, event: &WorkEvent) -> Result<(), StoreError>;
    fn put_change_result(&mut self, change_id: &str, params_hash: [u8; 32], result: &StoredResult) -> Result<(), StoreError>;
    /// Append the call's one origin row (next origin_seq, org_position) with one child reference per
    /// committed (target, version) (§2.7), and its one feed row; returns the feed position.
    fn record_change(&mut self, org_position: Position, refs: &[OriginRef], change_id: Option<&str>) -> Result<WorkPosition, StoreError>;
    fn set_org_cursor(&mut self, applied: Position) -> Result<(), StoreError>;
    fn org_cursor(&mut self) -> Result<Position, StoreError>;
}

pub trait Clock { fn now_ms(&self) -> u64; }
```
`OrgView` is wider than ADR-0003's `MemberDirectory` (projects, areas, teams); `store.rs` implements both over the same queries. `org-work` defines its own port rather than depending on the messaging library, so the two domains stay independent.

Exact names and signatures are the library owner's, agreed with server-dev (AGENTS.md: the trait's owner and its implementer agree changes). **The library decides before it writes:** it reads, checks every rule and computes the whole outcome (automatic changes included) first, and only then writes. So a refusal has nothing to undo: it writes only the change-result row (plus any org catch-up, which has its own savepoint, §2.5) and returns `Ok(TxnOutput::Refused(error))`, and refusals are stored (§2.3). `Err` is kept for storage failures, which roll everything back and aren't stored. What is fixed here: **all reads a rule depends on happen inside `transact`**, so a decision is never made on state another write changed in between; and **one call = one `transact`**, its automatic changes and its origin records included. The one exception is a call that writes git (`feature.create`, `doc.*`): an intent transaction, then the git work, then a record transaction (§4.9).

The library's public face is a service per family (`Tasks`, `Bugs`, `Requests`, `Checklists`, `Links`, `Features`, `Kpi`, `Cascades`), each a function of `(&dyn OrgView, &mut dyn WorkTxn, &dyn Clock, Actor, Params) → Result<ServiceOutcome, WorkError>`, where `ServiceOutcome` (not §1.5's request `Outcome`) holds the changed items, the events and **effects** for the office: `Notify` (a message to send, e.g. "Bug B-7 fixed, please verify"), `DocWrite` (a feature or debt status commit), `Handover` and `ChangeRequest` (phase 2). Effects are written to a **`work_effects` outbox in the same transaction** and carried out after the commit; on start the office replays every effect not marked done. Each is idempotent by its effect id (target, version, kind, ordinal): a doc write is a docs intent (§4.9) keyed by the effect id, recovered the same way, and a notice is sent with a message id derived from the effect id (ADR-0003 dedups by id). A failed effect never undoes the commit: doc writes retry with backoff and, after repeated failure, the feature or debt shows "status not written to the doc yet".

### 3.3 Tables (a sketch for server-dev, `office.db`)
| Table | Columns (sketch) | Notes |
| --- | --- | --- |
| `work_items` | `id` PK, `kind`, `project_id`, `number`, `title`, `status`, `resolution`, `priority`, `severity`, `area_id`, `team_id`, `assignee_id`, `created_by`, `created_at_ms`, `updated_at_ms`, `owner_office_id`, `generation`, `version`, `deleted`, `feature_id`, `kind_data` (JSON for the remaining kind fields) | indexes: `(project_id, kind, status)`, `(assignee_id, status)`, `(kind, number_scope, number)` unique where set (`number_scope` is the project id for tasks and bugs and `org` for requests, fixed at numbering, so a request moving project keeps its REQ number), `(feature_id)` |
| `work_events` | `item_id`, `version`, `at_ms`, `actor_id`, `kind`, `aspects` (the fields it changed), `data` (JSON), `auto`, `cause_item_id`, `cause_version`, `org_epoch`, `org_seq` | PK `(item_id, version)`; immutable, never updated or deleted |
| `checklist_steps` | `id` PK, `item_id`, `ord_key` (text, for cheap moves), `text`, `required`, `done`, `task_id`, `version`, `removed` | |
| `work_links` | `id` PK, `from_item_id`, `kind`, `to_item_id`, `to_doc_project_id`, `to_doc_key`, `created_by`, `created_at_ms`, `removed` | index on `(to_item_id, kind)` for backlinks and the bug rule, and on `(to_doc_project_id, to_doc_key)` for decision spans (§6) |
| `work_watchers` | `item_id`, `member_id` | PK both |
| `work_numbers` | `scope` PK, `next` | HQ only |
| `work_change_results` | `change_id` PK, `params_hash`, `result` (JSON), `expires_at_ms` | purged after 30 days |
| `work_feed` | `seq` PK (autoincrement), `change_id`, `targets` (the (id, version) pairs the call committed) | one row per committed call (or received change, phase 2): the clients' `WorkPosition`; kept 30 days |
| `work_origin_log` | `origin_office_id`, `incarnation`, `origin_seq` (unique together), `org_epoch`, `org_seq`, `kind` | **shared with messaging** (ADR-0003; owned by `org-sync`); one counter per origin for messages and work; one row per call |
| `work_origin_refs` | `origin_seq` (with origin and incarnation), `target_id`, `version` | the versions each call committed; payloads are built at stream time (§2.7) |
| `work_org_cursor` | one row: `epoch`, `seq` | §2.5 |
| `work_effects` | `id` PK (target id, version, kind, ordinal), `kind` (`notify`, `doc_write`, …), `payload` (JSON), `done`, `attempts`, `next_try_at_ms` | the after-commit outbox (§3.2) |
| `features` | `id` PK, `project_id`, `number`, `path`, `title`, `status`, `delivered_at_ms`, `retired_reason`, `replaced_by`, `version` | P3; events in `work_events` with `item_id` = the feature id |
| `doc_commits` | `project_id`, `sha` PK, `branch`, `path`, `change_id`, `member_id` (who asked), `made_at_ms` | P3; **persistent**, not a cache: the commits the office made. Attribution (`agent_member_id`) and the retry lookup come from here, never from commit messages |
| `doc_branches` | (`project_id`, `branch`) PK, `path`, `created_by_change`, `merged` | P3; office-made `docs/…` branches and the doc each one changes |
| `doc_index` | `project_id`, `path`, `kind`, `doc_id`, `title`, `status`, `front_matter` (JSON), `last_commit`, `last_at_ms`, `author_member_id`, `coauthor_member_id`, `branch` (empty = integration), `problems` | P4; a cache rebuilt from git |
| `doc_aliases` | `project_id`, `old_path` or `old_id`, `doc_id`, `branch` | P4; a cache rebuilt with the index from each doc's `former_paths` and `former_ids` (§4.9), so a link stored as an old path or id (§1.7) still resolves; a live path or id always wins |
| `doc_ids` | (`project_id`, `path`) PK, `doc_id`, `change_id`, `base_commit` (the integration commit the reservation was made at) | P4; ids reserved for docs that had none, so every branch that first saves one writes the same id (§4.9) |
| `doc_mentions` | `project_id`, `path`, `branch`, `ref` | P4; item and doc refs found in each doc |
| `doc_numbers` | `project_id`, `kind`, `number`, `change_id` | P3/P4; reserved numbers, so concurrent creates never clash |
| `member_status.current_task_id` | (ADR-0003's table) | `work.set_current` (§6) |

The time figures can be cached (`work_kpi` per item), but the cache is always recomputable from `work_events` and a test proves they agree (§8).

---

## 4. The contract: `org_proto::work`

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
                    anyway: Option<DoneAnyway> },   // Done or Fixed with required steps left
    Assigned { from: Option<String>, to: Option<String> },
    Triaged { area_id: String, severity: Severity, priority: Priority, assignee_id: Option<String> },
    Verified { outcome: VerifyOutcome, by_reporter: bool, note: Option<String> },
    Reopened { note: Option<String> },
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

`NewStep { step_id, text, required }`. `work.set_current` sets the bound agent's current task (C14) in `member_status`; it isn't an item change and takes no `change_id`; its result is `Empty`.

### 4.5 Bugs (P2)
| Call | Params (after `change_id`) | Errors |
| --- | --- | --- |
| `bug.report` | `item_id`, `project_id`, `title`, `what_happened`, `severity`, `area_id?`, `expected?`, `steps?`, `environment?`, `from?: { link_id, to: LinkTarget }`, `affects_feature_id?` (P3), `checklist?` | `not_found`, `invalid_request` (limits; archived project; area not in the project or deleted), `already_exists` |
| `bug.update` | `item_id`, `expected_version`, `title?`, `what_happened?`, `expected?`, `clear_expected?`, `steps?`, `clear_steps?`, `environment?`, `clear_environment?`, `severity?`, `priority?`, `area_id?`, `affects_feature_id?`, `clear_affects_feature?` | `not_found`, `invalid_request`, `conflict` |
| `bug.triage` | `item_id`, `expected_version`, `area_id`, `severity`, `priority`, `assignee_id?` | `not_found`, `invalid_request` (wrong state; area), `unknown_recipient`, `conflict` |
| `bug.assign` | `item_id`, `expected_version`, `assignee_id?` | `not_found`, `unknown_recipient`, `conflict` |
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
| `request.ask_changes` | `item_id`, `expected_version`, `comment_id`, `note` | `not_found`, `invalid_request`, `conflict` |
| `request.approve` (**app only**) | `item_id`, `expected_version`, `tasks: Vec<ApprovedTask>` (1–50) | `not_authorized`, `not_found`, `invalid_request` (wrong state; no project; a task over limits), `unknown_recipient`, `conflict`, `already_exists` |
| `request.decline` | `item_id`, `expected_version`, `reason` | `not_found`, `invalid_request`, `conflict` |
| `request.merge` | `item_id`, `expected_version`, `into` (a request id), `comment_id` | `not_found`, `invalid_request` (itself; `into` not open), `conflict` |

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
| `project.create` | `project_id`, `name`, `key?` (absent: HQ derives one, §below), `description?`, `teams?: Vec<{ project_team_id, team_id }>`, `general_area_id`, `general_area_name?` (default "General"), `repo?: RepoSpec`, `integration_branch?` | `not_found` (a team), `invalid_request` (key format or reserved; limits; a repeated team; repo spec), `invalid_name` (name, area name), `already_exists` (an id; the key; the name, ignoring case) | P1 |
| `project.update` | `project_id`, `expected_version`, `name?`, `description?`, `clear_description?`, `integration_branch?`, `clear_integration_branch?` | `not_found`, `invalid_request`, `invalid_name`, `conflict`, `already_exists` | P1 |
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
All go through `org-client` to the agent's own office, bound as the agent (ADR-0003). Every command takes `--json` (the result objects of §4) and `--office`; none takes an actor. Refs: `T-12` / `B-7` resolve in `--project <key>` or, if absent, the project of the agent's current task; `chk-T-12`, `REQ-14` and short ids resolve on their own. Text arguments accept `-` for stdin. The words match the app's: To do, In progress, Blocked, Done; New, Triaged, In progress, "Fixed — verify", Closed · Verified / Duplicate / Won't fix / Can't reproduce; severities and priorities as words.

```
org-agent task new "<title>" --project <key> [--assignee <member>|--me] [--team <team>] [--priority urgent|high|normal|low]
                 [--feature FEAT-n] [--fixes <bug-ref>]... [--desc <text|->] [--dod <text>] [--step "<text>"[!]]...
org-agent task list [--project <key>] [--mine] [--assignee <member>|--none] [--status <s>[,<s>]] [--all] [--json]
org-agent task show <ref>
org-agent task start <ref>                      # In progress; becomes your current task
org-agent task block <ref> "<reason>"
org-agent task unblock <ref> [--start]            # to To do, or In progress with --start
org-agent task done <ref> [--cancelled] [--anyway --reason "<why>"] [--debt "<what I did instead>" | --no-debt]   # --debt/--no-debt: P4
org-agent task reopen <ref> [--start]
org-agent task assign <ref> <member>|--none
org-agent task edit <ref> [--title …] [--priority …] [--team …|--no-team] [--desc …] [--dod …] [--feature FEAT-n|--no-feature]
org-agent task current [<ref> | --clear]
org-agent task comment <ref> "<text>" [--ask owner|<member>] [--answer <comment-id>]
org-agent task link <ref> --fixes <bug-ref> | --relates <ref|doc> ;  task unlink <link-id>
org-agent task checklist <ref>
org-agent task check|uncheck <ref> <n|step-id>
org-agent task step add <ref> "<text>" [--required] [--after <n|step-id>]
org-agent task step edit|rm|move|make-task <ref> <n|step-id> [...]
org-agent bug report "<title>" --project <key> --severity critical|major|minor|trivial --what <text|-> [--area <name|id>]
                 [--expected …] [--steps …] [--env …] [--from <task-ref>]                                   # P2
org-agent bug list|show|comment|assign|edit   (as task)
org-agent bug triage <ref> --severity … --priority … [--area …] [--assignee <member>|--none]
org-agent bug fixed <ref> "<what fixed it>" [--anyway --reason …]
org-agent bug verify <ref> --fixed | --still "<note>"
org-agent bug close <ref> --duplicate-of <bug-ref> | --wont-fix "<reason>" | --cant-reproduce "<reason>"
org-agent bug reopen <ref> "<note>"
org-agent request add "<text>" [--project <key>] [--kind feature|change|technical|question] [--to <member>]          # P3
org-agent request show|list|accept|comment <REQ-n> …
org-agent request ask <REQ-n> "<question>"                     # a question to the owner (waiting on you)
org-agent request reroute <REQ-n> --to <member> [--reason "<why>"]
org-agent request scope <REQ-n> [--new-feature "<title>" | --extends FEAT-n] [--doc <key>]... --task "<title>"... [--effort …] [--cost <min>-<max>]
org-agent request decline <REQ-n> --reason "<why>" ;  request merge <REQ-n> --into REQ-m
org-agent undo <ref> <event-version>
org-agent doc new|edit|status|schema|validate|list|show|supersede|link …  ;  org-agent debt add|list|show|accept|pay …   # P4, per decisions spec §3.1 and C16
                 # doc new --here, and debt add by default inside a task worktree (--docs-branch to opt out): doc.render, then the CLI
                 # writes the file into that worktree (refusing symlinked directories on the way and an existing file: O_NOFOLLOW, O_EXCL)
                 # and the agent commits it with the returned --author and message (trailer included)
org-agent doc template show <kind>                 # P4 (TPL-4): the project's docs/.templates/<kind>.md, or the built-in default
org-agent doc template edit <kind> [--file <path>|-]   # P4: opens $EDITOR (or takes the file); doc.save on a docs branch, lands
                                                   # through the merge flow; shows in Activity ("ada added the field `risk` to Decision")
                                                   # Reset to default (TPL-4) is the app's: a save of the built-in default (doc.get, built_in)
```
- `request scope --new-feature` calls `feature.create` first (the agent then fills the doc with `doc edit`, which writes on the branch `feature.create` made, as `doc.save` does by default), then `request.scope` with `New { feature_id }`.
- `--step "<text>"!` marks a required step. `task check` takes a number or a step id; scripts should use ids (C21).
- `task done` with required steps left exits **1** and prints the same message as the app ("2 required steps aren't done: …") on stderr (and the error object with `--json`), unless `--anyway --reason`.
- Exit codes follow ADR-0003 §2.6: `checklist_incomplete`, `conflict`, `not_found`, `invalid_request`, `invalid_name`, `already_exists`, `not_authorized`, `not_bound`, `too_large`, `expired`, `clock_skew` → 1; `unknown_recipient` → 3; `storage_error`, `unknown` → 4; office unreachable or `unavailable` → 5.
- `--as <member>` is ADR-0003's binding flag (accepted risk M-A), not an actor field. `FEAT-n` arguments resolve through `feature.list`.
- Every write sends a fresh `change_id` and retries once with the **same** id on a lost reply.
- Output: one line per write, e.g. `T-17 · Coupon edge cases · To do · assigned to River`, `B-15 reported · Severity major · goes to Riley (General, Checkout) for triage`.

## 6. Time figures: how they're derived
Everything comes from the item's events (`at`, the owning office's clamped clock, §2.7). Nothing is typed in or stored as a separate timestamp.

1. **Rounds.** A round opens at `Created` or at a reopen (`StatusChanged` out of a terminal status, `Reopened`) and closes at the next transition into a terminal status: task `done`; bug `closed`; request `approved`, `declined` or `merged`. A bug's verification wait is part of its round (Fixed isn't terminal). `rounds` = the number of rounds.
2. **Lead time** = the sum of each round's length; the last round, if open, runs to `as_of` (now), and `open` is true ("so far").
3. **Cycle time** = the sum, over rounds that started, of (round end or now) − (the round's start), where a round's **start** is its first transition into `in_progress` (tasks and bugs; a bug fixed by hand without passing through In progress starts at that `fixed`), or a request's `scoping`. `None` when no round started.
4. **The split.** Every instant of every round belongs to exactly one bucket, by this precedence:
   1. **working**: at least one session attributed to the item is active (`WorkingStarted` / `WorkingStopped` events, keyed by session). Only when working time is tracked for the item (point 5);
   2. **waiting on the owner**: at least one open waiting span (point 6), or a bug in `fixed` whose reporter is the owner (the verify request is the owner's; FIX-4 sends it to the owner's Needs you only then), or a request in `scoped` (waiting for approval);
   3. **blocked**: a task in `blocked`;
   4. **the rest**: everything else.
   So `waiting_on_owner + blocked + rest = lead`, always (TIME-1).
5. **Working time is untracked until sessions report it** (session tracking, W14/W15). Until then working time is **never folded into idle**: idle means nobody worked on the item, and an untracked hour may have been worked. So the rest stays one bucket, working and idle together, and `working_ms` and `idle_ms` are `None`. The app shows, with Eden's wording (agreed, §9):
   ```
   Lead time 1 d 4 h · waiting on you 3 h · blocked 0 · the rest 1 d 1 h (working time isn't tracked yet)
   ```
   Once sessions report it, the rest is split: `working_ms` (precedence 1 above) and `idle_ms` (the rest minus working), with `working + idle = rest`. The split is shown only for an item whose working time was tracked for its **whole** lead time: the office was already recording working events (organization-wide) before the item's first round started, and every member who was its assignee, or ran a session attributed to it, had working events recorded (their runtime reports sessions) since before the item's first round started, and no span of its lead time had a session of its assignee that the costs spec's rule 4 (below) attributed to nothing. Otherwise the item keeps "the rest" for good, so an untracked runtime never turns into idle. Never guessed. **Working events** are written by the office when a session attributed to the item becomes active or stops, using the costs spec's rule 4: the task the session was started on; else the agent's current task (`work.set_current`, set automatically on `task start`); else the agent's only In progress task; else nothing. A session switching task produces a stop and a start.
6. **Waiting spans** are opened and closed by the office's bookkeeping events (`WaitingStarted` / `WaitingEnded { on_member_id: owner, reason }`, §2.3), one span per reason, so overlapping reasons are counted once. Each reason is derived from events and data the office already has:
   - **A question** (`reason: question`): `work.comment` with `comment_kind: question, asks: owner` opens it; the matching `answer` closes it.
   - **A message** (`reason: message`): an agent's `msg.ask` or `msg.send` to the owner that carries `meta.work_item = <item id>` (ADR-0003 messages; a new meta convention) opens it; the owner's reply, or marking it done, closes it.
   - **Triage** (`reason: triage`, bugs, P2): open while the bug is `new` **and** its suggested triager (§1.4) is the owner, matching Home › Needs you ("2 bugs need triage in checkout" only when the owner is the suggested triager). The calls that can change either side write the span in their own transaction: `bug.report`, a reroute (`bug.update { area_id }`), `bug.triage`, `bug.close` and a reopen to `new`. Organization changes that change the suggested triager (an area's lead or team, a team's lead, membership) write it through the org cascade, reconciled from current state (§2.5).
   - **A decision** (`reason: decision`, P4): open while the item is linked to an ADR (`relates_to`) or named in its front matter `related`, and that ADR's status is `proposed`, so it waits for the owner's acceptance (a proposed ADR reaches the owner's Needs you, DEC-5; anyone may accept in phase 1, but the app asks the owner, and U15 recommends keeping it so). When P4 lands, a backfill reconcile opens the spans that already hold, from the backfill time on. `work.link` / `work.unlink` write the span in their own transaction (the ADR's status from `WorkTxn::doc_status`). When the docs index changes (an Accept or Reject commit by the office, a merge, a fetch), the office runs `on_docs_indexed` in its own work transaction: for each ADR whose status or `related` list changed, it reconciles the spans of the open items linked to it or named in it, from current state, as the org cascades do. An Accept or Reject through the office (the app, or an agent's `doc.set_status`) ends the span at the office's commit, not at the merge ("Accepted · waiting to merge" is no longer waiting on the owner). A proposed ADR that exists only on an agent's unmerged `task/…` branch opens no span until it reaches the integration branch or an office-made branch. It needs no cursor: every index change (integration or overlay) triggers it, and on start, after the index is built, the office reconciles every decision span, so a crash between an index change and its reconcile loses nothing. Its changes are recorded like a cascade's (`record_change`, one origin row and one feed row, sent as `work.changed`); in phase 2 a change to an item another office owns becomes a change request to that owner (§2.4).

   A round's terminal transition writes `WaitingEnded` for every open span in the same transaction, and a reopen writes `WaitingStarted` for the reasons that still hold, so a span is never opened twice. A span left open by a deleted question ends at the item's next terminal transition.
7. **Medians** (Insights, P5) are over items whose last round closed in the period, by project, area, team, assignee and kind; never averages (TIME-2). Medians of working time and idle are over tracked items only. Waiting and blocked medians mix tracked items (where working time takes precedence) and untracked ones; Insights says so beside them.
8. **Recomputing:** the figures are a pure function `time_figures(events, owner_id, tracking, as_of)` in `org-work`, where `owner_id` names the owner (for the verify wait in point 4) and `tracking` holds when the office began recording working events (none in phase 1, so nothing is ever split), per member when that began for them, and the spans when that member's sessions were unattributed; any cache must equal it.

## 7. Phases and owners
| Phase | What | Owners |
| --- | --- | --- |
| **P1** | The contract (§4: `org_proto::work` with every P1 call, the project and area commands, `checklist_incomplete`, the `subscribe` extension). `org-work`: the core, tasks, checklists (incl. make-task and auto-tick), comments, watch, links (`relates_to`, `part_of`), Undo, numbering, change ids and per-field conflicts, the time figures (waiting on the owner from questions and messages, blocked, and the rest; working and idle stay one bucket until W14/W15, §6), the org cascades for members and teams, ownership decisions with a two-office fake. `project.create { teams, general_area_id, repo }`, `project.update`, `project.archive`, `area.create/rename/set_team/set_lead`, the area-lead cascade, `project.remove_team { areas_to_team_id }` and `team.delete`'s area cascade (so an area's team is always one of its project's teams). Store, wiring and restart safety. `org-agent task …`. The app's projects and tasks screens on the real office. | ada (contract); `org-work`: **TBD by ada** (codex-dev if back, otherwise River or server-dev); server-dev (store.rs, office wiring, the org apply code for projects and areas, the org cursor); River (`new_local` / `existing_local` repository creation and checks, with W4); Remy (`org-agent task`); app-frontend-dev (UI) |
| **P2** | Bugs: report, triage, reroute, fix tasks and the automatic bug status, mark fixed, verify, close, reopen, duplicates; triage waiting spans (§6). Areas complete: `area.delete` with moving open bugs. Commits and branches on items (FIX-10, §7.1). `org-agent bug …`. | as P1 |
| **P3** | Requests (all calls, routing reasons, questions and waiting spans), `request.approve`. Feature records, `feature.create/set_status/adopt`, the automatic In progress and Delivered with Undo, `affects_feature_id`. Request attachments and the open-question flag (§7.1). The docs write path (`org-docs` core, `DocRepo` in `org-git`, docs branches, owner/co-author commits) for the feature kind. `org-agent request …`. | ada (contract additions reviewed before build); `org-work` owner; `org-docs`: **TBD by ada**; River (`org-git` commit-on-branch API); server-dev; Remy; app |
| **P4** | Docs for every kind: index with overlays, `doc.*`, templates and schemas, validation, numbering, mentions, history, the brief. Debt (`pays_back`, Planned/Paid), limitations, exceptions, agent guidance files, checklist templates (`docs/.templates/checklists/`). `task done --debt/--no-debt` and `shortcut`. Stable doc ids and `doc.rename`'s link rewriting, decision waiting spans (§6), `org-agent doc …` (incl. `doc template show|edit`), `debt …`. From §7.1: area ids in debt and feature front matter, ADR numbering next to v0, and the project's default checklist. | as P3 |
| **P5** | Retros (triggered by Delivered and the interval), Insights (medians, breakdowns), status reports, working-time events from session tracking (with W14/W15), which split the rest into working and idle (§6). From §7.1: feature cycle time and typical ranges, and the project settings for budget, retro and report schedules and health rules. | ada (designs), owners per the retros and costs specs |

Each step: its own branch, an independent review, the reviewer's READY, my go, then a merge into `v1`. Before P2–P5 are built, I review their contract additions again against what P1 taught us.

Also on my side (follow-ups to this design, not in this commit): update `AGENTS.md` with `org-work` and `org-docs`; amend `projects-git-github.md` (the `task/<ref>-<slug>` branch rule, the project.create shape, docs branches per A36), `teams-and-org-chart.md` (area cascades, `project.remove_team`'s new param), `offices-hq-and-satellites.md` (`ItemNumbered`, shared `work_origin_log`) and ADR-0003 (`meta.work_item`, `work.set_current`, and rows in its call-by-call table for every work call: any bound connection, with the app-only ones marked).

### 7.1 Gaps from Eden's review, as plan items
Eden's review listed spec features this design had no model for yet. Each is now a plan item in the phase below; its contract additions come with that phase's review (§7).

| Gap | Spec | Phase | What the plan item delivers |
| --- | --- | --- | --- |
| Commits and branches on items | `work-items-ux.md` FIX-10 (and decisions spec: commits show their Why) | P2 | The office's git index (River, `org-git`, reading `mirror.git`) lists the commits and branches whose message or name mentions an item's reference (the §1.1 patterns), plus the task's own `branch`, on the item page. A read call, derived from git and never stored on the item. |
| Request attachments | `feature-requests-ux.md` (pasting an image attaches it) | P3 | Files attached to a request's text and comments, screenshots first: where the bytes live (office storage, not the repository, since a request is transitional), size and type limits, and how they travel between offices in phase 2. |
| An open-question flag on request summaries | `feature-requests-ux.md` RQS-7, Home › Needs you | P3 | `WorkItemSummary` gains a flag set while a question or message span to the owner is open (§6), so Needs you and the Requests pill's Needs you group list those requests without a `work.get` per item. |
| Project settings | `costs-and-insights-ux.md` (budget), `retrospectives-ux.md` and `project-status-ux.md` (schedules, health rules), `work-items-ux.md` §9.1 (default checklist) | P4 (default checklist), P5 (budget, retro and report schedules, health rules) | A settings record per project, organization data changed by app-only HQ commands (a budget changes spend, §2.2); each phase adds its keys. The default checklist instead stays with the checklist files (CHK-8: `docs/.templates/checklists/`, a `default` field), by the storage rule of §0. |
| Feature cycle time and typical ranges | `costs-and-insights-ux.md` TIME-3, `project-status-ux.md` health rules ("past 2× its typical cycle time"), `retrospectives-ux.md` | P5 | A feature's cycle time (its first linked task started → delivered), derived from the feature's and its tasks' events like §6; typical ranges for tasks and bugs (by team and kind) and for features, computed with the medians. The range's definition (which percentiles, which window) is settled with Eden then. |
| An area id in debt and feature front matter | `decisions-and-tech-debt-ux.md` (debt `area`), `features-ux.md` (`area`) | P4 | The front matter carries a stable `area_id` beside the area's name, written by the office, so renaming an area never orphans a doc; a name without an id is matched by name and reported when it matches none or several. |
| ADR numbering next to the v0 ADRs | `decisions-and-tech-debt-ux.md` decision 10, A18, OURDOC-1 | P4 | The v0 ADRs (`docs/adr/v0/`) keep their numbers, as their own series shown under Earlier decisions (v0); their ids name the series (`ADR-v0-0001`), written by the import that moves them into `docs/adr/v0/` (§4.9: a numbered doc there always gets a v0 id), so they never clash with current ids. A plain `ADR-n` always means the current series; the import rewrites the v0 docs' references to each other as `ADR-v0-n` and keeps their links to current successors as `ADR-n`. The same import adds front matter to our current ADRs (ADR-0001 to ADR-0003), whose status is in a body Status line today. New ADRs get the highest current number + 1, v0 not counted: after ADR-0003 comes ADR-0004. |

## 8. Tests that prove the rules
Each test must fail without the rule it names.

**`org-work`, against an in-memory fake store, a fake `OrgView` and a fixed clock:**
- *State machines:* every allowed transition per kind succeeds and records one event with the actor; every disallowed one is `invalid_request` and changes nothing (a table-driven test over all (from, call) pairs).
- *Required steps:* Done with a required step unticked is `checklist_incomplete` and changes nothing; with `anyway_reason` it is Done and records `done_anyway` with the step ids; `cancelled` skips the guard; optional steps never block; the same for `bug.mark_fixed`.
- *Per-field conflicts:* a title edit at version 3 succeeds after a tick (v4) and a comment (v5); a title edit at v3 after another title edit (v4) is `conflict`; two triages from v2: the second is `conflict` (not `invalid_request`).
- *Idempotency:* the same `change_id` and params return the stored result after the item changed; different params are `invalid_request`; a new `change_id` 29.5 days old is `expired` while a stored one of the same age is still answered; a refusal (`conflict`) is answered from the store on retry after the item changed; 6 minutes ahead is `clock_skew`; `storage_error` is not stored.
- *Identity:* no params field can set the actor; a bug reported over an agent connection has that agent as reporter; `request.approve` and `work.delete` over an agent connection are `not_authorized` before any other check.
- *Recipients:* an offboarded agent, an unknown id and a removed person are `unknown_recipient`; a paused agent, a person and the owner are accepted.
- *Automatic changes:* bug → In progress on the first fix task start; → Fixed only when every fix task is Done and one completed; no change with a required bug step left (and the event says so); back to In progress when a fix task is reopened; deleting or unlinking a fix task changes nothing; Undo restores the earlier status, is refused after an hour, after a later status change (`conflict`) and for a non-automatic event; an undone change isn't re-applied until a new trigger. The same table for features (P3) and made-task steps.
- *Numbering:* separate T and B sequences per project and one REQ sequence; no reuse after delete; 1,000 concurrent creates through the store give 1,000 distinct consecutive numbers (store test).
- *Cascades:* offboarding a member unassigns exactly its open items and leaves closed ones; requests it scoped go back to New; deleting an area moves only open bugs; running `on_org_applied` twice gives the same state (idempotent); the cursor advances in the same transaction.
- *Ownership:* with a two-office fake, assigning to an agent in Office B returns a `Handover` effect; to a person returns HQ; unassigned stays with the creator; phase 1's single office never produces one.
- *Time figures:* hand-built event sequences with known answers: a task blocked for 2 h inside a 10 h lead; a question to the owner overlapping a session in a tracked item (counted as working); a reopened bug (two rounds, cycle per round); an open item ("so far"); an item with no working events reports `working_ms` and `idle_ms` as `None` and its untracked time in the rest, never in idle; an item created before its assignee's tracking began keeps "the rest" after it, and so does an item worked by an agent whose runtime doesn't report sessions; a reopen with a span still holding opens it once (the close wrote `WaitingEnded`); and a property test that waiting + blocked + rest always equals the lead time, that working + idle equals the rest when tracked, and that `time_figures` is the same however the events are batched.
- *Waiting on the owner:* a question and a message span overlapping count once; a New bug whose area has no lead and no team waits on the owner until it's triaged, and stops waiting when it's rerouted to an area with a lead; an `area.set_lead` that makes the owner no longer the triager closes the span through the cascade, once (idempotent); linking a task to a proposed ADR opens a decision span and the ADR's Accept closes it; a waiting event never makes a title edit stale.
- *Duplicates and links:* duplicate of itself, of a bug in another project, of a closed-as-duplicate bug: refused; reopening a duplicate goes to New and clears it; `fixes` from a bug or to a task: refused.

- *More rules:* a concurrent step text edit and tick both succeed and both are kept; `checklist.remove` after a make-task is `conflict`; `expected_version` above the current version is `invalid_request`; a writer sending `Unknown` for an enum is `invalid_request`; creates in an archived project (`task.create`, `bug.report`, `checklist.make_task`, `request.approve`) are refused; `feature.set_status { planned }` over an agent connection is `not_authorized`; `work.set_current` on someone else's task is refused; `in_progress → todo` keeps the round's cycle start. A P1 server refuses a later-phase field in a write (`task.create { feature_id }`, `task.set_status { shortcut }`) with `invalid_request`; `work.list { linked_to, link_kind }` returns exactly the backlinks of that kind, past the first 200. `checklist.check` on a made-task step is `invalid_request`; a made task Done · Cancelled leaves its step unticked, and `checklist.unlink_task` makes it a plain step; all `pays_back` tasks Done · Cancelled leave the debt Planned.
- *Features:* approving a request leaves its new feature Planned; the first task start moves it to In progress; unlinking the last open task delivers it; status commits made before the create branch merges land on top of it, and that branch then merges into the integration branch without a conflict; once the doc is on the integration branch, an edit branch that doesn't touch the front-matter lines around `status` and its status branch merge without a conflict.
- *Cascades vs Undo:* `work.undo` on a cascade event is `invalid_request`.
- *Sizes:* a page stops before 768 KiB however large the items are; `request.approve` over 768 KiB is `too_large`; a cascade over 5,000 items is sent as several `work.changed` parts with one `change_id`.

**`org-office` (server-dev), on a real SQLCipher database in a temporary directory:**
- the trait contract on the real store: `put_item` refuses a version gap; one call's writes commit together or not at all (a forced failure in an automatic change leaves nothing); restart keeps items, events, numbers, change results and the cursor;
- **crash between the organization commit and the work cascade**: kill after `member.offboard` commits and before the cascade; on restart the items are unassigned exactly once;
- **the race between a work write and an org cascade**: an `assign` checked at P-1 interleaved with a `member.offboard` at P, in both orders: the item ends unassigned; a write that sees org position P before the cascade ran runs it first;
- **effects survive a crash**: kill after a commit that queued a doc write and a notice; on restart each is carried out exactly once;
- **office doc writes** never touch an agent's worktree or a `task/…` branch, and `doc.render` commits nothing; a hostile `work.git` config (a filter, a hook path, `core.fsmonitor`, `commit.gpgsign` with a `gpg.program`, `log.showSignature`, `diff.external`, a textconv) never runs on writes **or reads**; a retried `doc.save` with the same `change_id` makes one commit, also after a kill between `update-ref` and its record; an agent moving a docs branch tip in `work.git` never gets its commit recorded as the office's; a `Co-authored-by` in someone else's commit is shown as from git; a docs branch checked out in an agent's worktree is never moved; a mailmap never changes the authors `doc.history` shows; `doc.save` can't change `decided.by`, and a `decided.by` set with git shows as from git; a docs branch is pushed after each commit and re-pushed after a crash; an agent commit on a docs branch marks it diverged and nothing is dropped; a kill between the two intent steps reuses the reserved number;
- **re-run safety**: a forced SQLite rollback of a batch re-runs a work request with the same result and no duplicate effect;
- every committed call writes one `work_origin_log` row with a growing `origin_seq` shared with messages, and one reference per version it committed;
- `work.snapshot` paged while another connection writes, then `subscribe { from_work_position }` from the first page: the client's copy equals a fresh snapshot;
- org commands: project.create makes General and the links in one event; key derivation and clashes; area name uniqueness ignoring case among live areas; the area-lead cascade on `team.remove_member`, `member.move`, `member.offboard`, `member.remove`; `project.remove_team` with and without `areas_to_team_id`; `team.delete`'s areas; every command refused over an agent connection.

**`org-proto`:** round trips of every type; unknown enum values, unknown `type` tags on `LinkTarget`, `Shortcut`, `FeatureOutcome` and `WaitReason`, and unknown `WorkChange`/`WorkEventKind` variants decode as `Unknown` without failing the whole message; the `ProjectSummary` → `Project` change reads old JSON (no `created_at` included); `checklist_incomplete` decodes as `Unknown` on a build that doesn't know it.

**`org-agent` (Remy):** each command's wire call against a fake office; `task done` exits 1 with the message on `checklist_incomplete` and succeeds with `--anyway --reason`; ref resolution (`T-12` with and without `--project`, `chk-T-12`, a short id, an ambiguous short id); the retry reuses the `change_id`.

**Docs (P3/P4):** path traversal, symlinks, `..`, absolute paths and over-size content are refused; a write never lands on the default or integration branch; author and co-author trailer are right; `expected_blob` mismatch is `conflict`; front matter outside the YAML subset or over the limits is refused on write and reported (not fatal) on read; doc numbers don't clash under concurrent creates; the index rebuilt from scratch equals the incremental one; `doc.new` and `doc.render` write an `id`, the first `doc.save` of a doc without one adds it, and `doc.save` refuses changing or removing it; `doc.rename` rewrites relative links and front-matter paths to the old path in other docs in the same commit, leaves code blocks alone, and a link stored as the old path still resolves; a link by id survives a rename made with git; a new ADR after the v0 series takes the highest current number + 1; two branches that each first save the same id-less doc write the same id and merge cleanly; a git-made doc at a path whose reserved id is now carried by a renamed doc gets a new id; a numbered doc without front matter takes its id from its file name; a Renumber keeps references to the old id on the doc that still holds it; `on_docs_indexed` run twice, or after a restart, leaves the same spans; `doc template show` on a project without the template returns the built-in default (`built_in: true`); `doc.rename` of a doc with unmerged edits or an unmerged status branch (a feature's included) is `conflict`; a status commit or `doc.save` while a rename is unmerged goes on top of the rename branch, and `doc.rename` skips other docs with unmerged branches; `doc.rename { renumber }` takes the next number and records `former_ids`; a doc under `docs/adr/v0/` gets `ADR-v0-n`, and a plain `ADR-n` in it resolves to the current series.

**End to end (P1 finish line):** create an organization → create a project with a new local repository and a team → the app creates T-1 with a required step → an agent (`org-agent`) starts it, ticks the step, comments, marks it done → the app sees every change live, the activity names the agent, and the Time block shows lead and cycle times.

## 9. Changes to Eden's specs and other designs
Eden reviewed rev 1 against her specs and **accepted** the departures marked **Agreed** below; she updates her specs to match. She also agreed these points where this design and her specs now match: the 6-character pending id ("Pending · 3k9f2x", §1.1), `chk-FEAT-0007` outside the project (§1.1), `org-agent task current` (§5), Delivered going back to In progress when a task reopens (§1.10), a bug's required steps blocking the automatic Fixed (§2.4), a debt's status written after its doc merges ("Planned once merged", §2.4), and Approve asking for a project when the request has none (§1.10). Rows without the mark change other designs, or answer her questions.

| Where | Change | Why |
| --- | --- | --- |
| `work-items-ux.md` decision 3, C1, BUG-7, examples (`chk-12`) | **Separate sequences:** tasks `T-12`, bugs `B-7` per project; outside the project `chk-T-12`. The UI always shows the prefix. | **Agreed.** ada's decision for W7: the kind is in the reference itself, so it can't be misread even without the icon. |
| `work-items-ux.md` C3 | Answered: the library, as the spec preferred. | |
| C4 | Answered: Done has a resolution, `completed` or `cancelled` ("Done · Cancelled"). | |
| C5 | Answered: §5. | |
| C6 | Minimal watchers: the creator, duplicate reporters, anyone who comments with `watch` or calls `work.watch`. Notifications to watchers come with the messaging work. | |
| C7 | Linking commits and branches by reference (FIX-10) is a P2 plan item (§7.1: the office's git index, River). Refusing Done for a dirty worktree is not in this design (open question 2). | |
| C8, C9, C20 | Same project only; delete is app-only and keeps the number; checklist items as designed (C20 yes). | |
| C10 | The brief is `docs/brief.md`. A project without a repository has no docs (§2.9). | |
| A13 | General can't be deleted. | |
| AREA-7 (team deleted) | The model is unchanged: `team.delete` leaves its areas team-less. The app's delete dialog keeps the **Give its areas to** picker and issues `area.set_team` before `team.delete` when the user picks a successor team (§2.5); the owner triages only if the user skips it. | **Agreed** (Eden's Q5). `team.delete` has no place for a choice; the app makes it before the delete. |
| §7.3, FIX-11 | The verify request moves to the area lead when the reporter left: derived (`suggested_verifier`), nothing stored. | |
| `feature-requests-ux.md` decision 3 | Adds **Routed** (chosen, not yet accepted) between New and Scoping; "With agents" covers both. The UI shows "Sent to Eden · not accepted yet", then "Eden is scoping". | **Agreed.** `org-agent request accept` needs a state to leave. |
| `features-ux.md` decision 5 | The feature's status is an office record; the doc's `status`, `tasks` and `delivered` are generated from it on each status commit. | **Agreed.** So Delivered is recorded by the office (FTR-7), not by a merge. |
| `projects-git-github.md` | Task branches are `task/<ref>-<slug>` where `<ref>` is `chk-T-12`, or the short id if the task has no number when its worktree is made; the branch never changes name, and the office maps branch → task by the stored `branch`. `project.create` takes `teams` with link ids, `general_area_id`, `key?` and a phase-1 `RepoSpec`. Docs branches (A36). | The push check needs a stable mapping, not parsing. |
| `teams-and-org-chart.md` | Area cascades on membership changes; `project.remove_team { areas_to_team_id? }`; `team.delete` clears its areas. | §2.5 |
| `offices-hq-and-satellites.md` | `ItemNumbered` is HQ's own record, not an item version. Messages and work items share one origin counter. | §2.6, §2.7 |
| `work-items-ux.md` FIX-2, `features-ux.md` decision 5, `decisions-and-tech-debt-ux.md` DBT-5 | Automatic Fixed, Delivered and (debt) Paid also need **at least one completed** task (all Done · Cancelled doesn't count; a debt whose payback tasks were all cancelled stays Planned). Then the bug says "All its fix tasks were cancelled. Create a fix task, or close the bug", and the feature "Add tasks or retire it". | **Agreed.** Cancelled work didn't fix or deliver anything. |
| `projects-git-github.md` G1 | `project.create` answers `CommandAccepted` and emits `org.changed` (`project_upserted`), not `→ Project` and `project.created`. | One style for every organization command (T1). |
| `work-items-ux.md` CHK-6 | A made-task step ticks only when its task is Done · **completed**, not Done · Cancelled, and can't be ticked by hand. A cancelled task shows "T-42 was cancelled" on the step, with **Unlink**. | **Agreed.** A cancelled task didn't do the step. |
| `decisions-and-tech-debt-ux.md` C18, AUTH-1, AUTH-6 | The **office** commits only on branches it made. A doc on the agent's `task/…` branch (C18 yes) is written by the agent's CLI into its own worktree (`doc.render`) and committed by the agent; `debt add` does that by default when the agent is in a task worktree, else uses a docs branch. | **Agreed.** Writing into an agent's worktree from the office can't be made safe (symlinks, rebases, its index) or crash-safe cheaply; the agent writing its own worktree crosses no boundary. |
| `decisions-and-tech-debt-ux.md` §0.2, §3.1 | `org-agent doc new/edit/status` default to office-made docs branches; only `debt add` (and `doc new --here`) default to the agent's task branch. Agent commits made with git carry the owner's author and the trailer only because the agent uses `doc.render`'s author and message. Their attribution, and office-only fields such as `taken.by` set in such a commit, read **from git**: a quiet note, not a warning (§4.9). | **Agreed** (doc writes; Eden's ask (b) for the wording). The office only commits where it is safe (above); the trailer is the agent's own claim, and `debt add` on a task branch is the normal path. |
| `decisions-and-tech-debt-ux.md` §3, §3.2 (front matter, templates), TPL-4, DEC-7 (Renumber) | Every doc has a stable `id` in its front matter: numbered kinds keep their number, other kinds (specs, designs, …) get `doc-<ULID>`, assigned at `doc.new` or on a doc's first save. Links are by id, and `doc.rename` also rewrites path links (§4.9). Agents read and edit templates with `org-agent doc template show|edit` (§5), on a docs branch through the merge flow. Renamed docs carry `former_paths`, renumbered ones `former_ids`. | **Agreed** (Eden's asks (c) and (d)). Specs and designs had no id, and links must keep working after a rename. |
| `costs-and-insights-ux.md` decision 5, §2, TIME-1; `work-items-ux.md` §9.2, TSK-11 | Until sessions report working time (W14/W15), the split is waiting on you · blocked · **the rest**: "Lead time 1 d 4 h · waiting on you 3 h · blocked 0 · the rest 1 d 1 h (working time isn't tracked yet)" (Eden's wording; her example's "22 h" is corrected so the parts add up to the lead time). Working and idle are shown separately only for items tracked for their whole lead time (§6). | **Agreed** (Eden's Q6). Untracked working time must not read as idle: idle means nobody worked on it. |
| `costs-and-insights-ux.md` decision 5 ("Waiting on you") | Waiting on you also counts a New bug whose suggested triager is the owner and an item linked to a decision awaiting the owner's acceptance, each derived from events (§6). | **Agreed** (Eden's ask (a)): it follows the triage and decision rows of the owner's Needs you. Critical bugs reach Needs you whoever triages them (TRI-5) but count as waiting only when the owner is their suggested triager. |
| `org.rs` error order | Work calls put `conflict` before a wrong-state `invalid_request` (§2.3); organization commands keep `org.rs`'s order. | A stale caller should hear "someone changed it", not "you can't do that". |
| ADR-0003 | `meta.work_item` on a message to the owner opens a waiting span; `work.set_current` sets `member_status.current_task_id` (the `who` card's `current_task_id`). | §6 |

## 10. Open questions
**For the user**
1. **Project key:** projects get a short key (`chk`) used in references and branches. Fine to have it, derived when not given, and never changeable?
2. **Done with uncommitted work** (C7): should the office refuse Done for a task whose worktree has uncommitted changes, or only warn? I lean to warn in phase 1.
3. **Separate T and B sequences** change Eden's one-sequence design (§9). Eden has agreed; confirm, so she can update the spec and its criteria.
4. **The owner's commit identity in phase 1** (local git, no GitHub yet): a name and email you set once in Settings › Git. OK?

**For Eden** (answered in her review of rev 1)
5. ~~The `Routed` request state and the AREA-7 change for deleted teams.~~ Routed: agreed. AREA-7: the model stays, and the app's delete dialog issues `area.set_team` before `team.delete` (§2.5, §9).
6. ~~The "working —" copy.~~ Replaced: untracked working time isn't folded into idle; the Time block shows "the rest" until session tracking (§6, §9).

**For Eden (rev 2)**
11. Decision spans (§6): while an ADR linked to a task is `proposed`, the task counts as waiting on you, even while an agent works on it (before tracking, waiting comes before the rest) and after **Request changes…**, when the next move is the author's. Keep that, or count only until Request changes, or only `relates_to` links and not `related` mentions?

**For server-dev and the `org-work` owner**
7. `WorkStore::transact` with a boxed closure fits ADR-0002's writer thread? If another shape is simpler on your side, propose it; the rule to keep is "all reads a decision depends on happen inside the write request".
8. Running the org cascade after the org commit with a cursor (§2.5) versus one SQLite transaction across both databases (`ATTACH`). I chose the cursor to keep one writer per database; say if `ATTACH` is clean enough on the writer thread.

**For River**
9. The `org-git` API for "commit a set of path changes (write, rename, delete) on a branch with expected blobs, with author and co-author", push and sync it (built with plumbing in `mirror.git` with deterministic identity and dates, then fetched into `work.git`, §4.9), and repository creation for `new_local`, before P3 and P1 respectively.

**For the user (P3 dependency)**
10. Docs branches need a merge flow, and `projects-git-github.md` leaves merging for later. For phase 1, is "a person merges them through the same review and go as code" acceptable, or do you want the office to merge docs-only branches itself after an approval in the app?
