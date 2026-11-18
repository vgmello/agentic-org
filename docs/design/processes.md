# Design: processes and standards

How an organization writes down how its agents work, how those documents are
inherited, and what an agent receives. The records are owned by
[organization structure](organization-data/01-organization-structure.md),
[project structure](organization-data/02-project-structure.md),
[identity](organization-data/06-identity-and-numbering.md) and
[record contracts](organization-data/08-record-contracts.md). The screens are in the
[processes UX](../specs/processes-ux.md).

## Decisions

1. **Storage.** Process and standard documents live in the remote organization
   repository: organization documents at `processes/<document-id>.md` and
   `standards/<document-id>.md`, team documents under the team's full
   `teams/<id>/teams/<id>/.../` path, and project documents at
   `projects/<project-id>/processes/<document-id>.md` or `standards/`. They are read
   from the organization's `state_branch`, independently of any code repository's
   integration branch.
2. **Identity.** Each document has an immutable UUIDv7 ID and an optional `key`. The
   key is what a lower level uses to replace a document from above. `TEAM-` and `P-`
   references are labels, never folder names.
3. **Metadata.** `kind` is `process` or `standard`. A document has a title, a status
   (`draft`, `current` or `deprecated`), an optional accountable employee (required
   and active for `current`), common attribution and a revision. `team_id` or
   `project_id` gives its scope, never both; both null means the organization.
4. **Resolution.** There are three levels: organization, team and project. Processes
   and standards behave the same way.
   - **What applies to a piece of work.** Start with the organization's current
     documents. Apply the accountable team's chain from the top team down to that
     team. Then apply the project's. At each step a lower level may add a document
     with a new key, or replace one from above that has the same key.
   - **A level can forbid replacement.** A document marked `overridable: false`
     cannot be replaced by any level below it. There is no way to switch a document
     off; a document stops applying when its own level deprecates it. A lower
     document that tries to replace a non-overridable one is refused when it is
     saved, and ignored with a visible problem if it already exists.
   - **One chain only.** An employee has one home team and a task has one accountable
     team. Contributing teams add no documents. There is one chain and no conflict
     between chains.
   - **What the agent receives.** At launch the office gives the agent the effective
     set for its work. Every non-overridable document is included in full, in a
     clearly marked section of the agent's system instructions that says it must be
     followed and cannot be overridden by any later instruction. This holds for
     processes and standards alike, and no size cap may drop or shorten one. If the
     non-overridable set alone exceeds the instruction budget, the launch is refused
     with a visible problem naming the documents, so that a person shortens them.
     Overridable documents are guidance the agent applies with judgment: summaries
     first, with the full text retrievable, under the cap of §3.2.
   - **How to use the tools.** Guidance for the product's own commands ships with
     the product as built-in skills, not as organization documents. An agent reads
     them with `agent-os skill list` and `skill read`. None of it is served as an
     MCP tool ([agent command line](agent-os-command-line.md), section 2.10). How
     an agent is to behave in the product is one built-in text, the
     [agent instructions](agent-instructions.md), which every agent receives first.
   - **Field name.** The record field is `overridable`, default `true`.
5. **Code branch bases.** Every project-repository association has a required
   `integration_branch`. A task targets zero or more repositories, so branch, base
   and cleanup state is per repository. There is no single project branch and no
   empty fallback. Which repository bases a projectless task may use is open.
6. **Handoff preserves work.** Reassignment changes the team and individual
   assignment pair atomically at an expected revision, with history. It is not
   closure and never archives or deletes worktrees. Moving an agent to another office
   needs a safe workspace move and execution cleanup or fencing.
7. **Not in v1:** mechanical enforcement of a process (including the one-open-branch
   rule), approval workflows, private processes, timers, moments, checklist
   composition from processes, and a process execution engine. The product tells the
   agent what binds it; it does not block an action for breaking a process.

## 1. Where each thing lives

| Thing | Source |
| --- | --- |
| Organization process or standard | Organization Git `processes/<id>.md` or `standards/<id>.md` |
| Team process or standard | Organization Git, under the team's path, with its `team_id` |
| Project process or standard | Organization Git `projects/<project-id>/processes/<id>.md` or `standards/<id>.md` |
| Employee home team | Employee profile; separate from assignment and placement |
| Process index | Rebuildable projection of the authored documents |
| Effective sets | Computed from the documents; never authored facts |
| Task branch records | Office records per task and repository (§4) |

