# org-mailbox

Status: slice 1 in progress; not ready for release.

The crate defines ADR-0003's directory, message-store and session-auth ports,
typed domain errors, storage DTOs, and the canonical SHA-256 content hash. The
mailbox currently implements local/HQ send, inbox, read and cancellable wait.
The server supplies the canonical member id from its authenticated binding.

`Mailbox::new` takes thread-safe store/directory handles, office identity, a
clock and an event sink. Events are published after commit and never hold bodies.
The server enforces each connection's held-call limit. It retains a `CancelHandle`
and passes the associated `Cancellation` to workers; dropping the handle removes
registrations and wakes blocked workers. Registry locks never surround storage
calls. Deadlines bound condition waits; the synchronous storage port cannot
preempt a read or fsync already in progress.

The `testkit` feature exports in-memory store and directory implementations.
Reusable backend conformance tests and full mailbox integration/race tests are
still being added. Scoped tests currently cover the pinned hash, UTC boundaries
and waiter bookkeeping; they do not establish the complete mailbox behavior yet.

The capability split requested by the SQL-store implementer is awaiting the
contract owner's ruling. The original ADR traits remain unchanged meanwhile.
Snooze scheduling, expiry transitions, asks and cross-office routing are not
implemented by this slice's mailbox surface yet.
