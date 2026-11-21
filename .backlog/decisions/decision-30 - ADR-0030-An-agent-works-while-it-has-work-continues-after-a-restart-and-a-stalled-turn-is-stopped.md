---
id: decision-30
title: >-
  ADR-0030: An agent works while it has work, continues after a restart, and a
  stalled turn is stopped
date: '2026-11-21T00:47:00Z'
status: accepted
---

> **Review note (2026-11-21):** Implementation pending in `TASK-42` (C3 session runner/recovery) and `TASK-47` (C6/C8/C10). **Known drift vs `TASK-2`:** Decisions 2 and 3 list "HQ not reachable" as a turn-permit admission refusal; `TASK-2` decoupled Doc 11 turn permits from HQ reachability so offline offices can keep starting local turns without HQ.

# ADR-0030 · An agent works while it has work, continues after a restart, and a stalled turn is stopped

Status: **decided by the owner 2026-11-18** (decisions 1, 5 and 8). Implementation
pending. Decisions 2 to 4, 6, 7 and 9 to 12 are its design. It extends
[ADR-0023](0023-an-agents-ask-starts-a-turn.md),
[ADR-0028](0028-an-agent-is-always-available.md),
[ADR-0020](0020-required-asks-suspend-and-resume.md) and
[ADR-0025](0025-agents-start-sub-agents.md). The
[session design](../design/agent-sessions.md), the
[supervision design](../design/employee-supervision.md) and
[agent execution](../design/organization-data/11-model-profiles-and-agent-execution.md)
follow it.

The owner's words: "Agents should be almost live all the time if they have work to do.
If they don't, they should go idle. After a restart they should continue,
automatically. I have seen a lot of sub-tasks get stuck, so we need to identify when
one gets stuck or is not doing anything, and then we make everyone idle."

## Context

A review of the designs found four ways an agent could stop for good with nobody
told:

- A start that could not happen was tried once. If the agent was busy, admission
  refused it or the hourly limit held it, nothing started the agent later.
- Two agents that each required an answer from the other both suspended, and neither
  could be started by the other's question.
- Every restart of the office left each agent that was in a turn waiting for a person
  to recover it.
- A turn, or a sub-agent inside it, that stopped making progress held the agent's
  only turn without limit.

## Decision

### An agent works while it has work

1. **An agent runs while something is waiting for it and is idle otherwise** (owner).
   What waits for it is what starts it under ADR-0023 and ADR-0028: a message, a
   mention, an ask or a hand-off addressed to it, a task assigned to it, and an
   answer to its own question.
2. **A start that cannot happen now is kept.** When one of those arrives and the
   agent cannot be started now, for any reason, the office records a **pending
   start** for the agent: a durable record of the agent, the items waiting and the
   reason. The reasons are: the agent has an open turn; it is paused; it is
   suspended on a required answer; admission refuses the start (no permit, no
   connection, HQ not reachable, no route that is ready, its execution
   configuration is off, a budget hold); the hourly limit holds it; automatic starts
   are stopped for it (decision 12). The record is a file of the office's local
   records, written through the acceptance journal, so a restart does not lose it.
3. **The office tries a pending start again when its reason can have changed:**
   - the agent's turn ends, or its suspension ends;
   - a pause is lifted, or a person clears the stop of decision 12;
   - a turn permit or a pool connection is freed, or HQ is reachable again;
   - a route becomes ready (a sign-in, a binding or a driver), or the execution
     configuration is turned on;
   - a budget hold is released;
   - the hour of the limit has passed;
   - the office starts.

   One turn takes everything that is waiting: its instruction lists the items.
   Before the start, the office removes an item whose message was withdrawn or whose
   question is closed; if nothing is left, no turn starts. When the agent is
   offboarded, its pending start is discarded and each sender is told.
4. **Ending a turn means "nothing more I can do now".** The office does not start an
   agent again only because it has open tasks. It starts it when something new
   waits for it. The [agent instructions](../design/agent-instructions.md) tell an
   agent not to end its turn while it has work it can do.
5. **There is no organization-wide limit on starts** (owner). The hourly limit of
   ADR-0023 decision 5 stays, for one agent, as the guard against two agents starting
   each other without end. It only delays: what it holds starts when the hour has
   passed.
6. **A reply starts the asker only while its question is open.** A reply in the
   thread of a question that is answered, withdrawn or closed is delivered as an
   ordinary thread reply and starts nothing, unless it mentions the agent.
7. **Two agents waiting on each other do not stop each other.** An agent suspended on
   a required answer (ADR-0020) is started by that answer, and also by a question or
   a hand-off from the employee it is waiting for. It is resumed with that message
   and told that its own question is still open. This start is recorded as an `ask`
   or a `handoff`, not as a `resume`, and the hourly limit counts it, so two agents
   cannot start each other this way without end. Its presence reads "Waiting for an
   answer from" that employee; this is one of the states of ADR-0028 decision 6.

### After a restart

