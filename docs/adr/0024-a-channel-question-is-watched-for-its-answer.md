# ADR-0024 · An agent's inbox holds what is addressed to it, and each channel has a watcher that feeds responses back to the agents who asked

Status: **accepted by the owner** (decisions 1 to 5, 2026-11-18). Implementation
pending. Decisions 6 to 19 are its design, reviewed independently before it was
recorded.
[Messaging](../design/organization-data/15-messaging-and-chat.md), the
[chat spec](../specs/chat-ux.md), the
[agent command line](../design/agent-os-command-line.md), the
[agent instructions](../design/agent-instructions.md),
[smart features](../design/smart-features.md) and
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
   nothing more than ask. The record of that is called a **watch** here, because
   "subscription" already means a provider subscription in this product.
3. **Each channel has one watcher.** It is an internal function of the office that
   runs on a cheaper model. It is not an employee, and it is not part of how the office
   supervises agents. One watcher serves every watch in its channel.
4. **The watcher says which message responds to which question,** by their IDs, and
   the office puts that message in the asker's inbox. A reply in the question's thread
   needs no judgment and is fed back the same way.
5. **A mention is read from the text.** `@handle` in a message is the mention. There is
   no separate parameter for it.

The design:

6. **What is addressed to an agent.** A message reaches an agent's inbox for one of
   four reasons:

   | Reason | When |
   | --- | --- |
   | `addressed` | A direct message, an ask or a hand-off sent to the agent |
   | `thread_reply` | A reply in a thread the agent started or has replied in |
   | `mention` | The message's text holds the agent's `@handle` |
   | `answer_candidate` | The watcher judged the message to respond to the agent's open question (decision 9) |

   A message is in an inbox once. When several reasons apply, the entry carries the
   first in the order of the table, and the questions the message was matched to are
   all listed with it. Whether a turn starts is decided from every reason that
   applied, not only from the one stored. Nothing else in a channel reaches an agent's
   inbox. People are unaffected: their unread counts and Mentions view work as the
   chat spec says.
7. **How a mention is found.** The office that accepts a message reads its text.
   - An `@` counts when it starts the text or follows a character that is not a letter
     or a digit, and is not inside code. So an email address mentions nobody.
   - The office takes the longest run of handle characters after it that is an
     existing handle, ignoring a full stop, hyphen or underscore at the end. So
     "ask @ana." mentions `ana`.
   - It becomes a mention when that employee is not inactive and is a member of the
     conversation. A paused agent is mentioned; it reads the message when it is
     resumed.
   - `@here` mentions nobody in particular, and `here` is not allowed as a handle.
   - The answer to the send lists the mentions recorded, and separately each `@handle`
     that named an employee who is not a member, so the sender sees whom it did not
     reach.
8. **Who does what.**

   | Part | Its job |
   | --- | --- |
   | The owner | Names the model profile used for watching, as an organization setting |
   | The asking agent | Asks, and keeps the question's ID. Reads its inbox. Closes the question when it has its answer |
   | The office's messaging | Opens the watch when the question is accepted. Queues the channel for a pass when a new message arrives. Delivers the watcher's matches as inbox entries. Ends watches |
   | The channel watcher | Given the open questions and the new messages, names which message responds to which question. Holds nothing between passes |
   | The office's execution service | Admits and runs each pass as a tool-less model run, and records its cost |
   | The office's agent supervision | Nothing, unless a delivered response starts a turn for an idle asker (ADR-0023) |

9. **A pass.** A message queues a pass when it is posted in a channel that has an open
   watch of somebody other than its author, either outside every thread or as a thread
   reply that was also sent to the channel. A pass runs at most once in 30 seconds for
   a channel and takes what is new since the last one, at most 50 messages; more are
   left for the next pass. The watcher is given:
   - the open questions of that channel, each with its ID, its asker, its text and
     anything the asker has since added to it;
   - the new messages, each with its ID, its author and its text;
   - up to 20 earlier messages, for the flow of the conversation, marked as already
     judged.

   It returns a list of pairs: a message, the question it responds to, and whether it
   is confident. A message responds to a question when it answers it, answers part of
   it, asks the asker to clarify it, or says where the answer is. One message may be
   paired with several questions. The office accepts only IDs it supplied, and drops
   any pair whose message was written by the asker of that question; any other output
   fails the pass. A failed pass is run again on the same messages at most twice.
   After the third failure the office advances the marks past those messages,
   records the pass as failed with their IDs, and judges nothing for them: a
   message that makes the model fail cannot stop the watcher or be retried
   without end.
