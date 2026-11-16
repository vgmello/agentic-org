# ADR-0012: bounded office channel ownership and closure

Status: ACTIVE, CH-LIFE-1 revision2, 2026-11-16. Owner: Codex lead/root.
Independent focused architecture recheck READY; scoped dependent implementation authorized by root.
This narrowly completes ADR0006 protected sessions and ADR0011 public recovery;
it does not replace their authority, custody, revocation or production gates.

## Requirements and observations

- P1 REQUIRED: ordinary authenticated client capacity exhaustion must not terminate
  an office or disconnect unrelated clients. Preserve bounded descriptors/threads
  and one local slot beyond the gateway allowance. Existing worker policy is
  eight physical channels total and seven gateway channels; writer-control is a
  distinct single stream, not one of these eight. Recovery public reads consume
  the ordinary local/total budget. Authority: active assignment contract and
  current worker transfer/channels.rs register. Verified source at c3ada005.
- P2 DISPROVED implementation assumption: host MAX_CHANNELS64 matches worker
  admission. Actual host Endpoint::open allows63gateway, worker rejects eighth;
  transfer::run treats register error as fatal _exit(1). This is a defect in the
  implementation, not permission to enlarge worker budgets.
- P3 DISPROVED implementation assumption: Session ChannelSlot lifetime represents
  physical socket lifetime. Session retains slots while Endpoint exists even
  after browser/native peer FD closes. Endpoint count therefore leaks until
  session end. Dropping a permission witness also cannot safely claim a still
  open physical socket was closed. Verified host channels.rs and authentication.rs.
- P4 REQUIRED: authenticated rendezvous remains the only worker lifecycle source;
  stdout is untrusted, descriptor receipt ownership and malformed-rights fatal
  handling remain. Existing bounded startup::write_event uses serialized writer,
  five-second whole-frame deadline and shuts down on I/O failure. Verified source.
- P5 REQUIRED: no host duplicates of client endpoints for liveness monitoring;
  such duplicates can suppress EOF. Worker already owns registration/shutdown FD
  and serving thread; it can report actual serving-channel disposal. No new
  dependency, signing authority, browser secret or host-control command.
- P6 UNVERIFIED until implementation checks: concurrent open, EOF, ACK/refusal,
  endpoint failure and recovery are bounded and deadlock-free. Tests below must
  falsify accounting/ordering errors. Recheck whenever these paths change.

## Selected candidate mechanism

Put physical budget constants8total/7gateway and a fixed channel profile name
in org-office-launch::v3, consumed by both endpoints. Add mandatory-known
channel_profile=office-channels/1 to ReadyForSecrets. Keep v3 framing, strict
unknown-field validation and existing identity checks. A missing/different profile
is refused BEFORE keys/descriptors are sent. Old strict hosts likewise refuse the
new readiness field. This deliberately refuses mixed old/new channel builds;
matched complete signed releases are the supported production composition.
No inference that arbitrary v3 peers implement lifecycle notifications. No v2 or
legacy task data migration. Reviewer must assess this compatibility choice.

Add two strictly decoded rendezvous events, bounded by existing frame limits:
channel_closed {of:channel_id} and fds_refused {of:channel_id,reason:capacity}.
Only capacity is an ordinary refusal, after identity/descriptor shape/type checks;
wrong office/duplicate or invalid ID/invalid stream/ancillary truncation/count/
rights/corruption/unknown refusal reason still fail closed. Worker closes its
rejected descriptor and then sends refusal; no thread/registration or positive
receipt for that transfer. Host drops its pending FD copy only on this explicit
negative ownership receipt, releases the reservation, returns ordinary unavailable
and keeps office/other channels alive. Transfer uncertainty/timeouts still fail
Endpoint and supervisor lifecycle as before.

Host reserves a per-ID physical entry with kind and liveness witness BEFORE send,
under one short accounting lock, including pending reservations in both budgets.
No occupancy lock is held during send, ACK wait, event write or RPC. Pending rights
ownership remains separately bounded by16 and exact correlation. Roll back failures
before transfer; uncertain transfer invalidates Endpoint. Positive FdsReceived is
exactly once for pending ID and releases sender's FD copy. A refused transfer gets
exactly one negative receipt instead. Writer-control receipts remain supported
but never consume ordinary capacity. Duplicate/unknown/misordered lifecycle
receipts fail Endpoint instead of granting capacity or permissions.

Worker arranges positive FdsReceived BEFORE serving/disposal can publish
channel_closed: after successful registration and thread creation, serving waits
on a small start gate until receipt succeeds. A spawn/ACK failure cannot emit a
normal close event for an unacknowledged channel. After a successful start,
serving drops its descriptors/registration before exactly one close notification,
including immediate EOF and one-shot public recovery. No registry lock is held
while writing lifecycle events. On event-write failure shut down the rendezvous;
normal transfer supervision makes uncertain lifecycle fatal rather than leaking
capacity silently. Keep joining/shutdown of actual threads, not only registry IDs.

Host closes liveness witness/removes physical entry only on matching authenticated
channel_closed, explicit capacity refusal, pretransfer rollback or Endpoint failure.
It does NOT release physical occupancy merely when a session or ChannelSlot drops:
a client may still hold the native descriptor. Dropped session still revokes its
grants using the existing Revoked mechanism; physical peer closure then frees its
slot. This keeps worker/host actual live occupancy consistent. Unique generated
IDs are not reused by the host; no growing closed-ID tombstone set is necessary.
Close may arrive immediately after ACK before caller stores its permission witness;
that witness is already dead and cannot mint grants. Recheck live witness when
opening/binding and granting, alongside all current connection/session/org checks.

