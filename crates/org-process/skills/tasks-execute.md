# Executing a task

## 1. Read before you plan

```bash
agent-os task inspect --task <task>
agent-os task deps --task <task>
agent-os process list --task <task>
```

Read the body, acceptance criteria, checklist, comments, attachments and history. If a dependency is not done, say so before you start. Download any task files you need with `agent-os file list --scope task --task <task>` and `agent-os file fetch --id <attachment-id> --output <path>`.

## 2. Inspect the live code

Plan against the code as it is now, not as the task describes it. Read the files the task references and the code around them. If the task is wrong about the code, say so in a comment before you build on it. For a repository task, prepare your own worktree (`agent-os worktree prepare --task <task> --repo <repo>`).

## 3. Private notes and the 4-Signal Decision Bar

Notes (`agent-os note add`) are private to you and the people of the organization; other agents never see them unless you `note publish` a copy. `--task` binds automatically from your active run (use `--standalone` only for cross-task notes). Aim for **0 to 3** decisions per task. Record a `--kind decision` note only when one of these four signals fires:

1. **Non-obvious trade-off**: you chose between real alternatives (`--why`, `--alternatives`).
2. **Shortcut or scope cut**: you intentionally deferred work or took a bounded shortcut (`--tag shortcut`).
3. **Unverified assumption or default**: you chose a timeout, limit or behavior without confirmation (`--tag assumption`).
4. **Costly-to-reverse constraint**: a contract, schema or cross-turn invariant later turns must preserve.

**Do NOT record** routine file edits, passing test runs, obvious single-path fixes, tool output chatter or status updates already in the checklist.

```bash
agent-os note add --kind decision --title <title> --why <text> --alternatives <text> --tag shortcut --text <text>
agent-os note add --task <task> --checkpoint --text <text>
agent-os note update --id <note-id> --if-match <revision> --input-file <path>
agent-os note publish --id <note-id> --if-match <revision>
```

For compact Turn-40 recall (default list omits `body_markdown`), fetch only headers and inspect one full body when needed:

```bash
agent-os note list --kind decision --task <task> --fields id,title,why,tags --format compact
agent-os note inspect --id <note-id>
```

## 4. Start and work in verified loops

Move the task to `in_progress` with its current revision:

```bash
agent-os task transition --task <task> --status in_progress --if-match <revision>
```

- Change a little, then verify: build, run tests, read the result. On `conflict`, re-inspect and retry with the new `--if-match`.
- Notes are for thinking; the checklist is for evidence. Tick a step only when verified.
- Attach deliverables to the task with `agent-os file upload --path <path> --scope task --task <task> --title <title> --status final` or `agent-os task update --task <task> --if-match <revision> --attach <path> --add-attachment <attachment-id> --remove-attachment <attachment-id>`.
- Commit and push on your task branch as you go.

## 5. Stop when the scope moves

If the work turns out larger than the task or needs a decision you cannot make, ask with `agent-os msg ask`, block the task if you cannot continue, and propose follow-up work as a new task:

```bash
agent-os task transition --task <task> --status blocked --blocked-reason <text> --if-match <revision>
```

When the work is done, follow `tasks-finalize`.