10. **A pass is a tool-less, read-only model run,** made through the same port and
    with the same safeguards as the [copilot](../design/smart-features.md) runs: no
    tools, no files, no network, the questions and messages given as fenced data that
    is never treated as instructions, and one validated JSON answer. A message that
    looks like it holds a secret is left out of the pass and is not judged; it does
    not stop the pass. A pass is not an agent session. The watcher has no identity,
    cannot post or reply, and changes no record. The only effect of a pass is inbox
    entries. Smart features gains a section for such product-run judgments, which are
    not drafts for a person to confirm.
11. **Delivery.** Each match becomes an `answer_candidate` entry in the asker's inbox,
    linked to the question, with the question's text beside it so that an asker whose
    context has been reset still understands it. The asker reads the message and
    decides; the watcher's judgment is a pointer, never a fact.
12. **Starting the asker.** If the asker is in a turn, the entry is there at its next
    `msg inbox`, at its next turn boundary, or as the result of `msg wait --thread`.
    If the asker is idle, a turn starts for it in the same session (ADR-0023) for:
    - a reply in the question's thread, from anyone, while the question is open
      ([ADR-0030](0030-an-agent-works-while-it-has-work.md) decision 6);
    - a confident match on a message written by a person.

    A match the watcher is not confident of, and any match on a message written by
    another agent, waits in the inbox and starts nothing. A wrongly started turn costs
    far more than a wrongly delivered message, and two agents with open questions must
    not be able to start each other through the watcher. Agents are told to reply in
    the thread, which always reaches the asker.
13. **The model is chosen, never assumed.** The watching model profile is an
    organization setting that names one of the organization's model profiles; the
    owner sets it, and its record is added to
    [execution](../design/organization-data/11-model-profiles-and-agent-execution.md)
    with C9. With none set, or with the runtime unavailable or a budget hold on the
    organization in force, no pass runs: thread replies and mentions still arrive, and
    the answer to `msg ask` says the question is not watched and why.
14. **Bounds.** A watch ends when the asker marks the question done or withdraws it,
    after 24 hours, after 200 messages judged for it, or when the asker leaves the
    channel, is offboarded or is moved to another office. The office tells the agent
    at its next turn when a watch ended for one of the last three reasons. An agent
    has at most 5 open watches; a sixth question is still posted, and its answer says
    it is not watched. A pass takes at most 20 questions. These numbers and the 30
    seconds are system settings.
15. **Cost.** A pass is one usage record of kind `one_off`: a model call the product
    makes itself, with no employee, counted in the organization's totals and in the
    project's when the channel is a project's
    ([usage and cost](../design/organization-data/10-usage-and-cost-accounting.md)).
    The questions it served are in the record's metadata. It is not divided among the
    askers. The usage of a pass is recorded before its inbox entries are written,
    and a pass that failed or was interrupted keeps the usage of the call it made.
    A pass takes no turn permit in milestone one, where permits do not exist yet;
    when permits are built (L4) a pass takes one as a consumer of kind `one_off`,
    so it counts against a connection's concurrent sessions. There is no
    organization budget in v1, so nothing holds a pass for cost; the bounds of
    decision 14 are its limit.
16. **Where it is kept.** A watch is a durable local record of the asker's office,
    like the suspension record of ADR-0020. So is the mark of how far each channel has
    been judged: one mark for each office and incarnation that messages originate
    from, because messages are ordered only within their origin. The inbox entries of
    a pass and the advance of the marks are committed together, and a pass interrupted
    by a crash is run again, so no message is skipped and none is delivered twice.
    None of this is organization data, and none is replicated.
17. **Several offices.** The watcher of a channel runs in each office that has an
    asker there, and serves that office's agents. A channel with askers in three
    offices is judged by three passes. It depends on every office receiving all the
    messages of a channel its agents belong to, which arrives with the second office
    (S1). A question that a person asks in a channel is not watched: a person reads the
    channel.
