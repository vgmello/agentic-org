# Design: processes (how agents work, written down and inherited)

Status: **proposed, rev 1** 2026-11-08. Owner: ada. It implements Eden's spec [`processes-ux.md`](../specs/processes-ux.md) rev 2 (PROC-1 to PROC-19, on `v1` since adc6968) and doesn't redefine its UX. It builds on [`tasks-projects-docs.md`](tasks-projects-docs.md) (W7: docs as repository files, the doc index, stable doc ids, the docs write path), [`agent-sessions.md`](agent-sessions.md) (how the office composes instructions, what agents can reach), [`projects-git-github.md`](projects-git-github.md) (worktrees, task branches, git hardening) and [`teams-and-org-chart.md`](teams-and-org-chart.md) (nested teams, project-team links). The first process is [`docs/processes/branch-and-merge.md`](../processes/branch-and-merge.md); its front matter is the reference shape.

## In one paragraph
A process is a Markdown doc with front matter (kind `process`), kept in a repository and changed through the merge flow. Organization, team and private processes live in the organization's **handbook repo**; project processes live in the project's repo. HQ keeps an **index** of them, never their text. For an agent working on a project, the office **resolves** one effective set from merged content only (organization → team chain(s) → project → agent), matched on the `process:` key, the lowest level winning unless a level above sealed it. It caches the result by the commits involved and gives it to the agent at session start and wake. The office also **enforces** the first process's key rule: an agent with an unmerged task branch can't start another task (`one_branch_open`), unless a lead or the owner allows it once. A new pure library, **`org-process`**, holds the parsing, the resolution and the rule; the office does the I/O.

## Decisions
All decided 2026-11-08 (ada, with the user's brief), except A55's docs-branch limit, which waits on the user (U28).
1. **A51 Storage.** Everything persistent lives in repositories. Project processes go in the project repo's `docs/processes/`. Organization and team processes go in the organization's handbook repo (`processes/`, `teams/<team key>/processes/`), which is created with the organization and changed through the same branch-and-merge flow. HQ keeps only an **index** (process key, level, scope, version, path, commit sha, status, overridable or sealed, overrides), rebuilt from the repos and never holding the text. *Teams have no key today (`Team { id, name, … }`), so this design writes `teams/<team id>/` until A60 is settled.*
2. **A52 Private processes** live in the handbook at `agents/<member id>/processes/`. The organization owns them and the owner can see them. They survive offboarding as history. The agent changes them through the merge flow, and the owner approves.
3. **A53 Resolution.** The office computes the effective set per (agent, project) from **default or integration branches only**: merged content, never a working branch. Order: organization → team(s) → project → agent. Processes match on the `process:` key, the lowest level wins, and a **sealed** process can't be overridden. The result is cached, keyed by the set of commit shas involved, and pushed at session start and wake. `process why <key>` reads the same result and shows each layer.
4. **Multi-team conflict.** If two teams both override the same key for one shared project, and the project doesn't resolve it, the agent follows the **organization's** version. If the organization has none, neither override applies. The conflict shows in Needs you for the project's lead and the owner. Work isn't blocked.
5. **Identity.** A process keeps the W7 doc identity rule (`tasks-projects-docs.md` §4.9: a number or `doc-<ULID>`, assigned at creation or first save through the tool). The **match key** is separate: `process: <key>`, unique per (level, scope). Overrides match on it. A hand-written `id:` is never used as the match key.
6. **A54 Enforcement.** The office refuses `task start` and the creation of a new task branch while the agent has an open task branch that isn't merged. The office knows this from the merge flow. The error is `one_branch_open`, naming that branch. The lead or the owner can grant a one-time exception, recorded on the task. Hotfixes follow the same rule.
7. **A55 Which branches count.** Only the task branch blocks a task start. Office-opened docs, process and retro branches don't. *Pending the user's answer (U28):* at most **one** open docs branch per agent (a second doc change waits or joins it), and one open longer than a day shows in the lead's Needs you.
8. **A56 Base branch.** Task branches start from the project's `integration_branch` (the default branch when that's empty; `Project.integration_branch` in `org_proto::org`). No new setting. `projects-git-github.md` is amended to match.
9. **A57 One source for a project's guidance.** The office reads a project's instruction files, its guidance files (`docs/.guidance/<moment>.md`) and its project processes all from the project's `integration_branch` (the default branch when that's empty), so all three follow merged work. `agent-sessions.md` decision 8 is amended to match.
10. **A58 Branch cleanup.** After a merge, the office deletes the remote task branch only if its head is an ancestor of the project's `integration_branch` (checked on the mirror), after pushing a `merged/<branch>` tag at that head. A task closed without a merge gets an `archive/<branch>` tag pushed first, then the branch is deleted. The office only ever deletes `task/*` branches it created: never the default or integration branch, never a force push, never a tag. The local worktree and branch are removed too. It uses the same `contents: write` token as today's push. `projects-git-github.md` is amended to match.

## 1. Where each thing lives
| Thing | Where | Changed by |
| --- | --- | --- |
| Organization processes | handbook, `processes/<key>.md` | docs branch → owner's approval → merge (PROC-11) |
| Team processes | handbook, `teams/<team id>/processes/<key>.md` | docs branch → team lead or owner → merge |
| Project processes | project repo, `docs/processes/<key>.md` | docs branch → a project lead or owner → merge |
| Private processes | handbook, `agents/<member id>/processes/<key>.md` | docs branch → owner → merge (A52) |
| Templates | handbook `.templates/process.md`; project `docs/.templates/process.md` | like any template (W7 §4.9) |
| Process index | HQ's `office.db`, a cache | rebuilt from git after each fetch or merge |
| Effective sets | `office.db` of the office running the agent, a cache | recomputed when an input changes (§3.3) |
| What each agent last read, per (agent, key) | `office.db` | the office, after delivery and on `process ack` |
| Task branch records, exceptions | `office.db` (work data) | the office (§4) |
| The handbook's record | organization data: `OrgSnapshot.handbook` | HQ, at organization create |

