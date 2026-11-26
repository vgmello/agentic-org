# Finishing a task

Done means someone else can verify it. Finish in this order.

## 1. Verify against the acceptance criteria

Read the task again and check each criterion against objective evidence: a test that passes, a command and its output, a commit, a pushed branch. Your belief that it works is not evidence.

```bash
agent-os task inspect --task <task>
```

## 2. Tick the checklist with evidence

Tick a step only when you have checked its result:

```bash
agent-os checklist check --task <task> --step-id <step-id> --done
```

If a step turns out not to apply, say why in a comment rather than ticking it. A required step that is not ticked keeps the task from closing (`checklist_incomplete`).

## 3. Update what others read

- Docs, specs and guides that the change makes wrong are part of the work. Update them in the same branch.
- Record the files you changed: `agent-os task update --task <task> --add-modified-files <path> --if-match <revision>`.
- Push your branch: `agent-os repo push --task <task>` (see `code-and-worktrees`).

## 4. Write the closing comment

One short comment for the people on the task:

- what changed, in a sentence or two;
- the evidence for each criterion: test names, commands, commit or branch;
- anything left over, with the follow-up task you created for it.

```bash
agent-os task comment --task <task> --input-file <path>
```

## 5. Close

Move the task to `done` with the revision you last read:

```bash
agent-os task transition --task <task> --status done --if-match <revision>
```

If the work will not be done, use `--status cancelled` and give the reason with `--resolution <text>`.

After closing, answer any open messages about the task and stop. Do not keep working on a closed task; open a new one for new work.
