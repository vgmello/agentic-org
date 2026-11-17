# Design: processes (how agents work, written down and inherited)

Revision 5 · 2026-11-15. Owner: lead. [Organization structure](organization-data/01-organization-structure.md), [project structure](organization-data/02-project-structure.md), [identity](organization-data/06-identity-and-numbering.md) and [record contracts](organization-data/08-record-contracts.md) select process/standard **content**, scopes and default-allowed override flags. Effective resolution, approval workflows, private processes, timers, checklist composition and process execution remain unselected/deferred. Existing `org-process` parsing/resolution/branch helpers and read-only UI are implementation evidence of the earlier contract, not a selected end-to-end process service. The [authority map](documentation-authority.md) governs precedence.

The detailed delivery/branch algorithms below remain reviewable proposals or versioned library behavior where stated. Selected files cannot be fed to the old parser as though their lifecycles/fields were identical. No new CLI/wire command is claimed implemented by this revision. Human controls use the authenticated HQ boundary; a browser connection or office certificate alone grants no role authority.

## Decisions
1. **Selected storage:** one remote organization metadata repository; organization processes at `processes/<document-id>.md`, standards at `standards/<document-id>.md`, nested-team variants under the full `teams/<id>/teams/<id>/.../` chain, and project variants at `projects/<project-id>/processes/<document-id>.md` or `standards/`. Canonical reads follow organization `state_branch`, independent of code-repository integration branches.
2. **Selected identity:** immutable stable document ID, UUIDv7 for new records, preserved IDs for existing records; optional `key` is separate. TEAM- and P- references are readable labels, not folder names. An override is a separate content record with its own stable ID.
3. **Selected metadata:** kind process/standard; title, lifecycle Draft/Current/Deprecated, optional accountable employee (required active owner for Current), common attribution and revision. `team_id` or `project_id` supplies the scope, never both. Both null means organization scope. Overrides default allowed; an explicitly prohibited override cannot be defeated by a lower-level project edit.
4. **Deferred resolution:** organization → accountable team's ancestor chain → optional project is a candidate resolution order. Employees have one home team; tasks have one accountable team, independent assigned team/individual. Optional contributing teams are not extra owning teams and grant no managerial/process authority. Whether contributor chains participate is an unresolved policy, not an inferred multi-team engine.
5. **Deferred private processes:** the earlier personal layer, owner-read access and agent folder proposal are not selected. Storage/privacy and offboarding retention need a separate decision; shared Git cannot enforce restricted readership. This is distinct from author-only private task notes, which have no owner/admin read exception.
6. **Deferred enforcement/branch policy:** retain the proposed one-open-task-branch guard, explicit one-time exceptions, review/merge and safe cleanup algorithms below. They do not follow merely from storing a process document. Current repository development rules remain governed by AGENTS.md, not this proposed product engine.
7. **Code branch bases:** every project/repository association requires integration_branch. A task targets zero or more repositories, so branch/base/cleanup state must be repository-specific. There is no single project branch or empty fallback. Projectless tasks need explicitly admitted repository bases; policy remains open.
8. **Handoff preserves work:** reassignment is an atomic revision-checked team/individual assignment pair with durable activity history. It is not task closure and must not archive/delete worktrees. Agent/office transfer needs safe workspace migration and execution cleanup/fencing. Guarded branch review may block transfer; it cannot silently discard or archive ongoing task work.
9. **Deferred approvals:** authorize the actual human/agent subject through existing boundaries, validate scope and expected commit/revision, and retain actor-bound decisions before acknowledging. Editable owner/status fields and co-author trailers do not grant powers. Exact private-layer rights and contributor approvers remain open.
10. **Deferred delivery:** bounded process guidance, tool help and standards lookup preserve whole entries and source versions; fresh contexts must receive governing content or retrieval instructions. Automatic wakes, checklist composition and retrospective actions are separate deferred engines, not selected by these content fields.

## 1. Where each thing lives
| Thing | Selected source / implementation boundary |
| --- | --- |
| Organization process/standard | Organization Git `processes/<id>.md` / `standards/<id>.md` |
| Team process/standard | Organization Git recursive team path and explicit team_id |
| Project process/standard | Organization Git `projects/<project-id>/processes/<id>.md` / `standards/<id>.md` |
| Employee home team | Employee profile; separate from assignment/placement |
| Process index | Rebuildable SQL/query projection of authored content; no independent lifecycle authority |
| Effective sets, delivery acknowledgements | Deferred resolver/delivery contract; current library outputs are not durable authored facts |
| Task branch records/exceptions | Existing helper shapes need per-repository and durable accepted-write design before target use |
| Private processes | Unselected storage/access proposal; no canonical shared-Git location |

The separate `workspace/handbook/handbook.git` and `OrgSnapshot.handbook` design is superseded for selected content by organization repository registration and state_branch. Its old fixed `offices/*/workspace/handbook/**` sandbox deny remains **legacy security evidence**: removing that directory name must not remove protection. Re-derive target organization-metadata, credential, Git-metadata and other-task denies for the actual runtime layout and validate before launch. No agent-owned working branch/ref may substitute for the trusted publication source.

## 2. The model

### 2.1 The process file (PROC-1)

Selected authored envelope (illustrative retained IDs):

```yaml
schema_version: 1
id: existing-process-id
key: review-and-merge
revision: 3
kind: process
title: Review and merge
status: current
owner_employee_id: existing-employee-id
team_id: null
project_id: null
overridable: true
created_at: "2026-11-15T10:00:00Z"
created_by_kind: employee
created_by_id: existing-employee-id
updated_at: "2026-11-15T10:00:00Z"
updated_by_kind: employee
updated_by_id: existing-employee-id
```

