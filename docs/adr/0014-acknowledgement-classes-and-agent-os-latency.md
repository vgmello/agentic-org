# ADR-0014 · Acknowledgement classes and the agent-os latency target

Status: **accepted by the owner 2026-11-17** during the architecture review (conflict
plan entry F-02), including the operations of decision 3 and the engine-independent
contract of decision 8. Implementation pending. Amends
[ADR-0002](0002-storage-sqlite-sqlcipher-one-writer.md) decision 4,
[ADR-0003](0003-agent-messaging-office-first.md) section 5,
[ADR-0006](0006-selected-task-acceptance.md) for the task changes named in decision 3,
and the acknowledgement sentence in
[messaging data](../design/organization-data/15-messaging-and-chat.md).

## Context

- ADR-0003 section 5 targets a same-office send at a median of 15 ms. Its own
  breakdown is about 2 ms to start the `agent-os` process, 0.5 ms to connect, and 6 to
  10 ms for one full flush to the drive. Those are design estimates for an Apple
  Silicon Mac, not measurements.
- ADR-0002 decision 4 acknowledges every write only after a full flush, so that an
  acknowledged write survives a power cut. On macOS that requires the slow full-flush
  operation.
- Measured on a Linux build machine on 2026-11-17: starting a program that does nothing
  takes a median of 1.1 ms and `agent-os --version` 1.5 ms. A SQLite commit in WAL
  mode takes a median of 0.29 ms with a flush per commit and 0.013 ms without one. A
  Mac will differ, but the shape holds: the database accepting a write costs
  microseconds, and waiting for the drive and starting a process cost milliseconds.
- So a 1 to 2 ms budget cannot include a full flush, and cannot include starting a
  process for every call.

## Decision

1. **Target.** With the session mode of decision 5, an office-local `agent-os` read or
   message send completes with a median of at most 1 ms and a 95th percentile of at
   most 2 ms on the reference Mac. One-shot process mode is measured and reported; it
   is bounded by process start and is not held to this target.
   A plain send is acknowledged within single-digit milliseconds in every topology
   (owner decision 2026-11-17, conflict plan entry E-02): the target above applies to
   the sender's office whether the recipient is local or in another office, because
   relaying is never part of the acknowledgement (decision 9).
2. **Two acknowledgement classes.**
   - **Committed.** The office answers once the write is committed in its durable
     local store, without forcing the drive. The write survives a crash of the office process. It
     can be lost only if the machine loses power or the kernel crashes before the next
     flush. The writer forces a full flush within a bounded interval after the commit
     (proposed default 50 ms; to be fixed by measurement). This class is not the
     browser API outcome `accepted` (HTTP 202), which reports a durable record.
   - **Durable.** The office answers only after the full flush. This is the existing
     rule of ADR-0002 decision 4.
3. **Which operations use which class (owner decision 2026-11-17).**
   - Committed, messaging: `msg.send`, `msg.reply`, `msg.ask`, and the recipient-state
     changes `msg.read`, `msg.done` and `msg.defer`; and `status.set`.
   - Committed, non-critical task activity: `work.comment`, `work.watch` and
     `checklist.check`. A comment that opens or answers an owner question is the
     exception: it starts or ends a waiting span, so it is durable.
   - Reads involve no write, and presence is not persisted.
   - Durable: everything else. That includes task creation, status and resolution
     changes, assignment, ownership and scope changes, links, every other `checklist.*`
     method, the bug lifecycle methods,
     organization commands, numbering, turn permits and placement, budget actions, HQ
     creation, and account and session changes.
   A committed-class task change is still one atomic, recoverable operation: its
   snapshot, event and idempotency result commit together. Only the wait for the drive
   is dropped. Under a power cut such a change can be lost whole; it is never half
   applied.
4. **A caller may always ask for durable.** An explicit option upgrades a
   committed-class call to durable for a sender that needs certainty.
5. **`agent-os` gains a session mode.** One long-running `agent-os` process per agent
   session keeps its office connection open and takes requests in the existing
   `org-proto` frames. The one-shot command stays for people and scripts.
6. **Commit first, materialize after.** The point of commitment is the office's
   durable local store: the authoritative office database today, and in the selected
   storage layout a journal under `durable/`. It is never the rebuildable projection
   under `runtime/`, because losing a cache must not lose an acknowledged message. The
   protected message envelope files of document 15 are written afterwards, in the
   flush batch. This also answers the durability-cost question recorded in document 15.