**The handbook repo.** HQ's office server creates it when it creates the organization, and at first start for an organization made before this lands. In phase 1 it's one office-only bare repository at HQ, `workspace/handbook/handbook.git` (local git; GitHub later, `projects-git-github.md`). Its first commit holds `README.md`, `.templates/process.md` and `processes/branch-and-merge.md`, on by default (spec §4). Only the office reads and writes it: writes go on office-made `docs/…` branches with the W7 write path (intents, plumbing, hardening). Agents never get a worktree or a read of it: they read processes through the office (§6).
- **Required change to `agent-sessions.md`** ("What agents can reach under `<office home>`"): a fixed read and write deny on `offices/*/workspace/handbook/**`. Without it, any agent at HQ could read every member's private processes with git from Bash. R3 doesn't ship without it.
- Its record carries no path: `path` stays empty, so the location isn't published organization-wide the way a project's local `repo.path` is.

**This repository's `docs/processes/branch-and-merge.md`** sits in a project repo with `level: organization`, a stand-in until the handbook exists (its own note says so). Once R3 reads project processes it would show as invalid (a level that doesn't match its path), so R3 moves it: the generic text seeds the handbook, and this repo keeps only what's specific to it (`v1`, ada as lead) as a project override or plain doc.

## 2. The model

### 2.1 The process file (PROC-1)
Front matter as in the spec §1 and `branch-and-merge.md`:

| Field | Rule |
| --- | --- |
| `id` | the doc id (W7 §4.9). Assigned by the office; never used for matching |
| `process` | the match key: 1–64 of `[a-z0-9-]`, starting with a letter or digit. Unique per (level, scope) |
| `kind` | `process` |
| `name`, `summary` | one line each; `summary` ≤ 200 characters (it's sent at every start) |
| `level`, `scope` | must match the file's location (§2.2). A mismatch is invalid front matter |
| `version` | positive integer, bumped by each merged change (§5) |
| `owner`, `applies_to` | free text, shown as is |
| `overridable` | `true` (default) or `false` (sealed) |
| `status` | `draft`, `active`, `off` (turned off at this level, needs `reason`), `retired`. Only `active` and `off` take part in resolution |
| `overrides` | `<key>@<version>`: the key and the parent version this override was written against. Used for drift only |
| `moments` | optional: `starting_task`, `waking_up`, `finishing_task`, one prompt each, ≤ 500 characters |
| `checklist` | optional: `[{ text, required }]`, at most 50 steps |

Parsing uses W7's front-matter rules (the YAML subset, depth and size limits, one pinned parser in `org-docs`). Reads never fail on invalid front matter: the file is listed with a problem naming the field, its body still renders, and it's left out of every effective set until fixed (PROC-16).

### 2.2 Levels, scopes and paths
The location decides the level and scope. Front matter must agree.

| Level | Scope | Repo, branch, folder |
| --- | --- | --- |
| `organization` | none | handbook, default branch, `processes/` |
| `team` | team id | handbook, default branch, `teams/<team id>/processes/` |
| `project` | project key | project repo, `integration_branch` (else default), `docs/processes/` |
| `agent` | member id | handbook, default branch, `agents/<member id>/processes/` |

Only `*.md` files directly in those folders count. The tool names a new file `<key>.md`, but the key in the front matter is what matches, not the file name (so a clash can exist, PROC-1). **Reads never use `work.git`**, whose refs agents can move: project files are read in the project's `mirror.git` at the branch as the office fetched it (W7 §4.9's "reads only in mirror.git"), and handbook files in the office-only `handbook.git`. The handbook has no `work.git`, so W7's sync step is skipped there and `process.save` answers `synced: true`. Paths follow W7's path rules (no symlinks or submodules along the path, case clashes refused), except that the handbook's allowed roots are `processes/`, `teams/`, `agents/`, `.templates/` and `README.md` instead of `docs/`. W7's id reservations (`doc_ids`, keyed by project and path) get a repository key, so the handbook has its own.

### 2.3 The index (HQ)
One row per file on a source branch, holding A51's fields plus what's needed to find and check the file: `{ repo (handbook | project id), path, commit, blob, doc id, key, level, scope, version, status, overridable, overrides, problems }`. No text: names and summaries are read from the blob when a set or a view is built. Old rows are kept as a history (key, level, scope, version → commit), so a drift diff can read the parent at the version an override names (PROC-9). HQ rebuilds the handbook part after each change to the handbook's default branch, and a project's part after each fetch or merge that moves its integration branch. Like W7's doc index, it's a cache and never the source of truth. While the handbook can't be read, reads answer from the last index with `stale_since` (PROC-16: "Showing the last copy from <time>").

## 3. Resolution (A53)

### 3.1 Inputs
For (agent A, project P):
- **O**: the organization's processes.
- **T**: for each team linked to P, its chain from the outermost parent team down to that team (`owner_team_id` links). A linked team that is an ancestor of another linked team is dropped (its files are already in that chain). A shared project has several chains.
- **J**: P's project processes, from P's integration branch (A57).
- **G**: A's private processes.

With no current work (no current task, no task session), the set is **O ⊕ G** (PROC-2). The home session uses the agent's current task (`work.set_current`) when it has one. A task session uses its task's project. A project with no repository has an empty J.

