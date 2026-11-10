# Design: processes (how agents work, written down and inherited)

Status: **proposed, rev 3** 2026-11-08. Owner: ada. It implements Eden's spec [`processes-ux.md`](../specs/processes-ux.md) rev 3 (PROC-1 to PROC-19) without redefining its UX, except decision 13, which spec rev 3 adopts, and builds on [`tasks-projects-docs.md`](tasks-projects-docs.md) (W7), [`agent-sessions.md`](agent-sessions.md), [`projects-git-github.md`](projects-git-github.md) and [`teams-and-org-chart.md`](teams-and-org-chart.md). The first process, [`branch-and-merge.md`](../processes/branch-and-merge.md), is the reference shape.

**Proposed amendment, rev 4 (2026-11-09, owner request):** §3.5 extends delivery for
fresh contexts, standards lookup and tool help (spec rev 5, PROC-27..30). The rev 3
design's existing autonomy/branch-rule differences from spec rev 4 still need
reconciliation; this amendment does not silently decide them.

**Standalone-v1 integration (2026-11-09):** human controls use the authenticated,
authorized HQ HTTP boundary in [standalone-server.md](standalone-server.md).
Process composition, enforcement and handbook access remain office responsibilities;
closing the browser does not unload instructions or suspend enforcement. Native
desktop trust is not an authorization mechanism in v1.

A new pure library, **`org-process`**, holds the parsing, the resolution and the one-branch rule; the office does the I/O.

## Decisions
All decided 2026-11-08, except A55's docs-branch limit, which waits on the user (U28).
1. **A51 Storage.** Everything persistent lives in repositories. Project processes go in the project repo's `docs/processes/`. Organization and team processes go in the organization's handbook repo (`processes/`, `teams/<team key>/processes/`), created with the organization and changed through the same branch-and-merge flow. HQ keeps only an **index** (process key, level, scope, version, path, commit sha, status, overridable or sealed, overrides), rebuilt from the repos and never holding the text. *Teams have no key, so this design writes `teams/<team id>/` (A60).*
2. **A52 Private processes** live in the handbook at `agents/<member id>/processes/`. The organization owns them and the owner can see them. They survive offboarding as history. The agent changes them through the merge flow, and the owner approves.
3. **A53 Resolution.** The office computes the effective set per (agent, project) from default or integration branches only, never a working branch. Order: organization → team(s) → project → agent. Processes match on the `process:` key, the lowest level wins, and a sealed process can't be overridden. The result is cached, keyed by the commit shas involved, and pushed at session start and wake. `process why <key>` reads the same result and shows each layer.
4. **Multi-team conflict.** If two teams both override the same key for one shared project, and the project doesn't resolve it, the agent follows the organization's version. If the organization has none, neither override applies. The conflict shows in Needs you for the project's lead and the owner. Work isn't blocked.
5. **Identity.** A process keeps W7's doc identity (`tasks-projects-docs.md` §4.9: a number or `doc-<ULID>`). The match key is separate: `process: <key>`, unique per (level, scope). Overrides match on it; a hand-written `id:` never does.
6. **A54 Enforcement.** The office refuses `task start` and the creation of a new task branch while the agent has an open task branch that isn't merged, which it knows from the merge flow. The error is `one_branch_open`, naming that branch. The lead or the owner can grant a one-time exception, recorded on the task. Hotfixes follow the same rule.
7. **A55 Which branches count.** Only the task branch blocks a task start; office-opened docs, process and retro branches don't. *Pending the user (U28):* at most one open docs branch per agent (a second doc change waits or joins it), and one open longer than a day shows in the lead's Needs you.
8. **A56 Base branch.** Task branches start from the project's `integration_branch` (the default branch when it's empty). No new setting.
9. **A57 One source for a project's guidance.** Instruction files, guidance files (`docs/.guidance/<moment>.md`) and project processes are all read from the project's `integration_branch` (the default branch when empty), so all three follow merged work.
10. **A58 Branch cleanup.** After a merge, the office deletes the remote task branch only if its head is an ancestor of `integration_branch` (checked on the mirror), after pushing a `merged/<branch>` tag at that head. A task closed without a merge gets an `archive/<branch>` tag pushed first, then the branch is deleted. The office only deletes `task/*` branches it created: never the default or integration branch, never a force push, never a tag. The local worktree and branch go too. It uses the push's `contents: write` token.
11. **Own work stays on the agent's plate** (decided 2026-11-08). A task with own unmerged work (judged from its branch copied into `mirror.git`) can be closed other than as merged, reassigned or unassigned only by a project lead or the owner, recorded on the task; the work is then archived and the agent freed. The agent gets `one_branch_open`. A task with no own work moves freely (§4.2).
12. **Merges are real merges** (decided 2026-11-08). Task branches merge with `--no-ff`, seen by ancestry. Work merged another way is marked "merged by hand" by a project lead or the owner (never the agent), naming the integration commit. A record idle over a day shows in the lead's Needs you (§4.1, §4.3).
13. **Team seals on shared projects** (decided 2026-11-08; reconciles spec decisions 3 and 6). A seal binds the sealing team's own projects. On a shared project, a sibling team's override or `off` doesn't silently cancel it: it's a conflict in Needs you, the sealed version applies meanwhile, and the project can settle it with its own version, as any conflict (§3.2). This departed from the spec's rev 2 (decisions 3 and 6, PROC-6); spec rev 3 adopts it, including the conflict wording "sealed by <team>; applies until the project decides".
14. **Start point from the mirror** (decided 2026-11-08). The office starts a task branch at the integration branch's sha in `mirror.git` and records it as `base_sha` (§4.2).
15. **Lead powers need a trusted binding** (decided 2026-11-08; human binding amended for standalone v1, 2026-11-09). `task.allow_second_branch`, `task.mark_branch_merged`, guarded moves by a non-assignee, `process.approve`/`reject` and any approval that leads to an office merge of a handbook branch require an authorized human principal through HQ, or an authorized agent bound with a live session token (office-run). Existing role and owner-only checks still apply; neither a browser connection nor an office certificate grants lead powers by itself. A hand-run lead's request becomes a Needs you item for the owner instead.

