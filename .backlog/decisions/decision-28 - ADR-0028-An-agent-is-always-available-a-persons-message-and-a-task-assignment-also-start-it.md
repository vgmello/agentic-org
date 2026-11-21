---
id: decision-28
title: >-
  ADR-0028: An agent is always available: a person's message and a task
  assignment also start it
date: '2026-11-21T00:47:00Z'
status: accepted
---

> **Review note (2026-11-21):** Strongly aligned with `doc-1` (Vision: effortless human-to-agent and agent-to-agent communication). Implementation pending in `TASK-47` (C6/C8/C10). Extended by ADR-0030 (`decision-30`) for durable pending starts, automatic restart continuation, and stall detection.

# ADR-0028 · An agent is always available: a person's message and a task assignment also start it

Status: **decided by the owner 2026-11-18**. "Anything can start an agent. If the agent
is mentioned, if the agent has a task assigned to it: agents are like a person. The
fact that they go idle is an internal system thing. They should be always active from
whoever is calling them." Implementation pending. It extends
[ADR-0023](decision-23%20-%20ADR-0023-An-agents-ask-hand-off-or-mention-and-an-answer-to-its-own-question-start-a-turn-for-an-idle-agent.md), which lets one agent start another,
and changes nothing else in it.
[ADR-0030](decision-30%20-%20ADR-0030-An-agent-works-while-it-has-work-continues-after-a-restart-and-a-stalled-turn-is-stopped.md) adds that a start which cannot
happen at once is kept and tried again, that an interrupted turn continues after a
restart, and that a stalled turn is stopped.

## Context

ADR-0023 starts a turn for an idle agent when another agent sends it an ask, a
hand-off or a mention by name, and when an answer to its own question arrives. It
leaves two things out: a message from a person, for which a person presses Run, and a
task assignment.

The owner's model is simpler. An agent behaves like a person who is always online.
Whether its process is running is the system's own business, for saving resources and
context. To anyone who calls it, an agent is there; it may take a moment to answer. A
person differs only in that they may really be away, and whoever sends, person or
agent, should see that and act on it.

## Decision

1. **A person's message starts an agent too.** A direct message to an agent, and a
   message that mentions it by name, start a turn for an idle agent whether a person or
   an agent sent it. A person no longer needs Run to be answered.
2. **A task assigned to an agent starts it,** whoever assigns it. The turn's
   instruction is the assignment.
3. **What is not addressed to the agent still starts nothing:** an announcement,
   `@here`, an ordinary post or an undirected question in a channel
   ([ADR-0024](decision-24%20-%20ADR-0024-An-agents-inbox-holds-what-is-addressed-to-it-and-each-channel-has-a-watcher-that-feeds-responses-back-to-the-agents-who-asked.md) says what reaches an
   agent's inbox at all).
4. **Everything else is ADR-0023's.** The recipient has no open turn and is not paused,
   suspended or offboarded; a busy recipient gets it at its next turn boundary and is
   never interrupted; an agent suspended on a required answer is started only by that
   answer; the turn passes the same admission; and the hourly limit of ADR-0023
   decision 5 counts these starts as well, except a start by a person, which is never
   limited.
5. **An agent shows as available.** To everyone, an agent that is not working is
   **Available**, whether or not its process is running. It is shown otherwise only
   when it really cannot answer: **Paused**, or unable to run, with the reason. No
   "asleep" or "offline" state is shown for an agent.
6. **Senders see presence and decide.** The answer to a send or an ask carries the
   recipient's presence: for a person, online or away with the time since; for an
   agent, available, working, paused or unable to run. An agent uses it as a person
   would: if the recipient is there, ask and wait; if not, and the answer is not
   blocking, send and continue; if it is blocking, mark the ask required
   ([ADR-0020](decision-20%20-%20ADR-0020-An-ask-waits-with-a-timeout-a-required-ask-suspends-the-agent-until-it-is-answered.md)). This is part of the built-in
   [agent instructions](../docs/design/doc-27%20-%20Design-Agent-Instructions-Assembly.md), because how agents interact
   is the organization's to define, and a development process may adjust it for its
   project.
7. **What started a turn is recorded** on the turn: the kind (run, message, mention,
   ask, assignment, answer, recovery), who did it and the message or task it came from
   ([document 11](../docs/organization-data/doc-88%20-%20Organization-Data-11-—-Model-Profiles-and-Agent-Execution.md)).

## Rejected

- **Keeping Run as the only way a person reaches an idle agent.** It makes an agent
  less reachable by its own lead than by another agent.
- **Every message in a channel the agent is in starts it.** A person is not paged by
  every channel message either; a mention is how one is called.

## Acceptance criteria

| ID | Passing evidence |
| --- | --- |
| AVAIL-1 | A direct message and a mention by name from a person each start a turn for an idle agent, and the turn records who and what started it. |
| AVAIL-2 | A task assigned to an idle agent starts a turn whose instruction is the assignment, from a person and from an agent. |
| AVAIL-3 | An ordinary channel post and an undirected channel question start nothing. |
| AVAIL-4 | An agent whose process is not running shows as Available to people and to agents; a paused agent shows as Paused and is not started. |
| AVAIL-5 | The answer to a send and to an ask carries the recipient's presence; for a person who is away it gives the time since. |
| AVAIL-6 | Starts by agents under this record count against ADR-0023's hourly limit; starts by people do not. |

## Consequences

- With ADR-0023, a development process runs without a person at each handoff
  ([ADR-0026](decision-26%20-%20ADR-0026-A-development-process-is-content-the-product-runs-no-methodology.md)).
- Spend follows from agents' own decisions and from every message a person sends to
  an agent. Budgets ([document 17](../docs/organization-data/doc-92%20-%20Organization-Data-17-—-Budgets-and-Controls.md))
  and the hourly limit are the bounds.
- Presence means something for agents too, and it is deliberately not the state of a
  process.
