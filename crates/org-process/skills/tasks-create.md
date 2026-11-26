# Creating tasks

A task is a durable record of work someone has to do. Create one only when the work is real and nobody is tracking it yet. Work in three phases and finish each before the next.

## 1. Search first

Look for an existing task before you write a new one:

```bash
agent-os task list --query <words> --fields id,title,status --format compact
agent-os task labels --query <word>
```

If a task already covers the work, comment on it or depend on it instead of creating a duplicate:

```bash
agent-os task comment --task <task> --body <text>
```

## 2. Decide the scope

- One task is one outcome someone can verify. If the work has outcomes that can ship separately, create one task each and link them.
- If a task cannot start until another is done, record that: `agent-os task dep-add --task <task> --depends-on <other-task>`.
- Decide which team owns it (`--team-id`) and whether it is build or run work (`--work-type build` or `--work-type run`). Assign a person or agent only when you know who will do it.

## 3. Write it

Write the body to a file and pass it with `--input-file`:

```bash
agent-os task create --title <title> --team-id <team-id> --priority normal --input-file <path>
```

A good body has:

- **Why**: the problem and who has it, in a few sentences. This is what lasts; the reader will not have your context.
- **Acceptance criteria**: a short list of checks someone else can verify, each with an observable result.
- **References**: files, tasks or messages that matter, by ID or path. `--reference-files <path>` records files on the task.

Do not write an implementation plan when you create a task. The person who executes it plans against the code as it is then; a plan written now goes stale and anchors them on guesses.

Use `--labels <label>` for labels that already exist, and `--priority urgent` only for work that must interrupt other work.

## After creating

- The answer's `data` has the new task's ID and number; quote it when you tell someone.
- If you created it for someone else, tell them with a comment or a message and say why it matters.
