# ADR-0020 · An ask waits with a timeout; a required ask suspends the agent until it is answered

Status: **accepted by the owner 2026-11-17**. Implementation pending. The rule in
decisions 1 to 8 is the owner's, stated when he answered the open point in
[REQ-04](../specs/req-04-channel-messaging-and-question-components.md). Decisions 9 to 13
are the lead's design for it; the owner confirmed the two default values. Amends
[ADR-0003](0003-agent-messaging-office-first.md) §2.5 and §3.3,
[ADR-0014](0014-acknowledgement-classes-and-agent-os-latency.md) decision 9, the
[session design](../design/agent-sessions.md) and the [chat spec](../specs/chat-ux.md).

## Context

An agent sometimes needs an answer from a person or from another agent. Two cases are
different and the product did not tell them apart:

- The agent would like an answer and can carry on without it.
- The agent cannot continue without it.

A person may take hours to reply. An agent must not hold a model turn open for hours,
and it must not guess. The owner's rule: a waiting ask has a timeout, the agent says
whether the answer is required, and when a required answer does not come in time the
system stops the agent and starts it again, in the same context, when the answer
arrives. For the person, the agent is simply idle until they reply.

## Decision

1. **Every ask waits for a bounded time.** The default wait is an organization setting,
   10 minutes unless changed. It is the same whether the recipient is a
   person or an agent. The asker may state how long it expects a reply to take, up to
   a configured maximum.
2. **The asker says whether the answer is required.** `required: true` means: I cannot
   continue this work without the answer. The default is false.
3. **An answer in time is returned to the agent,** and it continues.
4. **Timeout, answer not required.** The ask returns a timeout. The agent evaluates:
   it may continue without the answer, ask someone else, or decide that it does need
   this answer after all and say so, which puts it on the path of decision 5.
5. **Timeout, answer required.** The system tells the agent, as the result of the ask:
   the required answer has not arrived; finish or stop any background work you started;
   then stop; you will be started again when the answer arrives. It does not stop the
   agent without telling it first.
6. **The agent winds down, then the system suspends it.** The agent closes out its
   background work and ends its turn. The system then shuts the agent's process down.
   Its context is already on disk, so nothing is lost.
7. **The question stays open while the agent is suspended.** A required ask does not
   expire under its asker. It ends when it is answered, when the asker withdraws it,
   or when a person with authority over the work cancels it.
8. **The answer starts the agent again, in the same context.** When the answer arrives,
   the system starts the agent by resuming the same session and gives it the answer as
   its next input. To the agent it is as if it had been working all along; to the
   person, the agent was idle until they replied.

The lead's design for the rule:

9. **Who does what.**
   - `agent-os` carries `required` and the expected wait on an ask, and returns one of
     four results: answered, timeout, required answer pending (decision 5), or
     withdrawn.
   - The office owns the timer, the notice to the agent, the suspension and the resume.
     It keeps a durable record of each suspended agent and the ask it is waiting on,
     so that an office restart neither loses the suspension nor misses the answer.
   - The agent decides honestly whether an answer is required and winds down when told.
10. **Background work and the grace period.** After the notice of decision 5 the agent
    has a grace period (an organization setting, 2 minutes unless changed) to end its turn.
    When the turn ends, or the grace period passes, the office stops the process the
    normal way and then checks for processes the agent started that are still running.
    It stops those too, records which, and tells the agent on resume what was stopped.
    Nothing is stopped before the notice and the grace period.
11. **A suspended agent holds nothing.** It has no open turn, no turn permit and no
    subscription slot, and it spends nothing. Its presence reads "Waiting for an answer
    from" the person or agent asked, with the time since.
12. **Resume goes through the normal start checks:** a turn permit, the budget hold and
    a pool connection. If one refuses, the agent stays suspended with that reason shown
    and the start is retried; the answer is not lost. If several required asks are
    open, the first answer resumes the agent, which is told which are still open and
    may be suspended again on them.
13. **People can see what they are holding up.** A required ask is marked as blocking
    its asker in the inbox and on the card, with how long the agent has been stopped.
    Guidance on when to mark an ask required ships with the product as built-in
    guidance, not as an organization process.
14. **A default value never answers a required ask.** For an ask that is not required and
    carries a default, the timeout result includes the default, so the agent can
    continue with it; the card then shows that nobody answered and what the agent
    continued with. A person's later answer still arrives as a message.

## How this changes the earlier rules

- ADR-0003 §2.5 set default deadlines by recipient kind (120 seconds for an agent, 24
  hours for a person). The wait is now one organization setting for both. The 24 hours
  remains the default life of a question that is **not** required: after the asker's
  wait ends, the question stays answerable until it expires, and a late answer is
  delivered as an ordinary message.
- ADR-0014 decision 9 stands: the ask itself is acknowledged like a send, and only
  waiting for the reply is slow. The wait is now bounded by this ADR.
- The session design lists what may start a turn. The answer to a required ask is
  added as one of them.

## Acceptance criteria

| ID | Passing evidence |
| --- | --- |
| ASK-1 | An ask with no stated wait uses the organization setting; a stated wait above the maximum is refused or capped as the setting says. |
| ASK-2 | A non-required ask that is not answered in time returns a timeout to the agent, which keeps running. A later answer arrives as a message. |
| ASK-3 | A required ask that is not answered in time returns the notice of decision 5; the agent's process is not stopped before its turn ends or the grace period passes. |
| ASK-4 | After suspension the agent holds no turn permit and no subscription slot, and its presence names whom it is waiting for. |
| ASK-5 | Answering the ask starts the agent in the same session with the answer as its next input; a transcript shows one continuous conversation. |
| ASK-6 | With the office restarted while an agent is suspended, the answer still resumes the agent. |
| ASK-7 | A background process still running after the grace period is stopped, and the resume input names it. |
| ASK-8 | With a budget hold in force when the answer arrives, the agent stays suspended with that reason, and resumes when the hold clears. |
| ASK-9 | The same behaviour holds when the recipient is another agent. |

## Open points

- The owner confirmed 10 minutes for the wait and 2 minutes for the grace period on 2026-11-17. The maximum wait an asker may state is still to be set.
- Whether each vendor tool reports its own background work, or the office can only see
  the processes it started. This is part of the first runtime adapter's spike.
- An ask whose asker is a person is unchanged: a person is not suspended.

## Consequences

- An agent never burns a model turn, a subscription slot or budget while it waits for
  a person.
- The office gains a durable "suspended on an ask" state and one more reason to start
  a turn. Stopping and resuming a session becomes a routine path, so the resume of
  each vendor tool must be reliable; that is already required for restarts.
- A person who never answers leaves an agent stopped. The inbox must make that visible.
