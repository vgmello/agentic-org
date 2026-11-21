---
id: decision-12
title: 'ADR-0012: Bounded office channel ownership and closure (CH-LIFE-1)'
date: '2026-11-21T00:43:00Z'
status: accepted
---

> **Review note (2026-11-21):** Implementation pending; channel lifecycle and browser-to-office bridge work are tracked in `TASK-5` and `TASK-16` (dedicated async multiplexed office/SSE channels in `agentic-org-web`). The `RecoveryPublicRead` use case referenced from ADR-0011 (`decision-11`) is deferred to v2 (`TASK-72`).

# ADR-0012 · Bounded office channel ownership and closure (CH-LIFE-1)

Status: accepted. Implementation pending. Completes the protected sessions of
[ADR-0006](0006-selected-task-acceptance.md) and the public recovery read of
[ADR-0011](0011-protected-recovery-delivery.md) without changing their authority,
custody or revocation rules.

## Requirements

1. **Running out of channels is ordinary.** When authenticated client channels are
   exhausted, the office keeps running and unrelated clients stay connected.
   Descriptors and threads stay bounded. An office has eight physical channels in
   total, of which at most seven are web channels, so one local slot is always
   left. The writer-control stream is a separate single stream and not one of the
   eight. A recovery public read uses the ordinary local and total budget.
2. **Host and worker count the same thing.** Both enforce the same budget. The host
   refuses the eighth web channel itself; a capacity refusal from the worker is never
   fatal to the office.
3. **A permission is not a socket.** A session's channel permission does not
   represent the physical socket's lifetime. A client may still hold a native
   descriptor after its session ends, and a closed browser or native peer must free
   its slot even while its session lives.
4. **The rendezvous is the only lifecycle source.** The worker's standard output is
   untrusted. Descriptor receipt ownership and fatal handling of malformed rights
   stay as they are. Lifecycle events go through the existing serialized writer with
   its five-second whole-frame deadline, and the worker shuts down on an I/O failure.
5. **No duplicate descriptors for liveness.** The host never keeps a copy of a client
   endpoint to watch it, because a copy suppresses EOF. The worker owns registration,
   the shutdown descriptor and the serving thread, and reports when a serving channel
   is actually disposed of. No new dependency, signing authority, browser secret or
   host-control command is added.

## Mechanism

**Shared profile.** The budget constants (8 total, 7 web) and a fixed channel
profile name live in the launch contract in `org-office-launch`, used by both ends.
The worker's ready-for-secrets message carries the mandatory field
`channel_profile = office-channels/1`. A missing or different profile is refused
before keys or descriptors are sent. Host and worker come from one signed release.

**Two lifecycle events** on the rendezvous, strictly decoded and within the existing
frame limits: `channel_closed { of: channel_id }` and
`fds_refused { of: channel_id, reason: capacity }`. Capacity is the only ordinary
refusal, and only after the identity, descriptor shape and type checks. A wrong
office, a duplicate or invalid ID, an invalid stream, truncated ancillary data, a
wrong count or rights, corruption or an unknown reason still fail closed. On a
capacity refusal the worker closes the rejected descriptor and then sends the
refusal, with no thread, registration or positive receipt. The host drops its
pending descriptor copy only on that explicit negative receipt, releases the
reservation, returns an ordinary unavailable result and keeps the office and its
other channels alive. Transfer uncertainty and timeouts still fail the endpoint and
its supervisor as before.

**Host accounting.** The host reserves a per-ID physical entry, with its kind and a
liveness witness, before sending, under one short accounting lock that counts
pending reservations in both budgets. No occupancy lock is held during the send, the
acknowledgement wait, an event write or an RPC. Pending rights ownership is bounded
separately by 16, with exact correlation. A failure before transfer is rolled back;
an uncertain transfer invalidates the endpoint. A positive `FdsReceived` comes
exactly once for a pending ID and releases the sender's copy; a refused transfer
gets exactly one negative receipt instead. Writer-control receipts never consume
ordinary capacity. A duplicate, unknown or misordered lifecycle receipt fails the
endpoint rather than granting capacity or permission.