Amended to match: `projects-git-github.md` (A56, A58, decision 14, the push rule) and `agent-sessions.md` (A57). Still to amend: `agent-sessions.md` (the handbook deny; "Git in worktrees": no worktree passes to a new assignee), `projects-git-github.md` ("When a task is done"), W7 (decision 8, `task.branch`), the `work.rs` docs, and ADR-0003 §2.1.4's rows for the decision-15 calls.

## 1. Where each thing lives
| Thing | Where | Changed by |
| --- | --- | --- |
| Organization processes | handbook, `processes/<key>.md` | docs branch → owner (U27) → merge (PROC-11) |
| Team processes | handbook, `teams/<team id>/processes/<key>.md` | docs branch → team lead or owner → merge |
| Project processes | project repo, `docs/processes/<key>.md` | docs branch → a project lead or owner → merge |
| Private processes | handbook, `agents/<member id>/processes/<key>.md` | docs branch → owner → merge (A52) |
| Process index | HQ's `office.db`, a cache | rebuilt from git after each fetch or merge |
| Effective sets, last reads | `office.db` of the agent's office, derived caches (not the index) | the office (§3.3, §3.4) |
| Task branch records, exceptions | `office.db` of the office that owns the task (HQ in phase 1) | the office (§4) |
| The handbook's record | organization data: `OrgSnapshot.handbook` | HQ, at organization create |

