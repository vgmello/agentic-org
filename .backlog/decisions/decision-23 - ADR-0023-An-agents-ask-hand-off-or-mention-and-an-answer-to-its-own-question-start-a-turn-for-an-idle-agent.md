---
id: decision-23
title: >-
  ADR-0023: An agent's ask, hand-off or mention, and an answer to its own
  question, start a turn for an idle agent
date: '2026-11-21T00:47:00Z'
status: accepted
---

> **Review note (2026-11-21):** Aligned with `doc-1` (Vision: autonomous agents communicating and coordinating). Implementation pending in `TASK-47` (C6/C8/C10). Extended by ADR-0028 (`decision-28`) and ADR-0030 (`decision-30`). Note `TASK-2` decoupled Doc 11 turn permits from HQ reachability so local wake-ups work offline.

# ADR-0023 · An agent's ask, hand-off or mention, and an answer to its own question, start a turn for an idle agent

Status: **accepted by the owner** (decision 1, 2026-11-18). Implementation pending.
Decisions 2 to 9 are its design. The
[run modes spec](../docs/specs/doc-50%20-%20Spec-Agent-Run-Modes-and-Availability-UX.md), the
[session design](../docs/design/doc-29%20-%20Design-Agent-Sessions-and-Runtime-Isolation.md) and the
[agent command line](../docs/design/doc-28%20-%20Design-agent-os-Command-Line.md) follow it. The increment is C8
in the [implementation plan](../docs/doc-5%20-%20v1-Implementation-Plan.md).
[ADR-0028](decision-28%20-%20ADR-0028-An-agent-is-always-available-a-persons-message-and-a-task-assignment-also-start-it.md) extends it: a direct message, a
mention and a task assignment also start an idle agent, under the same rules.
[ADR-0030](decision-30%20-%20ADR-0030-An-agent-works-while-it-has-work-continues-after-a-restart-and-a-stalled-turn-is-stopped.md) adds what happens to a start that
cannot happen at once: it is kept and tried again, never dropped.

## Context

An agent ran only when a person gave it an instruction, or when the answer to its own
required ask arrived ([ADR-0020](decision-20%20-%20ADR-0020-An-ask-waits-with-a-timeout-a-required-ask-suspends-the-agent-until-it-is-answered.md)). A message
to an idle agent waited for that agent's next turn. So a question from one agent to
another went unanswered until a person happened to start the second agent, and an
agent could not hand work to a colleague. The owner's requirement for the command line
([REQ-08](../docs/specs/doc-59%20-%20REQ-08-agent-os-CLI-and-Kernel-Primitives.md)) assumes agents consult
each other.

The built-in HR lead already works the other way: it runs when it is called and stops
when the turn ends.

## Decision

1. **An agent can wake another agent** (owner). When an agent sends an ask or a
   hand-off to an agent that is idle, the office starts one turn for the recipient.
2. **What starts a turn.**
   - A message sent by an agent, of kind `request` (an ask) or `handoff`, addressed to
     one agent: by name, or because the office resolved a team or a project to its
     lead.
   - A message sent by an agent that mentions the recipient by name.
   - An answer to a question the recipient itself asked, whoever answers: a reply in
     the question's thread, or a message written by a person that the channel's watcher
     is confident responds to it
     ([ADR-0024](decision-24%20-%20ADR-0024-An-agents-inbox-holds-what-is-addressed-to-it-and-each-channel-has-a-watcher-that-feeds-responses-back-to-the-agents-who-asked.md) decision 12). The turn
     continues the asker's session.

   Under this record nothing else does: not an announcement, not `@here`, not an
   ordinary post or an undirected question in a channel. A task assignment and a
   direct message start a turn under ADR-0028. An answer starts the asker only while
   its question is open (ADR-0030 decision 6).
