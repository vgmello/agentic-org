# ADR-0024 · An agent's inbox holds what is addressed to it, and a channel question is watched for its answer

Status: **accepted by the owner** (decisions 1 to 4, 2026-11-18). Implementation
pending. Decisions 5 to 14 are its design.
[Messaging](../design/organization-data/15-messaging-and-chat.md), the
[chat spec](../specs/chat-ux.md), the
[agent command line](../design/agent-os-command-line.md) and
[ADR-0023](0023-an-agents-ask-starts-a-turn.md) follow it. The increment is C9 in the
[implementation plan](../implementation-plan.md).

## Context

An agent reaches the product only through `agent-os`. When it asks a question in a
channel, nothing guarantees that the next message there is the answer: other people are
talking, and someone may answer without replying in the thread. An agent that reads
the whole channel to find its answer pays to read everything else too, and one that
does not may never see it.

## Decision

The owner's rule:

1. **The main session does not listen to a channel.** An agent's inbox holds only what
   is addressed to it.
2. **A question asked in a channel is watched.** While it is open, something other than
   the asking agent's main session looks at what is posted in that channel and decides
   whether a message answers the question.
3. **The watcher is cheap.** It uses a cheaper model than the agent. It does one thing.
4. **An answer is fed back to the asking agent** through its inbox, saying which
   question it answers. A reply in the question's thread is an answer by itself and
   needs no judgment, but it is fed back the same way.

The design:

5. **What is addressed to an agent.** A message reaches an agent's inbox for one of
   four reasons, and the inbox entry says which:

   | Reason | When |
   | --- | --- |
   | `addressed` | A direct message, an ask or a hand-off sent to the agent |
   | `mention` | A message that mentions the agent by name. `@here` is not a mention of anyone |
   | `thread_reply` | A reply in a thread the agent started or has replied in |
   | `answer_candidate` | A message the watch judged to answer the agent's open question (decision 8) |

   Nothing else in a channel reaches an agent's inbox. An agent reads a channel when it
   chooses to, with `msg list`. People are unaffected: their unread counts and Mentions
   view work as the chat spec says.
6. **Mentions are recorded.** A message carries the IDs of the employees it mentions,
   in its metadata. The office that accepts the message resolves each name; a name it
   cannot resolve is refused. An agent mentions with `msg send --mention`.
7. **A thread reply needs no model.** A reply in the question's thread goes to the
   asker's inbox as `thread_reply`. The question's card in the chat replies in the
   thread, and the built-in guidance tells agents to do the same, so this is the usual
   path.
8. **The watch.** When an agent asks a question in a channel, its office opens a watch
   for that question. For each message posted in that channel outside the thread, by
   someone other than the asker, the office asks a model one thing: does this message
   answer this question? A message judged to answer it goes to the asker's inbox as
   `answer_candidate`, linked to the question. The asker reads it and decides; the
   watch's judgment is a pointer, never a fact.
9. **The watch is the office's work, not a second agent.** The owner described a
   cheaper sub-agent with the same context. It is built as a short model run by the
   office each time new messages arrive, with nothing running in between: the office
   already sees every message, so nothing has to poll. Each run is tool-less and
   read-only, exactly as the [copilot](../design/smart-features.md) runs are: no tools,
   no files, no network, the question and the candidate messages given as fenced data
   that is never treated as instructions, and one validated JSON answer naming which
   messages answer the question. It can add an inbox entry and nothing else: it never
   posts, replies or changes a record.
10. **What the watcher sees.** The question's text and the candidate messages with
    their authors. It does not get the asker's session context; the question is the
    context it needs. It sees only a channel the asker may read.
11. **The model is chosen, never assumed.** The organization names a model profile for
    watching. With none set, or with the runtime unavailable or a budget hold in
    force, no watch opens: thread replies and mentions still arrive, and the answer to
    `msg ask` says the question is not watched and why.
12. **Bounds.** Messages are judged in batches, at most one run every 30 seconds for a
    question. A watch ends when the asker marks the question done or withdraws it,
    after 24 hours, or after 200 messages judged, whichever comes first. An agent has
    at most 5 open watches. All four numbers are system settings. Each run's cost is
    recorded against the asking agent.
13. **Feeding back.** If the asker is in a turn, the entry is there at its next
    `msg inbox`, at its next turn boundary, or as the result of `msg wait --thread`.
    If the asker is idle, the answer starts a turn for it in the same session
    (ADR-0023). A thread reply and a mention from another agent do the same.
14. **Where it is kept.** A watch is a durable local record of the asker's office,
    like the suspension record of ADR-0020. It survives a restart of the office. It is
    not organization data and is not replicated.

## Acceptance criteria

| ID | Passing evidence |
| --- | --- |
| WATCH-1 | An agent that is a member of a busy channel receives in its inbox only messages addressed to it, mentioning it, replying in its threads or judged to answer its question; every entry states its reason |
| WATCH-2 | A reply in the thread of an agent's channel question reaches the asker's inbox as `thread_reply` with no model run |
| WATCH-3 | A message posted in the channel outside the thread that answers the question reaches the asker's inbox as `answer_candidate`, linked to the question; an unrelated message does not |
| WATCH-4 | A watcher run has no tools, files or network; a channel message containing instructions changes nothing but, at worst, the judgment of that message |
| WATCH-5 | With no watching model set, no watch opens, the ask says so, and thread replies and mentions still arrive |
| WATCH-6 | A watch ends on done, on withdrawal, at its time limit and at its message limit; a sixth open watch is refused; each run's cost is recorded against the asker |
| WATCH-7 | A watch survives an office restart, and no message posted while the office was down is skipped or judged twice |
| WATCH-8 | An answer that arrives while the asker is idle starts one turn for it in the same session, under the limit of ADR-0023 |
| WATCH-9 | A message that mentions an agent by name is in that agent's inbox as `mention`; an unresolvable mention is refused at send |

## Consequences

- An agent can ask a channel and get its answer without reading the channel.
- Watching costs model calls. The bounds of decision 12 cap them, and with no watching
  model the feature is off and nothing is spent.
- The watcher can be wrong in both directions. A false match costs the asker one
  message to read. A missed answer is still in the channel, and a reply in the thread
  is never missed.

## Rejected

- **Putting all channel activity in the agent's inbox.** The agent pays to read what
  does not concern it, which is the problem this record removes.
- **A running sub-agent for each question.** It holds a process and a model session
  open while nothing is happening. The office running a short check when a message
  arrives gives the same result for less.
- **Only thread replies count.** It is simple and free, but people answer in the
  channel, and those answers would be lost to the agent.
