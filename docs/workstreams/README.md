# Workstreams: where we are and how to continue

Three lead sessions build v1 at the same time, each in its own stream. This folder is their
shared handover: what each stream has done, what is left, and what a lead starting again picks
up first. It sits apart from the requirements and plans so that the owner can delete it once
all three streams are finished. Only the session-start step in [AGENTS.md](../../AGENTS.md) points
here; when the folder is deleted, delete that line too.

The order of work and the requirements stay where they are: the
[implementation plan](../implementation-plan.md), the [UI plan](../plans/v1-ui-screens.md) (it reaches
`v1` with the UI release; until then it is on branch `v1-ui`) and [FEATURES](../FEATURES.md). This folder only records progress and points into them.

## The three streams

| Stream | Lead session | File | Scope |
| --- | --- | --- | --- |
| Backend (tracks A and B) | backend-dev | [backend.md](backend.md) | `crates/**`, the office (`apps/agentic-org-office`), the host and command-line apps (`apps/agentic-org`, `apps/agent-os`, the server side of `apps/agentic-org-web`), the wire between servers, `docs/design/**`, `docs/adr/**`, `.agent-kit`, the backend rows of FEATURES |
| Linux (track X) | linux-dev | [linux.md](linux.md) | Linux and the container: the platform crates, services, CI on both systems, the release image |
| Product UI (track U) | ui-dev | [ui.md](ui.md) | `apps/agentic-org-ui/**`, `docs/specs/**`, `docs/plans/v1-ui-*`, the UI rows of FEATURES |

A file in another stream's scope is changed only after that stream's lead hands it over
explicitly, and is handed back when done.

## Restarting a stream

A lead session that starts again:

1. Reads this README, then its own stream file (backend-dev: `backend.md`, linux-dev: `linux.md`,
   ui-dev: `ui.md`), then the "For the other streams" part of the
   other two files.
2. Starts with its file's **Next up**, using the branches and worktrees it lists.
3. Tells the other two leads it is back and what it is starting.

## Status tags

Every row in a stream file has exactly one of these:

- **Done**: the increment's whole "Done when" is met on `v1`. The evidence is the `v1` commit.
- **In progress**: started; the row says what remains. The evidence is the `v1` commit or the
  branch holding the work.
- **Not started**.
- **Blocked**: the row says why and what unblocks it.

## Rules

Leads reach each other through Claude Code's messages between sessions (to `backend-dev`,
`linux-dev` or `ui-dev`), or through the owner when a session isn't running.


- Each lead edits only its own stream file, in the same merge to `v1` that changes a status.
  This README changes only with all three leads' agreement.
- Before merging into `v1`, merge `origin/v1` into your branch, then message the other two leads.
  A lead that is mid-merge says so, and the other waits.
- One writer per file at a time. Hand a file over by message and say when it is handed back.
- When a change in one stream changes what another stream sees (a wire code, a field, a
  behaviour), message that stream's lead instead of editing their code.
- Anything a new session needs goes in the stream file. Session-local notes (handover files,
  checkpoints outside the repository) are working copies only.