The Markdown body explains the procedure. Standards use the same structural envelope with `kind: standard` and describe required constraints. Current requires an active owner; Draft can be unassigned. Content does not enact an approval or inherited resolution engine.

**Versioned earlier library contract:** `org-process`/`org-docs` parsing helpers use `process`, `name`, `level`, `scope`, `version`, `owner`, `summary`, `applies_to`, statuses active/off/retired, `overrides`, `moments` and `checklist`. Those are not aliases that may be blindly serialized as selected fields. Target adaptation maps identity/key/title/owner/revision/scope explicitly; current→active is only a candidate resolver input mapping, not authority to overwrite selected file status. Off, drift metadata, moments and checklist merge rules need separately selected extension contracts. Preserve existing helper tests until an intentional compatibility change.

Retained parser limits for the earlier proposal: key 1–64 ASCII lowercase letters/digits/hyphens starting alphanumeric; summary and applies_to one line ≤200 characters; moment prompt ≤500 characters; checklist ≤50 steps. Invalid metadata names the field and leaves readable content available; a future resolver must visibly exclude invalid content rather than invent rules.

### 2.2 Levels, scopes and paths
| Scope | Authored fields | Organization-Git folder |
| --- | --- | --- |
| Organization | team_id=null, project_id=null | processes/ or standards/ |
| Team | stable team_id, project_id=null | teams/<root-id>/teams/<child-id>/.../processes/ or standards/ |
| Project | team_id=null, stable project_id | projects/<project-id>/processes/ or standards/ |
| Private agent | not selected | no selected path |

Use `<document-id>.md` and require path/scope agreement. Reads admit regular Markdown blobs, never symlinks/submodules/traversal. No `work.git` ref chooses canonical content. Read the trusted organization cache/ref at state_branch; the concrete publication/cache interface still needs implementation. Resolve code guidance per explicitly selected repository integration branch, not from this document store.

### 2.3 The index (HQ)
For the earlier resolver proposal, an index row may carry: `{ repo, path, commit, blob, doc id, key, level, scope, version, status, overridable, overrides, problems }`. No text: names and summaries come from the blob. Old rows stay as a history (key, level, scope, version → commit) for drift diffs (PROC-9). It's rebuilt after each change to a source branch; while the organization repository can't be read, reads answer from it with `stale_since` (PROC-16).

## 3. Resolution (A53)

**Deferred engine / earlier helper algorithm.** Preserve these detailed behaviors for review, with the selected constraints below. active/off/version names in this section are earlier resolver input vocabulary (§2.1), not the authored lifecycle.

