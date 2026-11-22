---
id: doc-35
title: 'Design: Projects, Git and GitHub'
type: specification
created_date: '2026-11-21T00:19:00Z'
updated_date: '2026-11-22T04:30:00Z'
tags:
  - design
  - git
  - github
  - worktrees
  - v1
  - v2
---
> [!NOTE] Review note (2026-11-21)
> Reviewed against Organization Data contracts and `doc-1` (Vision).
> - **Delivery timing:** Bare cache and per-task worktrees (`org-git`) are planned for `C7` (`TASK-50`). Provider-managed GitHub App (§8) is explicitly `v2`.
> - **Design prerequisites settled (2026-11-22, `TASK-51` C7):** All 11 open questions below are resolved and recorded for v1 plain-Git implementation (`org-git`, `org-office` ports, `org-office-server` composition, `/api/v1` BFF, and `agent-os` `repo`/`worktree` commands).

# Design and plan: projects, repository registrations and Git

<a id="design-and-plan-projects-as-git-repositories-connected-to-github"></a>
<a id="design-and-plan-projects-repository-registrations-and-github"></a>

**Status: v1 plain-Git backend and CLI in progress (`TASK-51` / C7); `org-github` is v2.** `crates/org-git`, `org-office` repository/project/task-branch projections, `org-office-server` `GitService`, `/api/v1` BFF routes, and `agent-os`/`agentic-org` `repo`/`worktree` commands are implemented; remote credential store wiring, post-closure cleanup/handoff orchestration, and G5 Web UI remain pending (tracked in `doc-3` `FC-139`/`FC-140`).

Two rules frame everything below:

- Every algorithm, path and wire sketch uses the selected storage layout, with one bare cache per repository per office, many repositories per project and per task, and one worktree per employee, task and repository.
- Hosting-provider integrations are v2. v1 is plain Git with the office's own Git credentials.

