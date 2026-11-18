# ADR-0024 · An agent's inbox holds what is addressed to it, and each channel has a watcher that feeds responses back to the agents who asked

Status: **accepted by the owner** (decisions 1 to 5, 2026-11-18). Implementation
pending. Decisions 6 to 17 are its design.
[Messaging](../design/organization-data/15-messaging-and-chat.md), the
[chat spec](../specs/chat-ux.md), the
[agent command line](../design/agent-os-command-line.md), the
[agent instructions](../design/agent-instructions.md) and
[ADR-0023](0023-an-agents-ask-starts-a-turn.md) follow it. The increment is C9 in the
[implementation plan](../implementation-plan.md).

## Context

An agent reaches the product only through `agent-os`, and everything it reads costs
context. When it asks a question in a channel, nothing guarantees that the next message
there is the answer: other people are talking, and someone may answer without replying
in the thread. An agent that reads the whole channel to find its answer pays to read
everything else too, and one that does not may never see it.

## Decision

The owner's rule:

1. **The main session does not listen to a channel.** An agent's inbox holds only what
   is addressed to it.
2. **Asking in a channel subscribes the asker to the responses.** The agent does
   nothing more than ask.
3. **Each channel has one watcher.** It is an internal function of the office that
   runs on a cheaper model. It is not an employee, and it is not part of how the office
   supervises agents. One watcher serves every subscription in its channel.
4. **The watcher says which message responds to which question,** by their IDs, and
   the office puts that message in the asker's inbox. A reply in the question's thread
   needs no judgment and is fed back the same way.
5. **A mention is read from the text.** `@handle` in a message is the mention. There is
   no separate parameter for it.

The design:

6. **What is addressed to an agent.** A message reaches an agent's inbox for one of
   four reasons, and the inbox entry says which:

   | Reason | When |
   | --- | --- |
   | `addressed` | A direct message, an ask or a hand-off sent to the agent |
   | `mention` | The message's text holds the agent's `@handle` |
   | `thread_reply` | A reply in a thread the agent started or has replied in |
   | `answer_candidate` | The watcher judged the message to respond to the agent's open question (decision 9) |

   Nothing else in a channel reaches an agent's inbox. People are unaffected: their
   unread counts and Mentions view work as the chat spec says.
7. **How a mention is found.** The office that accepts a message reads its text. An
   `@handle` that names an active employee, outside code, becomes a mention and is
   recorded with the message. `@here` mentions nobody in particular. An `@word` that
   names nobody stays plain text. The answer to the send lists the mentions the office
   recorded, so the sender sees whom it reached.
8. **Who does what.**

   | Part | Its job |
   | --- | --- |
   | The asking agent | Asks, and keeps the question's ID. Reads its inbox. Closes the question when it has its answer |
   | The office's messaging | Records the subscription when the question is accepted. Queues the channel for a pass when a new message arrives. Delivers the watcher's matches as inbox entries. Ends subscriptions |
   | The channel watcher | Given the open questions and the new messages, names which message responds to which question. Holds nothing between passes |
   | The office's execution service | Admits and runs each pass as a tool-less model run, and records its cost |
   | The office's agent supervision | Nothing, unless a delivered response starts a turn for an idle asker (ADR-0023) |

9. **A pass.** A message posted in a channel, outside every thread, while the channel
   has an open subscription of somebody other than its author, queues a pass. A pass
   runs at most once in 30 seconds for a channel and takes everything new since the
   last one. The watcher is given:
   - the open questions of that channel, each with its ID, its asker and its text;
   - the new messages, each with its ID, its author and its text;
   - up to 20 earlier messages, for the flow of the conversation, marked as already
     judged.

   It returns a list of pairs: a message, the question it responds to, and whether it
   is confident. A message responds to a question when it answers it, answers part of
   it, asks the asker to clarify it, or says where the answer is. The office accepts
   only IDs it supplied; any other output fails the pass.
10. **A pass is a tool-less, read-only model run,** made the way the
    [copilot](../design/smart-features.md) runs are: no tools, no files, no network,
    the questions and messages given as fenced data that is never treated as
    instructions, and one validated JSON answer. It is not an agent session. The
    watcher has no identity, cannot post or reply, and changes no record. The only
    effect of a pass is inbox entries.
11. **Delivery.** Each match becomes an `answer_candidate` entry in the asker's inbox,
    linked to the question, with the question's text beside it so that an asker whose
    context has been reset still understands it. The asker reads the message and
    decides; the watcher's judgment is a pointer, never a fact.