### 3.2 Rules, in order (`org_process::resolve`, pure)
1. **Valid files only.** Invalid front matter, a level or scope mismatch, `draft` or `retired`: left out, with a problem.
2. **Key clash.** Two files with the same key at one (level, scope): both are left out, with the clash problem (PROC-1, "Neither applies until one is renamed"). Seals fail closed: if a file left out for invalid front matter, a level or scope mismatch or a key clash is sealed (or can't be read far enough to tell), the key stays sealed for the levels below. `draft` and `retired` files never seal.
3. **Seals.** A file that overrides or turns off a key sealed by the organization, by a team above it in its own chain, or (for a project file) by any team in the project's chains, is not applied and is recorded as refused, with the sealing level (PROC-3, spec decision 3). This runs before the rules below, so a refused file never makes a conflict. The write path refuses it too (§5); this catches files merged with plain git.
4. **Fold each team chain**, outermost first, the nearer team winning.
5. **Combine the chains** (decision 4, PROC-6). Per key, only **team-level** winners count (a chain that only inherits the organization's file has none). If exactly one distinct team file wins across the chains (an override, an addition or `off`), it applies. If two or more distinct team files win (by repository and path), it's a **conflict**: the organization's version applies, or nothing if the organization has none. The conflict is recorded. A team's seal on a shared project is A71.
6. **Project.** J's version of a key wins over the team and organization result, unless the key is sealed above (rule 3). This also resolves a conflict on that key.
7. **Turned off.** An `off` that survives removes the key from the set and records "Turned off for <team or project name>: <reason>" (PROC-4).
8. **Private only adds** (PROC-5). A private file isn't applied, and is flagged on the agent's profile, if it sets `overrides` or `status: off`, or if its key appears in **any** O, T or J file of this project, whatever that file's state (so a private file can't settle a conflict or fill a clash for one agent).
9. **Drift** (PROC-9). An override whose `overrides: key@v` is below the parent's current version is marked "Parent changed". An override without `overrides` still applies, and its drift shows as unknown.

**Output:** `EffectiveSet { inputs, entries, turned_off, conflicts, problems }`. Each entry has the winning file (key, name, summary, version, level, scope, repo, path, commit, blob, `kind`: inherited, override or addition), and its **chain**: every layer that had the key, with what happened to it (applied, overridden, turned off, sealed, refused, conflict, clash, invalid). `process why` prints the chain (PROC-8).

### 3.3 Caching and project-wide checks
The office caches each set under a fingerprint of everything it read: the handbook commit, the project's integration commit, and the team chains (the ordered team ids with each team's link and parent, at the org position read). Teams are organization data, not repository content, so they're part of the key. A change to any input drops the sets that used it. Agents' sets are recomputed lazily, at the next start, wake or read. The last good set per (agent, project) is kept with its time, for the fallback below.

**Conflicts belong to the project**, not to an agent. After every index or org change, HQ resolves O ⊕ T ⊕ J for each affected project (no agent layer) and keeps one Needs you item per (project, key) for the project's leads and the owner, cleared when the conflict is gone. Clashes, invalid files and drift are kept the same way, for the Processes views and the level's approvers.

**Agents who are leads or approvers** (spec decision 11) get each of these items, and every other Needs you item in this design (a refused start, a change waiting for approval, a drifting override, a docs branch open too long), as an office notice in their inbox (ADR-0003), waking them if on call, as hire approval does. The notice carries what the CLI needs: the task ref, or the changed file's doc id, level, scope and diff. The office sends it once per item, and again only when the item changes.

**Can't compute** (PROC-17): the office uses the last good set and tells the agent "Couldn't load your processes; using the last set from <time>". After an hour it adds a warning to the owner's Needs you. With no earlier set, it says "Couldn't load your processes, and there's no earlier copy." and warns the owner at once. The one-open-branch rule is then treated as on (§4). An `integration_branch` that doesn't exist in the repository counts as can't compute.

### 3.4 Delivery to sessions (PROC-7, PROC-12)
The effective set is one more input to the composed instructions (`agent-sessions.md` decision 8). `org_process::render` builds a **Your processes** section:
- the set: each process's name, level, version and summary, plus turned-off ones and any conflict, in the spec §3 wording;
- **changed since last read**: the full text of each process whose winning file (its blob) isn't among the blobs the agent has read for that key. So a new version, a new winner at another level (an override added or retired) or a conflict's fallback all count as changed, and moving between projects where different files win doesn't repeat texts already read;
- each process's `moments` prompt, after that moment's guidance text, only while the moment is On (Settings › Agent guidance).

**Size cap: 24 KiB** for the section. Summaries go first; if they alone pass the cap, the list is cut with "and N more: run `org-agent process list`". Changed texts follow by level, organization first, while they fit; the rest are named with "run `org-agent process show <key>`". On a start it's part of the instruction file; on a wake it's part of the wake prompt (a resumed session gets changed instructions as a message, `agent-sessions.md` decision 5). The **Starting a task** moment also fires on `org-agent task start` (`decisions-and-tech-debt-ux.md` §6.2), so `task start`'s result carries the same section for the task's project. **Finishing a task** prompts come with `org-agent task done`'s result. Prompts count toward the cap. A change merged mid-run waits for the next start or wake (spec decision 8). The office records the texts as read only once the runtime has accepted that start or wake, so a failed start doesn't lose them. `process ack` records a read too.

**Checklists** (PROC-12): when a task is created in P, the office asks `org-process` for the steps of P's processes (O ⊕ T ⊕ J, never G) and passes them to `task.create` after the project's default checklist, duplicates by text merged, required winning. A later assignment doesn't change it.

## 4. Enforcement: one open task branch (A54, A55)

