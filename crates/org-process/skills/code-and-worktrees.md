# Code, worktrees and pushes

Repository work happens in a Git worktree of your own, one per task, on the task's branch. The office prepares it and publishes your branch; you never push with your own credentials.

## Find the repository

```bash
agent-os repo list --project <project>
agent-os repo status --repo <repo>
```

`agent-os repo list` shows each repository with the project's integration branch. `agent-os repo status` reports whether the office's cache of it is ready.

## Prepare your worktree

```bash
agent-os worktree prepare --task <task> --repo <repo>
```

- The answer's `data.worktree.worktree_path` is where you work, and `data.worktree.branch` is your task branch.
- `data.writable_roots` lists the folders your sandbox may write; Git metadata in `data.fixed_write_denies` stays read-only to you.
- `--base-branch <branch>` starts the branch from another branch than the integration branch.
- Preparing again for the same task returns the same worktree; it does not reset your work.

List your worktrees with `agent-os worktree list`, or `agent-os worktree list --task <task>` for one task.

## Work

- Work only inside the worktree path. Do not edit another task's worktree or the office's caches.
- Commit with plain `git commit` inside the worktree; your author identity is already set.
- Commit in small steps with clear messages. Run the project's checks before each push.
- Do not rewrite history that you have already pushed.

## Publish

```bash
agent-os repo push --task <task>
```

The office pushes your task branch for you and answers with `data.branch` and `data.commit`. Pass `--repo <repo>` when the task has more than one repository. `git push` from the worktree is not how work is published here.

Record what you pushed on the task, for example in your closing comment (see `tasks-finalize`).

## When something is wrong

- `not_found`: check the task and repository with `agent-os task inspect --task <task>` and `agent-os repo list`.
- `unavailable` from `agent-os repo status` or `agent-os repo push`: the office cannot reach the remote right now. Retry later with the same `--idempotency-key`, or tell the task owner if it persists.
- A merge or integration decision that is not yours: ask the project lead (see `messaging-and-asks`).