**Worker ordering.** The worker sends the positive `FdsReceived` before serving or
disposal can publish `channel_closed`: after registration and thread creation, the
serving thread waits on a small start gate until the receipt succeeds. A spawn or
acknowledgement failure never emits a normal close for an unacknowledged channel.
After a successful start, serving drops its descriptors and registration before
exactly one close notification, including immediate EOF and the one-shot recovery
read. No registry lock is held while writing a lifecycle event. If an event write
fails, the worker shuts down the rendezvous, and transfer supervision treats the
uncertainty as fatal rather than leaking capacity. Shutdown joins the actual
threads, not only registry IDs.

**Release of a slot.** The host closes the liveness witness and removes the physical
entry only on a matching authenticated `channel_closed`, an explicit capacity
refusal, a rollback before transfer, or endpoint failure. It does not release
occupancy when a session or its channel permission is dropped. A dropped session
still revokes its grants through the existing revocation mechanism, and the peer's
physical close then frees the slot. Host-generated channel IDs are unique and never
reused, so no tombstone set is needed. A close that arrives right after the
acknowledgement, before the caller stores its witness, finds the witness already
dead, and it cannot mint grants. The witness is rechecked when opening, binding and
granting, alongside every connection, session and organization check.

**Session bound.** `Session.channels` prunes closed or dead entries before checking
its 64-entry bound. That bound limits permission records across offices; it is not an
office's physical capacity. Endpoint failure invalidates every physical witness and
pending receipt waiter.

**Control listener.** The native and web control listener uses the same per-channel
liveness witness for pending descriptor receipts and acknowledged channel IDs.
`Authentication::open_channel` returns a cloned witness with the ID and socket, which
the service passes to `Connection::send_descriptor`. Session, listener and host
physical accounting share liveness; dropping a witness never releases a live
channel. A grant requires the connection-bound exact ownership acknowledgement and a
live witness; a closed channel cannot mint grants. The listener prunes only closed
or dead acknowledged entries before its 512 limit, and counts live acknowledged
entries plus all pending receipt reservations atomically, in pending-then-acknowledged
lock order, bounded at 512 combined and 16 pending. These locks are never held across
a blocking enqueue, descriptor send or wait, and office occupancy locks are never
taken under them. A matched delayed client receipt still resolves its pending
ownership waiter even if the physical witness has died; it grants nothing new.
Pending ownership receipts are not deleted merely on socket EOF. Unknown, duplicate,
wrong-connection or timed-out receipts fail closed.

Reaching 16 pending or 512 combined is a typed capacity result. The handler returns
the existing Unavailable response for that open request, drops the newly opened
socket (the worker's close reclaims occupancy) and keeps the control connection and
other sessions alive. Malformed protocol, uncertain descriptor ownership and writer
or transport failures keep their fail-closed handling. There is no new external
control method, wider limit, removed acknowledgement or runtime override.

## Rejected alternatives

Raising the worker budget, which hides a mismatch; duplicating descriptors to poll,
which masks EOF; dropping slots when a session is removed, which counts revoked but
open channels as absent; a new external close endpoint; notifications on standard
output or accepting a missing profile; holding the count lock across the
acknowledgement, which can deadlock the receiver that resolves the wait.

## Acceptance

Real host and worker tests, with no real user roots, physical authentication or
live service:

- Seven live web channels plus the reserved local one; the eighth web channel is
  refused before descriptor transfer.
- A forced worker-side capacity refusal through a real transfer keeps the office and
  existing hello and RPC alive; malformed descriptors, identities and receipts are
  still fatal.
- Closing a peer releases occupancy with the control session alive, across more than
  512 sequential native and web control cycles, several sessions, and concurrent
  closes and opens, with no underflow or over-admission. Immediate EOF preserves the
  acknowledged-then-closed order.
- Failures before transfer, at spawn, at acknowledgement or write, and on endpoint
  death dispose of descriptor copies and neither mask EOF nor leave live grants.
- A recovery read uses the total and local slot and releases on EOF; writer control
  stays independent; after logout or revocation, denial holds while a held
  descriptor's occupancy is reported honestly.
- Strict decoding and a profile mismatch are refused before secrets.
- The listener admits correctly at the concurrent 512 boundary of pending plus
  acknowledged, returns a typed capacity result that preserves control and other
  sessions, and handles a physical close before the client acknowledgement and
  duplicate or wrong-connection acknowledgements.

No durable schema changes. The source dependency graph is unchanged: host and office
apps depend on the launch contract, never the reverse.