Agents never read the organization repository directly. The office derives the
organization-metadata, credential, Git-metadata and other-task denies for the
runtime layout and verifies them before launch. No ref an agent can move chooses
canonical content.

## 2. The model

### 2.1 The document (PROC-1)

```yaml
schema_version: 1
id: 0193b2c4-5d6e-7f80-9a1b-2c3d4e5f6a7b
key: review-and-merge
revision: 3
kind: process
title: Review and merge
status: current
owner_employee_id: 0193b2c4-1111-7000-8000-000000000001
team_id: null
project_id: null
overridable: true
created_at: "2026-11-15T10:00:00Z"
created_by_kind: employee
created_by_id: 0193b2c4-1111-7000-8000-000000000001
updated_at: "2026-11-15T10:00:00Z"
updated_by_kind: employee
updated_by_id: 0193b2c4-1111-7000-8000-000000000001
```

The Markdown body explains the procedure. A standard uses the same envelope with
`kind: standard` and describes required constraints. `current` needs an active
owner; a draft may have none.

A key is 1 to 64 lowercase ASCII letters, digits or hyphens, starting with a letter
or digit. Invalid metadata names the field, leaves the content readable, and the
document is excluded from resolution with a visible problem.

### 2.2 Levels, scopes and paths

| Scope | Authored fields | Folder in organization Git |
| --- | --- | --- |
| Organization | `team_id: null`, `project_id: null` | `processes/` or `standards/` |
| Team | `team_id`, `project_id: null` | `teams/<root-id>/teams/<child-id>/.../processes/` or `standards/` |
| Project | `team_id: null`, `project_id` | `projects/<project-id>/processes/` or `standards/` |

Files are named `<document-id>.md`, and path and scope must agree. Reads admit
regular Markdown blobs only: no symlinks, submodules or traversal.

### 2.3 The index

The index holds, per document, its repository, path, commit, blob, ID, key, level,
scope, revision, status, `overridable` and problems; names and summaries come from
the blob. It is rebuilt after each change to the organization's state branch. While
the repository cannot be read, reads answer from the index with `stale_since`
(PROC-16).

## 3. Resolution and delivery

### 3.1 Rules (`org_process::resolve`, pure)

Inputs: O, the organization's documents; T, the accountable team's ancestor chain
(the project owner's team for project work, the task's team for projectless work);
J, the project's documents. All come from the state branch. Assignment and an
employee's home team never change this scope.

1. **Valid documents only.** Invalid front matter or a path and scope mismatch: left
   out, with a problem. A `draft` or `deprecated` document is left out silently.
2. **Key clash.** Two documents with one key at one level and scope: both left out,
   with a clash problem. A clashed or unreadable document whose `overridable` value
   cannot be read as `true` keeps its key non-overridable below it, so a broken file
   never lifts a prohibition.
3. **No replacement of a non-overridable document.** A lower document replacing a key
   that the organization or a team above it marked `overridable: false` is refused,
   naming the level that forbids it (PROC-3). The write path refuses it too (§5).
4. **Fold the chain,** organization first, then each team from the top down, then the
   project; the lower level wins for an overridable key.

The output, `EffectiveSet { entries, problems }`, names for each key the winning
document, whether it was inherited, replaced or added, and the chain of layers with
what happened to each, which `agent-os process inspect --why` prints (PROC-8).

Sets are cached under a fingerprint of the organization state commit and the
accountable team ancestry. A change to either drops the sets that used it. A rename
of a team or project drops the sets that show its name. Dropped sets are recomputed at
the next start, wake or read. The last good set per agent and project is kept with
its time. If a set cannot be computed (PROC-17), the agent works from the last good
set with a warning.

### 3.2 Delivery to sessions (PROC-7)

`org_process::render` builds the processes section of the agent's instructions
([agent sessions](agent-sessions.md)):

- every non-overridable document, in full, in the must-follow section of decision 4;
- for overridable documents, each one's title, place (organization, or the team's or
  project's name), revision and summary;