**The handbook repo.** HQ creates it with the organization (or at first start for an older one): one office-only bare repository, `workspace/handbook/handbook.git`, seeded with `README.md`, `.templates/process.md` and `processes/branch-and-merge.md`, on by default (spec §4). Only the office reads and writes it (W7's write path); agents read processes through §6. Its record has no `path`, so its location isn't published.
- **Required:** a fixed read and write deny on `offices/*/workspace/handbook/**` in `agent-sessions.md`, or any agent at HQ could read private processes with git. R3 doesn't ship without it.

## 2. The model

### 2.1 The process file (PROC-1)
The front matter is the spec's §1. This design adds:
- `process`: 1–64 of `[a-z0-9-]`, starting with a letter or digit. `summary`: one line, ≤ 200 characters. `moments`: ≤ 500 characters each. `checklist`: ≤ 50 steps.
- `level` and `scope` must match the location (§2.2). Only `active` and `off` resolve. `overrides: <key>@<version>` is for drift only.

Parsing uses W7's front-matter rules and `org-docs`' parser. Invalid front matter never fails a read: the file is listed with a problem naming the field and left out of every set until fixed (PROC-16).

### 2.2 Levels, scopes and paths
| Level | Scope | Repo, branch, folder |
| --- | --- | --- |
| `organization` | none | handbook, default branch, `processes/` |
| `team` | team id | handbook, default branch, `teams/<team id>/processes/` |
| `project` | project key | project repo, `integration_branch` (else default), `docs/processes/` |
| `agent` | member id | handbook, default branch, `agents/<member id>/processes/` |

Only regular `*.md` blobs directly in those folders count (no symlinks or submodules); the front-matter key matches, not the file name (PROC-1). **Reads never use `work.git`**, whose refs agents can move: project files are read in `mirror.git` (W7 §4.9), handbook files in `handbook.git` (no `work.git`, so no sync step). W7's path rules apply, with the handbook's roots (`processes/`, `teams/`, `agents/`, `.templates/`, `README.md`), and its id reservations gain a repository key.

### 2.3 The index (HQ)
One row per file on a source branch: `{ repo, path, commit, blob, doc id, key, level, scope, version, status, overridable, overrides, problems }`. No text: names and summaries come from the blob. Old rows stay as a history (key, level, scope, version → commit) for drift diffs (PROC-9). It's rebuilt after each change to a source branch; while the handbook can't be read, reads answer from it with `stale_since` (PROC-16).

## 3. Resolution (A53)

### 3.1 Inputs
For (agent A, project P): **O**, the organization's; **T**, for each team linked to P, its chain from the outermost parent down (a linked ancestor of another linked team is dropped); **J**, P's, from its integration branch (A57); **G**, A's private ones.

With no current work, it's **O ⊕ G** (PROC-2). The home session uses the current task (`work.set_current`), a task session its task. No repository means an empty J.

### 3.2 Rules, in order (`org_process::resolve`, pure)
1. **Valid files only.** Invalid front matter or a level or scope mismatch: left out, with a problem. A `draft` or `retired` file is left out silently: it is a normal state, not a problem (amended 2026-11-09: ada, from the Processes screens' review).
2. **Key clash.** Two files with one key at one (level, scope): both left out, with the clash problem (PROC-1). Seals fail closed: if a file left out for invalid front matter, a mismatch or a clash is sealed (or can't be read far enough to tell), the key stays sealed below. `draft` and `retired` files never seal.
3. **Seals inside a chain.** A file that overrides or turns off a key sealed by the organization, or by a team above it in its own chain, is refused, with the sealing level (PROC-3). A project file is refused if the organization sealed the key, or a team seal binds it and rule 5 finds no team-level conflict on the key (that check runs after rule 5). Otherwise this runs first, so a refused file never makes a conflict. The write path refuses these too (§5).
4. **Fold each team chain**, outermost first, the nearer team winning.
5. **Combine the chains** (decision 4, PROC-6). Only team-level winners count. One distinct winning team file applies. Two or more are a **conflict**: the organization's version applies, or nothing if it has none, except that a **sealed** team winner applies meanwhile (decision 13; two different seals: the organization's version, else the first by team id that isn't an `off`, A71). A sibling team's override or `off` never silently cancels a seal.
6. **Project.** J's version wins over the result unless the key is sealed (rule 3). It settles any conflict on the key, a sealed one included (decision 13).
7. **Turned off.** A surviving `off` removes the key and records "Turned off for <team or project name>: <reason>" (PROC-4).
8. **Private only adds** (PROC-5). A private file isn't applied, and is flagged, if it sets `overrides` or `status: off`, or if its key appears in **any** O, T or J file of this project, whatever that file's state.
9. **Drift** (PROC-9). An override whose `overrides: key@v` is below the parent's current version is marked "Parent changed"; without `overrides`, its drift shows as unknown.

**Output:** `EffectiveSet { inputs, entries, turned_off, conflicts, problems }`: per key, the winning file (with its kind: inherited, override or addition) and its **chain** of layers and what happened to each, which `process why` prints (PROC-8).

### 3.3 Caching and project-wide checks
Sets are cached under a fingerprint of the handbook commit, the project's integration commit and the team chains (teams are org data). Any input change drops the sets that used it; they're recomputed at the next start, wake or read. The last good set per (agent, project) is kept with its time.

**Conflicts belong to the project.** After each index or org change, HQ resolves O ⊕ T ⊕ J per affected project and keeps one Needs you item per (project, key) for its leads and the owner; clashes, invalid files and drift likewise. Agent leads and approvers get Needs you items in their inbox (spec decision 11).

**Can't compute** (PROC-17), including a missing `integration_branch`: the last good set, with the spec's wording and warnings; the one-branch rule counts as on.

### 3.4 Delivery to sessions (PROC-7, PROC-12)
`org_process::render` builds a **Your processes** section for the composed instructions (`agent-sessions.md` decision 8):
- the set: each process's name, level, version and summary, plus turned-off ones and conflicts, in the spec §3 wording;
- **changed since last read**: the full text of each process whose winning blob isn't among the blobs the agent has read for that key (a new version, a new winner or a conflict fallback all count);
- each process's `moments` prompt, after that moment's guidance text, only while the moment is On.

**Size cap: 24 KiB**, prompts included: summaries first (past the cap, "and N more: run `org-agent process list`"), then changed texts, organization first; the rest are named with "run `org-agent process show <key>`". It goes in the instruction file at a start, the wake prompt at a wake (a message for a resumed session, `agent-sessions.md` decision 5), and `org-agent task start`'s result; `task done` returns the Finishing a task prompts. The texts actually delivered count as read once the runtime accepted the start or wake, or on `process ack`.

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
`org-agent` help explains tools, syntax and permissions. Inject concise tool help
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

## 4. Enforcement: one open task branch (A54, A55)

### 4.1 Task branch records
The task's owning office keeps one record per task branch: `{ task_id, member_id, project_id, branch, base_sha, own_tip?, state: reserved | open | closing | merged | archived, merged_by_hand?, stage?, opened_at, last_moved_at, closed_at? }`. Phase 1 has one office, so records don't move; in phase 2 they follow their task on a handover or reclaim (A66).
- **Reserved** at `task start`, before any git; **open** once the branch exists. Nothing is reserved without a repository.
- **Branch names.** `task/<ref>-<slug>` (W7), with a `-2`, `-3`… suffix whenever a record, a `merged/` or `archive/` tag, a remote branch or a local `work.git` branch already has the name (never `-B`).
- **`own_tip`** (decided 2026-11-08) is the latest tip the office has seen on the branch (through `org-agent push`, or a fetch into `mirror.git`) that descends from `base_sha` and is reachable from no ref in `mirror.git` other than this branch's own copies (`refs/heads/task/<b>` as fetched, `refs/office/task/<b>`, `refs/office/task-head/<b>`). A fast-forward or reset onto the integration branch or someone else's branch never sets it. Residual: agents share `work.git`'s objects, so one could push another's commits first; review catches it.
- **Merged** when `own_tip` is reachable from the **integration branch** in `mirror.git` (a `--no-ff` merge, decision 12), or when a project lead or the owner marks it **merged by hand**. `own_tip` stays checkable after the branch is deleted. Once merged, the office refuses pushes to the branch; more work needs a new branch, checked like any other.
- **Pushes need a record.** `org-agent push` takes a `task/*` branch only from the holder of its open record.
- **Released** (removed): a reservation whose task leaves In progress, closes, is deleted or moves before its branch exists. A failed `git worktree add` removes it and restores its exception in one transaction; startup reconciles reservations with existing branches and clears a stale closing. `task.branch` (W7) holds the current record's branch; branch → task goes through records.
- **Stuck.** A record idle for over a day goes to the lead's Needs you, with **Mark merged…** (decision 12). Until the merge flow gives `stage`, the work state shows open or merged and the refusal says "isn't merged yet" (A63).

### 4.2 The check and the guarded moves (`org_process::one_branch`, pure)
**Start.** Inputs: the agent's reserved and open records, the target task, its exception, and whether the rule is on. Answer: allowed, or `OneBranchOpen { branch, task, stage }`.
- **The rule is on** unless a valid, approved `off` of `branch-and-merge` (its blob approved by a non-author, §5) survives resolution in the target task's project. A missing, clashed or invalid key, or no set, leaves it on (A64, A72).
- **Blocked** when the agent has a reserved or open record for another task. Its own task's record never blocks it.
- **An exception** on the target task allows one record for that task and assignee, then is used up.

It runs at A54's two points, each in one write transaction with the record it creates (A67):
1. `task.set_status { in_progress }` by the assignee agent (`org-agent task start`), when the task has no reserved or open record for that assignee: check, then reserve.
2. The creation of a task branch, whoever triggered it (an app move, a task session start, a hotfix): check and reserve (or reuse the task's own reservation), then create the branch (below). A refused creation ends that session's run as `blocked`.

**Creating the branch** (decision 14), as one locked operation on the repository (the name's suffix included; the mirror fetch takes the same lock): the office resolves the branch the task names, else `integration_branch`, else the default branch, to a sha in `mirror.git`, fetches that sha into an office-made ref in `work.git` (`refs/office/base/<task>`, deleted after the add; an agent could move it, but the add uses the sha), runs `git worktree add -b task/… -- <path> <sha>` (it fails rather than overwrite a planted ref) and records the sha as `base_sha`. A missing `integration_branch` refuses the creation (`unavailable`, reason `integration_branch_missing`).

**Guarded moves** (decision 11). When a task is set to Done (either resolution) while its record is open, or reassigned or unassigned (a reassignment to no one), the office first checks the caller's role and binding (decision 15), then marks the record closing (pushes refused) and, under the repository's lock, fetches the integration branch (a failed fetch is `unavailable`) and force-copies the `work.git` branch to `refs/office/task/<b>` and the worktree's HEAD to `refs/office/task-head/<b>` in `mirror.git`. **Own unmerged work** means any of those copies, the recorded `own_tip` or the remote head descends from `base_sha`, differs from it and isn't in the integration branch, or the worktree has uncommitted changes (untracked files that aren't ignored included). A missing branch, an unreadable HEAD or a failed copy counts as own work. So a reset or a detached HEAD hides nothing; work only in `git stash` or on another local branch is a residual. The record is merged instead only if `own_tip` is in the integration branch and no copy has work beyond it.
- **Sessions** (decided 2026-11-08), for every caller: the office answers first. A refusal clears closing and ends nothing; an accepted move ends the task's sessions gracefully after the answer. This is safe because the push freeze keeps the remote head still, the copies are refs only the office writes, and commits made after the copy are caught by §4.5's local checks.
- **Re-check.** The record's version, `own_tip`, the remote head and the stored copy shas go into the work write, which refuses (`conflict`) if any changed. The owner's cascades that unassign or close tasks (offboarding, `work.delete`, a retired feature's tasks) are guarded moves by the owner. Uncommitted work now blocks the assignee's own Done (W7 open question 2).
- With own unmerged work, the move needs a project lead (not the assignee) or the owner: from the assignee it's `one_branch_open`, naming the branch; from anyone else, `not_authorized`. When a lead or the owner makes it, it's recorded on the task, the branch is archived (§4.5) and the agent freed. A new assignee starts a new branch; the work stays at `archive/<b>`.
- With no own work, the move goes through and the record is archived (or merged, above).

These permission checks, and those of §4.3, amend W7 decision 8 ("no permissions in phase 1") for these calls only. A non-assignee's guarded move, a grant and a mark need a trusted binding (decision 15).

**Refusals** use the spec §4 wording. The office keeps one open refusal per (agent, target task), a bookkeeping event that doesn't change the task's version, in the project leads' and owner's Needs you with **Allow once…** and **Open chk-T-17** (PROC-14), until the branch merges or an exception is granted.

### 4.3 Exceptions and merged by hand
- `task.allow_second_branch { change_id, item, reason }`, from the app or `org-agent task allow-second-branch`. The caller is a lead of one of the project's teams or the owner (a project with no lead: the owner only), and never for their own task (PROC-11), except the owner. The reason is required (1–1,000). It's recorded as `BranchExceptionGranted { reason, open_branch, assignee }` with the actor, covers one record for that task and assignee, and a reassignment voids it.
- `task.mark_branch_merged { change_id, item, integration_commit, reason }` (decision 12): the same callers; never the record's own agent. The office checks that the commit is on the integration branch in `mirror.git`, records `BranchMergedByHand` and marks the record merged. If its head isn't an ancestor of the integration branch, A58 deletes nothing remote: the office pushes `merged/<b>` at the head, removes the local worktree and branch (§4.5's local checks), and leaves the remote branch to a person.

### 4.4 Docs branches (pending U28)
If the user picks the limit: at most one open office-made docs branch per agent for the changes it starts itself; a second change waits or joins it. One open more than a day shows in the lead's Needs you. Docs branches are listed in the work state ("Also waiting: 1 docs branch", PROC-19).

**Reviewer** (spec §4, R3). Project settings name the project's reviewer (an agent). A review request unanswered for 30 minutes starts a one-off reviewer run (`agent-sessions.md`) and tells the leads.

### 4.5 Cleanup (A58)
In `org-git`'s hardened push path, with the push's `contents: write` token. "The head" is the remote's `refs/heads/task/<b>` as last fetched into `mirror.git`.
- **Nothing local is lost.** The office ends any live task session first (a checkpoint turn), unless a guarded move already did. It stops, keeping the worktree, if the HEAD copy holds commits not reachable from the head or the integration branch; `refs/office/task*/<b>` are kept with the record. For an archive, it copies the `work.git` branch into `mirror.git`, pushes it and refreshes the head; if the push is refused (pushed history was rewritten), it keeps the worktree and branch and raises Needs you. A merged record isn't pushed. On either path, local commits beyond the head or uncommitted changes stop the cleanup: the worktree stays and the leads get a Needs you item, as for offboarding (`agent-sessions.md`).
- **Merged:** if the head is an ancestor of the integration branch, push `merged/<b>` at the head, then delete. Otherwise nothing remote is deleted (merged by hand, §4.3), and later commits on the remote raise Needs you.
- **Archived:** push `archive/<b>` at the head, then delete. With no own work and no remote branch, there's nothing to push or tag.
- **The delete is a compare-and-swap**, `git push --force-with-lease=refs/heads/task/<b>:<head> <url> :refs/heads/task/<b>`: it removes the ref only at the checked head and never moves one, so it isn't a force push in A58's sense; if the remote moved, it waits for the next fetch. A tag already at the same commit counts as pushed; at another, the office stops and raises Needs you.
- Then the worktree goes, and the local branch with a compare-and-swap (`update-ref -d refs/heads/task/<b> <copied sha>`). Only `task/*` branches with a record are deleted, never a tag. Tags keep everything forever; only a person removes one.

## 5. Changing processes (PROC-11)
Process files are docs written with W7's write path (office-made `docs/…` branches, the owner as author, the agent as co-author): `doc.new`/`doc.save` with kind `process` for a project, `process.save` for the handbook. Every write checks: valid front matter matching the path; a key unique at that (level, scope) (`already_exists`); no override or `off` of a sealed key (`invalid_request`, reason `sealed`); a private file with no `overrides`, `status: off` or organization key (reason `private_only_adds`; team and project keys are checked at resolution); an agent writing only its own private folder (`not_authorized`); sealing an organization process (`overridable: false`) only by the owner (`not_authorized`, spec decision 3); and `branch?` naming an unmerged office-made branch for that file, opened by the caller.

**Only through the tool.** `doc.render`, `doc new --here`, `doc.set_status`, `doc.rename` and `doc.delete` refuse kind `process`. In the handbook only the office commits: the index follows its last recorded commit, and any other commit is ignored and raised to the owner. In a project repo, a process changed by plain git is shown "from git" and still applies, except that an unapproved `off` of the enforced key doesn't turn the rule off (A72).

**Approval** (spec decisions 8, 10, 11). `process.approve` and `process.reject` name the changed file's doc id and the exact branch commit the approver saw; a moved branch is `conflict`, and a later save voids an approval. The caller is the level's approver, on a trusted binding (decision 15), and none of the change's authors (everyone who saved on the branch), except the owner. *Interim (A62):* an approved handbook branch is merged by the office with a real merge, since nothing else can reach the handbook; if it doesn't merge cleanly, it's `conflict` and re-proposed. A project branch's approval is recorded, and the merge flow merges it (A62). Retro items are approved by the owner's approval of the retro (spec decision 10).

**Version.** Each save writes `version: <base> + 1`; if another change merged first, the branch needs a new save and approval.

**Drift fixes** (PROC-9): **Keep my override** rewrites `overrides` to the parent's version; **Take the new version** sets `status: retired`; **Edit**, **Fix** and **Rename…** are ordinary changes for the level's approver.

**Residual risk.** The handbook deny binds office-run agents only. Hand-run agents (no `run_mode`, no session token) and other same-user programs can still read `handbook.git` and forge hand-run agent identities (ADR-0003, M-A), but can't use lead powers over the rules, which need a trusted binding (decision 15), short of stealing a live token from another process's environment (`agent-sessions.md`, Security). The tampering check limits handbook writes; real isolation is v2.

## 6. The contract (additive)
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

**Project rule.** `project.create` and `project.update` refuse an `integration_branch` starting with `task/`, `docs/`, `merged/` or `archive/` (`invalid_request`), since it's now the trusted source of instructions, processes and merges. (amended 2026-11-08: proto follow-ups) The office reads the integration branch as `refs/heads/<name>`, never through git's short-name lookup, so the name can't reach a tag or another ref. As defence in depth, the prefix is compared ignoring ASCII case and through one leading `refs/heads/` or `heads/`. The office and the app's mocks are to share the rule as `org_proto::has_reserved_branch_prefix`.

**Events.** `process.changed { repo, commit, levels_and_scopes }` (agent scopes only to the owner and that agent); `work.changed` gains `StartRefused` (bookkeeping), `BranchExceptionGranted`, `BranchMergedByHand`, `GuardedMove { by, archived_branch }` and `WorkChange::BranchState { member_id, state }`. Unknown values decode as `Unknown`.

**Trusted binding** (decision 15). From a hand-run lead, `task.allow_second_branch`, `task.mark_branch_merged`, `process.approve`/`reject` and a guarded `task.set_status`/`task.assign` by a non-assignee answer `not_authorized` with reason `forwarded_to_owner`, and change nothing. The office adds a Needs you item for the owner ("<lead> asks to <action> on <task or change>: <reason>") with **Do it** (the owner makes the call) and **Dismiss**.

**Errors.** One new code, **`one_branch_open`**; its message names the branch, task and stage, and `work.branch_state` gives them as data. Reused: `invalid_request`, `already_exists`, `not_found`, `not_authorized`, `conflict`, `unavailable` (reasons `handbook_unreachable`, `repository_busy`, `integration_branch_missing`, `forwarded_to_owner`, `sealed` and `private_only_adds` on `ApiError.reason`, which the agent-sessions contract adds; R1 and R2 depend on that field landing in `org-proto` first).

**Organization data.** `OrgSnapshot.handbook: Option<ProjectRepo>` (`#[serde(default, skip_serializing_if = "Option::is_none")]`, no `path`), set by `OrgChange::HandbookSet { handbook }`. It changes `org-proto`'s tests, the desktop mock and `proto.ts` (not `#[non_exhaustive]`).

## 7. Where the code lives
- **`org-process`** (new; owner TBD by ada): front matter, path → level and scope, `resolve`, the fingerprint, `render`, the checklist merge, `one_branch` (start check, guarded moves, own work from given shas), the write checks. Pure: no SQL, git, network or clock; no ports. Depends on `org-proto`, `org-docs` and `serde`: `Ce = 2` for the proposed internal package graph, or 3 including `serde`; calculate I only after identifying consumers at the same level.
- **`org-office`** (server-dev): the index, caches, project-wide items, last reads, records, exceptions, the calls and the checks inside the work write; one new stable-ward edge, to `org-process`.
- **`org-office-server`** (server-dev): repo reads through `org-git`; the handbook at organization create.
- **`org-git`** (River): W7's `DocRepo` API, the handbook's repository and merge, branch creation at a sha, and the §4.5 cleanup.
- **`org-agent`** (Remy): the PROC-8 commands (Appendix B), `task allow-second-branch`, the `one_branch_open` exit code.
- **`apps/desktop`** (app-frontend-dev): the Processes views, the profile tab, Allow once…, Mark merged…, the work state.

One new library, three stable-ward dependencies, no cycle.

## 8. Phases
| Phase | What | Owners |
| --- | --- | --- |
| **R1** | The contract (§6). `org-process` with its tests (Appendix C). Needs `org-docs`' parser first (A70). | ada; `org-process` owner |
| **R2** | Enforcement: records and the backfill (Appendix A), the start check, guarded moves, branch creation at a sha, `one_branch_open`, refusals, exceptions, merged by hand, `work.branch_state`, cleanup, inbox notices, the CLI. *Interim (A64):* until R3 the rule is on for every project. PROC-14, 15, 19. | server-dev; River; Remy |
| **R3** | The handbook (create, seed, the deny, migration), the index, sets and cache, project-wide conflicts, delivery, the fallback, the read calls, the reviewer setting and its fallback. PROC-2 to 7, PROC-8 reads, 10, 16, 17. | River; server-dev; Remy; ada (the deny) |
| **R4** | Writes, the write checks, approve and reject, the handbook merge, private proposals, versions, drift fixes, checklists and moments, the CLI writes, the app's views. PROC-8 writes, 9, 11, 12, 18. Project merges depend on A62. | server-dev; app-frontend-dev; Remy |
| **R5** | Retro items of kind Process (PROC-13), with retros (W7 P5). Branch-wait insights (spec §4). | as retros |

Each step: its own branch, an independent review, the reviewer's READY, my go, a merge into `v1`.

## 9. Open questions
- **A60** · Team folders: A51 says `teams/<team key>/`, but teams have no key. This design uses the team id, as the spec does. Confirm, or add team keys.
- **A61** · The handbook's record and layout (`OrgSnapshot.handbook` without a path; `workspace/handbook/handbook.git` at HQ). River and server-dev to confirm.
- **A62** · The merge flow (W7 open question 10) also carries process approvals and version bumps. Confirm the interim office merge of handbook branches (§5).
- **A63** · Branch stages come from the merge flow; until then open or merged only (§4.1). Acceptable for R2?
- **A64** · The rule is judged in the target task's project: on unless an approved `off` applies there (§4.2). If U26 splits `branch-and-merge`, it binds to `one-open-branch`.
- **A65** · Closed by decision 11 (reassignment).
- **A66** · Phase 2: do satellites compute their agents' sets, or does HQ send them? And records across offices.
- **A67** · The checks inside the work write: the office calls `org_process::one_branch` in `WorkStore::transact`'s closure, or `org-work` owns it with "rule on" as an input. server-dev and the `org-work` owner pick.
- **A68** · Two teams with byte-identical overrides of one key: still a conflict (this design), or not?
- **A69** · Can a project's leads read members' private processes, or only the owner and the agent (this design)?
- **A70** · Build `org-docs`' front-matter parser before R1, or let R1 wait?
- **A71** · Two teams of a shared project seal different versions of one key. *Interim:* a conflict, with the organization's version applying, else the first seal by team id that isn't an `off`, until the project settles it. (One seal is decision 13.)
- **A72** · Project process changes made with plain git still apply, shown "from git", except an unapproved `off` of the enforced key. Keep that, or ignore every unapproved change?

For the user (spec): U25, U26, U27, U28.

## Appendix A. Migration
- **R2 backfill.** Records come from each task's `branch`: `base_sha` is its merge base with the integration branch, `own_tip` its tip when not in the integration branch. Merged when the tip is in the integration branch and the task is Done · Completed; archived (with its tag) for a cancelled or deleted task, or a closed one with no own work; open otherwise. An existing `integration_branch` with a reserved prefix goes to the owner's Needs you. A squash-merged Done task shows as stuck, for **Mark merged…**.
- **This repository's `branch-and-merge.md`** has `level: organization` in a project repo, a stand-in until the handbook exists. R3 moves the generic text into the handbook seed, keeps ours (`v1`, ada as lead) as a project override, and updates `AGENTS.md` rule 2's link.

## Appendix B. The CLI (PROC-8)
`process list` is `process.effective` (`--level`: `process.view`); `why`, `show`, `ack`, `approve` and `reject` are their calls. `new`, `override`, `off`, `edit`, `rename`, `keep` and `take-parent` build the content and send it with `process.save` (handbook) or `doc.save` (project). `new --private "<name>"` slugs the key from the name, with a clash check, as for doc keys. `task allow-second-branch` and `task mark-merged` are the work calls.

## Appendix C. Tests that prove the rules
Each test must fail without its rule.

**`org-process`, table-driven:**
- Matching by key, never `id`; level order; nearer team wins; no current work gives O ⊕ G.
- Seals: an org seal refuses lower overrides and `off` without a conflict; a team seal refuses its sub-teams' and projects'; on a shared project, a sibling's override is a conflict with the sealed version applying, and a project version settles it; two team seals give the A71 interim; a clashed sealed key stays sealed.
- Conflicts: two team overrides give the org version (or none); one team's version applies; a project version settles it; a parent-and-sub-team link or a diamond isn't a conflict.
- Turned off, private only adds, validity, drift, the fingerprint, the render cap and order, the checklist merge.
- One branch: another task's record blocks, its own and docs branches don't; an exception is used once; only an approved `off` turns the rule off; can't compute counts as on. Own work: a tip at `base_sha` or in the integration branch is none; a commit beyond it, or uncommitted changes, is some.

**Office and store:**
- Concurrency: of two starts or branch creations for one agent, exactly one succeeds; a failed `git worktree add` leaves no reservation and restores the exception.
- Records: one own commit sets `own_tip` (its own `refs/office/task/<b>` copy doesn't hide it); a fast-forward or reset onto another branch doesn't; a `--no-ff` merge closes it, also after a delete; pushes after a merge or without a record are refused; reused names get a suffix; an idle record reaches Needs you.
- Guarded moves with own unmerged work: the assignee's `task done --cancelled`, `task done` without a push, `task assign` and `task assign --none` are `one_branch_open`; by another lead or the owner they archive and free the agent; with no own work they go through.
- Merged by hand: by the record's agent, `not_authorized`; by a lead with a commit not on the integration branch, `invalid_request`; otherwise recorded and merged, with a `merged/` tag and the remote branch kept.
- Bindings (decision 15): a hand-run lead's `allow_second_branch`, `mark_branch_merged`, guarded move or `approve` answers `forwarded_to_owner`, changes nothing and kills no session, and the owner's Needs you item appears; a token-bound lead's is accepted.
- Guarded-move copies: an unpushed commit on the branch with HEAD detached at `base_sha` is own work, and so is a branch at `base_sha` with an own commit only on a detached HEAD or another checked-out branch; a merged `own_tip` with a later local commit stays guarded; a failed copy counts as own work; an accepted Done ends the session only after the answer; a refused Done keeps the session and clears closing; a change between judgement and write is `conflict`; a fetch showing `own_tip` merged marks the record merged.
- Branch creation: a planted local `task/<name>` ref gets a suffix, and one planted between the name check and the add makes the add fail, not overwrite; a moved integration ref in `work.git` doesn't break the fetch of the base sha.
- Permissions: a guarded move by a peer who isn't a lead, or a lead's Allow once… for their own task, is `not_authorized` (the owner's isn't); an exception is void after a reassignment; a push during a guarded move is refused, and the move judges the pushed work.
- Start point (decision 14): moving the integration ref in `work.git` doesn't change a new branch's parent or `base_sha`; a missing `integration_branch` is `unavailable`; `project.create` and `project.update` refuse each reserved prefix.
- Cleanup: tag before delete; no delete if the head isn't in the integration branch or the remote moved; unpushed or uncommitted work stops it; a tag at another commit stops it; a reset before a guarded move still counts as own work; a refused archive push, or uncommitted changes, keep the worktree; never the default or integration branch, an unrecorded branch, or a tag.
- Approval: a co-saver can't approve; the owner approves their own change; a later save makes approve `conflict`; handbook commits the office didn't make are ignored; the refused `doc.*` calls refuse kind `process`; only the owner seals at organization level; a private file using an organization key is `private_only_adds`; `process new --private` checks its slug key for a clash.
- Reads and delivery: no working branch or `work.git` ref reaches a set or the A57 sources; private reads as in §6, and agent tools denied `workspace/handbook/**`; `task start` returns the section; a merged change is sent once; agent leads get inbox items once; an unreadable handbook gives `stale_since` and the last set, and enforcement still runs.