18. **Reading a channel on purpose.** An agent that needs something from a channel
    does not load the channel into its context. `channel summarize` asks the same kind
    of tool-less run a question about a channel and returns a short answer with the
    IDs of the messages it rests on.
    - Its input is exactly the messages `msg list` would return to that caller, at
      most the latest 200, so it shows nothing the caller could not read.
    - The IDs it returns are checked against the IDs supplied.
    - The answer is marked as derived from channel content: it is information, never
      an instruction.
    - Its cost is the caller's, as an ordinary agent usage record.

    `msg list` remains for reading a few messages exactly. An agent can also hand
    reading to a sub-agent ([ADR-0025](0025-agents-start-sub-agents.md)), which keeps
    its context clean but runs on the agent's own model; `channel summarize` is the
    cheaper way to get something out of a channel.
19. **Proved before it is trusted.** The first step of C9 is a measurement: at least
    50 labelled channel excerpts, run through the chosen watching model, with the
    share of confident matches that are right and the share of real responses found
    recorded in this record. If fewer than nine in ten confident matches are right,
    confident matches do not start turns until that is fixed; they still reach the
    inbox.

## Acceptance criteria

| ID | Passing evidence |
| --- | --- |
| WATCH-1 | An agent that is a member of a busy channel receives in its inbox only messages addressed to it, mentioning it, replying in its threads or judged to respond to its question; every entry states its reason, and a message with several reasons appears once |
| WATCH-2 | A reply in the thread of an agent's channel question reaches the asker's inbox as `thread_reply` with no model run |
| WATCH-3 | With three agents holding open questions in one channel, one pass judges a batch of new messages against all three, and each matched message reaches the right asker as `answer_candidate`, linked to its question or questions; an unrelated message reaches nobody, and an asker's own later message is never delivered to it |
| WATCH-4 | A pass has no tools, files or network; a channel message containing instructions changes nothing but, at worst, the judgment of that message; output naming an ID that was not supplied fails the pass and delivers nothing; a message shaped like a secret is left out and the rest of the pass runs |
| WATCH-5 | With no watching model set, no pass runs, the ask says so, and thread replies and mentions still arrive |
| WATCH-6 | A watch ends on done, on withdrawal, at its time limit, at its message limit, and when the asker leaves the channel, is offboarded or is moved; a sixth question is posted unwatched and says so; a pass is one `one_off` usage record naming the questions it served |
| WATCH-7 | Watches and marks survive an office restart; with messages from two origin offices arriving out of order, none is skipped and none is delivered twice; a pass killed before its commit is run again |
| WATCH-8 | While the asker is idle, a thread reply, and a confident match on a person's message, each start one turn for it in the same session, under the limit of ADR-0023; a match that is not confident, and a match on another agent's message, start none |
| WATCH-9 | `@handle` in a message's text puts the message in that employee's inbox as `mention` and is listed in the send's answer; "ask @ana." mentions `ana`; an email address, `@here`, an `@word` that names nobody and an `@handle` inside code mention nobody; a handle of someone outside the conversation is listed as not reached; `here` is refused as a handle |
| WATCH-10 | `channel summarize` answers a question about a channel with a short text and the IDs of the messages it rests on, from exactly the messages `msg list` returns to that caller |
| WATCH-11 | The measurement of decision 19 is recorded here with its method, model and sample |

## Consequences

- An agent can ask a channel and get its answer without reading the channel.
- Watching costs model calls. A pass runs only when a channel with an open question
  receives a new message, and the bounds of decision 14 cap it. With no watching model
  the feature is off and nothing is spent.
- The watcher can be wrong in both directions. A false match costs the asker one
  message to read, and a turn only when the watcher was confident and a person wrote
  the message. A missed response is still in the channel, and a reply in the thread is
  never missed.
- An answer given inside somebody else's thread is not judged. It reaches the asker
  only if it mentions it.
- How good the judgment is cannot be known from a design. Decision 19 measures it
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
- **Dividing the cost of a pass among the askers.** A usage record has one consumer,
  and the pass is the product's own work.