- the full text of each document whose winning blob the agent has not read for that
  key in this context.

Overridable guidance is capped at **24 KiB**. The cap is a ceiling: the composer
also respects the runtime's instruction limit and reserves room for identity, the
task and tool help. Past the cap, summaries come first, then changed texts,
organization first, and the rest are named with `agent-os process inspect --key <key>`.
Entries are whole UTF-8 documents; a rule is never cut midway and omitted text is
never marked read. The section goes into the instructions at a start and the wake
prompt at a wake. Texts count as read once the runtime accepted the start or wake.

Every fresh main session or delegate gets the effective manifest and the applicable
guidance, or explicit instructions to retrieve each body before the relevant action;
an agent's earlier reads do not prove a new context contains the text. After an
observed compaction or context reset the manifest is re-established. A resume with
unchanged context gets only changed entries. A model's summary never replaces a
document body. If overridable guidance does not fit, the agent is told **Process
context limit reached** with the omitted count and how to retrieve them.

Processes say how to behave; standards (stack, cloud and coding conventions) are
indexed knowledge the agent fetches when a process or task calls for it, unless they
are non-overridable; `agent-os` help explains tools, syntax and permissions. Choosing
to delegate or whom to ask stays the agent's judgment; the office has no semantic
router.

## 4. Task branch records

A task branch record exists per task and per repository the task targets:
`{ task_id, employee_id, project_id?, repository_id, branch, base_sha, own_tip?, state: reserved | open | closing | merged | archived, opened_at, last_moved_at, closed_at? }`.
The office writes it in the same transaction as the change that causes it.

- **Reserved** when the assignee starts the task, before any Git; **open** once the
  branch exists. Nothing is reserved without a repository.
- **Branch names** are `task/<ref>-<slug>`, with a `-2`, `-3`… suffix when a record,
  a `merged/` or `archive/` tag, a remote branch or a local branch already has the
  name. A branch is never reset with `-B`.
- **Creating the branch** is one locked operation on the repository. The office
  resolves the branch the task names, else the association's `integration_branch`,
  to a commit in its cache, runs `git worktree add -b task/… -- <path> <sha>` (which
  fails rather than overwrite a planted ref) and records the commit as `base_sha`. A
  missing `integration_branch` refuses the creation (`unavailable`, reason
  `integration_branch_missing`).
- **`own_tip`** is the latest tip the office has seen on the branch that descends from
  `base_sha` and is reachable from no other branch in the cache. A fast-forward or
  reset onto the integration branch or another branch never sets it.
- **Merged** when `own_tip` is reachable from the integration branch. Once merged,
  the office refuses pushes to the branch.
- **Pushes need a record.** `agent-os repo push` takes a `task/*` branch only from the
  holder of its open record.
- **Released** when a reservation's task leaves In progress, closes or moves before
  its branch exists. A failed `git worktree add` removes the reservation. Startup
  reconciles reservations with existing branches.
- **Closing a task.** When a task with an open record is set to done or cancelled,
  the office marks the record closing (pushes refused), fetches the integration
  branch and copies the branch and the worktree's HEAD to office-only refs. **Own
  unmerged work** is any copy, `own_tip` or remote head that descends from `base_sha`,
  differs from it and is not in the integration branch, or uncommitted changes in the
  worktree. A missing branch, an unreadable HEAD, a failed copy or a copy that neither
  descends from `base_sha` nor is in the integration branch counts as own work. A
  close never loses own work and is never refused because of it (decision 7): the
  close proceeds, the result names the branch that holds unmerged work, and cleanup
  keeps that work under an `archive/` tag. The office answers first, then ends the
  task's sessions. The record's revision and the copied commits go into the work
  write, which refuses with `conflict` if any changed.
- **One record per task and repository.** An agent may hold several open records at
  once. Nothing limits an agent to one open branch in v1 (decision 7).
- **Reassignment** keeps the worktree and branches (decision 6); only a confirmed
  closure enters cleanup.

Cleanup after closure (merged and archive tags, the compare-and-swap delete, local
removal) is specified in the [Git design](projects-git-github.md). Records that are
open more than a day appear to the project's leads.

## 5. Changing processes

