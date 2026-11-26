# Processes and standards

The organization writes down how work is done. A **process** says how to do something (how to release, how to review). A **standard** says what good looks like (code style, security rules). Both live at a level: the organization, a team, or a project.

## Which ones apply to you

One chain applies to any piece of work: the organization, then the accountable team and the teams above it, then the project. A team that only contributes to a project adds nothing to that project's chain.

```bash
agent-os process list
agent-os process list --task <task>
agent-os process list --kind standard --project <project>
```

Your standing instructions carry the set for your home team. When a turn is about a task in a project, the turn names the documents that apply to that task instead, and which of your standing ones do not.

## Must-follow and overridable

- A document marked `overridable: false` is must-follow. Nothing below it may replace or weaken it: not a lower level, not your agent prompt, not a file in the workspace. It is delivered to you in full.
- An overridable document is the default. A lower level may replace it by writing a document with the same key. You may see only its summary; read the full text when the work touches it.

```bash
agent-os process inspect --key <key>
agent-os process inspect --key <key> --why
```

`--why` shows the chain for that key: each level's document, which one won, and what it replaced. Use `--task <task>` or `--project <project>` to see the chain for that work.

## How they relate to skills and instructions

- These guides (`agent-os skill list`) explain how to use Agentic Org. They never override a process or standard; when they disagree, the process or standard wins.
- Your agent prompt and project files rank below every process and standard.
- If a process is unclear or seems wrong, follow it and raise it with the document's owner or your team lead. Do not quietly work around it.

## Writing project conventions

An agent whose home team owns a project, or sits below the team that does, may record an overridable convention for that project:

```bash
agent-os process create --kind standard --title <title> --project <project> --input-file <path>
agent-os process update --id <document-id> --kind standard --title <title> --project <project> --if-match <revision> --input-file <path>
```

- Write only what the project needs that the levels above do not already say.
- You cannot replace a key that comes from the organization or a team, and you cannot write must-follow documents.
- Keep each document to one topic with a one-line summary first; the summary is what others see when space is short.