### 4.1 Task branch records
The office keeps one record per task branch: `{ task_id, member_id (the task's assignee), project_id, branch, base_branch, base_sha?, own_tip?, state: reserved | open | merged | archived, stage?, opened_at, closed_at? }`.
- **Reserved** at `task start` (before any git), **open** once the branch exists, with `base_branch` (where it started: the integration branch, or a branch the task names) and `base_sha`. A project with no repository has no branches, so nothing is reserved there.
- **`own_tip`** is the latest tip the office has seen on the branch (through `org-agent push`, or at a fetch of the remote's `refs/heads/task/*` into `mirror.git`) that, when seen, descends from `base_sha` and is reachable from no other ref in `mirror.git` (the integration branch, another `task/*` or `docs/*` branch, `refs/office/*`): the branch's own work. A tip that's only a fast-forward or reset to the integration branch, or to someone else's branch, never sets it.
- **Merged** when the merge flow records the merge. Until that flow is built, the office decides it after each fetch, in **`mirror.git` only**: `own_tip` is set and is now reachable from the **integration branch** (never from a `task/*` or `docs/*` ref, whatever the branch started from). A branch with no work of its own stays open, and moving refs in `work.git` changes nothing. `own_tip` stays checkable after the branch is deleted. Squash and rebase merges aren't seen, so the merge flow must use real merges, as W7 requires for docs branches (A63). Once merged, the office refuses further pushes to that branch: more work on the task needs a new branch (`task/<ref>-<slug>-2`), checked like any other.
- **Pushes need a record.** `org-agent push` accepts a `task/*` branch only when the caller holds its open record, so a branch an agent makes by hand in `work.git` can't be pushed around the check (a follow-up amendment to `projects-git-github.md`'s push rule).
- **Archived** when the task closes without a merge (Done · Cancelled, or deleted, with the branch unmerged): §4.5. A record with no `own_tip` (no work of its own) is archived when its task closes in any way, Done · Completed included, so it never blocks for good.
- **Released** (the record is removed): a reservation whose task leaves In progress, closes, is deleted or is reassigned before its branch exists. If `git worktree add` fails, the reservation is removed and an exception it used is restored, in one transaction.
- **Reassignment** moves an open record to the new assignee. *Interim (A65):* a reassignment that would give the new assignee a second open record is refused (`one_branch_open`) unless the caller is one of the project's leads, and isn't the new assignee, or is the owner. They get a Needs you item when they do it. So an agent can't hand its open task away, start another and take the first one back.
- `stage` (waiting for review, for the go, to merge) comes from the merge flow. Until then the work state shows only open or merged (A63).
- **R2 backfill.** Records are made for existing task branches from each task's `branch`: `own_tip` is the branch tip when that isn't in the integration branch. Then the usual rules apply: **merged** when the tip is in the integration branch and the task is Done · Completed, **archived** for a cancelled or deleted task or a closed one with no `own_tip` (§4.5), and **open** otherwise (In progress or Blocked, or Done with its own work unmerged). Docs branches are tracked separately and never block (A55).

### 4.2 The check (`org_process::one_branch::check`, pure)
Inputs: the agent's reserved and open task records, the target task, the exception on it, and whether the rule is on. Answer: allowed, or `OneBranchOpen { branch, task, stage }`.
- **The rule is on** for the target task's project unless a valid `off` of the enforced key (`branch-and-merge`) survives resolution there, and the commit that made it was **approved** (§5: a non-author approver's `process.approve` recorded at that commit). A missing, clashed or invalid enforced key leaves it on, and so does a set that can't be computed (A64, A72).
- **Blocked** when the agent has a reserved or open record for **another** task. Its own task's record never blocks it (a task back from Blocked carries on).
- **An exception** on the target task allows one record to be created, and is used up when it is.

**Where it runs.** At the two points A54 names, each inside one write transaction with the record it creates (one writer per database, ADR-0002; A67):
1. `task.set_status { in_progress }` by the assignee agent (`org-agent task start`), when the task has no record yet: check, then **reserve** the record in the same transaction. So two starts can't both pass.
2. the creation of a task branch and worktree, whoever triggered it (an app move to In progress, a task session start, a hotfix): check and reserve (or reuse the task's own reservation) in one transaction, then run `git worktree add`. If git fails, the reservation is removed. A refused creation ends that session's run as `blocked`.

A refusal is `one_branch_open` with the spec §4 wording: "Your branch task/chk-T-17-… isn't merged yet (waiting for review). Finish it before starting chk-T-18." The office keeps one open **refusal** per (agent, target task), a bookkeeping event like W7's `WaitingStarted` that doesn't change the task's version. It puts the refusal in the project leads' and the owner's Needs you (an agent lead's inbox, §3.3) with **Allow once…** and **Open chk-T-17**, until the branch merges or an exception is granted (PROC-14).

### 4.3 The one-time exception
`task.allow_second_branch { change_id, item, reason }`, from the app or `org-agent task allow-second-branch <task> --reason "…"`. The caller must be a lead of one of the project's teams, or the owner (`not_authorized` otherwise; a project with no lead: the owner only). No one grants it for their own task (PROC-11): it goes to another of the project's leads or the owner. The owner is the exception (spec decision 8). The reason is required (1–1,000 characters). It's recorded on the task as a `BranchExceptionGranted { reason, open_branch, assignee }` event with the actor, and covers one record for that task and that assignee. A reassignment voids it.

### 4.4 Docs branches (pending U28)
If the user picks the limit: an agent has at most one open office-made docs branch for the doc and process changes it starts itself. A second change waits or joins it. A docs branch open more than a day shows in the lead's Needs you. With no limit, only the Needs you warning applies. Either way, docs branches are listed in the work state ("Also waiting: 1 docs branch", PROC-19).

