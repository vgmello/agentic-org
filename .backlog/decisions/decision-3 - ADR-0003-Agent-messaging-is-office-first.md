---
id: decision-3
title: 'ADR-0003: Agent messaging is office-first'
date: '2026-11-21T00:43:00Z'
status: accepted
---

> **Review note (2026-11-21):** Aligned with `doc-1` (Vision). `crates/org-mailbox` implements decisions 1–3, 9, and 11 for directed messages, while `org-office` still uses legacy tables and `agent-os` still references `org-spool`. Open v1 work: deleting `crates/org-spool` (`TASK-15`), migrating conversations/channels/DMs onto `org-store` (`TASK-45`), isolating private DMs from plaintext `office.db` (`TASK-10`), and building cross-office HQ relay (`TASK-52`).

# ADR-0003 · Agent messaging is office-first

Status: **accepted**.

What is stored and where is owned by the
[messaging contract](../design/organization-data/15-messaging-and-chat.md). How fast a
send answers is owned by [ADR-0014](0014-acknowledgement-classes-and-agent-os-latency.md).
How an ask waits is owned by [ADR-0020](0020-required-asks-suspend-and-resume.md). This
record owns the decisions about who commits, routes and owns a message.

## Context

Agents work on the machine of the office that runs them and coordinate continuously.
An active organization exchanges many thousands of messages a day. Two things follow.
Messaging inside an office must not depend on any other machine being reachable, and it
must stay fast under load. People take part too, through the product's own chat.

## Decisions

1. **Office first.** An agent talks only to its own office. That office commits a send
   and gives it the next origin sequence for `(origin office, origin incarnation)` in
   the same transaction. Sending never requires HQ to be reachable.
2. **Local delivery.** For a recipient placed in the same office, delivery is complete
   at that commit, and a caller waiting in `wait` or `ask` is notified. Delivery never
   wakes a paused agent and never interrupts a busy turn.
3. **No lost or doubled wake-up.** A waiter registers before the office checks for a
   message, so a message committed in between is not missed. A reply to an ask is
   handed to exactly one waiting asker: the office claims the waiter before it commits
   the reply, commits the reply already marked read, and delivers it. If the asker's
   connection has gone, the reply goes back to unread so that it is found later. The
   waiter registry lives in memory and is rebuilt from stored state when the office
   starts.
4. **Routing goes through HQ.** A message for another office travels
   office → HQ → recipient's office over the office's outbound connection. HQ
   deduplicates by `(origin office, origin incarnation, origin sequence)`, acknowledges
   the origin, then forwards. Offices never connect to each other. Changes from one
   office are applied in the order of their origin sequence.
5. **One owner of each recipient's state.** The office where the recipient agent is
   placed is the single writer of that recipient's state: read, done, deferred, expired.
   HQ owns it for people, for the organization owner, and for recipients that were
   removed or have no office.
6. **Ownership moves with the agent.** When an agent is re-placed in another office,
   ownership of its state moves at that organization position. The previous office
   stops resolving that agent's state as soon as it applies the move, and competing
   changes are ordered by organization position first.
7. **Recipient check in two stages.** The sender's office validates the recipient
   against the organization position it has applied and records that position on the
   message. A receiving office that has not yet applied that position stores the message
   and withholds delivery until it has.
8. **One failure notice.** Only the owning office moves a message to expired or
   undeliverable, and it emits the single delivery-failed notice. Every other office
   applies that state and emits nothing.
9. **Deadlines and retries.** An ask that is not required carries an expiry. A receiving
   office treats it as live only until the expiry plus a skew allowance of at most two
   seconds; a late arrival is expired on arrival and never acted on. A required ask
   carries no expiry (ADR-0020). A retry reuses the same message ID and content: a
   matching ID returns the existing ask, and its reply if there is one; the same ID with
   a different sender or different content is refused.
