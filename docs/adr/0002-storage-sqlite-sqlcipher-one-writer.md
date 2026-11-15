# ADR-0002 · Storage: SQLite with SQLCipher, one writer

Status: accepted 2026-11-08; reconciled 2026-11-15. Owner: ada.
Refines [ADR-0001](0001-app-is-a-client-office-is-a-server.md). Standalone platform
and instance boundaries follow [ADR-0005](0005-standalone-server-v1.md).
The selected source/storage contract follows the
[authority map](../design/documentation-authority.md); no schema migration or
production recovery implementation is performed by this decision.

## Context
- Many agents will talk to an office server at once, and their writes must not fail or stall each other.
- Organizations will hold confidential information, so the server's database must be encrypted at rest.
- We looked at [Turso](https://github.com/tursodatabase/turso), a Rust rewrite of SQLite. It has concurrent writes (MVCC, `BEGIN CONCURRENT`) and encryption at rest. As of November 2026 both are marked **experimental**: its manual says encryption is not production ready and shouldn't hold critical data. Its Rust API is async-only and pre-1.0 (0.4.x).

## Decisions
1. **Engine: SQLite through `rusqlite`, built with SQLCipher** (AES-256, feature `bundled-sqlcipher`, exact pin). On macOS this uses Apple's CommonCrypto, so there's no OpenSSL to build or ship, as long as none of `OPENSSL_DIR`, `OPENSSL_LIB_DIR` + `OPENSSL_INCLUDE_DIR`, or their target-prefixed forms (e.g. `AARCH64_APPLE_DARWIN_OPENSSL_DIR`) are set at build time (otherwise it links `libcrypto` dynamically). Release builds clear them, and CI checks the server binary with `otool -L` to confirm it links no `libcrypto`. The standalone v1 Linux build must choose and validate its SQLCipher crypto linkage (including whether to use `bundled-sqlcipher-vendored-openssl`) before release. Only the authorized storage service opens its database; browser and agent clients use its domain operations (ADR-0001). The separate private-note service/store must enforce author-only access rather than reusing broad shared-task reads.
2. **Key custody belongs to the host.** The worker never creates, stores, loads
   or deletes a database key and never accesses the platform key store. The host
   provisions a random 256-bit office key only for a new office, passes it through
   the private authenticated launch contract, and refuses to synthesize a key for
   an existing office. A missing or unusable key is `key_missing`; existing data
   must never be overwritten to make startup succeed. Every reader/writer keys
   SQLCipher with the raw key (`PRAGMA key = "x'<64 hex digits>'"`); keys never
   enter logs, ordinary files, organization Git or browser responses. Tests use
   explicit in-memory keys; the debug-only dev-keys store is not a production
   fallback.

   The inspected macOS custody contract uses the Data Protection keychain,
   generic-password service `org.agentic.office-key`, account = office ID,
   the signed host's `keychain-access-groups` entitlement,
   `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly` and no synchronization.
   The earlier worker-owned `org.agentic.office` service is retired. These are
   persisted security namespaces, not executable names to rename. The existing
   first-line launch/key and discard acknowledgement contract is retained in
   [office-host-boundary.md](../design/office-host-boundary.md). The host removes
   a key only after the matching office's verified discard; it never sweeps keys
   for unrelated offices or instances. Standalone instance scoping and Linux
   production custody require their reviewed implementations, not a guessed
   namespace or a plaintext fallback.

   Device-bound keys do not follow an ordinary backup onto another machine.
   Restoring canonical sources, restoring legitimate key access and reconciling
   owned processes are separate recovery obligations. CA escrow/recovery-code
   delivery does not by itself prove database-key or private-note recovery.
   REC-1 and document 16 retain the unresolved production recovery evidence.
3. **Concurrency: one writer.** The server keeps one write connection on one thread and sends every write to it through a channel. Reads use their own connections (WAL) with a `busy_timeout`. So any number of client connections can write at the same time without contending for the write lock. Separate offices each have their own database and never share a file.
4. **Durability:** WAL with `PRAGMA synchronous = FULL` and `PRAGMA fullfsync = ON`. On macOS a plain fsync doesn't flush the drive's cache, so this is what makes a committed write survive a power loss. A full flush is slow per commit, so the writer thread **batches**: it runs everything queued in one transaction, each request inside its own `SAVEPOINT` so an ordinary failure (a broken rule, a constraint) rolls back only that request, commits once, and only then reports each request's result. After each failed request the writer checks whether the transaction is still open (`is_autocommit()`). SQLite can end it on its own (e.g. some disk-full, I/O, out-of-memory or busy errors, an interrupt, `ON CONFLICT ROLLBACK`, `RAISE(ROLLBACK)`), which also undoes requests that already succeeded. If it's gone, the failed request gets its error, and every other request in the batch, including those that had succeeded, is re-run in a new transaction. If the commit itself fails, the writer rolls back if the transaction is still open, and every request that had succeeded gets the error. Requests that had already failed keep their own error.
5. **Private persistence adapters.** The inspected office keeps SQL and
   `rusqlite` types in its private `store.rs`; domain-module extraction may split
   that implementation without exposing SQL or weakening transaction ownership.
   Domain callers use typed operations. The target shared projection and separate
   private-note database retain distinct access and durability boundaries; the
   consolidated draft schema does not combine their writers or permissions.
6. **Turso later:** we look again when Turso's MVCC and encryption leave experimental. Changing engines means rewriting `store.rs` and migrating the data. A SQLCipher file can't be opened by stock SQLite or Turso, so the migration copies the data in-process from the SQLCipher connection straight into a new database encrypted by the new engine. Decrypted data never lands on disk in between.

## Canonical sources and acknowledgement

| Data | Durable authority | Role of SQLite |
| --- | --- | --- |
| Organization/team/employee/project definitions, task and feature records | Organization Git content mirrored as durable files; acknowledged unpublished mutations also need the durable journal | Indexed shared projection |
| Published task activity | Immutable `work-history/tasks/<task-id>/<revision>.json` plus the journal for acknowledged unpublished events | Ordered history projection |
| Usage observations and corrections | Durable accounting archive/checkpoints; immutable used price versions remain in organization Git | Derived totals and event indexes |
| Chat, recipient state and attachments | Protected messaging sources under document 15 | Authorized shared projection, not the sole recoverable copy |
| Budget policies, extensions, notices and holds | Protected budget sources and recoverable mutation receipts under document 17 | Evaluation/read projection; retained decisions are not regenerated from spend |
| Number allocations, command receipts, placement generations and process evidence | Durable authority/recovery/supervisor journals under documents 06/11/16 | Rebuildable indexes where specified; cache recreation grants no launch authority |
| Author-private task notes | Separate durable private database, protected mirrors/receipts and retention policy under document 05 | Canonical private store, not a disposable shared cache |
| Credentials and authentication state | Existing protected host/office credential subsystem | Not made disposable or public by the organization-data schema |

The shared draft requires `foreign_keys` and `recursive_triggers` on every write
connection. Its tables and structural tests do not implement an application
migration, authorization or a cross-file transaction. An accepted mutation must
have one recoverable commit boundary covering its domain change, required journal
and receipt/outcome. Independent file and SQL writes are not atomic merely because
they share an operation ID. Document 16 requires evidence before acknowledgement;
document 11 specifies complete-envelope checksums, temporary-file publication,
file/directory flushes and gap detection. Apply equivalent platform power-loss
guarantees to those journals, including macOS full flush; ordinary fsync alone
is insufficient there.

A rebuild inventories verified sources and fails explicitly for missing coverage.
Git alone cannot restore chat, usage, budget decisions, private notes or execution
authority. Existing processes must be reconciled or fenced before replacement
launch. A task handoff does not authorize deleting its workspace. Code that is
local-only or unpushed needs the protected recovery checkpoints defined by
document 16 before its runtime cache can be discarded.

## Not covered here

- **Ordinary files are not encrypted by SQLCipher.** Organization Git content and
  runtime checkouts are ordinary files; platform disk encryption and protected
  file/access policies are separate obligations. Never put secrets, chat bodies,
  private notes or budget amounts in shared Git to obtain recoverability.
- Automatic backup/restore, key rotation and new-machine key recovery remain
  implementation work. The selected data contracts specify what must survive;
  they do not establish a completed service or an encryption scheme for every file.
- Historical storage/API layout is retained as implementation evidence, not an
  instruction to load the draft SQL into an existing office database.

## Consequences

- SQLCipher remains bundled. macOS CommonCrypto linkage and Linux production
  crypto linkage must be verified separately; no production plaintext fallback.
- Each database has one writer. Readers use WAL; request acknowledgements follow
  the durable commit and failures never manufacture successful receipts.
- The host accesses macOS credentials after the first unlock since boot. Stable
  Developer ID signing and its keychain access-group entitlement remain release
  prerequisites. A filename change does not change a signing identity or grant.
- The private-note store adds a distinct authorization and recovery boundary.
  Administrator, owner, assignee or team-lead status grants no implicit note access.
- Recovery tests must cover preserved message visibility, accounting gaps, consumed
  budget extensions, independent holds, numbering allocations and live processes,
  as well as successful SQL replay.

## Earlier requirements and implementation evidence

The [pre-reconciliation ADR](https://github.com/vgmello/agentic-org/blob/c6e520a36c4dc7c738c68915fac5e7d5d1cbe804/docs/adr/0002-storage-sqlite-sqlcipher-one-writer.md)
records the superseded worker-owned key and desktop packaging discussion. The
current host custody, SQLCipher, batching and platform flush obligations above
retain their security meaning. Full source contracts are
[private notes](../design/organization-data/05-operational-data.md#private-notes-attached-to-tasks),
[accounting](../design/organization-data/10-usage-and-cost-accounting.md),
[execution](../design/organization-data/11-model-profiles-and-agent-execution.md),
[messaging](../design/organization-data/15-messaging-and-chat.md),
[recovery](../design/organization-data/16-recovery-and-office-continuity.md) and
[budgets](../design/organization-data/17-budgets-and-controls.md).
