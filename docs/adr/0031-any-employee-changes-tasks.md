# ADR-0031 · Any employee changes tasks; a private note is read by its author and by people

Status: **decided by the owner 2026-11-18**. Implementation pending. It replaces the
"initial mutation policy" paragraph of
[ADR-0006](0006-selected-task-acceptance.md) and the author-only reading rule of
private task notes. [Task assignment](../design/organization-data/12-task-assignment-and-history.md)
and [operational data](../design/organization-data/05-operational-data.md#private-notes-attached-to-tasks)
follow it.

The owner's words: "Agents should be allowed to change tasks. They can create tasks,
they can edit tasks, they can add private comments to their tasks that only they can
see, and that does not affect other agents. The private comments the people can see,
but not other agents. It is the agent's individual rationale on that task: used for
implementation plans, testing and so on. The task also has public comments: something
everyone should be aware of. They can also assign tasks to other agents, to other
people, at their discretion, and all the changes are tracked."

## Context

ADR-0006 allowed only the organization's owner to change a task, and said every other
principal needed "an explicit task-mutation capability". No document defined that
capability. The plan and the agent command line assumed agents and the other people
create, edit and assign tasks. Private task notes were readable by their author only,
so a person could not read an agent's plan for a task.

## Decision

1. **Any active employee, person or agent, creates, edits, assigns and moves tasks**
   in its organization, at its own discretion. There is no separate task capability.
   Being assigned a task, leading a team or belonging to a project is not required.
2. **The office still checks every call.** It checks that the caller is an active
   employee of the organization, bound as itself, and that the change is valid for
   the task (its revision, its status rules, the assignee can take work). It refuses
   an employee that is paused, offboarded or deactivated.
3. **Every change is tracked.** Each one is a task event with the actor, the time and
   what changed ([document 05](../design/organization-data/05-operational-data.md)).
   A mistaken change is corrected by a new change, never by editing history.
4. **A task has two kinds of written record.**

   | Kind | For | Who reads it |
   | --- | --- | --- |
   | Public comment | What everyone working on the task should know: decisions, questions, results, hand-over notes | Everyone who can read the task, people and agents |
   | Private note | The author's own working record on the task: its reasoning, its implementation plan, its test notes | The author, and people. Never another agent |

5. **A private note never reaches another agent.** It is left out of every answer,
   search result, feed, notification, summary and export that another agent can
   read, and out of the instructions and context the office gives another agent,
   including when the task is reassigned. It does not change how another agent works
   on the task.
6. **A person's private note is hidden from agents the same way.** People who can
   read the task can read it.
7. **Only the author edits or deletes a private note.** A person can read an agent's
   note and cannot change it.
8. **An agent uses private notes through `agent-os task note`** (`add`, `list`,
   `update`, `remove`), and public comments through `agent-os task comment`.
9. **Limits that are not task changes are unchanged.** Deleting a task, changing a
   project's or a team's settings, and budgets keep their own rules.

## Rejected

- **A capability record for each agent.** It adds a grant to manage for something
  every employee needs to do its job.
- **Private notes readable by the author only.** A person supervising an agent needs
  to read why it did what it did.
- **Private notes readable by agents on the same task.** One agent's plan would steer
  another's work without being a decision anyone made for the task.

## Acceptance criteria

| ID | Passing evidence |
| --- | --- |
| TASKAUTH-1 | An agent with no lead appointment creates a task, edits it, assigns it to another agent and to a person, and changes its status; each change is accepted and appears in the task's history with that agent as the actor. |
| TASKAUTH-2 | A second person who is not the owner does the same. |
| TASKAUTH-3 | A paused agent's and an offboarded employee's task change is refused. |
| TASKAUTH-4 | An agent adds a private note to a task. The same agent reads it back; a person who can read the task reads it; a second agent gets nothing through the note's ID, the task's answer, search, the inbox, a channel summary or an export. |
| TASKAUTH-5 | After the task is reassigned to the second agent, that agent's turn input and every command answer it can get contain nothing of the first agent's notes. |
| TASKAUTH-6 | A person's edit or delete of an agent's private note is refused; the author's succeeds. |
| TASKAUTH-7 | A public comment by the agent is read by the second agent and by people. |

## Consequences

- The private-note store keeps its own storage, apart from the shared task history
  and Git, as before. What changes is the reading rule.
- PN-02 in document 05 changes: an administrator or another person who can read the
  task can read a note; another agent cannot.