8. **An interrupted turn continues by itself** (owner). When the office starts and
   finds a turn that was open, it first makes sure every process of that run is
   gone. Then it resumes the same vendor session with one input: that the agent was
   interrupted by a restart, at what time, which tool call was running if one was,
   and that the agent must check the state of that work before repeating it.
   - The office knows every product change it accepted. The input lists the
     `agent-os` commands of this run that the office accepted after the last tool
     result the session holds, each with its command line and its answer, so the
     agent does not send them again. (A repeated command would carry a new
     idempotency key, because the first call never returned its key.)
   - What a shell command did is not known to the office. The agent checks.
   - The resumed turn is the same turn. It is not counted by the hourly limit.
   - It passes the normal start checks first: a turn permit and the budget hold. If
     one refuses, the turn is blocked and continues when the reason has gone.
   - The stall time of decision 11 is counted from the resume.
9. **A person is needed only when the office cannot continue safely:** it cannot
   prove the run's processes are gone, the vendor session cannot be resumed, or three
   resumes in a row fail. A resume cut short by another restart is not a failed
   resume. The turn is then `recovery_required`, the agent shows as
   unable to run with the reason, and what waits for it is kept as a pending start.
   This replaces "a crash never triggers an automatic rerun" in the supervision
   design: nothing is run again; the session is continued and the agent decides.

### A stalled turn

10. **The office watches every open turn for activity.** Activity is model output, a
    tool call starting or finishing, including one of a sub-agent, and an `agent-os`
    call from the run.
    - A `msg wait` for an open question counts while its hold lasts. A `msg wait`
      with no question does not: an agent with nothing to do ends its turn.
    - A tool action waiting for a person's approval counts while the request is
      open; the approval has its own expiry in the session design.
    - Every adapter must report the tool activity of sub-agents. For a vendor tool
      that cannot, the stall time of that adapter is set long enough for a
      sub-agent's whole run, and the adapter's notes say so.
11. **A turn with no activity for the stall time is stopped.** The stall time is a
    system setting, 15 minutes unless changed; it is longer than the longest tool
    call a vendor tool allows. The office asks the vendor tool to stop, then stops
    every process of the run, sub-agents included, records the turn as failed with
    the reason `stalled` and what was running, and the agent goes idle. A person
    sees it in the agent's history. The next turn is told that the last one stalled
    and what was stopped.
    - Where a vendor's output names sub-agents, the record says which one stalled.
    - The whole turn is stopped, not only the sub-agent: the main session was
      waiting for it and cannot continue without it.
12. **Three stalled turns in a row stop automatic starts for that agent.** It shows
    as unable to run, with the reason, until a person runs it or clears it. What
    waits for it is kept.

## Rejected

- **Trying a refused start on a timer.** It spends admission checks on nothing. The
  events of decision 3 are the only times the answer can change.
- **Starting an agent again because it has open tasks.** An agent that ended its turn
  blocked would be started in a loop.
- **An organization-wide limit on starts** (owner): an agent with work should be
  working.
- **Asking a person after every restart.** A reboot or an update would stop every
  working agent.
- **Stopping only the stalled sub-agent.** The office cannot give the main session a
  result the sub-agent never produced.

## Acceptance criteria

| ID | Passing evidence |
| --- | --- |
| LIVE-1 | A hand-off sent to an agent during its turn starts a new turn for it when that turn ends, with no person acting. |
| LIVE-2 | With one connection of one concurrent turn shared by agents A and B, A's required ask to B is answered: B starts when A's turn ends or suspends, and its answer resumes A. |
| LIVE-3 | A start held by the hourly limit happens after the hour has passed; a start refused for a budget hold happens when the hold is released. In both, the history names the reason and the time it waited. |
| LIVE-4 | With the office restarted between the arrival of a message and the start it caused, the agent still starts. |
| LIVE-5 | Three messages that arrive during one turn start one turn, whose instruction lists the three. |
| LIVE-6 | An agent that ends its turn with open tasks and nothing new waiting is not started. |
| LIVE-7 | A reply in the thread of a closed question starts no turn; the same reply with a mention starts one. |
| LIVE-8 | A and B each suspend on a required ask to the other. Both are resumed, and both questions are answered, with no person acting. |
| LIVE-9 | The office is stopped while an agent runs a shell command and started again. The agent's session continues in the same turn, its input names the interrupted command, and no person acted. A product change it had sent is not applied twice. |
| LIVE-10 | With a run process the office cannot stop, the turn is `recovery_required` and the agent shows as unable to run with that reason. |
| LIVE-11 | A turn whose sub-agent hangs is stopped after the stall time; no process of the run survives, the turn is recorded as stalled, the agent is idle, and its next turn is told. |
| LIVE-12 | A shell command that runs for less than the vendor tool's limit and prints nothing does not stall the turn. |
| LIVE-13 | After three stalled turns in a row the agent is not started automatically, and a person's Run starts it. |

## Consequences

- The office gains two durable local records: the pending start, and the count of
  stalled turns in a row for an agent.
- A restart costs each working agent one resume, which re-reads its session.
- A stalled turn loses the work of that turn that was not saved. The agent's notes,
  comments and commits made before the stall are kept.
