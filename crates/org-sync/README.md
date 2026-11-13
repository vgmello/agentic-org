# org-sync

Storage-independent organization replication for X-05. The core decides event order,
authority changes, and whether a snapshot is required. The office adapter owns native
payload meaning, durable storage, and snapshot validation. The connection adapter supplies
immutable authenticated peer facts. Neither replication crate performs filesystem or network I/O.

`org-sync-proto` owns the validated organization identifiers, epoch, position, codec, and
event digest and canonical authority signing preimage. Reproduce the independent digest vectors from the workspace root with
`python3 crates/org-sync-proto/test-vectors/generate.py`.
The event tests assert the frozen SHA-256 answers; the authority-record tests also
assert the complete signing preimage. See the protocol crate's
[test-vector notes](../org-sync-proto/test-vectors/README.md) for the exact coverage.

## Writer contract

`OrgStore::transact` runs a replayable attempt in one writer job with a consistent view
of state. Roll back any error; integrity, divergence, and invariant failures abort the
whole batch. An event whose bytes do not match its digest, or accepted snapshot staging
that has disappeared, is an integrity failure. Valid content conflicting with retained
history remains divergence. Event and published-output Debug include payload length,
never payload bytes.
Retry only busy or locked storage errors, at most three times after the
first attempt. Return the output, including any ACK, only after durable commit.

Apply native organization effects, the event log, and the cursor together. The native
apply step owns changes to organization policy, including its protocol level; writing
the event and cursor must preserve those changes. HQ appends the event within the same
attempt as its native mutation. Replaying a retained matching event ACKs without
applying its effects again. A gap requests the missing events; missing retained proof
for an older event requires a snapshot.

The core checks the authenticated peer's organization, office, role, and session epoch.
It checks signed restore authority against pinned organization, HQ, and CA facts. A
restore fence or an unmaterialized organization requires a trusted staged snapshot before
deltas. The adapter installs the snapshot, updates the cursor, and clears the bootstrap
flag in the same transaction, preserving office-local state. Ordinary snapshots cannot
rewind the cursor or lower the protocol level. A restore at sequence zero must match
the stored signed base.

Authority verifiers sign exactly `authority_record_preimage_v1`, including the restored
epoch, sequence and digest. The raw organization payload cap derives from the encoded
frame limit and leaves room for base64 and metadata.

Protocol negotiation uses the replicated organization level. Raising that level requires
the current registry, including HQ, and support from every office active in the last
30 days. An older returning office must upgrade before it can resume. The published profile table
fixes each level's wire/schema and codec bounds. Production has only level 1, wire/schema
1, and the two native v1 codecs; injected level 2 is confined to test fixtures.

## Adapter conformance

Run `CARGO_BUILD_JOBS=2 cargo test -p org-sync -p org-sync-proto --all-features`.
The `testkit` feature provides an in-memory writer with injected failures and
`testkit::conformance::organization_adapter`, which native adapters can run with real
codec payloads. The checks cover gaps, rollback, restart, duplicate ACKs, and native
application counts. The core fixtures also cover signature and peer refusal, epoch
fencing, bootstrap, snapshot install, and version negotiation.

At the crate level, excluding standard and third-party libraries, `org-sync` depends
only on `org-sync-proto`, which has no outgoing workspace dependencies. Native adapters
depend on the core contracts. SHA-256 is the protocol crate's sole production dependency.

## Independent message replication

Messages retain their native ID and original payload/fingerprint. They use a separate
queue with no command-state dependency and no replication sequence. Native message
counters are unchanged; Work streams own their separate per-origin ordering.

A message queue is bound to the pinned organization and local office. The same validator
checks its shape and binding on load and save. Native delivery and its dedup receipt
share one writer attempt; an output becomes visible only after durable commit. An ACK
lost after that commit is replayed before expiry from the retained receipt without enqueuing
or applying again. A failed attempt leaves queue, clock, native delivery and receipts unchanged.

Native adapters authorize recipients and fingerprints. The core checks the pinned organization,
endpoint, HQ identity, peer role, delivery/ACK route and highest authenticated session epoch
before reading restore state or receipts. Restore and unsupported organization protocol levels fence every
message operation. Payload limits derive from the encoded frame limit with base64 reserve;
pending message count and bytes are bounded. Debug prints payload length rather than bytes.