### 3.1 Inputs
**Deferred resolver inputs:** O is organization content; T is the task's accountable team's ancestor chain (project owner's team for project work, task team for projectless work); J is optional project content, all from organization state_branch. Assignment and employee home team do not silently alter this scope. G is the earlier private-layer proposal and remains disabled pending storage/access selection. Contributor-chain participation and no-current-task behavior require a policy decision. They cannot be inferred from project_contributing_teams or ordinary ancestor-team membership.

The existing library's multi-chain input supports its old linked-team proposal. Keep it as versioned helper behavior while adapting the target; it does not establish that projects have multiple owner teams.

### 3.2 Rules, in order (`org_process::resolve`, pure)
1. **Valid files only.** Invalid front matter or a level or scope mismatch: left out, with a problem. A `draft` or `retired` file is left out silently: it is a normal state, not a problem (amended 2026-11-09: ada, from the Processes screens' review).
2. **Key clash.** Two files with one key at one (level, scope): both left out, with the clash problem (PROC-1). Seals fail closed: if a file left out for invalid front matter, a mismatch or a clash is sealed (or can't be read far enough to tell), the key stays sealed below. `draft` and `retired` files never seal.
   A file whose key can't be read (no front matter, bad YAML or no valid `process`) seals every key below its place unless its `overridable` value can be read and is `true`, or it is a `draft` or `retired` one (ruled 2026-11-13: ada, R2b); what it refuses, and a private file whose key it might use, show "can't tell: <path> can't be read", not "sealed by" (ruled 2026-11-13: ada, R2). In rule 5 it counts as its chain's seal, as a seal does, but a conflict names it ("can't tell: <path> can't be read"), not its team, as sealing the key (ruled 2026-11-13: ada); the outermost team lower in that chain whose own file seals the key is named as sealing it too, beside the unreadable path (2026-11-13, from P3's review; how its name reaches a cached set: §3.3).
3. **Seals inside a chain.** A file that overrides or turns off a key sealed by the organization, or by a team above it in its own chain, is refused, with the sealing level (PROC-3). A project file is refused if the organization sealed the key, or an applicable ancestor team seal binds it (a sibling/contributor conflict does not remove that prohibition). Otherwise this runs first, so a refused file never makes a conflict. The write path refuses these too (§5).
4. **Fold each team chain**, outermost first, the nearer team winning.
5. **Combine the chains** (decision 4, PROC-6). Only team-level winners count. One distinct winning team file applies. Two or more are a **conflict**: the organization's version applies, or nothing if it has none, except that a **sealed** team winner applies meanwhile (decision 13; two different seals: the organization's version, else a visible unresolved conflict requiring policy resolution; no arbitrary team-ID ordering grants authority). A sibling team's override or `off` never silently cancels a seal.
   In a diamond, a sub-team's override that differs from a sibling chain's version is a conflict; only the same team reached twice is not (ruled 2026-11-13: ada, R1).
6. **Project.** J's version wins over the result unless the key is sealed (rule 3). It may settle an ordinary conflict only when every applicable ancestor permits overriding. The earlier project-settles-sealed-conflict rule is superseded; lifting a prohibition requires an authorized change to its source, not a lower-level bypass.
7. **Turned off.** A surviving `off` removes the key and records "Turned off for <team or project name>: <reason>" (PROC-4).
8. **Private only adds** (PROC-5). A private file isn't applied, and is flagged, if it sets `overrides` or `status: off`, or if its key appears in **any** O, T or J file of this project, whatever that file's state.
9. **Drift** (PROC-9). An override whose `overrides: key@v` is below the parent's current version is marked "Parent changed"; without `overrides`, its drift shows as unknown.

**Output:** `EffectiveSet { inputs, entries, turned_off, conflicts, problems }`: per key, the winning file (with its kind: inherited, override or addition) and its **chain** of layers and what happened to each, which `process why` prints (PROC-8).

### 3.3 Caching and project-wide checks
Sets are cached under a fingerprint of the organization state commit, accountable scope/team ancestry and any separately admitted legacy-layer input commits (teams are org data). Any change to those drops the sets that used it. Place names (the "Turned off for <name>", conflict and summary lines) are not in the fingerprint: when a team or project is renamed, the cache's owner drops the sets that name it (ruled 2026-11-13: ada). Dropped sets are recomputed at the next start, wake or read. The last good set per (agent, project) is kept with its time.

**Conflicts belong to the project.** After each index or org change, HQ resolves O ⊕ T ⊕ J per affected project and keeps one Needs you item per (project, key) for its leads and the owner; clashes, invalid files and drift likewise. Agent leads and approvers get Needs you items in their inbox (spec decision 11).

**Can't compute** (PROC-17), including a missing `integration_branch`: the last good set, with the spec's wording and warnings; the one-branch rule counts as on.

### 3.4 Delivery to sessions (PROC-7, PROC-12)
`org_process::render` builds a **Your processes** section for the composed instructions (`agent-sessions.md` decision 8):
- the set: each process's name, place (org, the team's or project's name, or private), version and summary, plus turned-off ones and conflicts, in the spec §3 wording (place names and the cache: §3.3);
- **changed since last read**: the full text of each process whose winning blob isn't among the blobs the agent has read for that key (a new version, a new winner or a conflict fallback all count);
- each process's `moments` prompt, after that moment's guidance text, only while the moment is On.

**Size cap: 24 KiB**, prompts included: summaries first (past the cap, "and N more: run `agent-os process list`"), then changed texts, organization first; the rest are named with "run `agent-os process show <key>`". It goes in the instruction file at a start, the wake prompt at a wake (a message for a resumed session, `agent-sessions.md` decision 5), and `agent-os task start`'s result; `task done` returns the Finishing a task prompts. The texts actually delivered count as read once the runtime accepted the start or wake, or on `process ack`.

**Checklists** (PROC-12): `task.create` in P adds the steps of P's processes (O ⊕ T ⊕ J, never G) after the project's default checklist, duplicates merged, required winning.

### 3.5 Context bounds, fresh contexts and standards (proposed 2026-11-09)

The **24 KiB cap above remains**, including moment prompts. It is a ceiling, not a
target. The composer also respects the runtime's verified total instruction/context
limit, reserves space for identity, goal, tool help and useful task work, and may use
a smaller process allowance. Render whole UTF-8 entries; never truncate a rule
midway or mark omitted text read.

- Every fresh main session and delegate gets the effective process manifest and
  concise applicable behavioral guidance. Agent-wide last-read history is not proof
  that a new context contains the text. Deliver unchanged applicable text again, or
  explicit instructions to retrieve its pinned body before the relevant action.
  Re-establish the current manifest, goal and retrieval instructions after observed
  compaction/context reset.
- Deferred bodies have a stable key/version/source digest and a `process show`
  reference. Retrieval follows the recorded effective version for that context;
  changes become visible at the existing next start/wake boundary. A model summary
  never silently substitutes for an approved process body.
- A resume with unchanged context gets changed entries and relevant moment prompts,
  not another concatenation of the handbook. Deduplicate reminders and updates by
  session/context generation and effective-set fingerprint.
- If concise guidance exceeds the remaining allowance, report **Process context
  limit reached**, with omitted count and retrieval path. Code-side rules still
  apply. Affected work needs either its guidance or a retrieval instruction that
  fits. If the minimum bootstrap cannot fit, refuse the launch with an actionable
  configuration error rather than silently omit governing instructions.

**Three inputs:** processes say how to behave; standards (stack, cloud and coding
conventions) are indexed knowledge fetched when a process/task calls for it;
`agent-os` help explains tools, syntax and permissions. Inject concise tool help
separately; retrieve detailed help progressively. Standard bodies are not bulk
injected. Keep source/version references to audit what the agent used.

**Behavior seeds:** provide overridable organization Markdown processes for
delegation/consultation, memory hygiene and channel questions/follow-up. They use
the existing approval, seal and inheritance rules. Private specialists are harness
assets, not private handbook processes, and cannot override inherited processes.
Choosing to delegate, selecting a specialist or moving a question to DM stays agent
judgment; the office has no semantic router. Limits/authorization remain code-side.
See [`employee-supervision.md`](employee-supervision.md). The draft
[`delegation-and-consultation.md`](../processes/delegation-and-consultation.md)
is an organization seed proposal, not a newly active repository rule.

The following branch algorithms retain legacy mirror.git/work.git role names and single-repository helper shapes. Target integration must execute them per explicitly targeted repository using one office/repository cache and task-specific worktrees. They are **deferred product enforcement**, not a promise of a completed service. Task handoff/reassignment preserves worktrees; only confirmed task closure can enter closure cleanup. Existing sandbox commit-path and credential-separation evidence remains valid for the old layout.

## 4. Enforcement: one open task branch (A54, A55)

### 4.1 Task branch records
The earlier single-repository helper record is: `{ task_id, member_id, project_id, branch, base_sha, own_tip?, state: reserved | open | closing | merged | archived, merged_by_hand?, stage?, opened_at, last_moved_at, closed_at? }`. Target records need explicit repository_id, optional project_id and a separately designed durable operation/history source. Task handoff transfers safe workspace custody only after fencing/cleanup; do not infer it from office ownership or copy a cache row as authority.
- **Reserved** at `task start`, before any git; **open** once the branch exists. Nothing is reserved without a repository.
- **Branch names.** `task/<ref>-<slug>` (W7), with a `-2`, `-3`… suffix whenever a record, a `merged/` or `archive/` tag, a remote branch or a local `work.git` branch already has the name (never `-B`).
- **`own_tip`** (decided 2026-11-08) is the latest tip the office has seen on the branch (through `agent-os push`, or a fetch into `mirror.git`) that descends from `base_sha` and is reachable from no ref in `mirror.git` other than this branch's own copies (`refs/heads/task/<b>` as fetched, `refs/office/task/<b>`, `refs/office/task-head/<b>`). A fast-forward or reset onto the integration branch or someone else's branch never sets it. Residual: agents share `work.git`'s objects, so one could push another's commits first; review catches it.
- **Merged** when `own_tip` is reachable from the **integration branch** in `mirror.git` (a `--no-ff` merge, decision 12), or when a project lead or the owner marks it **merged by hand**. `own_tip` stays checkable after the branch is deleted. Once merged, the office refuses pushes to the branch; more work needs a new branch, checked like any other.
- **Pushes need a record.** `agent-os push` takes a `task/*` branch only from the holder of its open record.
- **Released** (removed): a reservation whose task leaves In progress, closes, is deleted or moves before its branch exists. A failed `git worktree add` removes it and restores its exception in one transaction; startup reconciles reservations with existing branches and clears a stale closing. `task.branch` (W7) holds the current record's branch; branch → task goes through records.
- **Stuck.** A record idle for over a day goes to the lead's Needs you, with **Mark merged…** (decision 12). Until the merge flow gives `stage`, the work state shows open or merged and the refusal says "isn't merged yet" (A63).

### 4.2 The check and the guarded moves (`org_process::one_branch`, pure)
**Start.** Inputs: the agent's reserved, open and closing records, the target task, its exception, and whether the rule is on. Answer: allowed, or `OneBranchOpen { branch, task, stage }`.
- **The rule is on** unless a valid, approved `off` of `branch-and-merge` (its blob approved by a non-author, §5) survives resolution in the target task's project. A missing, clashed or invalid key, or no set, leaves it on (A64, A72). A conflict on the key leaves the rule on, even when the version applying meanwhile is an approved `off` (ruled 2026-11-13: ada). So does an invalid or clashed file of the key, or one whose key can't be read, anywhere in the set, failing closed (ruled 2026-11-13: ada).
- **Blocked** when the agent has a reserved or open record for another task. Its own task's record never blocks it. A `closing` record blocks it too: it is an open record mid-move (ruled 2026-11-13: ada).
- **An exception** on the target task allows one record for that task and assignee, then is used up. Any start that makes a record uses it up, even one nothing blocked or with the rule off; reusing the task's own reserved or open record (point 2 after point 1) uses none (ruled 2026-11-13: ada).

It runs at A54's two points, each in one write transaction with the record it creates (A67):
1. `task.set_status { in_progress }` by the assignee agent (`agent-os task start`), when the task has no reserved or open record for that assignee: check, then reserve.
2. The creation of a task branch, whoever triggered it (an app move, a task session start, a hotfix): check and reserve (or reuse the task's own reservation), then create the branch (below). A refused creation ends that session's run as `blocked`.

**Creating the branch** (decision 14), as one locked operation on the repository (the name's suffix included; the mirror fetch takes the same lock): the office resolves the branch the task names, else that repository association's required `integration_branch`, to a sha in `mirror.git`, fetches that sha into an office-made ref in `work.git` (`refs/office/base/<task>`, deleted after the add; an agent could move it, but the add uses the sha), runs `git worktree add -b task/… -- <path> <sha>` (it fails rather than overwrite a planted ref) and records the sha as `base_sha`. A missing `integration_branch` refuses the creation (`unavailable`, reason `integration_branch_missing`).

**Guarded moves** (decision 11). When a task is set to Done (either resolution) while its record is open, or reassigned or unassigned (a reassignment to no one), the office first checks the caller's role and binding (decision 15), then marks the record closing (pushes refused) and, under the repository's lock, fetches the integration branch (a failed fetch is `unavailable`) and force-copies the `work.git` branch to `refs/office/task/<b>` and the worktree's HEAD to `refs/office/task-head/<b>` in `mirror.git`. **Own unmerged work** means any of those copies, the recorded `own_tip` or the remote head descends from `base_sha`, differs from it and isn't in the integration branch, or the worktree has uncommitted changes (untracked files that aren't ignored included). A missing branch, an unreadable HEAD or a failed copy counts as own work. So does a copy that neither descends from `base_sha` nor is in the integration branch (a reset onto another branch's history), while a missing `own_tip` or remote head (none yet) doesn't (ruled 2026-11-13: ada). So a reset or a detached HEAD hides nothing; work only in `git stash` or on another local branch is a residual. The record is merged instead only if `own_tip` is in the integration branch and no copy has work beyond it.
- **Sessions** (decided 2026-11-08), for every caller: the office answers first. A refusal clears closing and ends nothing; an accepted move ends the task's sessions gracefully after the answer. This is safe because the push freeze keeps the remote head still, the copies are refs only the office writes, and commits made after the copy are caught by §4.5's local checks.
- **Re-check.** The record's version, `own_tip`, the remote head and the stored copy shas go into the work write, which refuses (`conflict`) if any changed. The owner's cascades that unassign or close tasks (offboarding, `work.delete`, a retired feature's tasks) are guarded moves by the owner. Uncommitted work now blocks the assignee's own Done (W7 open question 2).
- With own unmerged work, the move needs a project lead (not the assignee) or the owner, also on the owner's own task; a lead who is the assignee counts as the assignee (ruled 2026-11-13: ada): from the assignee it's `one_branch_open`, naming the branch; from anyone else, `not_authorized`. On confirmed closure, preserve/archive the branch before closure cleanup (§4.5). On reassignment, record the full old/new assignment pair atomically with expected revision and preserve the worktree/branches; require a verified workspace transfer before any replacement process starts. The earlier helper archiving every guarded move is legacy behavior requiring adaptation, not the selected handoff rule.
- With no own work, an authorized revision-checked assignment can proceed while retaining the task workspace. Only a separately confirmed closure enters archive/merge cleanup.

These permission checks, and those of §4.3, amend W7 decision 8 ("no permissions in phase 1") for these calls only. A non-assignee's guarded move, a grant and a mark need a trusted binding (decision 15).

**Refusals** use the spec §4 wording. The office keeps one open refusal per (agent, target task), a bookkeeping event that doesn't change the task's version, in the project leads' and owner's Needs you with **Allow once…** and **Open chk-T-17** (PROC-14), until the branch merges or an exception is granted.

### 4.3 Exceptions and merged by hand
- `task.allow_second_branch { change_id, item, reason }`, from the app or `agent-os task allow-second-branch`. The caller is an explicitly authorized project approver or authenticated owner; contributor-team membership alone confers no grant, and never for their own task (PROC-11), except the owner. The reason is required (1–1,000). It's recorded as `BranchExceptionGranted { reason, open_branch, assignee }` with the actor, covers one record for that task and assignee, and a reassignment voids it.
- `task.mark_branch_merged { change_id, item, integration_commit, reason }` (decision 12): the same callers; never the record's own agent. The office checks that the commit is on the integration branch in `mirror.git`, records `BranchMergedByHand` and marks the record merged. If its head isn't an ancestor of the integration branch, A58 deletes nothing remote: the office pushes `merged/<b>` at the head, removes the local worktree and branch (§4.5's local checks), and leaves the remote branch to a person.

### 4.4 Docs branches (pending U28)
The retained UX proposal has no docs-branch cap (U28); one open more than a day is shown to the lead. Automatic reminders remain deferred. Docs branches are listed in the work state ("Also waiting: 1 docs branch", PROC-19).

**Reviewer** (spec §4, R3). Project settings name the project's reviewer (an agent). A review request unanswered for 30 minutes starts a one-off reviewer run (`agent-sessions.md`) and tells the leads.

### 4.5 Cleanup (A58)
Closure only: reassignment/handoff never enters this cleanup path. In `org-git`'s hardened push path, with the push's `contents: write` token. "The head" is the remote's `refs/heads/task/<b>` as last fetched into `mirror.git`.
- **Nothing local is lost.** The office ends any live task session first (a checkpoint turn), unless a guarded move already did. It stops, keeping the worktree, if the HEAD copy holds commits not reachable from the head or the integration branch; `refs/office/task*/<b>` are kept with the record. For an archive, it copies the `work.git` branch into `mirror.git`, pushes it and refreshes the head; if the push is refused (pushed history was rewritten), it keeps the worktree and branch and raises Needs you. A merged record isn't pushed. On either path, local commits beyond the head or uncommitted changes stop the cleanup: the worktree stays and the leads get a Needs you item, as for offboarding (`agent-sessions.md`).
- **Merged:** if the head is an ancestor of the integration branch, push `merged/<b>` at the head, then delete. Otherwise nothing remote is deleted (merged by hand, §4.3), and later commits on the remote raise Needs you.
- **Archived:** push `archive/<b>` at the head, then delete. With no own work and no remote branch, there's nothing to push or tag.
- **The delete is a compare-and-swap**, `git push --force-with-lease=refs/heads/task/<b>:<head> <url> :refs/heads/task/<b>`: it removes the ref only at the checked head and never moves one, so it isn't a force push in A58's sense; if the remote moved, it waits for the next fetch. A tag already at the same commit counts as pushed; at another, the office stops and raises Needs you.
- Then the worktree goes, and the local branch with a compare-and-swap (`update-ref -d refs/heads/task/<b> <copied sha>`). Only `task/*` branches with a record are deleted, never a tag. Tags keep everything forever; only a person removes one.

## 5. Changing processes (PROC-11)

**Deferred approval/write protocol.** The exact calls below are earlier sketches. For selected content, publish in organization Git with stable IDs, selected lifecycle and revisions; organization/team/project content no longer chooses different code/handbook repositories. Private storage and old process-only extension fields remain unselected.
Process files are docs written with W7's write path (office-made `docs/…` branches, the owner as author, the agent as co-author): `doc.new`/`doc.save` with kind `process` for a project, `process.save` for the handbook. Every write checks: valid front matter matching the path; a key unique at that (level, scope) (`already_exists`); no override or `off` of a sealed key (`invalid_request`, reason `sealed`); a private file with no `overrides`, `status: off` or organization key (reason `private_only_adds`; team and project keys are checked at resolution); an agent writing only its own private folder (`not_authorized`); sealing an organization process (`overridable: false`) only by the owner (`not_authorized`, spec decision 3); and `branch?` naming an unmerged office-made branch for that file, opened by the caller.

**Only through the tool.** `doc.render`, `doc new --here`, `doc.set_status`, `doc.rename` and `doc.delete` refuse kind `process`. In the handbook only the office commits: the index follows its last recorded commit, and any other commit is ignored and raised to the owner. In a project repo, a process changed by plain git is shown "from git" and still applies, except that an unapproved `off` of the enforced key doesn't turn the rule off (A72).

**Approval** (spec decisions 8, 10, 11). `process.approve` and `process.reject` name the changed file's doc id and the exact branch commit the approver saw; a moved branch is `conflict`, and a later save voids an approval. The caller is the level's approver, on a trusted binding (decision 15), and none of the change's authors (everyone who saved on the branch), except the owner. *Interim (A62):* an approved handbook branch is merged by the office with a real merge, since nothing else can reach the handbook; if it doesn't merge cleanly, it's `conflict` and re-proposed. A project branch's approval is recorded, and the merge flow merges it (A62). Retro items are approved by the owner's approval of the retro (spec decision 10).

**Version.** Selected files advance `revision`; the older protocol uses `version: <base> + 1` as a compatibility input, not a second authored counter; if another change merged first, the branch needs a new save and approval.

**Drift fixes** (PROC-9): **Keep my override** rewrites `overrides` to the parent's version; **Take the new version** sets `status: retired`; **Edit**, **Fix** and **Rename…** are ordinary changes for the level's approver.

**Residual risk.** The handbook deny binds office-run agents only. Hand-run agents (no `run_mode`, no session token) and other same-user programs can still read `handbook.git` and forge hand-run agent identities (ADR-0003, M-A), but can't use lead powers over the rules, which need a trusted binding (decision 15), short of stealing a live token from another process's environment (`agent-sessions.md`, Security). The tampering check limits handbook writes; real isolation is v2.

## 6. The contract (additive)

**Versioned legacy protocol proposal, not target wire commands.** Check org-proto support before claiming availability. Target commands must use selected document identity/scope/revision and organization repository publication; repository-specific branch operations need explicit target IDs. Private-layer access, approval receipts and durable branch/exception history require separate design.
A new module, `crates/org-proto/src/process.rs`. The historical sketch was additive under API_VERSION 1; the first standalone release uses local API_VERSION 2 under ADR-0001, with additive evolution within that version. Any bound member reads organization, team and project processes. Private ones only their agent and the owner (A52, A69): for anyone else, another member's `process.effective`, `why` and `show` leave out its private layer and `last_read`, and `process.show { level: agent }` or `process.view { agent }` is `not_authorized`.

`For`: `current` (default) · `none` (O ⊕ G) · `project { project_id }` · `item { item }`.

| Call | Params | Result | Errors |
| --- | --- | --- | --- |
| `process.effective` | `member_id?`, `for?` | `{ set, last_read: Vec<{ key, blob, version, at }>, computed_at, stale_since? }` | `not_found`, `not_authorized`, `unavailable` |
| `process.why` | `key`, `member_id?`, `for?` | `{ key, chain: Vec<Layer>, winner? }` | as above |
| `process.show` | `key`, `member_id?`, `for?`; or `level`, `scope?`, `version?`; `offset?` | `{ path, repo, commit, blob, version, content (≤ 512 KiB), total_bytes, next_offset? }` | as above |
| `process.view` | `scope: organization \| team { team_id } \| project { project_id } \| agent { member_id }` | `{ rows: Vec<ProcessRow>, indexed_commit, stale_since? }` (PROC-9, PROC-10) | as above |
| `process.ack` | `key`, `blob` | `Empty` | `not_found` |
| `process.save` | `change_id`, `level`, `scope?`, `key`, `content`, `expected_blob?`, `message?`, `branch?` | `{ path, branch, commit, blob, pushed, synced }` | `invalid_request`, `already_exists`, `not_authorized`, `conflict`, `unavailable` |
| `process.approve` | `change_id`, `doc_id`, `expected_commit` | `{ branch, merged, commit? }` | `not_found`, `not_authorized`, `conflict`, `unavailable` |
| `process.reject` | `change_id`, `doc_id`, `expected_commit`, `reason` | `Empty` | as above |
| `task.allow_second_branch` (work) | `change_id`, `item`, `reason` | `WorkAccepted` | `not_found`, `not_authorized`, `invalid_request` |
| `task.mark_branch_merged` (work) | `change_id`, `item`, `integration_commit`, `reason` | `WorkAccepted` | `not_found`, `not_authorized`, `invalid_request` |
| `work.branch_state` (work) | `member_id?` | `{ task_branch?: { task, project_id, branch, state, stage? }, docs_branches }` (others see a count only; PROC-15, PROC-19) | `not_found` |

`ProcessRow` carries the key, name, per-level versions, winner, kind, drift, conflicts, problems and pending branches. **Existing calls gain `one_branch_open` and `not_authorized`:** `task.set_status` (a start, or a guarded close) and `task.assign` (a guarded move), §4.2.

**Earlier project-rule contract:** `project.create` and `project.update` used a single `integration_branch`; target validation must apply the same reserved-ref safety to each project/repository association's required branch. The earlier calls refuse a branch starting with `task/`, `docs/`, `merged/` or `archive/` (`invalid_request`), to keep code-instruction and merge refs distinct from managed task/archive refs; selected process content instead uses organization state_branch. (amended 2026-11-08: proto follow-ups) The office reads the integration branch as `refs/heads/<name>`, never through git's short-name lookup, so the name can't reach a tag or another ref. As defence in depth, the prefix is compared ignoring ASCII case and through one leading `refs/heads/` or `heads/`. The office and the app's mocks are to share the rule as `org_proto::has_reserved_branch_prefix`.

**Events.** `process.changed { repo, commit, levels_and_scopes }` (agent scopes only to the owner and that agent); `work.changed` gains `StartRefused` (bookkeeping), `BranchExceptionGranted`, `BranchMergedByHand`, `GuardedMove { by, archived_branch }` and `WorkChange::BranchState { member_id, state }`. Unknown values decode as `Unknown`.

**Trusted binding** (decision 15). From a hand-run lead, `task.allow_second_branch`, `task.mark_branch_merged`, `process.approve`/`reject` and a guarded `task.set_status`/`task.assign` by a non-assignee answer `not_authorized` with reason `forwarded_to_owner`, and change nothing. The office adds a Needs you item for the owner ("<lead> asks to <action> on <task or change>: <reason>") with **Do it** (the owner makes the call) and **Dismiss**.

**Errors.** One new code, **`one_branch_open`**; its message names the branch, task and stage, and `work.branch_state` gives them as data. Reused: `invalid_request`, `already_exists`, `not_found`, `not_authorized`, `conflict`, `unavailable` (reasons `handbook_unreachable`, `repository_busy`, `integration_branch_missing`, `forwarded_to_owner`, `sealed` and `private_only_adds` on `ApiError.reason`, which the agent-sessions contract adds; R1 and R2 depend on that field landing in `org-proto` first).

**Obsolete organization-data sketch (retained for compatibility review only).** `OrgSnapshot.handbook: Option<ProjectRepo>` (`#[serde(default, skip_serializing_if = "Option::is_none")]`, no `path`), set by `OrgChange::HandbookSet { handbook }`. It changes `org-proto`'s tests, the desktop mock and `proto.ts` (not `#[non_exhaustive]`).

## 7. Where the code lives

The package names below are historical implementation ownership/proposals: current executables are agentic-org, agentic-org-office, agentic-org-web and agent-os. Existing org-process/org-docs/org-work helpers are partial evidence only; proposed adapters, services and commands must be verified separately.
- **`org-process`** (new; owner TBD by ada): front matter, path → level and scope, `resolve`, the fingerprint, `render`, the checklist merge, `one_branch` (start check, guarded moves, own work from given shas), the write checks. Pure: no SQL, git, network or clock; no ports. Depends on `org-proto`, `org-docs` and `serde`: `Ce = 2` for the proposed internal package graph, or 3 including `serde`; calculate I only after identifying consumers at the same level.
- **`org-office`** (server-dev): the index, caches, project-wide items, last reads, records, exceptions, the calls and the checks inside the work write; one new stable-ward edge, to `org-process`.
- **`agentic-org-office`** (server-dev): repo reads through `org-git`; the handbook at organization create.
- **`org-git`** (River): W7's `DocRepo` API, the handbook's repository and merge, branch creation at a sha, and the §4.5 cleanup.
- **`agent-os`** (Remy): the PROC-8 commands (Appendix B), `task allow-second-branch`, the `one_branch_open` exit code.
- **`apps/agentic-org-ui`** (app-frontend-dev): the Processes views, the profile tab, Allow once…, Mark merged…, the work state.

One new library, three stable-ward dependencies, no cycle.

## 8. Phases

Every R phase is deferred product integration/earlier planning, not a selected rollout or proof that storing the data implements its engine. Preserve existing helper code/tests while designing explicit target adapters.
| Phase | What | Owners |
| --- | --- | --- |
| **R1** | The contract (§6). `org-process` with its tests (Appendix C). Needs `org-docs`' parser first (A70). | ada; `org-process` owner |
| **R2** | Enforcement: records and the backfill (Appendix A), the start check, guarded moves, branch creation at a sha, `one_branch_open`, refusals, exceptions, merged by hand, `work.branch_state`, cleanup, inbox notices, the CLI. *Interim (A64):* until R3 the rule is on for every project. PROC-14, 15, 19. | server-dev; River; Remy |
| **R3** | The handbook (create, seed, the deny, migration), the index, sets and cache, project-wide conflicts, delivery, the fallback, the read calls, the reviewer setting and its fallback. PROC-2 to 7, PROC-8 reads, 10, 16, 17. | River; server-dev; Remy; ada (the deny) |
| **R4** | Writes, the write checks, approve and reject, the handbook merge, private proposals, versions, drift fixes, checklists and moments, the CLI writes, the app's views. PROC-8 writes, 9, 11, 12, 18. Project merges depend on A62. | server-dev; app-frontend-dev; Remy |
| **R5** | Retro items of kind Process (PROC-13), with retros (W7 P5). Branch-wait insights (spec §4). | as retros |

Each step: its own branch, an independent review, the reviewer's READY, a merge into `v1-integration`.

## 9. Open questions
- A60 · Resolved structurally: TEAM- references exist; all canonical paths use stable IDs and recursive ancestry.
- A61 · Superseded: selected process/standard content uses the registered remote organization repository and state_branch, not a second handbook record.
- A62 / A63 / A67 · Deferred approval, branch-stage, transaction and merge services need concrete durable contracts before integration.
- A64 · Deferred enforcement selection; selected process content alone does not turn a rule on. Existing AGENTS.md repository rules remain separately applicable.
- A65 / A66 · Assignment, office placement and workspace transfer must preserve task work/history and execution fencing; caches cannot be the accepted-write authority.
- A68 / A71 · Contributor-chain conflict resolution remains open. One owner team is required; no sibling/project edit can defeat an explicit applicable ancestor prohibition.
- A69 · Private process storage/access is unselected; do not infer owner read access to author-only private task notes.
- A70 · Existing parser helpers must be adapted through explicit selected-envelope mappings, preserving legacy IDs/tests.
- A72 · Import/direct Git publication validation and approval provenance remain design work; invalid authored content must be visible without granting powers.
- U25–U28 · Earlier UX preferences remain proposal context. They do not select process/branch/autonomy engines or override selected structure.

## Appendix A. Migration

**Deferred legacy migration sketch.** No backfill or data move is authorized/enacted by this document. Preserve stable IDs and evidence, and preview exact mappings before any future migration.
- **R2 backfill.** Records come from each task's `branch`: `base_sha` is its merge base with the integration branch, `own_tip` its tip when not in the integration branch. Merged when the tip is in the integration branch and the task is Done · Completed; archived (with its tag) for a cancelled or deleted task, or a closed one with no own work; open otherwise. An existing `integration_branch` with a reserved prefix goes to the owner's Needs you. A squash-merged Done task shows as stuck, for **Mark merged…**.
- **Software repository documentation remains here.** Earlier plans to move branch-and-merge.md into a handbook are superseded for selected storage. A future explicit organization import may map generic/project-specific content to selected scoped process records; this documentation reconciliation moves no files and changes no AGENTS.md rules.

## Appendix B. The CLI (PROC-8)

**Deferred legacy CLI proposal:** these agent-os command names are not claims of implemented agent-os entry points.
`process list` is `process.effective` (`--level`: `process.view`); `why`, `show`, `ack`, `approve` and `reject` are their calls. `new`, `override`, `off`, `edit`, `rename`, `keep` and `take-parent` build the content and send it with `process.save` (handbook) or `doc.save` (project). `new --private "<name>"` slugs the key from the name, with a clash check, as for doc keys. `task allow-second-branch` and `task mark-merged` are the work calls.

## Appendix C. Tests that prove the rules

**Earlier helper tests / deferred integration checks.** Existing tests are evidence of that versioned contract. Target checks must additionally cover canonical fields, state_branch reads, recursive scope, required per-repository integration branches and preservation across handoff; listed product integration tests are requirements, not results run here.
Each test must fail without its rule.

**`org-process`, table-driven:**
- Matching by key, never `id`; level order; nearer team wins; no current work gives O ⊕ G.
- Seals: an org seal refuses lower overrides and `off` without a conflict; a team seal refuses its sub-teams' and projects'; on a shared project, a sibling's override is a conflict with the sealed version applying, and a project version cannot defeat an explicit ancestor prohibition; multiple-seal policy remains unresolved; a clashed sealed key stays sealed.
- Conflicts: two team overrides give the org version (or none); one team's version applies; an ordinary overridable conflict may be settled by project content; a parent-and-sub-team link or a diamond isn't a conflict (the same team reached twice), but a sub-team's override in a diamond is (R1).
- Turned off, private only adds, validity, drift, the fingerprint, the render cap and order, the checklist merge.
- One branch: another task's record blocks, its own and docs branches don't; an exception is used once; only an approved `off` turns the rule off; can't compute counts as on. Own work: a tip at `base_sha` or in the integration branch is none; a commit beyond it, or uncommitted changes, is some.

**Office and store:**
- Concurrency: of two starts or branch creations for one agent, exactly one succeeds; a failed `git worktree add` leaves no reservation and restores the exception.
- Records: one own commit sets `own_tip` (its own `refs/office/task/<b>` copy doesn't hide it); a fast-forward or reset onto another branch doesn't; a `--no-ff` merge closes it, also after a delete; pushes after a merge or without a record are refused; reused names get a suffix; an idle record reaches Needs you.
- Legacy helper guarded-move tests (target handoff semantics differ): the assignee's `task done --cancelled`, `task done` without a push, `task assign` and `task assign --none` are `one_branch_open`; by another lead or the owner they archive and free the agent; with no own work they go through.
- Merged by hand: by the record's agent, `not_authorized`; by a lead with a commit not on the integration branch, `invalid_request`; otherwise recorded and merged, with a `merged/` tag and the remote branch kept.
- Bindings (decision 15): a hand-run lead's `allow_second_branch`, `mark_branch_merged`, guarded move or `approve` answers `forwarded_to_owner`, changes nothing and kills no session, and the owner's Needs you item appears; a token-bound lead's is accepted.
- Guarded-move copies: an unpushed commit on the branch with HEAD detached at `base_sha` is own work, and so is a branch at `base_sha` with an own commit only on a detached HEAD or another checked-out branch; a merged `own_tip` with a later local commit stays guarded; a failed copy counts as own work; an accepted Done ends the session only after the answer; a refused Done keeps the session and clears closing; a change between judgement and write is `conflict`; a fetch showing `own_tip` merged marks the record merged.
- Branch creation: a planted local `task/<name>` ref gets a suffix, and one planted between the name check and the add makes the add fail, not overwrite; a moved integration ref in `work.git` doesn't break the fetch of the base sha.
- Permissions: a guarded move by a peer who isn't a lead, or a lead's Allow once… for their own task, is `not_authorized` (the owner's isn't); an exception is void after a reassignment; a push during a guarded move is refused, and the move judges the pushed work.
- Start point (decision 14): moving the integration ref in `work.git` doesn't change a new branch's parent or `base_sha`; a missing `integration_branch` is `unavailable`; `project.create` and `project.update` refuse each reserved prefix.
- Cleanup: tag before delete; no delete if the head isn't in the integration branch or the remote moved; unpushed or uncommitted work stops it; a tag at another commit stops it; a reset before a guarded move still counts as own work; a refused archive push, or uncommitted changes, keep the worktree; never the default or integration branch, an unrecorded branch, or a tag.
- Approval: a co-saver can't approve; the owner approves their own change; a later save makes approve `conflict`; handbook commits the office didn't make are ignored; the refused `doc.*` calls refuse kind `process`; only the owner seals at organization level; a private file using an organization key is `private_only_adds`; `process new --private` checks its slug key for a clash.
- Reads and delivery: no working branch or `work.git` ref reaches a set or the A57 sources; private reads as in §6, and agent tools denied `workspace/handbook/**`; `task start` returns the section; a merged change is sent once; agent leads get inbox items once; an unreadable handbook gives `stale_since` and the last set, and enforcement still runs.