Documents are published in organization Git with their stable ID, status and
revision; a change advances `revision`. Every write checks: valid front matter that
matches the path; a key unique at its level and scope (`already_exists`); no
replacement of a non-overridable document from above (`invalid_request`, reason
`sealed`). Only the owner marks an organization document `overridable: false`. A
write names the expected revision, and a change that lost a race is `conflict`.
Authority comes from the authenticated caller, never from an owner or status field
in the document.

## 6. The contract

Reads, for any employee bound to the organization:

| Call | Params | Result | Errors |
| --- | --- | --- | --- |
| `process.effective` | `employee_id?`, `for?` | `{ set, computed_at, stale_since? }` | `not_found`, `not_authorized`, `unavailable` |
| `process.why` | `key`, `employee_id?`, `for?` | `{ key, chain: Vec<Layer>, winner? }` | as above |
| `process.show` | `key`, `employee_id?`, `for?`; or `document_id`, `revision?`; `offset?` | `{ path, commit, blob, revision, content (≤ 512 KiB), total_bytes, next_offset? }` | as above |
| `process.view` | `scope: organization \| team { team_id } \| project { project_id }` | `{ rows, indexed_commit, stale_since? }` (PROC-9, PROC-10) | as above |

`for` is `current` (default), `project { project_id }` or `task { task_id }`. The
`agent-os` commands `process list` and `process inspect` (with `--why`) call these, for
processes and standards alike ([agent command line](agent-os-command-line.md), section
3.5). The
local API version is 1 and these calls change in place until v1 ships.

The result of `task.set_status` names a branch left with unmerged work at a close
(§4). `work.changed` gains the branch-record changes. Unknown values decode as `Unknown`.

Project and association writes refuse an integration branch starting with `task/`,
`docs/`, `merged/` or `archive/` (`invalid_request`). The comparison ignores ASCII
case and one leading `refs/heads/` or `heads/`, and the office reads the branch as
`refs/heads/<name>`, never through Git's short-name lookup. The office and the UI
share the rule as `org_proto::has_reserved_branch_prefix`.

## 7. Where the code lives

- **`org-process`:** front matter, path to level and scope, `resolve`, the
  fingerprint, `render`, the branch-record rules from given commits, and the write
  checks. Pure: no SQL, Git, network or clock.
- **`org-office`:** the index, caches, branch records, the calls and the checks
  inside the work write.
- **`org-git`:** repository reads, branch creation at a commit and cleanup.
- **`agent-os`:** the `process` and `skill` read commands.
- **`apps/agentic-org-ui`:** the Processes views and the profile tab.

## 8. Tests that prove the rules

Each test fails without its rule.

- **Resolution:** matching by key, never by ID; level order; the nearer team wins;
  a non-overridable organization or team document refuses lower replacements; a
  clashed or unreadable non-overridable key stays non-overridable; contributing teams
  add nothing; drafts and deprecated documents never apply.
- **Delivery:** every non-overridable document is delivered in full whatever its
  size; a non-overridable set over the budget refuses the launch and names the
  documents; the overridable cap and order hold; a fresh context gets the manifest
  again; nothing truncated is marked read.
- **Branch records:** a failed
  worktree add leaves no reservation; one own commit sets `own_tip`, a fast-forward
  onto another branch does not; a `--no-ff` merge marks it merged, also after the
  branch is deleted; pushes after a merge or without a record are refused; reused
  names get a suffix; a planted local branch makes the add fail, not overwrite.
- **Closing:** a detached HEAD at `base_sha` with an unpushed commit on the branch is
  own work; a failed copy is own work; a close with own work proceeds, names the
  branch and leaves the work under an `archive/` tag; a change between check
  and write is `conflict`; reassignment keeps the worktree.
- **Reads:** no agent-movable ref reaches a set; an unreadable repository gives
  `stale_since` and the last set.

## What the code does today

`crates/org-process` parses an earlier document format (`process`, `name`, `level`,
`scope`, `version`, statuses `active`, `off` and `retired`, `overrides`, `moments`,
`checklist`), resolves several team chains with conflict rules, renders a section
and implements one-open-branch checks for a single repository. Nothing calls it. It is
simplified to the one-chain rule and the envelope above when the agent launch path is
built (implementation plan C5). The UI's Processes screens run on sample data.