7. **Measure and replace the estimates.** Before the targets are treated as met,
   measure on the reference Mac: process start, connect, committed-class commit,
   durable commit, and the full round trip in both modes. Replace the estimates in ADR-0003
   section 5 with the results. Check the binary size target on that build too; the
   Linux build measured 1.56 MB against a 1 MB target.

8. **The class belongs to the storage contract, not to the database engine.** The
   owner asked that this survive a later change of engine, for example to PostgreSQL.
   - Each domain operation declares its class once, in the crate that owns it, as a
     value of one small two-valued durability type: `org-mailbox` for messaging,
     `org-work` for task activity, and `org-office` for `status.set`, which it writes
     directly today. The classes of all operations are listed in one place per crate.
   - The storage ports carry that value with the write. Domain code never names a
     flush, a write-ahead log, a pragma or any engine setting.
   - The store adapter decides how to honour it. For another engine that is its own
     equivalent, such as turning off synchronous commit for that transaction in
     PostgreSQL. For the current SQLite store it means three changes. The writer
     batches up to 64 requests in one transaction and commits once with a full flush;
     each request needs its class on the write job, the commit must not force the
     drive, and a flush must complete before any durable reply in the batch is sent.
     `org-mailbox` has several write entry points and a port comment promising that
     success means durable; both are updated. Where the store publishes journal
     envelope files, as the work-acceptance journal does at HQ, a committed-class
     envelope may be published without a flush, a durable publication first flushes
     every earlier unflushed envelope, and recovery stops at the first missing or
     incomplete ordinal.
   - What every adapter must guarantee is stated in engine-neutral terms: a committed
     write survives a crash of the office process and is flushed within the bounded
     interval; a durable write is not acknowledged before it would survive a power
     cut. These guarantees are tested by a suite that does not depend on the engine.
   Conflict plan entry B-13 lists this as one of its steps.
9. **Send relays after it answers; ask holds with a deadline.** `msg.send` and
   `msg.reply` commit in the sender's office and return. Delivery to another office is
   relayed afterwards through HQ and retried with backoff until it is acknowledged or
   the message expires; a failure becomes a delivery notice, never a slower send.
   `msg.ask` is the one operation that holds its caller. Every ask has a deadline:
   the caller may state how long it expects a reply to take, otherwise the default
   for the recipient's kind applies (ADR-0003 §2.5: 120 seconds for an agent, 24 hours
   for a person), and the wait ends at the deadline with a timeout result. The ask itself
   is committed and acknowledged like a send; only waiting for the reply is slow.

## Acceptance criteria

| ID | Passing evidence |
| --- | --- |
| ACK-1 | In session mode on the reference Mac, send and inbox meet the target of decision 1 over at least 10,000 calls, with the method and hardware recorded. |
| ACK-2 | A committed-class write is present after the office process is killed immediately after the acknowledgement. |
| ACK-3 | A durable-class operation is never acknowledged before its full flush; a fault injected between commit and flush yields no acknowledgement. |
| ACK-4 | The class of every operation is declared in one place in its domain crate and matches decision 3; no operation's class depends on load. |
| ACK-5 | The bounded flush interval is enforced under continuous committed-class load. |
| ACK-6 | No domain crate refers to an engine-specific durability mechanism; the store adapter alone maps the two classes, and the engine-independent suite passes against it. |
| ACK-7 | A committed-class task change killed mid-write is either fully present or fully absent after restart, with its event and idempotency result consistent. |
| ACK-8 | With the recipient in another office and HQ unreachable, `msg.send` still returns within the session-mode target of decision 1, and the message is delivered after HQ returns. |
| ACK-9 | An ask with no stated deadline uses the default for the recipient's kind; an ask whose deadline passes returns a timeout to the caller, and a late reply is handled as the messaging contract states. |

## Consequences

- Sends become fast enough that many agents can message constantly.
- For the committed class only, a message, comment, checklist tick or watch acknowledged in the moments before a power
  cut or kernel crash can be lost while its sender believes it was sent. This is a
  stated exception to "acknowledge after durable evidence".
- `agent-os` gains a second way of running, which must enforce the same binding and
  permissions as the one-shot command.