The [organization repository registrations](../organization-data/doc-75%20-%20Organization-Data-01-—-Organization-Structure.md), [project links](../organization-data/doc-76%20-%20Organization-Data-02-—-Project-Structure.md), [task repository targets](../organization-data/doc-77%20-%20Organization-Data-03-—-Features-and-Work.md), the [shared storage contract](../organization-data/doc-72%20-%20Organization-Data-—-README.md#shared-storage-contract) and the [authority map](doc-62%20-%20Design-Documentation-Authority-Map.md) own the data model. Where this document and those contracts differ, they win. Nothing is deployed, so there is no existing layout to migrate from and no compatibility obligation.

## In one paragraph

A project is a long-lived scope of product work owned by one accountable team, with zero or more code repositories. Each repository is registered once in the organization, as remote or local. A project links a repository with its own required `integration_branch`, and the same repository can serve several projects. Each office keeps one bare cache per repository it uses. For every repository a task targets, the office creates one worktree on a branch `task/<ref>-<slug>` that starts from the link's integration branch. The office runs all Git that touches a remote, with its own Git credentials; agents commit locally and never hold a credential that can push. After the office pushes a task branch, a person opens any pull request and merges. Creating repositories, minting tokens and opening pull requests through a hosting provider are v2.

## Scope: v1 and v2

| Topic | v1: plain Git | v2: hosting-provider integrations (not selected for v1) |
| --- | --- | --- |
| Repositories | Registered remote or local repositories. A person creates a remote repository at its host and registers its URL | The product creates repositories through a provider API |
| Reaching a remote | Clone, fetch and push of task branches over the registered `remote_url`, with the office's own Git credentials | Short-lived, narrowly scoped tokens minted by HQ from a provider app |
| Local work | Per-office caches, per-task worktrees, local-only repositories | Unchanged |
| After a push | A person opens any pull request and merges, with the host's own tools | The product opens pull requests and reads CI results |
| Organization repository | Must be remote; reached the same plain way | Unchanged |
| Crates | `org-git` (implemented in `crates/org-git`) | `org-github` (deferred, no code) |

## What lives where

- **Organization Git:** `organization.yaml` selects a registered remote organization repository and its `state_branch`. `repositories.yaml` registers each repository once, with a stable ID, a `key`, `kind: remote | local` and a nullable `remote_url`. `projects/<project-id>/project.yaml` requires `owner_team_id` and lists repository links `{repository_id, integration_branch}`. A registration holds no integration branch and no credential.
- **Authored project knowledge and work:** organization Git holds `projects/<project-id>/features/<feature-id>/feature.md`, supporting scoped documents and `work/tasks/<task-id>.md`. A task file lists `repository_ids` and, per repository, `branches` and `integrated_commits` (the `task_repositories` projection). Stable IDs are UUIDv7; projects have P- references and tasks have office-minted T- references.
- **Office runtime**, under `<data-root>/<organization-id>/runtime/<office-id>/`:
  - `repositories/<repository-id>.git/`: the one bare cache of that repository in this office, shared by every project that links it. It holds no integration branch setting, no remote URL and no credential.
  - `teams/<root-team-id>/teams/<child-team-id>/.../agents/<employee-id>/projects/<project-id>/tasks/<task-id>/repositories/<repository-id>/`: the worktree of a project task for one repository.
  - `teams/<root-team-id>/.../agents/<employee-id>/tasks/<task-id>/repositories/<repository-id>/`: the worktree of a projectless (team) task.
  - `office.db`: the runtime-only rows `repository_caches` (office, repository, `bare_path`) and `task_workspaces` (office, employee, task, repository, `worktree_path`).
- **Local-only code:** `durable/local/offices/<office-id>/repositories/<repository-id>.json` holds the machine-specific `origin_path`. No remote backs this code up. Its history and edits must be preserved in `durable/checkpoints/` before a cache is deleted, and moving it to another office needs an explicit bundle and import, or a remote.
- **Git credentials:** the office's credentials for remotes are secrets. They are held by the host's credential provider or as key material only the service account can read. They never appear in organization Git, in a database in plaintext, in a cache's configuration, in a worktree, in a log, or anywhere under `runtime/` ([ADR-0016](../../decisions/decision-16%20-%20ADR-0016-No-application-level-encryption-of-organization-data.md)).
- **Protection at rest:** Git files are ordinary files. Disk encryption, private file permissions and the agent isolation mode protect them ([ADR-0015](../../decisions/decision-15%20-%20ADR-0015-Agent-isolation-is-a-selectable-mode-with-a-dedicated-OS-user-provisioned-at-setup.md), ADR-0016). Nothing here is encrypted by the application.

`<data-root>` is resolved once by the instance ([standalone design](doc-70%20-%20Design-Standalone-Agentic-Org-Server.md#3-instance-selection-and-storage)). It is never inferred from a working directory or `HOME`.

## Release knowledge and deployment records

[Releases/milestones](../organization-data/doc-76%20-%20Organization-Data-02-—-Project-Structure.md), [deployment attempts](../organization-data/doc-81%20-%20Organization-Data-07-—-Release-Deployments.md) and [release content/flags](../organization-data/doc-83%20-%20Organization-Data-09-—-Release-Content-and-Flags.md) are selected data, not a deployment or flag-evaluation engine.

| Record | Selected relationship and retained evidence |
| --- | --- |
| Release | One project, project-scoped R- reference, optional version_label independent of identity; HQ creates it. Explicit feature_ids/task_ids select same-project content, not all tasks currently in a milestone. |
| Milestone | Exactly one same-project release, release-scoped M- reference; HQ creates it. A task optionally references one milestone and its file stays in work/tasks/. |
| Release commits | Repository/commit snapshots for project repositories; repository registrations stay organization-wide and integration branches remain association data. |
| Deployment | One release/project/environment/target; operation deploy/rollback, queued/running/succeeded/failed/cancelled state, known start/end times, repository_commits and optional external_url. Retry and rollback receive new stable IDs; rollback_of_id identifies the undone attempt. |
| Product changelog | Release changelog.md explains additions/fixes/changes/removals/breaking changes and gated availability. |
| Technical changelog | Each deployment changelog.md explains actual repository/configuration/migration/flag changes, validation and rollback; optional same-project/environment/target baseline_deployment_id identifies the comparison baseline. |
| Flag definition | Project-unique registered key, stable ID, optional same-project feature, typed default_value, draft/active/retired status and active owner requirement. |
| Release flag intent | flag-settings.yaml records explicit environment/target value and optional rollout rules; absent means unspecified, not disabled. |
| Applied flag snapshot | Deployment flag_settings records confirmed applied values/rules/times, which may differ from intent; missing means unrecorded, not false. Past confirmed snapshots stay fixed. |

Canonical paths are `projects/<project-id>/releases/<release-id>/release.md`, `changelog.md`, `flag-settings.yaml`, `milestones/<milestone-id>.md`, and `deployments/<deployment-id>/deployment.md` with its separate `changelog.md`. Supporting release/deployment documents use their scope's `documents/<document-id>.md`; project flag definitions use `feature-flags/<flag-id>.md`. Each supporting document has required project and at most one same-project feature/release/deployment scope. Current requires an active owner. Feature definitions remain features/<feature-id>/feature.md, separate from documents.

Started deployment attempts require at least one pinned repository commit; preserve release/environment/target/commits once execution begins. Failed attempts may have partial effects and do not establish live environment state. Correcting release prose or manifest membership does not rewrite an attempt or historical captured release usage. Retain release-selected task identities/tombstones after closure. A rollback is new history, never an edit turning a prior success into a rollback. Build artifacts, pipeline integration, deployment authorization/execution and flag evaluation remain outside this data contract.

## v1: plain Git

In this part, `<runtime>` means `<data-root>/<organization-id>/runtime/<office-id>`, `<cache>` means `<runtime>/repositories/<repository-id>.git`, and `<worktree>` means the task worktree path for one repository given under "What lives where". "The service account" is the OS user that runs `agentic-org-office`. "The office" is that worker.

### Registering repositories

Creating a project and registering a repository are separate operations. A project can start without code, link several existing registrations, or link a registration made for it.

- **A remote repository** is created by a person at its host. v1 never creates one. Its registration carries the stable ID, a `key`, `kind: remote` and the `remote_url`. A registration URL carries no password or token; an SSH login name such as `git` is not a secret and is allowed.
- **A local repository** has `kind: local` and no URL. Each office that works on it has a local binding file with the `origin_path` of the repository on that machine.
- **Allowed transports** for a `remote_url` are `https` and `ssh`. Unauthenticated or unverified transports (`http`, `git`) and helper transports (`ext`, `fd` and the like) are refused at registration and again at every use. A local repository uses the `file` transport on its `origin_path` only.
- **A project link** is `{repository_id, integration_branch}`. The integration branch is required on every link. Two projects may link the same repository with different integration branches.
- **Checking before accepting.** Git never runs inside a deterministic command. Before a registration or link is submitted, an office that holds a credential for the remote, or that has the `origin_path` binding of a local repository, lists its branches with a hardened `git ls-remote`. A link whose integration branch does not exist there is refused. There is no empty-branch fallback.
- **A task** targets zero or more registered repositories. For a project task, each target must be linked by its project. A link to a project, a contributing team or an assignment does not by itself give an agent access to a repository; the task target does.
- **Preparing a cache.** An office prepares a repository's cache the first time a task there needs it, or ahead of time for repositories it follows. Status is reported per office and repository (`cloning`, `ready`, `failed: <reason>`). A project's readiness is the aggregate of its links; caches are never duplicated per project.

### The repository cache

Each office has exactly one bare cache per repository, at `<cache>`. It serves two users: the office, which fetches and pushes, and the worktrees in which agents commit. The rules below exist because of that sharing.

**Creation.** The office creates the cache with `git init --bare`, writes its canonical configuration and then fetches. It does not use `git clone`, which would write a remote and its URL into the configuration.

**Canonical configuration.** `<cache>/config` holds only the repository format version, `core.bare = true` and, in `dedicated-user` mode, `core.sharedRepository = group`. It holds no `[remote]` section, URL, credential helper, proxy, include, alias, filter, hook path or `extensions.worktreeConfig`. Git therefore never reads a `config.worktree` file. The office knows the exact bytes it wrote.

**Reference namespaces.**

| Namespace | Written by | Holds |
| --- | --- | --- |
| `refs/remotes/origin/*` | Office only | The remote's branches as last fetched, including pushed task branches |
| `refs/tags/*` | Office only | Fetched tags and the `merged/<branch>` and `archive/<branch>` tags the office creates |
| `refs/office/*` | Office only | Preservation copies of task heads taken before handoff or cleanup |
| `refs/heads/task/*` | The office creates and deletes; the task's agent advances by committing | Local task branches |

There are no other branches under `refs/heads/`. The integration branch exists locally only as `refs/remotes/origin/<integration_branch>`.

**Fetching.** The office fetches with an explicit URL from the registration and explicit refspecs: `+refs/heads/*:refs/remotes/origin/*` with pruning, and tags without force. It never uses a remote name. For a local repository the URL is the `origin_path`. Agents read the fetched state locally; they do not fetch.

**One operation at a time.** The office holds one lock per office and repository for every operation it runs on a cache: fetch, worktree add, move and remove, push, tagging and cleanup. Agents' own commits are not serialized by the office; Git's lock files handle them.

**Maintenance.** Automatic maintenance is off for agents and for the office (`gc.auto=0`, `maintenance.auto=false`). The office repacks under the repository lock and never prunes objects while any task workspace exists for that repository.

**Deleting a cache.** A cache is rebuildable only for what the remote already has. Linked worktrees store their commits in the cache, so deleting it destroys unpushed commits. Before deleting a cache the office removes or preserves every worktree that depends on it and confirms that unpushed commits and uncommitted edits are in `durable/checkpoints/` or on the remote. For a local repository the checkpoint is the only copy outside `origin_path`.

### Credentials and isolation

With one cache per repository, keeping the network credential away from agents cannot come from the directory layout. It comes from where the credential is, how Git is invoked and which isolation mode the agent runs in.

**Where the credential is.** An office's Git credential is bound to that office and one remote host or repository. It lives in the host's credential provider, or, for SSH, as a key only the service account can read or use. It is not in the cache, not in a worktree, not in organization Git and not in `office.db`.

**How the office supplies it.** Only the office's own `git` process for one remote operation receives the credential, and only for the life of that process:

- **HTTPS:** the office clears every configured helper (`credential.helper=` empty) and names one helper of its own, scoped to that exact URL, in the command's configuration. The helper obtains the secret from the office over a descriptor the `git` process inherits. The secret is never in the arguments, the URL, a file under `runtime/` or the cache's configuration. An inherited descriptor is preferred to an environment variable, because a process environment is readable by other processes of the same user on Linux.
- **SSH:** the office sets the SSH command for that invocation through its environment. The command ignores user SSH configuration, runs in batch mode, uses only the office's identity, and verifies the host key strictly against a known-hosts file the office owns. The key is reached through a key file or an agent socket that only the service account can open.
- **Local repositories** need no credential.

**What an agent's session receives.** Nothing that can authenticate to a remote. Its environment has no credential variable, no SSH agent socket and no askpass program. Git prompting is off (`GIT_TERMINAL_PROMPT=0`). System and global Git configuration are disabled, and `credential.helper` is set to empty through the environment-supplied configuration that [agent sessions](doc-29%20-%20Design-Agent-Sessions-and-Runtime-Isolation.md) already defines. The cache has no remote, so `git push` in a worktree has no destination and no credential. An agent publishes its work only by asking the office (`agent-os repo push`).

**What each isolation mode guarantees.**

| Question | `same-user` | `dedicated-user` |
| --- | --- | --- |
| Who can read the credential at rest | The service account, which is also the agent's OS user. Only the vendor tool sandbox and the office's fixed denies keep an agent's tools from the credential provider, key files and SSH agent socket | The service account only. The agent account cannot read `durable/`, the office database, the service account's home or its credential store |
| Can an agent read a credential in flight | Not while the sandbox holds. A process outside the sandbox can inspect the office's `git` child process, because it has the same OS user | No. Another account cannot read the process's memory, descriptors or environment |
| If a tool escapes the vendor sandbox | It runs as the service account and can reach the credential. No guarantee holds | It still runs as the agent account. The credential stays unreachable through files or processes |
| Cache configuration, hooks and office-only references | Protected by fixed denies only | Protected by file ownership as well: the agent account cannot write them |
| Other tasks' branches, objects and caches | Separated by the sandbox and the office's rules only | The same. All agents share one account in v1 |

`same-user` therefore gives defence in depth, not a boundary: the design guarantees that no credential is ever placed where an agent's session is allowed to look, and nothing more. `dedicated-user` adds an operating-system boundary between every agent and the credential. Neither mode separates one agent from another by OS permissions. The product must say so where the mode is shown (ADR-0015).

**Writing into the cache in `dedicated-user` mode.** A commit in a linked worktree writes into the cache: `objects/`, `refs/heads/task/`, `logs/refs/heads/task/` and that worktree's administrative directory `worktrees/<name>/`. In this mode those directories, and only those, are writable by the agent account through a group shared with the service account. Everything else in the cache is owned by the service account and not writable by the agent account. The cache and its parents under `<runtime>` are reached through directories the agent account may traverse and not list. Git refuses a repository owned by another user unless it is named in `safe.directory`, which Git honours only from protected configuration; the office supplies the exact cache and worktree paths through the session's environment-supplied configuration, and through its own command configuration when it works on a worktree owned by the agent account. ADR-0015 does not yet state this sharing; see open question 3.

### Agents and git

- **A worktree per task and repository.** For each repository a task targets, the office creates one worktree at the task's `<worktree>` path, on a new branch `task/<ref>-<slug>`. Agents never share a working copy. One task has at most one workspace per repository (`task_workspaces` primary key). Creation is one locked operation on the repository:
  1. Check the records: the task targets the repository and, for a project task, the project links it.
  2. Choose the branch name and validate it with `git check-ref-format`. If the name is already used in this repository by a local or remote branch, a `merged/` or `archive/` tag, or another task's record, append `-2`, `-3` and so on. The recorded name, not the task reference, identifies the branch.
  3. Resolve the base commit. For a project task it is the head of that link's `integration_branch`: the office fetches the branch and uses the commit ID the fetch reports. If the remote is unreachable it uses `refs/remotes/origin/<integration_branch>` as last fetched. If the branch does not exist, creation is refused (`unavailable`, reason `integration_branch_missing`). There is no empty-branch fallback. A task may instead start from an explicitly admitted task branch, as after a move between offices. A projectless task has no link and so no integration branch; see open question 6.
  4. Run `git worktree add -b task/<name> -- <worktree> <commit>`, never `-B`, so it fails instead of overwriting an existing branch. The start point is a commit ID, not a reference an agent could have moved.
  5. Find the administrative directory Git created, by reading `<cache>/worktrees/*/gitdir`. Git names it after the last component of the worktree path, which is the repository ID for every task, so names collide and Git appends a number. The name must be read back, not derived.
  6. Set ownership and permissions for the agent's isolation mode, derive the session's deny rules for these paths and verify them (see "Sandbox and deny rules for the selected paths").
  7. Record the branch in the task's `branches` map (`task_repositories.branch`), the workspace row and the base commit.

  A failure removes the partial worktree and records nothing. A task with several repositories gets one such operation per repository. If some succeed and others fail, the task start reports each repository's state; the office never picks one repository silently.
- **Committing.** Agents commit in their worktrees with ordinary `git`. Their Git settings come only from the session environment: no system or global configuration, the commit identity, an empty credential helper, hooks off (`core.hooksPath=/dev/null`), `core.fsmonitor=false` and automatic maintenance off. Agents have no writable Git configuration file.
- **Pushing.** Agents publish with **`agent-os repo push`**, a call to their office's local socket. In `dedicated-user` mode the socket admits the agent account, and the session binding is still required. The office:
  1. Checks that the caller's session belongs to the employee who holds the task's workspace for that repository in this office, that the task is open and that its branch record is open. A branch the agent made by hand, or one already merged, is refused.
  2. Takes the branch name from the task's record, never from the caller. It must be under `task/`. It must not be the remote's default branch or the `integration_branch` of any project's link to that repository.
  3. Under the repository lock, reads `refs/heads/task/<name>` and checks that it is a commit that descends from the recorded base commit.
  4. Pushes that commit by ID to the registered URL: `git push <url> <commit>:refs/heads/task/<name>`, with the hardening below and the office's credential. There is no `+` and no force option, so a push that is not a fast-forward is rejected by the remote and is never retried with force.
  5. Updates `refs/remotes/origin/task/<name>`, records the pushed head and emits a work change (see "Keeping offices in step").

  If the remote cannot be reached, the call fails with a retryable `unavailable` and the work stays in the worktree. The office never pushes a default or integration branch of a code repository, and never force-pushes.
- **Pull requests and merging.** v1 stops at the pushed task branch. A person opens any pull request and merges, using the host's own website or tools, or merges locally and pushes. The office learns about a merge only by fetching: when a task's pushed head is contained in the link's integration branch, it records the task's `integrated_commit` for that repository. Protection rules at the host are an optional extra guard, not the application's authorization model.
- **Commit identity:** commits are authored under the owner's name and email address, with the agent as co-author through a `Co-authored-by: <agent name> <agent address>` trailer. The office sets the author and committer through the session environment. The agent adds the trailer by following its instructions; the office does not rewrite commits. The host shows whatever identity the office's credential authenticates as the pusher. Where the owner's commit name and email are stored is open question 5.
- **When a task moves:** handoff is not closure and never starts cleanup. The office preserves every worktree, every per-repository branch and all uncommitted work.
  - *To another employee in the same office:* after the previous session has ended and been fenced, the office moves each worktree to the new employee's path with `git worktree move` under the repository lock, deletes any `config.worktree`, updates the workspace row, and derives the deny rules again for the new paths. It never copies the old rule set under a changed prefix. A change of the employee's team path is handled the same way.
  - *To another office:* the receiving office has its own cache and cannot see this one. For a remote repository the office first pushes the task branch under the normal push rules, and the receiving office creates its worktree from that admitted branch. A push does not carry uncommitted edits or commits that fail the push check, so those must be checkpointed and transferred explicitly, or the move is refused and raised to a person. A local-only repository needs a bundle and import, or a remote. The source worktree is removed only as part of a verified transfer, never by closure cleanup. A last pushed commit alone does not establish a lossless handoff.
  - Reassignment writes the independent team and individual assignment pair atomically with the expected task revision and durable history. It does not change the accountable team.
- **When a task is done:** cleanup is eligible only after the task is closed and the work in every repository it targets is confirmed preserved. A merge while the task is still open keeps its workspaces. Cleanup runs per repository, under that repository's lock, after a fresh fetch:
  - *Merged:* if the remote task branch's head is an ancestor of the link's `integration_branch` as just fetched, the office pushes a `merged/<branch>` tag at that head, then deletes the remote branch.
  - *Closed without a merge:* the office pushes an `archive/<branch>` tag at the head first, then deletes the remote branch.
  - *The delete is a compare-and-swap:* `git push --force-with-lease=refs/heads/task/<name>:<head> <url> :refs/heads/task/<name>`. It removes the reference only if it is still at the checked head and never moves a reference to another commit, so it is not a force push in the sense of the rule above. If the remote moved, the office waits for the next fetch.
  - *Local state:* the office then removes the worktree and deletes the local branch with a compare-and-swap (`git update-ref -d refs/heads/task/<name> <head>`).
  - *Nothing is lost:* uncommitted changes, or local commits that are on neither the remote branch nor the integration branch, stop the cleanup. The worktree stays and a person is asked.
  - The office deletes only `task/*` branches it created and that have a record. It never deletes a default or integration branch, or any tag.

  The branch-record states are in the [process design](doc-34%20-%20Design-Organization-Processes-and-Standards.md#4-task-branch-records).
- **Submodules and Git LFS** are not designed here. Office commands never recurse into submodules, and no filter driver is configured.

### Hardening every office Git command

`org-git` exposes one runner, and every Git command the office runs goes through it. The runner applies all of the following; a caller cannot turn any of it off.

- **The program.** The host's installed `git`, resolved once to its real path and version-checked at startup. Agents and the office use the same program. The minimum version must support environment-supplied configuration and `safe.directory`; the exact floor is fixed when `org-git` is built. v1 targets macOS and Linux, including a container.
- **The environment** is built from nothing, not inherited. It sets `GIT_CONFIG_NOSYSTEM=1`, `GIT_CONFIG_GLOBAL=/dev/null`, `GIT_ATTR_NOSYSTEM=1`, `GIT_TERMINAL_PROMPT=0`, `GIT_NO_REPLACE_OBJECTS=1`, a fixed `PATH`, and `HOME` pointing at an empty directory the office owns, so no user configuration, credential file or SSH configuration is read.
- **The repository** is named explicitly with `GIT_DIR` (and `GIT_WORK_TREE` for a worktree). The office never lets Git discover a repository through a worktree's `.git` file.
- **Command configuration** always includes `core.hooksPath=/dev/null`, `core.fsmonitor=false`, an empty `credential.helper`, `gc.auto=0`, `maintenance.auto=false`, no submodule recursion, and `protocol.allow=never` with exactly one transport allowed for the operation: `https` or `ssh` for a remote, `file` for a local repository, none for a purely local command.
- **Checks before any command on a cache,** under the repository lock. The office compares `<cache>/config` with the canonical bytes; a difference is restored, recorded as a security event and stops the operation. It refuses a cache that has `objects/info/alternates`, a `shallow` file, `info/grafts` or anything under `refs/replace/`. For a command that touches a worktree it first deletes that worktree's `config.worktree`, and rewrites the administrative `commondir` and `gitdir` files and the worktree's `.git` file to their known values.
- **Arguments.** An explicit URL from the registration and full refspecs, never a remote name. `--` before every path or reference that came from an agent. Every reference name validated with `git check-ref-format`.
- **Remote operations** add the credential as described above. TLS verification is always on. Proxy and certificate-authority settings come only from the instance's configuration, never from the repository or from a file an agent can write.
- **Never:** a force push, a `+` refspec on push, `--mirror`, `--all`, or a tag push other than the one named tag.

### Sandbox and deny rules for the selected paths

These rules are derived for the selected paths. They are not the earlier allow and deny lists with a new prefix, and the office verifies them against the real paths before it launches a session ([folder and privacy boundaries](doc-62%20-%20Design-Documentation-Authority-Map.md#folder-and-privacy-boundaries), [runtime location](../organization-data/doc-78%20-%20Organization-Data-04-—-Agent-Structure.md#runtime-location)). A runtime that cannot enforce a required rule must say so and the launch is refused, as the session contract already requires.

For a task session of one employee, with one `<worktree>` and one `<cache>` per targeted repository:

- **Writable roots:**
  - each `<worktree>`;
  - in each targeted `<cache>`: `objects/` except `objects/info/`; `refs/heads/task/`; `logs/refs/heads/task/`; and this worktree's own `worktrees/<name>/` except its `commondir`, `gitdir` and `config.worktree`.
- **Fixed write denies in every cache of the office** (`<runtime>/repositories/*.git/…`, not only the targeted ones): `config`, `config.worktree`, `hooks/`, `info/`, `objects/info/`, `HEAD`, `packed-refs`, `description`, `FETCH_HEAD`, `shallow`, `refs/remotes/`, `refs/tags/`, `refs/office/`, `refs/replace/`, every `worktrees/*/commondir`, `worktrees/*/gitdir` and `worktrees/*/config.worktree`, and the administrative directories of every other worktree. No deny is an ancestor of a writable root.
- **Fixed write deny** on each worktree's own `.git` file.
- **Other work:** no write to any other task's worktree. No read or write of another employee's workspace tree. No read or write of a cache for a repository the task does not target; the rule is one allow per targeted repository, never a wildcard over `repositories/`.
- **Managed state:** no read or write of `<data-root>/<organization-id>/durable/`, of `<runtime>/office.db`, or of another office's `runtime/` tree.
- **Read access:** the agent may read the whole of each targeted cache. It contains code and history, and nothing secret.

In `dedicated-user` mode file ownership enforces the cache write denies and the managed-state denies independently of the sandbox. The denies between tasks, between employees and between repositories still rest on the sandbox and the office's rules, because all agents share one account.

### Known limits

The single cache cannot give some guarantees the two-clone design aimed for. They are stated here instead of being papered over.

1. **No local copy that agents never write.** Agents write objects and task branches into the same cache the office reads. The office-only reference namespaces are protected by fixed denies in `same-user` mode and by file ownership as well in `dedicated-user` mode. To depend on that as little as possible, the office takes the commit IDs for decisions that matter (the base of a new branch, the head it tags, the head it deletes, containment in the integration branch) from the remote in the same locked operation whenever the remote is reachable, and pushes by commit ID.
2. **The office's credentialed Git runs in a repository agents partly write.** The cache's configuration is protected only by the denies above, by ownership in `dedicated-user` mode, and by the canonical-configuration check before every command. In `same-user` mode a process outside the sandbox could change the file between the check and the command.
3. **Objects and task branches are shared.** Any agent with access to a cache can move or delete another task's local branch, or corrupt or remove objects, because Git needs write access to the shared directories. Content addressing means an agent cannot change what an existing commit ID means, and the office refuses grafts and replacement objects. The push check, the recorded base and review catch a moved branch. Another task's unpushed commits can still be destroyed, which is why checkpoints and timely pushes matter.
4. **`same-user` is not a boundary.** Once a tool is outside the vendor sandbox it is the service account. The vendor program itself is not confined.
5. **The credential is broader and longer-lived than a per-job token.** The office's own credential lasts as long as its owner allows and may reach more than one repository. Every office that reaches a remote, satellites included, holds one. Use the narrowest credential the host offers, such as a key or token limited to one repository. Short-lived, single-repository tokens are v2.
6. **A push publishes what is on the branch.** The push check validates the task, the holder, the branch and the base. It does not inspect content, including CI configuration. Review before merge is the control.
7. **Agents can read every repository their sandbox allows.** In `dedicated-user` mode the one agent account can read any cache at the file level, so keeping an agent out of a repository its task does not target rests on the sandbox.

### Keeping offices in step

- **After a push, the office emits a work change:** "repository R, branch B is now at commit C, for task T". Offices that have a cache of R fetch when they next need it.
- **Changes made outside Agentic Org** (a person pushing or merging at the host) are picked up by a periodic fetch of each repository that has open task workspaces or is followed. There are no webhooks in v1.
- Each office reaches a remote with its own credential. No office relays Git traffic or credentials for another.

### The organization repository

The organization repository is a registered `kind: remote` repository and must stay remote. Each office reaches it the same plain way as any other remote: the same cache location, the same hardened runner and the same credential handling. One rule differs. Publishing organization content to its `state_branch` is the office's own operation under the organization-data contract, so the ban on pushing an integration or default branch, which governs agents' task branches in code repositories, does not describe it. This document defines only the transport; which office publishes, and how `durable/state/` becomes a commit, is open question 7.

## v2: hosting-provider integrations

**Not selected for v1.** Nothing here is built, and none of it is a prerequisite for the v1 part. It is kept short so that v1 work is not read as depending on it.

- **A provider app managed by HQ.** One GitHub App whose private key HQ holds in the host's credential provider. HQ mints short-lived installation tokens, each limited to one repository and to the permissions one job needs, and hands them to offices on request. Satellites keep no provider credential. The key is never in a database in plaintext or in Git content (ADR-0016).
- **Creating repositories through the provider.** HQ creates a private repository before submitting the registration, with an idempotent marker so a retry adopts the repository instead of duplicating it.
- **Pull requests and CI.** The office, or an agent holding a token that cannot push, opens a pull request for a pushed task branch and reads workflow results.
- **Promotion.** A promoted HQ needs a new provider key, and the old one is revoked.
- **Other providers and webhooks** are also v2.

When v2 is planned, it should change only how the office obtains the credential for one invocation and add the provider calls. The cache, worktree, push and cleanup rules of the v1 part stay as they are. The owner's earlier choices for this part (one app managed by HQ; new repositories private, under the owner's account) are recorded under "Decisions" and are to be confirmed again then, together with whether agents may change CI workflow files.

## Code layout (architecture rules)

Follow the [architecture rules](../doc-8%20-%20Architecture-Rules.md) and the [v1 project structure](doc-68%20-%20Design-v1-Project-Structure.md).

- **`org-git`** (`crates/org-git`): runs the host's `git` through the hardened runner. It owns cache creation and fetch, worktree add, move and remove, push, tagging, the compare-and-swap deletes and the per-repository lock. It knows paths, URLs and commit IDs. It does not know projects, tasks, the office database or the office's domain types.
- **The office's domain** defines the small contracts it needs from Git (prepare a cache, create a workspace, push a task branch, clean up) and owns the records and admission rules. It does not depend on `org-git`.
- **`agentic-org-office`** composes the two: it implements the domain's contracts by calling `org-git`, supplies the credential, and derives the session's writable roots and deny rules.
- **`agent-os`** gains the `repo` and `worktree` commands of the [agent command line](doc-28%20-%20Design-agent-os-Command-Line.md) (section 3.6), among them `repo push`. Each is a socket call only and does not depend on `org-git`.
- **`org-github`** is v2 and absent.

## Implementation plan

Every step is planned; none has started. Each step is a separate acceptance and merge unit under the [independent delivery rules](../doc-6%20-%20Independent-Delivery-and-Dependency-Boundaries.md) and the [dependency boundaries](../doc-8%20-%20Architecture-Rules.md#independent-delivery-and-dependency-boundaries). Each starts on its own branch from `origin/v1`, is reviewed locally and is merged into `v1`. The [feature checklist](../doc-3%20-%20Feature-Checklist.md) rows FC-139 and FC-140 track delivery; this plan does not replace them.

| Step | What | Kind | Depends on |
| --- | --- | --- | --- |
| G1 | The wire contract below in `org-proto`, checked against what `org-proto` actually supports | Internal foundation | Open questions 2 and 4 answered |
| G2 | `org-git`: the hardened runner, cache creation and fetch, worktree add, move and remove, push, tags and compare-and-swap deletes, the per-repository lock. Prove the smallest real path first: create a cache from a temporary remote, add a worktree, commit, push | Internal foundation | G1 |
| G3 | Office composition in `agentic-org-office`: registration and link checks, cache status, workspace and branch records, the push handler, credential supply, permissions for each isolation mode, and deny rules derived and verified for the selected paths | Internal foundation | G1, G2, open questions 1, 3 and 5 to 9 answered, the isolation launcher of ADR-0015 for `dedicated-user` |
| G4 | `agent-os repo push` | Usable outcome: an agent's committed work reaches the remote through its office | G3 |
| G5 | Web UI: register a repository, link it to a project with its integration branch, cache status per office | Usable outcome | G1, then G3 |
| G6 | End-to-end acceptance: register a remote repository, link it, start a task with two repositories, commit, push through the office, merge as a person, close, clean up; then the same for a handoff and for a local-only repository | Acceptance | All |

Tests use temporary directories only and never a user's organization workspace. `org-git` is tested against a real authenticated transport (a local server running `git http-backend` with authentication, and SSH where available), because the `file` transport would skip authentication. Required cases include: a hostile cache configuration (a credential helper, a URL rewrite, a proxy, a filter, an include) that must neither leak the credential nor take effect; a planted hook that must not run; grafts and replacement objects that must be refused; a push to a default or integration branch and a non-fast-forward push that must fail; and, in `dedicated-user` mode, the ISO-1 and ISO-7 evidence of ADR-0015 for the Git paths.

**G1 wire sketch** (a proposal; nothing here is in `org-proto` today):

- **`agent-os` to its office:** `git.push { task_id, repository_id } -> { branch, commit }`. The branch is not a parameter. The CLI may fill in `repository_id` when the task targets exactly one repository; the wire call always carries it.
- **Web to the office:** `repository.status { repository_id } -> { state: cloning | ready | failed, reason? }` for this office, and the task's per-repository workspace state in the task read model.
- **Events:** `repository.cache_status { office_id, repository_id, state, reason? }`.
- **Work change** (office-first stream): `repository.branch_pushed { repository_id, branch, commit, task_id }`.
- **Internal:** the periodic fetch, and the push before a move between offices.

Registration and project-link writes belong to the organization and project contracts, not to this sketch. Keys are always office and repository, or task and repository, never a project alone.

## Decisions

1. Hosting-provider integrations are v2. v1 is plain Git over the registered remote with the office's own Git credentials, local-only repositories, per-office caches and per-task worktrees. After a push a person opens any pull request and merges.
2. The algorithms use the selected layout: one bare cache per repository per office, many repositories per project and per task, one worktree per employee, task and repository.
3. Agents run in `same-user` or `dedicated-user` mode ([ADR-0015](../../decisions/decision-15%20-%20ADR-0015-Agent-isolation-is-a-selectable-mode-with-a-dedicated-OS-user-provisioned-at-setup.md)).
4. No application-level encryption of data; credentials and private keys are never in plaintext in a database or in Git content ([ADR-0016](../../decisions/decision-16%20-%20ADR-0016-No-application-level-encryption-of-organization-data.md)).
5. The office runs all Git that reaches a remote, and agents never hold a credential that can push. The office never pushes a default or integration branch of a code repository and never force-pushes.
6. Commits are authored as the owner, with the agent as co-author.
7. Cleanup happens only after task closure, with a `merged/<branch>` or `archive/<branch>` tag and a compare-and-swap delete. Handoff never triggers cleanup.
8. **v2, to be confirmed again:** one provider app managed by HQ, with short-lived tokens scoped to one repository; new repositories private, under the owner's account.

## Open questions (resolved 2026-11-22 for C7 / TASK-51)

All 11 design questions are settled for v1 plain-Git delivery:

1. **Git credentials (settled 2026-11-22).** Office-local remote credentials and `known_hosts` live outside `durable/`, `office.db`, and `runtime/` under the office's service-account-only `<office-home>/git-credentials/` (`0o700` dir, `0o600` files) or host credential store. HTTPS secrets are supplied per invocation via an office-controlled helper reading from an inherited descriptor/pipe (never CLI args, URLs, cache configs, or `runtime/`); SSH uses per-invocation `GIT_SSH_COMMAND` with `-F /dev/null -o BatchMode=yes -o StrictHostKeyChecking=yes -o UserKnownHostsFile=<office-known-hosts> -i <key-path>`. Local repositories need no credential.
2. **Branch and workspace records (settled 2026-11-22).** Shared organization records (`task_repositories`: `repository_id`, `branch`, `integrated_commit`) live durably on each task record under `state/work/tasks/<task-id>/task.json` and project into `task_repositories`. Office-local runtime state (`base_commit`, `pushed_head`, `branch_state`, `admin_dir_name`, `worktree_path`, `cache_state`, `last_error`) is stored under `<runtime>/<office-id>/git-state.json` (and projected into runtime `repository_caches`/`task_workspaces`) so machine-local paths never enter `durable/`.
3. **Cache writes in `dedicated-user` mode (settled 2026-11-22).** Confirm the selected one-bare-cache-plus-worktrees layout: in `dedicated-user` mode the cache sets `core.sharedRepository = group` and grants group write access only to `objects/` (excluding `objects/info/`), `refs/heads/task/`, `logs/refs/heads/task/`, and each worktree's own `worktrees/<name>/` (excluding `commondir`, `gitdir`, `config.worktree`), while supplying exact `safe.directory` entries through environment-level Git configuration.
4. **Identifier format (settled 2026-11-17).** Stable IDs are UUIDv7 here and in the [identity contract](../organization-data/doc-80%20-%20Organization-Data-06-—-Identity-and-Numbering.md).
5. **Owner commit identity & agent co-author address (settled 2026-11-22).** Commits default to the organization owner's employee `name` and `<owner-handle>@<org-slug>.local` (overridable by office-local owner Git identity config), with agent co-author trailer `Co-authored-by: <agent-name> <<agent-handle>@<org-slug>.local>`.
6. **Projectless tasks (settled 2026-11-22).** A projectless (team) task targeting a registered repository requires an explicit `base_branch` (or falls back to the repository's remote `HEAD` symbolic ref resolved via `git ls-remote --symref`, refused if absent/ambiguous). For project tasks, the project link's `integration_branch` is mandatory and authoritative.
7. **Organization publication (settled 2026-11-22 — deferred to S1 / TASK-52).** C7 implements code repository caches, task worktrees, and `task/*` branch push; multi-office `state_branch` publishing is owned by S1 (`TASK-52`) reusing `org-git`'s hardened runner.
8. **Local repositories and `repo push` (settled 2026-11-22).** For `kind: local` repositories, `git.push` pushes `refs/heads/task/<name>` into the bound `origin_path` over the `file` transport under the same recorded-base, fast-forward, and non-integration-branch rules, refusing if `origin_path`'s checked-out `HEAD` is on that branch.
9. **Code checkpoints (settled 2026-11-22).** `durable/checkpoints/<task-id>/<repo-id>/` stores a Git bundle (`git bundle create`) covering `<base>..<head>` plus an optional unified diff for uncommitted edits before cache deletion or cross-office handoff (bundle creation lives in `org-git`; cross-office transfer orchestration lands in S1 `TASK-52`).
10. **`integrated_commit` (settled 2026-11-22).** Records the task branch head commit SHA confirmed as an ancestor of `refs/remotes/origin/<integration_branch>` after fetch, remaining deterministic across merge commits and repeated fetches.
11. **Branch names for unnumbered tasks (settled).** There are none: the creating office numbers a task at once ([ADR-0019](../../decisions/decision-19%20-%20ADR-0019-Task-numbers-are-minted-by-the-creating-office.md)), so `task/<ref>-<slug>` always has its `<ref>`, for example `task/T-B37-fix-login`.

## What the code does today

`crates/org-git` implements the hardened `GitRunner`, canonical bare-cache integrity verification, `RepoLocks`, isolated per-task worktrees, non-force commit-OID push, cleanup tags/CAS deletes, checkpoint bundles, and `FC-103` sandbox rules; `org-office-server` (`GitService`), `/api/v1` BFF (`git_routes.rs`), and `org-agent-cli`/`agentic-org` (`repo`/`worktree`) compose it over `org-office`'s `project_repositories` and `task_repositories` records. Remote credential store loading, post-closure cleanup/handoff triggers, periodic fetch, and G5 Web UI remain follow-up work.
