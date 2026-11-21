---
id: doc-20
title: 'Process: Branch, Review and Integration'
type: guide
created_date: '2026-11-21T00:16:00Z'
updated_date: '2026-11-21T00:16:00Z'
tags:
  - processes
  - git
  - branch-and-merge
---
---
process: branch-and-merge
name: Branch, review and integration
kind: process
level: organization
version: 5
owner: lead
status: active
summary: Branch from origin/v1, verify and review locally, merge into v1, then clean up.
applies_to: every agent that changes this repository
overridable: true
---

# Branch, review and integration

> [!NOTE] Review note (2026-11-21)
> Active and current. Aligned with `AGENTS.md` Rule 2 (`origin/v1` base, `--no-ff` merge into `v1`, never push `main`, post-merge cleanup). Legacy relative links will be updated in `TASK-32`.

Two engineers build v1, each with a lead agent. A lead takes an increment from the
[implementation plan](../doc-5%20-%20v1-Implementation-Plan.md) and owns it end to end: UI, backend,
database, shared contracts, documentation and integration. This page is how that work
reaches the shared branch. [AGENTS.md](../../../AGENTS.md) and the
[kit workflow](../../../.agent-kit/WORKFLOW.md) apply alongside it. No pull request and
no outside reviewer is required.

## 1. Branch and plan

- Start each increment's branch from `origin/v1`.
- Before code, write the increment's short plan: the outcome, the files you own, what
  you will delete, and the decisions you take where the documents are silent.
- One writer per file at a time. A file that two increments need is handed over
  explicitly. Concurrent writers use separate worktrees.
- Each lead keeps one active implementation branch at a time. If it is blocked,
  preserve the branch and its checkpoint and continue independent work.
- Never delete unmerged work or overwrite another writer's changes.

## 2. Implement and verify

- Use the kit's task-routing and skill-routing. Delegate bounded pieces to local
  subagents when useful, within the writer limit.
- Agree a shared interface before another increment depends on it, and update every
  affected implementation together.
- Run the checks before review:
  - Rust: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
    `cargo test --workspace`.
  - UI: type check, tests, build and the relevant browser checks, with screenshots
    where the change is visible.
  - Data contract changes: `python3 docs/design/organization-data/test_schema.py`.
  - Documentation or workflow only: the relevant kit and catalog checks, and a review
    of the changed instructions.
- Tests use temporary directories, never a real organization's data.
- Update the [feature checklist](../doc-3%20-%20Feature-Checklist.md) row with evidence.

## 3. Review locally

Use the kit review cycle on the exact commit: a fresh independent local review,
fixes for confirmed findings with checks rerun, validation by the original author that
the result still meets the intent, then a separate fresh final review. A later source
change needs current checks and review again.

## 4. Integrate

After the checks and the final review pass, the lead merges `origin/v1` into the branch,
tells the other lead sessions, then merges with `git merge --no-ff` into `v1` and pushes it. Verify the pushed commit. Then
start the next increment from the updated `origin/v1`.

- `v1` is the integration branch for all v1 work (owner, 2026-11-18). `v1-integration`
  is retired; don't branch from it or merge into it.
- Never push to `main` without an explicit release instruction.
- Clean up a branch or worktree only after confirming it is fully merged.

## 5. Clean up

Once the work is merged into `v1`, and before reporting it finished or ending the session, the
lead removes what it created for the work. Start with `git fetch --prune origin`, so every check
below reads the current remote.

Never delete `main`, `v1`, unmerged work, or a branch kept on purpose (for example a WIP branch
whose code was later removed). Clean up only what you created; tell other leads which of their
branches are merged instead of deleting them.

- **Merged:** a local branch counts as merged when
  `git merge-base --is-ancestor <branch> origin/v1` succeeds; a remote branch when
  `git merge-base --is-ancestor origin/<branch> origin/v1` succeeds.
- **Worktrees:** remove a worktree only when its current commit is merged
  (`git -C <worktree> merge-base --is-ancestor HEAD origin/v1`, which also covers a detached
  worktree) and it has no uncommitted, untracked or needed ignored files. Check `git -C <worktree> status --short --ignored` for anything to keep,
  such as a local `HANDOVER.md`. Then `git worktree remove <path>` without `--force`, and
  `git worktree prune`. A worktree with files to keep stays and is reported.
- **Local branches:** delete your merged branches with `git branch -d <branch>`, never `-D`.
- **Remote branches:** after the fetch, take `sha=$(git rev-parse origin/<branch>)`, check that
  same commit (`git merge-base --is-ancestor $sha origin/v1`), then delete with a lease on it, so
  commits pushed since are never lost:
  `git push --force-with-lease=<branch>:$sha origin --delete <branch>`.
- **Stale tracking branches:** a local branch that tracks a deleted or retired remote branch is
  deleted only if it is merged (`git branch -d`), so a later push can't recreate it. If it isn't,
  it is unmerged work: push it under a new name and keep it.
- **Unmerged work:** commit it (as WIP if needed), push it, keep it and list it in your stream's
  handover; never delete it. Leave stashes alone unless the owner says to drop them.
- **Processes:** stop background loops, dev servers and headless sessions you started.
- **Main checkout:** leave it on `v1`, clean.
- Report what you removed and what you kept.