Local expiry retains its notice and identity; it does not establish remote non-delivery.
A later authenticated delivered receipt can correct that notice atomically. Backward clocks
cannot revive expired traffic. The test adapter injects individual read/write failures,
commit failures and reopen; the guard matrix checks exact refusals and unchanged state.
Real message-storage and framing adapters are separate slices.

Initial expiry also retains immutable identity. Sender-side expiry uses a local
undelivered notice; receiver-side expiry has a separate rejected-identity record,
sharing the native origin/ID uniqueness and dedup horizon with delivered records.
A matching receiver retry remains Expired without an ACK. A conflicting live intent is
divergence. The identity and clock commit together, including on the first refusal; expiry
writes preserve the first immutable identity and never downgrade a delivered receipt.

`MAX_MESSAGE_TTL_MS` is 30 days, matching X-05's outdated-office cutoff. Enqueue and receive
refuse a deadline more than that interval beyond `max(now_ms, clock_high_ms)`. `deadline_ms`
is the effective transport deadline with any native skew grace already folded in. Expiry
is checked before receipt or rejected-identity lookup: delivery or replay at or after that
deadline returns `Refused(Expired)`, even after native proof pruning. A later authenticated
ACK for an earlier committed delivery can still correct the sender's expiry notice.

Native adapters retain received and rejected identity through the effective deadline,
including across body pruning, snapshot installation and restart. Run
`testkit::message_conformance::assert_message_retention` against native pruning/reopen paths;
the contract test rejects pruning either identity type before its deadline. These are
minimum identity retention rules; compact native conversation headers may live longer.
`Ok(Refused(Expired))` commits identity/clock state; `Err(SyncError::Refused(_))` rolls back.

## Local Work append

`work::queue_local` runs inside the native append savepoint, after the native mutation
and source sequence allocation. Propagate every error to roll back domain versions,
feed rows, counter, queue and clock together. Return its source key only after COMMIT;
retry the entire native operation, never only the queue append. The core allocates no
sequence or clock and knows no native tables or event schema.

The native codec supplies frozen per-version event bytes, not a current-record snapshot.
Core and codec share `work_record_digest_v1` and `work_source_digest_v1`. The core recomputes
both before saving and for every retained row. Record identity/version/codec/body define
the source-independent record digest; source identity, Org prerequisite and creation time
define the source digest. Record generation and version must be positive. The required
Org position must be applied at the same epoch; an earlier sequence is allowed.

`LocalWorkTxn` exposes only context, local outbox, native codec support and save. A shared
load/save validator checks the pinned origin, organization, contiguous source positions,
shape, immutable digests and bounded count/bytes. Restore fences and the published protocol
profile gate new appends. A raw payload is at most half the encoded frame budget, leaving
base64 and envelope space. Debug never prints native event bytes or record IDs.

Native adapters can run `testkit::work_conformance::local_work_adapter` with two disposable
offices. It checks failed COMMIT rollback, gap-free retry, frozen bytes after reopen,
restore fencing, quota refusal and isolation using the adapter's own codec/savepoint.

## Local Work forwarding and receipts

`work::forward_local` plans an immutable prefix after stream activation, Gap or an
advanced receipt. It rechecks verified HQ identity, epoch and current connection generation,
then protocol/restore fences and the whole retained queue in the same retryable writer
snapshot. It returns at most 32 contiguous entries after the durable acknowledged floor,
only after the transaction commits. It never dequeues entries or changes a floor, outcome
or sent counter. Returning credit alone selects nothing and does not read the queue.

`LocalStreamOutput::ForwardCandidates` contains unframed candidates, not permission to send.
The transport must enforce the frozen encoded frame size, byte/frame credit and reserved
control slot before sending. This core policy neither encodes frames nor performs I/O.

After `acknowledge_local` finishes, `work::receipt_action` maps Advanced or Gap to keeping
the connection and replanning, Duplicate or RestorePending to keeping it, and storage errors
to dropping the receipt and keeping the connection. Other refusals close it; ConflictingProof
alone emits the existing `divergence` wire code before closing. Local refusal codes must not
be sent as receipt acknowledgments. Receipt application keeps immutable proof, outcomes and
the durable floor in one transaction; retries re-read that state rather than reusing a plan.

HQ ingestion, transport encoding/credit, delivery scopes and recovery are separate slices.
