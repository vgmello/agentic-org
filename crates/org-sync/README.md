# org-sync

Storage-independent organization replication for X-05. The core decides event order,
authority changes, and whether a snapshot is required. The office adapter owns native
payload meaning, durable storage, and snapshot validation. The connection adapter supplies
immutable authenticated peer facts. Neither replication crate performs filesystem or network I/O.

`org-sync-proto` owns the validated organization identifiers, epoch, position, codec, and
event digest and canonical authority signing preimage. Reproduce the independent digest vectors from the workspace root with
`python3 crates/org-sync-proto/test-vectors/generate.py`.

## Writer contract

`OrgStore::transact` runs a replayable attempt in one writer job with a consistent view
of state. Roll back any error; integrity, divergence, and invariant failures abort the
whole batch. Retry only busy or locked storage errors, at most three times after the
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
lost after that commit is replayed from the retained receipt without enqueuing or applying
again. A failed attempt leaves queue, clock, native delivery and receipts unchanged.

Native adapters authorize recipients, fingerprints, peer roles and routes. The core checks
the pinned organization, endpoint and highest authenticated session epoch before reading
restore state or receipts. Restore and unsupported organization protocol levels fence every
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
A matching receiver retry remains Expired without an ACK, and a conflicting intent
is divergence. The identity and clock commit together, including on the first refusal.
