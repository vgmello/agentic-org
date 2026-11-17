# ADR-0014 · Acknowledgement classes and the agent-os latency target

Status: **accepted in direction by the owner 2026-11-17** during the architecture
review (conflict plan entry F-02): an operation may be acknowledged once the office
database has committed it, and `agent-os` must respond in one to two milliseconds. The
list of operations in decision 3 is the lead's proposal and **needs the owner's
confirmation**. Implementation pending. Amends
[ADR-0002](0002-storage-sqlite-sqlcipher-one-writer.md) decision 4,
[ADR-0003](0003-agent-messaging-office-first.md) section 5 and the acknowledgement
sentence in [messaging data](../design/organization-data/15-messaging-and-chat.md).

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
2. **Two acknowledgement classes.**
   - **Committed.** The office answers once the write is committed in its durable
     local store, without forcing the drive. The write survives a crash of the office process. It
     can be lost only if the machine loses power or the kernel crashes before the next
     flush. The writer forces a full flush within a bounded interval after the commit
     (proposed default 50 ms; to be fixed by measurement). This class is not the
     browser API outcome `accepted` (HTTP 202), which reports a durable record.
   - **Durable.** The office answers only after the full flush. This is the existing
     rule of ADR-0002 decision 4.
3. **Which operations use which class (to confirm).**
   - Committed: message send and reply, recipient-state changes (read, done, defer)
     and status text. Reads involve no write, and presence is not persisted.
   - Durable: everything on the work-acceptance journal of ADR-0006, organization
     commands, numbering, turn permits and placement, budget actions, HQ creation, and
     account and session changes.
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

## Acceptance criteria

| ID | Passing evidence |
| --- | --- |
| ACK-1 | In session mode on the reference Mac, send and inbox meet the target of decision 1 over at least 10,000 calls, with the method and hardware recorded. |
| ACK-2 | A committed-class write is present after the office process is killed immediately after the acknowledgement. |
| ACK-3 | A durable-class operation is never acknowledged before its full flush; a fault injected between commit and flush yields no acknowledgement. |
| ACK-4 | The class of every operation is fixed in code in one place and matches decision 3; no operation's class depends on load. |
| ACK-5 | The bounded flush interval is enforced under continuous committed-class load. |

## Consequences

- Sends become fast enough that many agents can message constantly.
- For the committed class only, a message acknowledged in the moments before a power
  cut or kernel crash can be lost while its sender believes it was sent. This is a
  stated exception to "acknowledge after durable evidence".
- `agent-os` gains a second way of running, which must enforce the same binding and
  permissions as the one-shot command.