Session.channels prunes truly closed/dead entries BEFORE the64entry session bound.
Treat this64 as independent cross-office permission-record bound, not office
physical capacity. Endpoint failure invalidates every physical witness and pending
receipt waiter. Cross-office64/65 boundary tests must retain the original limit.

The native and gateway control listener uses the SAME per-channel liveness witness
for pending descriptor receipts and acknowledged channel IDs, replacing the
unconditional received-ID set. This is an internal host interface change only:
Authentication::open_channel returns a cloned witness alongside ID/socket for
service/mod.rs to pass to Connection::send_descriptor. Session, listener and host
physical accounting share liveness; dropping a witness never releases a still
live physical channel. Keep connection-bound exact ownership ACK, not only worker
acceptance, as a necessary grant condition. channel_received checks both completed
ownership receipt and live witness; a closed channel cannot mint grants.

Prune only closed/dead acknowledged entries before checking the512listener limit.
Count live acknowledged entries PLUS ALL pending receipt reservations atomically
under a consistent pending→acknowledged lock order, bounded512combined and16pending.
Do not hold these locks across blocking enqueue/FDsend/wait or touch office
occupancy locks under them. A matched delayed client receipt must still resolve
the exact pending ownership waiter even if the physical witness has died; it need
not retain a dead acknowledged record and cannot grant new permissions. Never
silently delete pending ownership receipts merely on socket EOF. Unknown, duplicate,
wrong-connection or timed-out receipts retain current failclosed handling.

Genuine16pending/512combined capacity exhaustion is a distinct internal typed
capacity result. The handler returns the existing Unavailable response for that
open request, drops its newly opened socket (worker closure reclaims occupancy)
and leaves control/other sessions alive. Malformed protocol, uncertain descriptor
ownership or writer/transport failure retain existing close/failclosed handling.
Do not convert every I/O error into ordinary capacity. There is no new external
control method, wider numeric limit, removed ACK, or runtime override.

## Alternatives rejected

Raising worker budget: hides mismatch and changes resource policy. Client/worker FD
duplicates for polling: masks EOF. Dropping slots on Session removal alone: counts
revoked but still physically open channels as absent. New external close endpoint:
unnecessary authority/transport expansion. Unauthenticated stdout notifications or
accepting missing profile: loses the trust/compatibility boundary. Holding count
lock across ACK: can deadlock the lifecycle receiver needed to resolve the wait.

## Validation and remaining limits

Required real host/worker tests:7live gateway + reserved local;8thgateway refusal
BEFORE FD transfer; forced worker-side ordinary capacity refusal via actual transfer
keeps office and existing hello/RPC alive; malformed descriptor/identity/receipt
still fatal. Closing peer releases occupancy with control session alive, including
more than512sequential native AND gateway control cycles, multiple sessions and concurrent closes/opens without
underflow/overadmission. Immediate EOF preserves ACK→closed order. Pretransfer,
spawn, ACK/write, endpoint-death paths dispose FD copies and cannot mask EOF or
leave live grants. Recovery uses total/local slot and releases on EOF; writer
control stays independent; logout/revoke retains denial while held FD occupancy
is honest. Protocol strict decoding/profile-mismatch before secrets tests. Listener tests must cover concurrent pending+acknowledged512 boundary admission, typed ordinary capacity preserving control/other sessions, delayed physical-close-before-client-ACK and duplicate/wrong-connection ACK. No
real user roots, physical authentication or live service use in tests.

Run targeted affected tests/fmt/Clippy, structural gate, then full current workspace,
kit/UI and affected protected native/browser journey. Independent initial and
fresh final code reviews plus original author validation remain distinct gates.
Architecture READY does not establish implementation or production readiness.
Authentic release/platform/existing-owner prerequisites are unchanged.

No durable SQL/data schema is changed. Rollback requires a coherent matched bundle;
old/new channel-profile peers refuse safely. Source dependency graph is unchanged:
host and office apps depend on existing launch contract, never launch→host/app.
Rough module coupling adds no source dependency edges. Runtime events extend the
existing authenticated worker→host channel only. Root integrates disjoint worktrees
and retains one overall plan and acceptance contract.


## Review history

Revision1 fresh independent architecture review session
01a1e402-2351-7161-9aba-899c9b6b55bf returned CHANGES_REQUIRED. The candidate was
otherwise feasible; it required resolving actual retained listener512IDs and
shared-control failure, concurrent receipt accounting, native/gateway>512cycles,
and exact ACK handling. Actual376.252s execution/result/ordered in-memory
preflight are retained under channel-architecture-review-1. Filesystem read-only
prevented reviewer artifact writes; source reading/hash checks and preflight
actually ran. Repowise unavailable; actual Rust analyzer/targeted source used.

Root revision2 resolves that concrete gap with a shared liveness witness, atomic
pending+acknowledged bounded accounting and typed capacity result. Source
requirements/trust/physical custody remain unchanged. Focused same-session
ARCHITECTURE recheck pending; no dependent implementation authorized yet.

Root activated revision2 after actual same-session independent SolXhigh recheck
READY, no findings,67.891s, session01a1e402-2351-7161-9aba-899c9b6b55bf.
Exact reviewed proposal SHA25606073c57959c1a1dae08f4a63f0d5178fe222fc4d6417275a2289e04829063aa
is archived as channel-architecture-review-1/proposal-r2.md. Only status/history
metadata changed upon activation. This permits planning/implementation, never
code acceptance. All tests, original-author intent and fresh final gates remain.
