# ADR-0023 · An ask or an assignment starts an agent

Status: **decided by the owner 2026-11-18** ("yes, agents start other agents"), with
the lead's rule below. Implementation pending. It changes the rule that a turn starts
only from a person's Run, the answer to a required ask, or a recovery
([agent sessions](../design/agent-sessions.md),
[run modes](../specs/agent-run-modes-ux.md) RUN-3).

## Context

A project that follows a development process is worked by agents that hand work to
each other: a product lead hands an issued specification to a technical lead, a lead
assigns a task to an engineer, an engineer asks the lead a question, an agent asks
another project's agent for what it needs. Under the earlier rule each handoff waited
for a person to press Run on the next agent. The owner decided that agents start
other agents.

Starting agents automatically has two risks the earlier rule avoided: cost that
nobody is watching, and two agents that keep each other running.

## Decision

1. **Two more things start a turn** for an agent that has none open:
   - **An ask addressed to it,** from a person or an agent of the same organization.
   - **A task assigned to it** by someone who directs its work: a person or agent in
     its lead chain, or a member of the team accountable for the task, which for a
     project task is the project's team.
2. **Nothing else starts one.** An ordinary message, a mention, a comment, a watcher
   notice and an assignment by anyone else wait in the agent's inbox, as today.
3. **A start never interrupts.** If the agent has an open turn, the ask or the
   assignment waits. When that turn ends the office starts the next one for what is
   waiting, oldest first. One turn may deal with several waiting items.
4. **Every start passes the normal checks:** the agent is active, has an execution
   configuration and a placement, and gets a turn permit, so a budget hold stops it.
   A refused start leaves the item waiting with the reason shown, and is tried again
   when the reason clears.
5. **A paused agent is not started.** An agent suspended on a required ask
   ([ADR-0020](0020-required-asks-suspend-and-resume.md)) is resumed in its session by
   a new ask or assignment, and is told which required answers it is still waiting
   for.
6. **A limit on starts by agents.** A system setting caps how many turns per hour
   agents may start for one agent, 20 unless changed. Above it the items wait, the
   agent shows "Too many starts by agents; press Run to continue", and the people
   above it see that in Needs you. Starts by people do not count.
7. **Who started a turn is recorded:** a person's Run, an ask from a named employee,
   an assignment of a named task by a named employee, an answer, or a recovery.
8. **Schedules and always-on agents stay out of v1.** So does starting an agent
   because it was mentioned or messaged.

## Rejected

- **Any message starts the agent.** A channel message with three mentions would start
  three agents. An ask is a deliberate request for an answer; a message is not.
- **Any assignment starts the agent.** Then anyone could run any agent by assigning
  it a task. Assignment by its leads or its project team is direction; assignment by
  others is a request that waits.
- **No limit.** Two agents that each answer by asking would run until a budget
  stopped them.

## Acceptance criteria

| ID | Passing evidence |
| --- | --- |
| WAKE-1 | An ask addressed to an idle agent starts a turn whose input is the ask, and the Runs tab names the asker. |
| WAKE-2 | A task assigned to an idle agent by its lead, or by a member of the project's team, starts a turn; the same assignment by an employee outside both does not. |
| WAKE-3 | A message, a mention and a comment start nothing. |
| WAKE-4 | An ask that arrives during an open turn does not interrupt it; a turn for it starts when the open one ends. |
| WAKE-5 | With a budget hold on the agent, the ask waits with that reason and the turn starts when the hold clears. A paused agent is not started. |
| WAKE-6 | An agent suspended on a required ask is resumed by a new ask and told which required answers are still open. |
| WAKE-7 | Above the hourly limit of starts by agents, further items wait, the agent's page says why, and a person's Run still works. |
| WAKE-8 | Two agents that answer each other's asks with asks stop at the limit without a person acting. |

## Consequences

- A development process runs without a person at each handoff. What a person still
  does is answer the asks addressed to them and approve what only they may approve.
- Spend follows from agents' own decisions. Task and agent budgets
  ([document 17](../design/organization-data/17-budgets-and-controls.md)) and the
  limit of decision 6 are the bounds, and budgets become more important to build
  early.
- The inbox is the queue. No second queue is added.