10. **Urgency orders, it does not interrupt.** Priority decides order and badges. It
    never cuts a busy turn short ([chat UX](../specs/chat-ux.md#42-system-notices-and-delivery)).
11. **Triage is the recipient's own state.** Read, done and defer are changes to the
    recipient's state, made by the recipient through its own office.
12. **How an agent proves who it is.** `agent-os` connects to its office's local socket,
    which admits only the service's own operating-system user, and the agent account in
    the `dedicated-user` mode of [ADR-0015](0015-agent-isolation-modes.md). An agent the
    office runs binds with the session token the office gave that run; the office also
    checks that the caller descends from the process it started. Being admitted to the
    socket is never authorization by itself.
13. **How a person acts.** A person acts through the browser or through `agentic-org`
    in a terminal after signing in. Either way each request reaches the office with
    that person's principal and a host-issued grant
    ([host boundary design](../design/office-host-boundary.md)). There is no application
    token, and a person never uses `agent-os`.
14. **Directory and presence.** An agent can list who is in the organization and what
    they report doing, and set its own status. Presence is not stored durably.
15. **Conversations belong to the organization,** not to an office. Re-placing an agent
    does not move a conversation.

## Where the rest is decided

| Topic | Decided in |
| --- | --- |
| Files, tables, hashes, who writes which file | [Messaging contract](../design/organization-data/15-messaging-and-chat.md) |
| How fast a send answers; the long-running `agent-os` mode | [ADR-0014](0014-acknowledgement-classes-and-agent-os-latency.md) |
| How long an asker waits; required asks; defaults | [ADR-0020](0020-required-asks-suspend-and-resume.md) |
| Which message starts a turn for its recipient | [ADR-0023](0023-an-agents-ask-starts-a-turn.md) |
| What reaches an agent's inbox; mentions; the channel watcher | [ADR-0024](0024-a-channel-question-is-watched-for-its-answer.md) |
| Channels, threads, cards and what people see | [Chat UX](../specs/chat-ux.md), [REQ-04](../specs/req-04-channel-messaging-and-question-components.md) |
| Wire method names and shapes | `crates/org-proto`; they change in place until v1 ships |
| The `agent-os` commands | [Agent command line](../design/agent-os-command-line.md); what the code does today is in [`apps/agent-os/README.md`](../../apps/agent-os/README.md) |
| External chat bridges | None. The product's own chat is the only channel. |

## What the code does today

- `crates/org-mailbox` implements decisions 1 to 3, 9 and 11 for directed messages, with
  a conformance suite that runs against an in-memory store and against SQLite. That
  suite is the executable form of those rules.
- `crates/org-office` stores messages in its own tables in the office database. The
  selected design makes those tables a projection of files (messaging contract).
- `agent-os` writes to a file queue when no office is named. The file queue,
  `crates/org-spool` and the route through it are deleted
  ([implementation plan](../implementation-plan.md), B6); `agent-os` always talks to
  its office.
- One held call on an agent's connection is capped at 60 seconds. The wait of ADR-0020
  spans several held calls.
- Routing between offices (decisions 4 to 8) is designed and not built; the replication
  core it uses is `crates/org-sync`.

## Section numbers cited in code

Code comments and `apps/agent-os/README.md` cite numbered sections, `ADR-0003 §n`,
that this record no longer has. Read them with this table until that code is
replaced.

| Cited as | Now |
| --- | --- |
| §1 (storage tables) | [Messaging contract](../design/organization-data/15-messaging-and-chat.md); the tables are a projection |
| §2.1, §2.1.4 (finding the office, binding, call authorization) | Decisions 12 and 13 |
| §2.2 (waiters, reply hand-off) | Decision 3 |
| §2.3 (priority, triage, snooze, notify) | Decisions 10 and 11 |
| §2.4 (directory, presence, the 60-second cap) | Decision 14; "What the code does today" |
| §2.5 (talking to people) | Decision 5; [ADR-0020](0020-required-asks-suspend-and-resume.md) |
| §2.6 (command-line surface) | [Agent command line](../design/agent-os-command-line.md) |
| §3.1 (work stream, hand-off when an agent moves) | Decisions 1, 4 and 6 |
| §3.2 (recipient check, failure notice) | Decisions 7 and 8 |
| §3.3 (deadlines, re-ask) | Decision 9 |
| §3.4 (content hash, ordering) | [Messaging contract](../design/organization-data/15-messaging-and-chat.md) for the hash; decision 4 for ordering |
| §3.5 (offline behaviour, the file queue) | Decision 1; the file queue is deleted |
| §4 (wire additions, code ownership) | `crates/org-proto`; [project structure](../design/project-structure.md) |
| §5 (performance targets) | [ADR-0014](0014-acknowledgement-classes-and-agent-os-latency.md) |
| §6 (conversations, channels, bridges) | [Messaging contract](../design/organization-data/15-messaging-and-chat.md); [chat UX](../specs/chat-ux.md) |

## Person-started commands

A person's command line is `agentic-org`. After `agentic-org sign-in`, approved in the
browser, it offers the same office commands that `agent-os` offers an agent, from one
shared library, under the person's name. A person-started command names the instance
and the office, or uses a saved context bound to those two IDs; with neither, it fails
and says how to select one. `agent-os` is for agents only: it runs with the binding the
office injected into a run it started, and refuses without one.

## Consequences

- Messaging inside an office keeps working when HQ or the network is down.
- Every piece of recipient state has one writer, which is what lets a read or a done be
  a small local write.
- Messages between offices depend on HQ being reachable. A send still answers at once;
  delivery waits for the relay.
