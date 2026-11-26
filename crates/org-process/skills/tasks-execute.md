# Executing a task

## 1. Read before you plan

```bash
agent-os task inspect --task <task>
agent-os task deps --task <task>
```

Read the body, the acceptance criteria, the checklist, the comments and the history. `agent-os task deps` shows what this task waits on and what waits on it; if a dependency is not done, say so before you start.

Check the processes that apply to this task's project: `agent-os process list --task <task>`.

## 2. Inspect the live code

Plan against the code as it is now, not as the task describes it. Read the files the task references and the code around them. If the task is wrong about the code, say so in a comment before you build on it.

For a repository task, prepare your own worktree (see the `code-and-worktrees` guide):

```bash
agent-os worktree prepare --task <task> --repo <repo>
```

## 3. Record your plan privately

Write the plan as a private note. Notes are visible to you and the people of the organization, never to other agents:

```bash
agent-os task-note add --task <task> --input-file <path>
agent-os task-note list --task <task>
agent-os task-note update --id <note-id> --if-match <revision> --input-file <path>
```

Use the note for the plan, open questions and the choices that are not obvious. Do not record routine steps. Publish a copy to the task only when the people on it need it: `agent-os task-note publish --id <note-id> --if-match <revision>`.

## 4. Start

Move the task to `in_progress` with the revision you last read:

```bash
agent-os task transition --task <task> --status in_progress --if-match <revision>
```

If the answer is `conflict`, someone changed the task: inspect it again, then retry with the new revision.

## 5. Work in short verified loops

- Change a little, then check it: build, run the tests, read the result.
- Notes are for thinking; the checklist is for evidence. Tick a step only when its result is verified.
- Commit and push on your task branch as you go, so work is never only on your disk.

## 6. Stop when the scope moves

If the work turns out larger than the task, or needs a decision you cannot make, stop and say so instead of growing the change:

- Ask the person who owns the decision with `agent-os msg ask` (see `messaging-and-asks`).
- If you cannot continue, block the task with the reason:

```bash
agent-os task transition --task <task> --status blocked --blocked-reason <text> --if-match <revision>
```

- Propose follow-up work as a new task (see `tasks-create`) rather than doing it here.

## Record non-obvious choices

When you choose between real alternatives, write one or two lines on what you chose and why, in your note or as a task comment if others need it.

When the work is done, follow the `tasks-finalize` guide.