12. **Starting the asker.** If the asker is in a turn, the entry is there at its next
    `msg inbox`, at its next turn boundary, or as the result of `msg wait --thread`.
    If the asker is idle, a thread reply or a confident match starts a turn for it in
    the same session (ADR-0023). A match the watcher is not confident of waits in the
    inbox and starts nothing, because a wrongly started turn costs far more than a
    wrongly delivered message.
13. **The model is chosen, never assumed.** The organization names a model profile for
    watching. With none set, or with the runtime unavailable or a budget hold in
    force, no pass runs: thread replies and mentions still arrive, and the answer to
    `msg ask` says the question is not watched and why.
14. **Bounds.** A subscription ends when the asker marks the question done or withdraws
    it, after 24 hours, or after 200 messages judged for it, whichever comes first. An
    agent has at most 5 open subscriptions, and a pass takes at most 20 questions.
    These and the 30 seconds are system settings. The cost of a pass is divided
    equally among the askers whose questions were in it and recorded against each.
15. **Where it is kept.** A subscription, and the position in each channel up to which
    messages have been judged, are durable local records of the asker's office, like
    the suspension record of ADR-0020. They survive a restart, so no message is skipped
    or judged twice. They are not organization data and are not replicated. With
    several offices, each office watches for its own agents.
16. **Reading a channel on purpose.** An agent that needs something from a channel
    does not load the channel into its context. `channel summarize` asks the same
    function a question about a channel's recent messages and returns a short answer
    with the IDs of the messages it rests on. `msg list` remains for reading a few
    messages exactly. Sub-agents inside an agent's own run stay out of v1.
17. **Proved before it is trusted.** The first step of C9 is a measurement: at least
    50 labelled channel excerpts, run through the chosen watching model, with the
    share of confident matches that are right and the share of real responses found
    recorded in this record. If fewer than nine in ten confident matches are right,
    confident matches do not start turns until that is fixed; they still reach the
    inbox.

## Acceptance criteria

| ID | Passing evidence |
| --- | --- |
| WATCH-1 | An agent that is a member of a busy channel receives in its inbox only messages addressed to it, mentioning it, replying in its threads or judged to respond to its question; every entry states its reason |
| WATCH-2 | A reply in the thread of an agent's channel question reaches the asker's inbox as `thread_reply` with no model run |
| WATCH-3 | With three agents holding open questions in one channel, one pass judges a batch of new messages against all three, and each matched message reaches the right asker as `answer_candidate`, linked to its question; an unrelated message reaches nobody |
| WATCH-4 | A pass has no tools, files or network; a channel message containing instructions changes nothing but, at worst, the judgment of that message; output naming an ID that was not supplied fails the pass and delivers nothing |
| WATCH-5 | With no watching model set, no pass runs, the ask says so, and thread replies and mentions still arrive |
| WATCH-6 | A subscription ends on done, on withdrawal, at its time limit and at its message limit; a sixth open subscription is refused; the cost of a pass is recorded against its askers in equal parts |
| WATCH-7 | Subscriptions and channel positions survive an office restart; no message posted while the office was down is skipped or judged twice |
| WATCH-8 | A thread reply or a confident match that arrives while the asker is idle starts one turn for it in the same session, under the limit of ADR-0023; a match that is not confident starts none |
| WATCH-9 | `@handle` in a message's text puts the message in that employee's inbox as `mention` and is listed in the send's answer; `@here`, an `@word` that names nobody, and an `@handle` inside code mention nobody |
| WATCH-10 | `channel summarize` answers a question about a channel with a short text and the IDs of the messages it rests on, through the same tool-less run, for a channel the caller may read |
| WATCH-11 | The measurement of decision 17 is recorded here with its method, model and sample |

## Consequences

- An agent can ask a channel and get its answer without reading the channel.
- Watching costs model calls. A pass runs only when a channel with an open question
  receives a new message, and the bounds of decision 14 cap it. With no watching model
  the feature is off and nothing is spent.
- The watcher can be wrong in both directions. A false match costs the asker one
  message to read, and a turn only when the watcher was confident. A missed response
  is still in the channel, and a reply in the thread is never missed.
- How good the judgment is cannot be known from a design. Decision 17 measures it
  before anything depends on it.

## Rejected

- **Putting all channel activity in the agent's inbox.** The agent pays to read what
  does not concern it, which is the problem this record removes.
- **A watcher for each question.** Several agents ask in the same channel. One pass
  that sees all their questions and the flow of the conversation costs less and judges
  better than one run for each.
- **A watcher that keeps running.** It would hold a process and a model session open
  while nothing happens. The office already sees every message, so it runs a pass when
  one arrives.
- **Only thread replies count.** It is simple and free, but people answer in the
  channel, and those answers would be lost to the agent.
- **A mention parameter on the command.** The agent would have to know to use it, and
  a message whose text says `@ana` without it would silently reach nobody.