3. **When.** The recipient has no open turn and is not paused, suspended or
   offboarded.
   - A busy recipient gets the message at its next turn boundary. A message never
     interrupts a turn. When the turn ends with the message still waiting, the office
     starts the next turn for it (ADR-0030 decisions 2 and 3).
   - A paused agent is never started.
   - An agent suspended on a required answer is started by that answer, and by a
     question or a hand-off from the employee it is waiting for (ADR-0030 decision
     7). Other messages wait for it.
4. **The turn is an ordinary turn.** Its instruction is the message. It passes the
   same admission as a turn a person starts: one open turn for each agent, the turn
   permit, the placement checks and, once budgets exist, the budget hold. If admission
   refuses it, no turn starts, the message waits, and the office tries the start
   again when the reason has gone (ADR-0030).
5. **A limit.** At most a set number of turns in an hour start for one agent under this
   record: one system setting, 12 unless changed. Past it, the message is delivered
   and waits, the office records that the limit held it, and the start happens when
   the hour has passed (ADR-0030 decision 5). A turn a person starts with Run is never counted or limited.
6. **A reply to a waiting asker starts nothing new.** A reply that satisfies a waiting
   ask goes to the asker's open turn. A reply to an asker that is suspended resumes it
   under ADR-0020, which is not counted against the limit. Only an answer that arrives
   when the asker holds no wait for it starts a turn (decision 2).
7. **The sender is not told to wait differently.** Its ask behaves as ADR-0020 says:
   it holds for its wait, then returns the answer, a timeout, or, when required,
   `suspended`. Whether a turn was started for the recipient is in the answer's `meta`.
8. **It is recorded.** The turn's history names what started it: the sending agent and
   the message. Its cost is the recipient's.
9. **Only the office starts a run.** There is no command by which an agent launches
   another agent's process. `runner spawn` in REQ-08 is not built.

This record is about what one agent starts for another, and about answers. What a
person's message, a person's mention and a task assignment start is decided by
[ADR-0028](decision-28%20-%20ADR-0028-An-agent-is-always-available-a-persons-message-and-a-task-assignment-also-start-it.md): they start an idle agent too. A
person can also still start a turn with Run.

The built-in HR lead is unchanged: it runs when it is called, by a person or by an
agent, as the [run modes spec](../docs/specs/doc-50%20-%20Spec-Agent-Run-Modes-and-Availability-UX.md) says.

## Acceptance criteria

| ID | Passing evidence |
| --- | --- |
| WAKE-1 | An ask from one agent to an idle agent starts exactly one turn for the recipient, whose instruction is the ask; the answer returns to the asker within its wait |
| WAKE-2 | A hand-off, and a mention by name, from one agent to an idle agent each start one turn; an announcement, `@here` and an ordinary channel post start none. A message or a mention from a person is tested by ADR-0028 (AVAIL-1) |
| WAKE-3 | A busy recipient is not interrupted and receives the message at its next turn boundary; a paused, suspended or offboarded recipient is not started |
| WAKE-4 | Past the hourly limit no turn starts, the message is delivered and waits, and the held start is recorded; it starts when the hour has passed; a person's Run is unaffected |
| WAKE-5 | Two agents that ask each other in a loop start no more turns for either than the limit allows |
| WAKE-6 | The started turn passes the same admission as a person's Run, and its history names the sending agent and the message |
| WAKE-7 | While an agent is idle and holds no wait, a reply in the thread of its own question from anyone, and a confident watcher match on a person's message, each start one turn that continues its session; a watcher match on another agent's message starts none |

## Consequences

- Agents can consult each other and hand work over without a person in between.
- Agents can now spend money without a person starting each turn. The hourly limit
  bounds it until budgets (L3) hold it properly.
- Automatic run modes for ordinary agents (Scheduled, Always on) stay out of v1.

## Rejected

- **A command to start another agent's run.** It would let an agent choose when a
  colleague's process runs, apart from any message that explains why.
- **Waking on any message.** Announcements and channel traffic would start turns nobody
  asked for.
- **No limit until budgets exist.** Two agents asking each other could run without end.