### 4.5 Cleanup (A58)
The office cleans up a task branch in `org-git`'s hardened push path, with the same `contents: write` token as a push. "The head" is the remote's `refs/heads/task/…` as last fetched into `mirror.git`.
- **First, nothing local is lost.** The office copies the `work.git` branch into `mirror.git` (as a push does) and pushes it. If the local tip isn't the remote head after that, or the worktree has uncommitted changes, it stops and raises Needs you for the project's leads.
- **Merged** (§4.1): if the head is an ancestor of the integration branch in `mirror.git`, it pushes a `merged/<branch>` tag at that head, then deletes the remote branch. Otherwise (work was added after the merge) it deletes nothing and raises Needs you.
- **Closed without a merge** (§4.1, Archived): it pushes an `archive/<branch>` tag at the head first, then deletes the branch.
- **The delete is a compare-and-swap** on the checked head (an expected old value, not a force push). If the remote moved since the check, it deletes nothing and starts over at the next fetch. A tag that already exists at the same commit counts as pushed; at another commit, the office stops and raises Needs you.
- Then it removes the local worktree and branch. It deletes only `task/*` branches it created (those with a record), never the default or integration branch, never with a force push, and never a tag. A failed step is retried; nothing is deleted before its tag is pushed.

## 5. Changing processes (PROC-11)
Process files are docs, written with W7's write path: office-made `docs/<slug>-<short id>` branches, intents, plumbing, hardening, the owner as author and the agent as co-author. Project processes use `doc.new`/`doc.save` with kind `process`. Handbook files use `process.save` (§6), the same path pointed at the handbook. On every write the office validates:
- the front matter (§2.1) and that level and scope match the path;
- the key is unique at that (level, scope), `already_exists` otherwise;
- not an override or `off` of a sealed key, `invalid_request` (reason `sealed`): "Branch, review and merge is sealed by the organization. You can add a process instead.";
- a private file sets no `overrides` or `status: off` and doesn't use an organization key, `invalid_request` (reason `private_only_adds`). Team and project keys are checked at resolution (§3.2 rule 8), per project;
- an agent writes only its own private folder (`not_authorized` for another member's);
- `branch?`, if given, is an unmerged office-made `docs/…` branch of the same repository for that file, opened by the caller (`invalid_request` otherwise). The same holds for `doc.save` of kind `process`.

**Only through the tool.** `doc.render`, `doc new --here`, `doc.set_status`, `doc.rename` and `doc.delete` refuse kind `process`, so every process change goes through `doc.save` or `process.save` and its approver, never on a task branch or a status branch. A project process file whose last change on the integration branch isn't an approved commit is shown "not approved". In the **handbook** only the office commits: the index follows the last commit the office recorded on its default branch, and any other commit there is ignored and raised to the owner as tampering. In a **project repo**, a process file changed by a commit the office didn't make (plain git) is indexed and shown "from git", as W7 does. It still applies, except that an `off` of the enforced key from git doesn't turn the rule off (§4.2, A72).

**Approval** (PROC-11, spec decisions 8 and 11). `process.approve { change_id, doc_id, expected_commit }` and `process.reject { change_id, doc_id, expected_commit, reason }` name the changed file by its doc id (the same key and version can wait at two levels) and the exact branch commit the approver saw. A branch that has moved since is `conflict`, and any later save voids an earlier approval. The caller must be that level's approver (spec decision 8) and not one of the change's authors, meaning every member who saved on that branch (not the git author, who is always the owner, W7 §4.9); otherwise `not_authorized`. The owner is the exception and approves their own. Once a handbook branch is approved, the office merges it into the handbook's default branch itself, with a real merge, since nothing else can reach the handbook; if it doesn't merge cleanly, the answer is `conflict` and the change is proposed again. A project process branch's approval is recorded, and the project's merge flow merges it (a person until A62). A rejection closes the branch with the reason. A private process is "Waiting for the owner's approval" until merged, and isn't in any set until then.

**Version.** The office writes `version: <base> + 1` on the branch at each save. If another change to the same file merged first, the branch needs a new save, and so a new approval, before it can merge: what merges is always what was approved. For a project branch the merge flow enforces this (A62).

**Residual risk.** The handbook deny (§1) binds office-run agents. Agents started by hand (`bin/agent`) and other programs running as the same macOS user can still read `handbook.git`, and can bind as any agent (ADR-0003, M-A for hand-run agents), so they could read private processes or approve and grant as a lead. The tampering check limits handbook writes. Real isolation is v2, as for the rest of the office (`projects-git-github.md`).

**Drift and other fixes** (PROC-9): **Keep my override** rewrites `overrides` to the parent's current version; **Take the new version** retires the override (`status: retired`); **Edit** and **Fix** open it; **Rename…** changes the key. Each is a change like any other, waiting for the level's approver.

## 6. The contract: `org_proto::process` (additive)
A new module, `crates/org-proto/src/process.rs`. `API_VERSION` stays 1. Calls go to the caller's own office on a bound connection. Organization, team and project processes are readable by any bound member. **Private processes** are readable only by their agent and the owner (A52, A69). So `process.effective`, `process.why` and `process.show` for another member answer the owner in full, and anyone else with the private layer and its `last_read` entries left out. `process.show { level: agent }` of another member's file (any `version`) and `process.view { agent }` of another member are `not_authorized` except for the owner.

`For` selects the work a set is for: `current` (the agent's current work, the default) · `none` (O ⊕ G) · `project { project_id }` · `item { item }`.

| Call | Params | Result | Errors |
| --- | --- | --- | --- |
| `process.effective` | `member_id?` (an agent reads its own), `for?: For` | `{ set: EffectiveSet, last_read: Vec<{ key, blob, version, at }>, computed_at, stale_since? }` | `not_found`, `not_authorized`, `unavailable` |
| `process.why` | `key`, `member_id?`, `for?` | `{ key, chain: Vec<Layer>, winner? }` | as above |
| `process.show` | `key`, `member_id?`, `for?`, or `level` + `scope?` (+ `version?`, from the history) for one file; `offset?` | `{ path, repo, commit, blob, version, content, total_bytes, next_offset? }` (content ≤ 512 KiB in JSON) | as above |
| `process.view` | `scope: organization \| team { team_id } \| project { project_id } \| agent { member_id }` | `{ rows: Vec<ProcessRow>, indexed_commit, stale_since? }`: the inheritance view (PROC-9, PROC-10), with pending branches ("Waiting to merge · v4 proposed by ada") | `not_found`, `not_authorized`, `unavailable` |
| `process.ack` | `key`, `version` | `Empty` | `not_found` |
| `process.save` | `change_id`, `level`, `scope?`, `key`, `content`, `expected_blob?`, `message?`, `branch?` | `{ path, branch, commit, blob, pushed, synced }` | `invalid_request` (front matter, `sealed`, `private_only_adds`), `already_exists`, `not_authorized`, `conflict`, `unavailable` |
| `process.approve` | `change_id`, `doc_id`, `expected_commit` | `{ branch, merged: bool, commit? }` (`merged` for a handbook branch, §5) | `not_found`, `not_authorized` (not the level's approver, or the change's author), `conflict`, `unavailable` |
| `process.reject` | `change_id`, `doc_id`, `expected_commit`, `reason` (1–1,000) | `Empty` | as above |
| `task.allow_second_branch` (in `org_proto::work`) | `change_id`, `item` (W7 §4.1: agents may type it), `reason` | `WorkAccepted` | `not_found`, `not_authorized`, `invalid_request` |
| `work.branch_state` (in `org_proto::work`) | `member_id?` | `{ task_branch?: { task, project_id, branch, state, stage? }, docs_branches: Vec<…> }` (PROC-15, PROC-19) | `not_found` |

**The CLI** (PROC-8) maps onto these calls: `process list` is `process.effective`, and `process list --level` is `process.view`. `process new|override|off|edit|rename|keep|take-parent` build the file's new content (from the template, `process.show` of the inherited text, or the current file) and send it with `process.save` for a handbook level or `doc.save` for a project. `process new --private` is `process.save` at the agent level. `process approve|reject <doc id>` are the calls above. `ProcessRow` carries the key, name, per-level versions, the winning level, the kind (override, addition, sealed, off), the drift, conflicts and problems, so the app needs no second read for a row.

**Events.**
- `process.changed { repo, commit, levels_and_scopes }`: the index moved. Clients re-read the views they show. Agent scopes are sent only to the owner and to that agent.
- `work.changed` gains the events `StartRefused { open_branch, task_ref }` (bookkeeping, §4.2) and `BranchExceptionGranted { reason, open_branch }`, and a `WorkChange::BranchState { member_id, state }` for the work state. Unknown values decode as `Unknown`.

**Error codes.** One new code, **`one_branch_open`** (`ErrorCode::OneBranchOpen`). The message names the branch, its task and its stage. The app and the CLI branch on it (**Allow once…**), so it can't hide in `invalid_request`. A client that needs the branch as data reads `work.branch_state`. Reused: `invalid_request`, `already_exists`, `not_found`, `not_authorized`, `conflict`, `unavailable` (reasons `handbook_unreachable`, `repository_busy`, `integration_branch_missing`). The reasons ride on `ApiError.reason`, which the agent-sessions contract adds (S2); this contract depends on it.

**Organization data.** `OrgSnapshot` gains `handbook: Option<ProjectRepo>` with `#[serde(default, skip_serializing_if = "Option::is_none")]` (`path` always empty, §1). It's set and changed by an org change, `OrgChange::HandbookSet { handbook }`, so live clients and later satellites learn it. `OrgSnapshot` isn't `#[non_exhaustive]`, so the struct literals in `org-proto`'s tests, the desktop mock (`apps/desktop/src-tauri/src/org_mock.rs`) and `proto.ts` change with it.

## 7. Where the code lives
```
apps/org-agent ─────────▶ crates/org-client ───────────────────────────┐
apps/org-office-server ─▶ crates/org-office (store.rs) ─┬──▶ crates/org-process ─┬──▶ crates/org-proto
                                                        │                       └──▶ crates/org-docs (front matter)
                                                        ├──▶ crates/org-work ────────▶ crates/org-proto
                                                        └──▶ crates/org-docs
   org-office-server implements the repo reads with org-git (River)
```
- **`org-process`** (new library; owner **TBD by ada**): front matter → `ProcessMeta` and its validation, path → level and scope, `resolve` (§3.2), the fingerprint, `render` (§3.4), the checklist merge, `one_branch::check` (§4.2), and the write checks (§5). Pure: no SQL, git, network or clock. Its inputs are plain values (files as repo, path, blob, bytes; teams as chains), so it defines no port. It depends on `org-proto`, `org-docs` (the one front-matter parser, at one pin) and `serde`: `Ce = 3`, expected `I ≈ 0.5` (a domain capability).
- **`org-office`** (server-dev): the index, the caches, the project-wide conflict items, last-read records, task branch records, exceptions, the calls, and calling `one_branch::check` inside the work write. W7 adds its edges to `org-work` and `org-docs`; `org-process` adds one more, stable-ward.
- **`org-office-server`** (server-dev): reads files at a commit and lists folders in `mirror.git` through `org-git`, the same way as W7's `DocRepo`. HQ creates the handbook at organization create.
- **`org-git`** (River): W7's `DocRepo` API, plus creating the handbook's bare repository, the handbook merge (§5) and the cleanup of §4.5 (tag push and compare-and-swap delete in the hardened push path).
- **`org-agent`** (Remy): the `process` commands of PROC-8 (§6), `task allow-second-branch`, and the `one_branch_open` exit code.- **`apps/desktop`** (app-frontend-dev): the Processes views, the profile tab, Allow once…, the work state.

Change-time check: one new library with three dependencies, all toward more stable crates. `org-process` and `org-work` don't depend on each other: the office composes them (checklist steps into `task.create`, the rule check inside the work write). No cycle, no new cross-domain dependency, no new edge into `org-proto` beyond additive types.

## 8. Phases
| Phase | What | Owners |
| --- | --- | --- |
| **R1** | The contract (§6). `org-process`: parsing, resolution, `one_branch::check`, the render, with the tests below. Needs `org-docs`' front-matter parser first (A70). | ada (contract); `org-process` owner |
| **R2** | Enforcement: task branch records (with the backfill), the two check points, `one_branch_open`, refusals, `task.allow_second_branch`, `work.branch_state`, cleanup (A58, §4.5), inbox notices to agent leads, the CLI. *Interim (A64):* until R3 there's no effective set, so the rule is on for every project. PROC-14, PROC-15, PROC-19. | server-dev; River (cleanup in `org-git`); Remy |
| **R3** | The handbook (create, seed, the deny in `agent-sessions.md`, migration for existing organizations, moving `branch-and-merge.md`), the index, effective sets and their cache, project-wide conflicts, delivery at start and wake, the fallback, `process.effective\|why\|show\|view\|ack`, the CLI reads. PROC-2 to PROC-7, PROC-8 (reads), PROC-10, PROC-16, PROC-17. | River (handbook repo); server-dev; Remy; ada (the deny) |
| **R4** | Writes: `process.save`, project processes through `doc.*`, the write checks, `process.approve\|reject` and the handbook merge, private proposals, versions, drift actions, checklists and moments (PROC-12), the CLI writes, the app's views (PROC-8 writes, PROC-9, PROC-11, PROC-18). Project process merges depend on the merge flow (A62). | server-dev; app-frontend-dev; Remy |
| **R5** | Retro action items of kind Process (PROC-13), with retros (W7 P5): the owner's approval of the retro's items is the level approval (spec decision 10), so they need no separate `process.approve`. Insights on branch waits (spec §4). | as retros |

Each step: its own branch, an independent review, the reviewer's READY, my go, then a merge into `v1`.

## 9. Tests that prove the rules
Each test must fail without the rule it names.

**`org-process`, table-driven over hand-built layers:**
- *Matching:* an override with a new `id` and the same `process` key replaces its parent; one with the same `id` and another key is an addition (ids never match).
- *Order:* organization ⊕ team chain ⊕ project ⊕ agent, the lowest winning; in a nested chain the nearer team wins; with no current work only O ⊕ G.
- *Seals:* an organization seal refuses team and project overrides and `off`; a team seal refuses its sub-teams' and its projects' overrides; the refusal names the sealing level; two teams overriding an org-sealed key give no conflict.
- *Conflict:* two teams overriding one key on a shared project give the organization's version and a conflict; with no organization version the key is absent; one team's version alone applies; a project version clears the conflict; a project linked to a parent team and its sub-team, and two sub-teams under one parent that alone overrides, give no conflict.
- *Turned off:* an overridable `off` removes the key with its reason; `off` of a sealed key is refused.
- *Private:* a private addition applies; a private file with an inherited key, `overrides` or `status: off` is flagged; a private file with a key dropped by a conflict or a clash is flagged too; the same private file applies in a project where no O, T or J file has the key.
- *Validity:* a key used twice at one (level, scope) drops both; invalid front matter, a level that doesn't match the path, `draft` and `retired` are left out with a problem; the others still resolve.
- *Drift:* `overrides: k@1` with a parent at v2 is drift; at v2 it isn't.
- *Fingerprint:* any change to the handbook commit, the project commit or a team's parent or link changes it; reordering inputs doesn't.
- *Render:* summaries first and cut with a count past the cap; changed texts in level order under 24 KiB, the rest named with the `show` hint; a winner moving from organization v3 to a project v1 counts as changed; a moment's prompt only when the moment is On.
- *Checklist:* project default steps first, then process steps, duplicates by text merged, required winning, never private steps.
- *One branch:* another task's reserved or open record blocks; the task's own doesn't; a docs branch never does; merged or archived doesn't; an exception allows one record and is then used; an approved valid `off` allows; a missing, clashed or invalid enforced key, an `off` from git, and "can't compute" all count as on.

**Office and store:**
- Two concurrent `task start` calls, and two concurrent branch creations, for two tasks of one agent: exactly one succeeds. A failed `git worktree add` leaves no reservation.
- An exception granted after a refused `task start` covers that task's start and its branch creation.
- An app move to In progress, then the task session start, is refused at branch creation and ends the run `blocked`; the refusal appears once in the leads' and owner's Needs you, repeated refusals add no events, and it clears on merge or exception.
- Merged state: a fresh branch with no commits, one reset to its base, and one fast-forwarded or reset to a later integration tip all stay open after a fetch; moving the integration ref or the task branch in `work.git` changes nothing; a real merge in `mirror.git` closes it, also when the branch was deleted before the fetch; a push to a merged branch is refused.
- Reservations: one whose task goes back to To do, closes or is reassigned before its branch exists is released; a failed `git worktree add` restores the exception it used. A reassignment that gives the new assignee two open records raises the Needs you item.
- Cleanup (A58): a merged branch gets its `merged/` tag before the delete, and isn't deleted if its head isn't in the integration branch; a cancelled task's branch gets its `archive/` tag first; the office never deletes the default or integration branch, a branch without a record, or a tag, and never force-pushes; a crash between tag and delete deletes nothing untagged.
- Cleanup safety: a remote branch moved after the check isn't deleted; local commits not yet pushed, or uncommitted changes, stop the cleanup with a Needs you item; a tag already at the same commit counts as pushed.
- Backfill: a Done task's branch already in the integration branch is merged on day one and blocks nobody; an In progress task's branch is open.
- Records: a branch fast-forwarded onto a named base that's a `task/*` branch stays open; `org-agent push` of a `task/*` branch without the caller's open record is refused; an agent's round trip (hand its open task to a peer, start another, take the first back) is refused at the hand-back, also for a lead taking it back to themselves; an exception is void after a reassignment; a branch reset to another agent's unmerged tip doesn't get an `own_tip` and stays open when that tip merges; a Done · Completed task with no own work releases its record; a task start in a project with no repository reserves nothing.
- Approval: `process.approve` by a member who saved on the branch, or by someone who isn't that level's approver, is `not_authorized`; the owner approves their own organization change; a save after the approver's notice makes the approval `conflict`; an approved handbook change is merged by the office; a handbook commit the office didn't make is ignored and raised; `doc new --here` and `doc.render` refuse kind `process`; `doc.save` on another member's process branch is refused.
- Seals fail closed: an organization sealed key that clashes still refuses lower overrides.
- Delivery: `org-agent task start` into another project returns that project's set and changed texts; `task done` returns the Finishing a task prompts; `process why` prints every layer of the chain; `process.show { version }` returns the parent at the version an override names.
- An agent lead gets a refused start and a waiting approval in its inbox, once each, and is woken if on call.
- A process merged between two wakes is sent in full on the next wake, once.
- A56: a task branch's parent is the integration tip when it differs from the default branch, and the default tip when `integration_branch` is empty. A missing `integration_branch` refuses branch creation (`unavailable`).
- `task.allow_second_branch` from an agent who isn't a lead of the project's teams, or from a lead for their own task, is `not_authorized`; from another lead or the owner it records the event with the actor and reason.
- The effective set never reads a working branch: a process change on an unmerged docs branch is absent until it merges, then present after the fetch.
- A57: an instruction file, a guidance file and a process changed only on the default branch, with a different integration branch, don't reach the session; moving the integration ref in `work.git` doesn't change them either.
- Private processes: for another agent and a non-owner person, `process.effective` and `process.why` of a member leave out its private layer and `last_read`, and `process.show { level: agent }` and `process.view { agent }` are `not_authorized`; the owner reads them all; an agent's Bash and file tools are denied `offices/*/workspace/handbook/**`.
- A conflict appears once per (project, key) in Needs you with no agent running, and clears when the project settles it.
- The handbook unreadable: reads answer from the index with `stale_since`; a start uses the last set and the owner is warned after an hour (at once with no earlier set); enforcement still runs. A start that the runtime refuses doesn't mark changed texts as read.

## 10. Open questions
**Architecture (for me to settle, with the named owners)**
- **A60** · Team folders: A51 says `teams/<team key>/`, but teams have no key (`Team { id, name, … }`). This design uses the team id, as Eden's spec does. Confirm, or add team keys.
- **A61** · The handbook's record and layout: `OrgSnapshot.handbook` (no path) and one office-only `workspace/handbook/handbook.git` at HQ, created at organization create and on first start for existing organizations. River and server-dev to confirm.
- **A62** · The merge flow for docs branches (W7 open question 10) now also carries project process approvals and the version bump when another change merged first. Until it's designed, `process.approve` records the approval and a person merges a project branch; the office merges approved handbook branches itself (§5), since nothing else can reach the handbook. Confirm the office merge, or fold it into the merge flow.
- **A63** · Branch stages (waiting for review, for the go, to merge) come from the merge flow. Until then the work state shows open or merged only. Acceptable for R2?
- **A64** · The rule is on if the **target task's** project has `branch-and-merge` active. Confirm. If U26 splits it into `one-open-branch` and `review-and-merge`, enforcement binds to `one-open-branch`.
- **A65** · Reassignment can hand an agent a second open branch. The interim rule (§4.1) refuses it unless a project lead or the owner does it. Keep that, or refuse it outright, or need an Allow once…?
- **A66** · Phase 2: does every satellite fetch the handbook and compute its agents' sets, or does HQ compute and send them?
- **A67** · The rule check inside the work write: the office calls `one_branch::check` in `WorkStore::transact`'s closure, or `org-work` gains a guard port. The check is about work and branch state more than processes, so it may belong in `org-work`, taking "rule on" as an input. server-dev and the `org-work` owner to pick.
- **A68** · Two teams with byte-identical overrides of one key: still a conflict (this design: distinct files), or no conflict?
- **A69** · Can a project's leads read a member's private processes, or only the owner and the agent (this design)?
- **A70** · `org-process` needs `org-docs`' front-matter parser, which W7 plans for P3. Build that part of `org-docs` first, or let R1 wait?
- **A71** · A team seal on a shared project: team A seals a key and team B overrides it. This design treats it as a conflict (the organization's version applies), and the project can't settle it, since A's seal also binds A's projects (§3.2 rule 3). Or should A's seal win across chains?
- **A72** · Project process files changed with plain git (not through the tool and its approver) still apply, shown "from git", but an `off` of the enforced key from git doesn't turn the rule off (§5). Keep that, or ignore every process change that didn't come through the tool?

**For the user (from Eden's spec, unchanged here):** U25 (where Processes sit in the nav), U26 (seal `branch-and-merge`, or split it), U27 (organization-level approvers), U28 (the docs-branch limit in §4.4).

**Follow-ups on my side (not in this change):** amend `agent-sessions.md` for the handbook deny (§1), its "Security" line on composed instructions and plan step D4, which still say "default branch" (A57); the doc comment on `Project.integration_branch` in `org-proto`, which still says only "the branch the Docs tab reads"; `projects-git-github.md`'s push rule (a push needs the caller's open record, and none after a merge, §4.1); and, when R3 moves `branch-and-merge.md`, the link in `AGENTS.md` rule 2.
