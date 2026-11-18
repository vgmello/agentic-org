# ADR-0023 · An agent is always available: what is addressed to it starts it

Status: **decided by the owner 2026-11-18**. "Anything can start an agent. If the agent
is mentioned, if the agent has a task assigned to it: agents are like a person. The
fact that they go idle is an internal system thing. They should be always active from
whoever is calling them." Implementation pending. It replaces the rule that a turn
starts only from a person's Run, the answer to a required ask, or a recovery
([agent sessions](../design/agent-sessions.md),
[run modes](../specs/agent-run-modes-ux.md) RUN-3). The file name keeps the first
wording of the decision.

## Context

Agents work with each other and with people: a lead hands a task to an engineer, an
engineer asks a lead a question, an agent mentions another in a channel, an agent asks
another project's agent for what it needs. Under the earlier rule each of these waited
for a person to press Run on the other agent.

The owner's model is that an agent behaves like a person who is always online. Whether
its process is running is the system's own business, for saving resources and context.
To anyone who calls it, an agent is there; it may take a moment to answer.

A person is different only in that they may really be away. Whoever sends, person or
agent, should see that and act on it: wait when an answer will come quickly, or send
and carry on when it will not.

## Decision

1. **What is addressed to an agent starts it,** when it has no turn open:
   - a direct message to it;
   - a mention of it;
   - an ask addressed to it;
   - a task assigned to it.
   It makes no difference whether a person or an agent did it, or where they sit in
   the organization.
2. **What is not addressed to it does not.** A channel message that does not mention
   the agent, a comment on a task it is not assigned, and a change it merely watches
   are in its inbox when it next works.
3. **An agent shows as available.** To everyone, an agent that is not working is
   **Available**, whether or not its process is running. It is shown otherwise only
   when it really cannot answer: **Paused**, or unable to run, with the reason. No
   "asleep" or "offline" state is shown for an agent.
4. **Senders see presence and decide.** A send, a mention or an ask returns the
   recipient's presence with its acknowledgement: for a person, online or away with
   the time since; for an agent, available, working, paused or unable to run.
   `agent-os who` shows the same. An agent uses it as a person would: if the recipient
   is there, ask and wait for the answer; if not, and the answer is not blocking, send
   and continue; if it is blocking, mark the ask required
   ([ADR-0020](0020-required-asks-suspend-and-resume.md)). This guidance ships as
   built-in organization guidance, because how agents interact is the organization's
   to define, and a development process may adjust it for its project.
5. **A start never interrupts.** If the agent has a turn open, what arrives waits.
   When the turn ends the office starts the next one for what is waiting, oldest
   first. One turn may deal with several waiting items.
6. **Every start passes the normal checks:** the agent is active, has an execution
   configuration and a placement, and gets a turn permit, so a budget hold stops it.
   A refused start leaves the item waiting with the reason shown, and is tried again
   when the reason clears.
7. **A paused agent is not started.** An agent suspended on a required ask is resumed
   in its session by something new addressed to it, and is told which required
   answers it is still waiting for.
8. **A limit on one agent starting another.** A system setting caps how many turns per
   hour one agent may start for another, 20 unless changed. It stops two agents from
   keeping each other running and does not limit a lead that many agents ask. Above
   it the items wait, both agents' pages say why, and the people above them see it in
   Needs you. Starts by people do not count.
9. **Who started a turn is recorded:** a person's Run, a message, a mention, an ask or
   an assignment from a named employee, an answer, or a recovery.
10. **Schedules and always-on processes stay out of v1.** An agent that is always
    available does not need its process kept running.

## Rejected

- **Only asks and assignments by an agent's leads start it,** the lead's first
  proposal. It made an agent less reachable than a person and needed rules about who
  may start whom. The owner chose the simpler model: an agent is reachable like anyone.
- **Every message in a channel the agent is in starts it.** A person is not paged by
  every channel message either; a mention is how one is called.
- **No limit.** Two agents that each answer by asking would run until a budget
  stopped them.

## Acceptance criteria

| ID | Passing evidence |
| --- | --- |
| WAKE-1 | A direct message, a mention, an ask and a task assignment each start a turn for an agent with none open, from a person and from an agent, and the Runs tab names who and what started it. |
| WAKE-2 | A channel message without a mention, and a comment on a task the agent is not assigned, start nothing and are in its inbox at its next turn. |
| WAKE-3 | An agent whose process is not running shows as Available to people and to agents; a paused agent shows as Paused and is not started. |
| WAKE-4 | Something that arrives during an open turn does not interrupt it; a turn for it starts when the open one ends. |
| WAKE-5 | With a budget hold on the agent, what arrives waits with that reason and the turn starts when the hold clears. |
| WAKE-6 | An agent suspended on a required ask is resumed by something new addressed to it and told which required answers are still open. |
| WAKE-7 | A send and an ask return the recipient's presence; for a person who is away it gives the time since. |
| WAKE-8 | Two agents that answer each other's asks with asks stop at the hourly limit without a person acting, while a third agent can still start either of them. |

## Consequences

- A development process runs without a person at each handoff. What a person still
  does is answer what is addressed to them and approve what only they may approve.
- Spend follows from agents' own decisions. Task and agent budgets
  ([document 17](../design/organization-data/17-budgets-and-controls.md)) and the
  limit of decision 8 are the bounds, and budgets become more important to build
  early.
- The inbox is the queue. No second queue is added.
- Presence now means something for agents too, and it is deliberately not the state
  of a process.
