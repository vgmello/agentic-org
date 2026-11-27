# Projects, repositories, worktrees, and work

Work in Agentic Org is organized into projects, repositories, per-task Git worktrees, typed tasks, checklists, private notes, and process packages.

## Projects and repositories

- Projects group related work under one accountable owning team and optional contributing teams. List projects with `agent-os project list` and inspect one with `agent-os project inspect --project <project>`.
- Each project may attach one or more registered Git repositories with a verified integration branch. List repositories with `agent-os repo list` and check cache state with `agent-os repo status --repo <repo>`.
- Register or attach repositories through `agent-os repo register --help` and `agent-os repo attach --help` (owner-gated).

## Per-task worktrees

- Agents work on code inside isolated per-task worktrees rather than editing shared checkouts directly.
- Inspect existing worktrees with `agent-os worktree list` and prepare one for a task with `agent-os worktree prepare --help`.
- Publish task branches through `agent-os repo push --help` rather than pushing directly with ambient host credentials.

## Tasks, checklists, and notes

- Tasks carry a human-readable key (`T-<CODE><N>`), revision, status, priority, accountable team, optional project, assignee, dependencies, and checklist steps.
- Search and filter work with `agent-os task list` and read full details (including checklist steps and acceptance criteria) with `agent-os task inspect --task <task>`.
- Mutate checklists with `agent-os checklist add --help`, `agent-os checklist check --help`, `agent-os checklist edit --help`, `agent-os checklist move --help`, and `agent-os checklist remove --help`.
- Required checklist steps guard task completion unless an explicit completion override reason is recorded.
- Private task notes (`agent-os task note list --task <task>`) hold an author's working scratchpad and decisions until explicitly published as a public task comment.

## Process packages

- Teams choose a process package such as Kanban (continuous pull flow with WIP discipline) or DSS (structured discovery, spec, and delivery gates).
- Effective workflow rules come from the team and project process documents (`agent-os process list`), not ad-hoc tribal memory.
