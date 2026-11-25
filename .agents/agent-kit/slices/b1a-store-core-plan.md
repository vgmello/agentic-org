# B1a plan: the store core on the organization family

Base: `v1-integration` at `34c2958b`. Repository root: `/Users/vgmello/projects/agentic-org-claude`. Paths below are relative to that root; the critical files at the end are absolute.

Read-only planning. Nothing was built or run. Two claims rest on SQLite documentation, not on a test: the deferred-foreign-key counter (Q2) and `defer_foreign_keys` (Q3). Block 8 starts with a spike test for both.

## 0. Summary

- B1a adds one crate (`crates/org-store`) and new files in `crates/org-office`. It deletes nothing and leaves the old store running.
- No new third-party dependency is needed. `org-store` uses `serde`, `serde_json`, `sha2`, `libc` (all workspace pins). `org-office` gains `syn` as a dev-dependency, already locked at 2.0.119 and used with `full`, `visit` by `crates/org-host/Cargo.toml:43`.
- Three file overlaps with other writers need sequencing: root `Cargo.toml` + `Cargo.lock` (K1), `crates/org-office/src/organization/mod.rs` (A4), and shared docs (A4.6). Details in section 2.
- Nine premises in the plan and design do not match the code or the schema. Four change the design: deferred foreign keys, the receipt reply, the missing repository record, and the host's custody rule. Section 5.

## 1. What B1a delivers and what it does not

Delivers: `org-store` (contract, writer, journal, flusher, files, receipts, sequences, change stream, rebuild, traits, testkit); the SQLite engine in `crates/org-office/src/store/sqlite/`; the organization family with two operations; `open_store`; one conformance suite and one rebuild harness run on both engines.

Does not deliver (B1b): `Office` using the store, the host custody adapter, the office lock, the clean-stop marker, the CA key record, deletion of the old store, the narrowed ENG-1 check.

Does not deliver (B1c): benchmarks, the WAL-size back-pressure bound under load, the fixed flush interval.

## 2. File ownership and sequencing

### B1a owns (new unless marked)

| Path | Note |
| --- | --- |
| `crates/org-store/**` | New crate |
| `crates/org-office/src/store/open.rs` | New. Holds `open_store`. B1b renames it to `store/mod.rs` when `store.rs` is deleted |
| `crates/org-office/src/store/sqlite/{mod,schema,receipts,organization}.rs` | New |
| `crates/org-office/src/organization/{records.rs, records/yaml.rs, ports.rs, projector.rs, testkit.rs, features/mod.rs, features/create_organization.rs, features/create_team.rs}` | New files inside a directory A4 owns by glob. See below |
| `crates/org-office/tests/engine_boundary.rs` | New. The SQL-outside-adapter check |
| `crates/org-office/Cargo.toml` | Edit: `org-store` dependency, `testkit = ["org-store/testkit"]`, `syn` dev-dependency |
| `crates/org-office/src/store.rs` | Edit: two lines, `pub(crate) mod open; pub(crate) mod sqlite;` after line 37 |
| `crates/org-office/src/lib.rs` | Edit: `pub use` lines for the seam and the two commands (see risk R1) |
| `docs/design/office-store.md`, `docs/adr/0021-office-store-architecture.md` | Edit: decisions from section 4 |

`store.rs` cannot coexist with `store/mod.rs`, so the seam file is `store/open.rs` in B1a.

### Overlaps to sequence

1. **Root `Cargo.toml` and `Cargo.lock` (K1 also adds a crate).** Block 1 is a five-minute commit: the crate skeleton and two member lines. Integrate it alone, either before K1 creates its crate or right after. Never concurrently. The second writer regenerates `Cargo.lock` offline once.
2. **`crates/org-office/src/organization/mod.rs` (A4's glob).** B1a needs four `mod` lines there. A4's only org-office edits so far are `organization/bootstrap.rs`, its tests, `hq_creation.rs` and `office/hq_creation/tests.rs` (`git diff --stat v1-integration...a4-create-organization`). A4.3 to A4.5 list no org-office file. Recommendation: block 7 starts after commit `1dbd5b41` (A4.1 + A4.2) is integrated, and the lead releases `organization/mod.rs` plus the new file names to B1a. Blocks 1 to 6 touch no A4 file.
3. **`docs/FEATURES.md`, `docs/implementation-plan.md`, data contract documents 01 and 06 (A4.6 edits FEATURES, the plan's B1b note, documents 01 and 04).** B1a's edits to these go in block 10, after A4.6 or in a window the lead assigns.

B1a touches nothing under `crates/org-host/**`, `crates/org-host-control/**`, `crates/org-proto/**`, `apps/**`, `crates/org-office/src/hq_creation.rs` or any existing file in `organization/` except `mod.rs`.

## 3. Blocks, in dependency order

Merge 1 is blocks 1 to 7. Merge 2 is blocks 8 to 10. Acceptance is at merge 2.

Every test uses `tempfile::tempdir()`. `StoreConfig` refuses a relative root. No test reads a default location.

### Block 1 · Crate skeleton and the commit contract

- **Files:** root `Cargo.toml` (members and default-members), `Cargo.lock`, `crates/org-store/{Cargo.toml, src/lib.rs, src/change.rs, src/engine.rs, src/error.rs}`.
- **Contracts:**
  - `Ack { Committed, Durable }`, `FileChange { Create, Put, Remove }`, `Change`, `Refusal { Rejected, Invalid }`, `Operation { id, kind, actor, fingerprint }`, `Actor { Employee(Id), System }`.
  - `DurablePath`: relative, normal components only, first component one of `state, authority, accounting, messaging, recovery, budgets, local`.
  - `Id`: validated lowercase UUIDv7 text. `org-store` mints no ID. Callers supply every ID.
  - `Bytes = Arc<[u8]>`. This avoids a direct `bytes` dependency.
  - `Timestamp`: Unix milliseconds. `Clock` trait, injected. Families render text with `crates/org-office/src/time.rs:102` (`format_epoch_ms`).
  - `Engine`, `CoreRead`, `CorePort`, `Projector<E>` as in the design section 3, with the additions of Q2, Q3 and Q4.
  - `EngineError` carries `transaction_ended: bool`, which the writer uses to rerun undecided operations.
- **Tests:** path validation (traversal, absolute, unknown tree, control characters, 255-byte component); `Change` builder; ID validation.
- **B1b consumes:** nothing yet.

### Block 2 · `files.rs`: publication primitives and `Files`

- **Files:** `crates/org-store/src/files.rs`, `src/disk.rs`.
- **Carried from** `crates/org-office/src/store/acceptance/durable.rs`: `relative`, `component`, `check_directories`, `flush` (the one `#[cfg(target_os = "macos")]` `F_FULLFSYNC`), `parent_flush`, `read`, `publish`. Three couplings are removed (premise P1).
- **Contracts:**
  - `Files::materialize(&[Entry])`, `Files::read`, `Files::walk(Tree)` sorted by path bytes.
  - `Create`: exclusive hard-link; identical bytes are a no-op; other bytes return `FilesError::Foreign { path }`.
  - `Put` and `Remove`: current bytes must hash to the first pending `previous_sha256` or to a later retained version; a missing file may be recreated.
  - Directories `0700`, files `0600`, `O_NOFOLLOW`.
  - A private `Disk` trait (append, write, full flush, rename, link, unlink, sync directory, truncate) with one real implementation. The testkit wraps it to inject errors and short writes. No `cfg(test)` hook in the logic.
- **Tests:** the two tests of `durable.rs` ported; each `Create`, `Put`, `Remove` rule; symlink escape refused; foreign bytes stop and name the path; a file is wholly old or wholly new at every injected failure; a flush error on a file or directory is returned and never retried.

### Block 3 · `journal.rs` and `sequences.rs`

- **Files:** `crates/org-store/src/{journal.rs, sequences.rs, custody.rs}`.
- **Contracts:**
  - Entry frame exactly as design section 4: `length:u32 ‖ header ‖ payloads ‖ sha256`. The header is `serde_json` of a struct with fixed field order. The hash is over stored bytes, so no canonicalization library is needed.
  - `anchor.json` (immutable, not entry zero), `segments/<first-ordinal>.log` rolled at 64 MiB, `checkpoint.json`.
  - `Journal::create(dir, identity, anchor_facts)` and `Journal::open(dir, identity, custody)`. `append(&[Entry])` is one write. `flush()`, `after(ordinal)`, `release_through(ordinal)`, `truncate_to_last_good()`.
  - `OfficeIdentity { instance_id, organization_id, office_id, journal_incarnation, writer_epoch, machine_id }`: the fields of `org_proto::work::WriterIdentity`, defined in `org-store`.
  - `WriterAuthority { checkpoint(&OfficeIdentity) -> Option<JournalHead>; confirm(&OfficeIdentity, before, after) }` with `JournalHead { ordinal, sha256, high_waters }`. Same two calls as today's trait (`crates/org-office/src/store/acceptance.rs:29`). The old trait stays untouched for the old store.
  - `Stream { Organization, Work, Message }`. High-waters are recovered from anchor, checkpoint and entries, never from the projection.
  - Limits: entry at most 16 MiB; ordinal and position exhaustion checked before append.
- **Tests:**
  - Chain and anchor verification; fork, missing predecessor and unknown format refuse to open.
  - Torn tail above the confirmed head is truncated at a random cut byte and everything before it is kept.
  - One flipped byte at or below the confirmed head refuses to open and names the ordinal.
  - A journal behind the custody checkpoint refuses.
  - Short append and append error leave the segment truncated to the last good entry; a failed truncation is fatal.
  - Segment roll and release only after `checkpoint.json` is written.
  - Arithmetic never wraps (port of `journal_and_origin_arithmetic_never_wrap_or_restart_incarnations`).
  - STORE-3 part 1: a test scans every header key and value of a mixed journal for table, column and engine names.

### Block 4 · Memory engine, receipts, engine suite

- **Files:** `crates/org-store/src/receipts.rs`, `src/testkit/{mod.rs, memory.rs, engine_suite.rs, ids.rs, clock.rs, custody.rs}`.
- **Contracts:**
  - `MemoryEngine`: core tables plus a type map of family tables. `begin` clones, `commit` swaps an `Arc`, a mark is a clone.
  - `FamilyTable: Any + Send + Sync { clone_box, dump }`, so a family in another crate adds its tables.
  - Receipt file `recovery/commands/<operation-id>.json` with the reply inline. A core projector owns `recovery/commands/` and calls `CorePort::put_receipt`.
  - `engine_conformance_tests!($open)`: cursor round trip, `set_source` and `sources_under`, receipts (match, conflict on actor or fingerprint, no replace), mark and undo, snapshot isolation, atomic `replace`, `check()` of Q2.
  - `testkit::id(n)` returns deterministic UUIDv7 text. Fixed and stepping clocks. `FakeCustody` can be closed and can hold a confirmed head.
- **Tests:** the engine suite on `MemoryEngine`.

### Block 5 · Writer, flusher, change stream, `Store`

- **Files:** `crates/org-store/src/{writer.rs, flusher.rs, changes.rs, store.rs, testkit/faults.rs, testkit/probe.rs}`.
- **Contracts:**
  - `Store::open(StoreParts { durable, identity, custody, engine, projectors, clock, flush_interval })`, `submit`, `read`, `subscribe`, `close()`.
  - Dropping a `Store` does nothing durable. That makes an in-process drop equivalent to a process kill.
  - Batch of at most 64 (`MAX_BATCH`, `crates/org-office/src/store.rs:59`). The four per-operation steps and the per-batch steps follow design section 4.
  - After each operation's apply the writer calls `CorePort::check()` (Q2). A failure undoes to the mark and fails that operation alone.
  - Fatal state: a flush error stops acceptance, fails every waiting durable request and reports the path. Reads continue.
  - Back-pressure on the two byte bounds (8 MiB unflushed, 64 MiB unmaterialized). The pending overlay backs `UnitOfWork::source`.
  - Change ring of 8,192 entries; an older position answers `Resync`.
  - The opening sequence of design section 4, minus the office lock and the clean-stop marker (B1b).
  - `testkit::CrashPoint`, one value per row of the design's crash table: `DuringDecide`, `DuringApply`, `MidAppend(cut)`, `AfterAppend`, `AfterCommit`, `AfterCommittedReply` (power cut), `MaterializeTempFlushed`, `MaterializeRenamed`, `BeforeMark`, `DuringRebuild`, `FlushError(Segment | File | Directory)`, `AppendError`, `ShortAppend`, `TruncateError`.
  - Power cut simulation: truncate each segment to its last flushed length; the projection is left as it was, or deleted in a second variant.
  - `testkit::probe`: a family that needs no SQL. It owns `local/offices/<office-id>/probe/`, its projector writes no rows, and its state is read through `CoreRead::source_hash`. It has a committed-class and a durable-class operation. It exists because both organization operations are durable (premise P6).
- **Tests (memory engine):**
  - Ported from `store.rs`: `one_failing_request_in_a_batch_fails_alone`, `when_sqlite_ends_the_transaction_the_other_requests_are_run_again`, `a_failed_commit_fails_every_request...` (restated: after append, a commit failure replays, never fails), `a_panicking_request_is_reported_and_leaves_the_writer_alive`, `readers_run_while_the_writer_is_busy`, `requests_queued_together_all_commit...`.
  - STORE-1: a decision counter proves no decision runs after its entry is appended.
  - STORE-4: a committed operation is answered while the flush is held; a durable one is not; no file and no `after()` consumer sees an unflushed entry.
  - STORE-5: reads answer with the disk failing every call and custody closed, and see every acknowledged change.
  - STORE-8: all three flush-error sites, append error, short append.
  - Idempotent retry: same ID returns the original reply; other actor or fingerprint conflicts; two operations with one ID in one batch; a retry of a durable operation waits for its flush.
  - `Refusal::Rejected` leaves a receipt-only entry; `Invalid` leaves nothing and takes no ordinal.

### Block 6 · Rebuild and the harness

- **Files:** `crates/org-store/src/rebuild.rs`, `src/testkit/{harness.rs, rng.rs}`.
- **Contracts:**
  - `rebuild(durable, journal, engine, families) -> RebuildReport` per design section 6. A file no family owns fails. `Projector::validate` runs after the walk.
  - `rebuild_equivalence::<E, F: FamilyScript>(open_engine, seeds)`: generated valid, invalid, retried and interleaved commands; `dump(A) == dump(B)`; then the same with a crash at a random `CrashPoint`.
  - `dump` is per engine and compared only within one engine.
  - `rng.rs` is a small seeded generator. The seed is printed on failure. No `proptest`.
- **Tests:** the harness on the probe family with `MemoryEngine`; delete-and-rebuild equality; rebuild during rebuild; an unowned file fails with its path.

### Block 7 · The organization family on the memory engine (needs the A4 release of `organization/mod.rs`)

- **Files:** the `organization/` files of section 2, `organization/mod.rs` (four `mod` lines), `crates/org-office/Cargo.toml`, `crates/org-office/src/lib.rs`.
- **Records and paths** (under `state/`): `organization.yaml`, `repositories.yaml` (P3), `offices/<office-id>.yaml`, `identity/office-codes.yaml`, nested `teams/<id>/.../team.yaml`, `people/<id>/profile.md`, `agents/<id>/profile.md`, `identity/counters/organization/<org-id>/team.yaml`, `identity/allocations/organization/<org-id>/team/<number>.yaml`.
- **Codec:** `records/yaml.rs` emits one `key: <JSON scalar>` line per field in fixed order, the form `organization/bootstrap.rs:120` writes today. It parses through `org_docs::parse` after wrapping in fences, as `bootstrap.rs:171` does. Unknown or missing fields are refused by path.
- **Ports:** `OrganizationRead` (organization, team, team by number, team path chain, employee, handle in use, office, office code, team counter) and `OrganizationWrite` (put and remove per record kind, including `record_refs` rows for the organization and each team).
- **Projector:** `OrganizationFiles::new(organization_id)`. Team and employee files do not store the organization ID, so the projector takes it from the store's identity.
- **Operations:**
  - `CreateOrganization { organization_id, office_id, repository_id, team_id, creator { employee_id, name, handle }, hr_employee_id, name, slug, repository_remote_url, state_branch, office_name }`. It is the family's own type, not `PreparedV1`, which A4 is still changing. It writes the ten files in one entry, takes organization position 1, and issues office code `A`.
  - `CreateTeam { id, name, parent }` as in the design's worked example.
  - `Op::ack()` returns `Durable` for both.
- **Rules live in `decide` and `validate`, not in SQL constraints**, because the memory engine has none: parent exists and is active, team ID unused, organization does not yet exist, handles distinct, HQ only.
- **Suite:** `organization_conformance_tests!($open)` in `testkit.rs`, driven through `Store::submit` and the read port:
  - creation writes exactly the ten files with exact bytes; generation 1; creator is lead; `may_hire` true for the person and false for HR;
  - retry returns the original reply; same ID with another fingerprint conflicts;
  - a second creation is refused;
  - `create_team` numbers 2, 3 with counter and allocation; archived parent is a recorded rejection; unknown parent records nothing;
  - a nested team path follows the parent chain;
  - rebuild `validate` refuses a missing organization record and a lead outside the team.
- **Tests:** the suite and `rebuild_equivalence` with an organization `FamilyScript`, on `MemoryEngine`.
- **Merge 1 here.**

### Block 8 · The SQLite engine

- **Files:** `crates/org-office/src/store/sqlite/{mod.rs, schema.rs, receipts.rs}`, `store.rs` (two lines).
- **First commit of the block:** a spike test for Q2 and Q3 on the contract schema.
- **Contracts:**
  - `SqliteEngine::open(path)` at `runtime/<office-id>/office.db`.
  - DDL is `include_str!` of `docs/design/organization-data/schema.sql` plus the adapter's tables. The digest is SHA-256 of all loaded DDL and is stored in the projection. A different digest deletes and rebuilds.
  - Write connection: WAL, `synchronous=NORMAL`, `foreign_keys=ON`, `recursive_triggers=ON`, `wal_autocheckpoint=0`, and `defer_foreign_keys=ON` at each `begin` (Q3).
  - Reader pool: `query_only`, four idle (`MAX_IDLE_READERS`, `store.rs:57`), one read transaction per `Store::read`.
  - `replace`: build `office.db.new`, then rename over, with readers drained.
  - A passive checkpoint hook the flusher calls after each materialization round.
  - No key pragma, ever.
- **Tests:** `org_store::engine_conformance_tests!` on SQLite; digest change deletes and rebuilds; plain SQLite header; a relative path is refused; reads run while the writer holds a transaction.

### Block 9 · Organization SQL, the seam, both engines

- **Files:** `store/sqlite/organization.rs`, `store/open.rs`, `lib.rs`.
- **Contracts:**
  - `StoreConfig { engine: EngineChoice, durable_root, runtime_root, identity, custody, clock, flush_interval, opening: Create | Existing }`.
  - `open_store(&StoreConfig) -> Result<OfficeStore, _>`. `EngineChoice::Memory` exists only with `testkit`. Any other name is refused.
  - `OfficeStore` and `StoreOp` per Q1.
- **Tests:**
  - `organization_conformance_tests!` invoked twice, each through `open_store`.
  - `rebuild_equivalence` for the organization family on both engines.
  - The full `CrashPoint` table on both engines, each asserting the entry is wholly absent or wholly present and the decision count is at most one after append.
  - Delete `runtime/<office-id>/` and reopen: equal dump.
  - STORE-3 part 2: open with one extra index; the digest changes, the projection rebuilds, and every journal and file byte is unchanged.
  - STORE-4, STORE-5 and STORE-8 repeated on SQLite with the probe family and `create_team`.
  - One real `SIGKILL` subprocess test on SQLite (pattern of `store/acceptance/tests/crash_reopen.rs`): kill after a committed reply, reopen, the change is present.

### Block 10 · Guards and records

- **Files:** `crates/org-office/tests/engine_boundary.rs`, the docs of section 2, `docs/FEATURES.md` rows FC-141, FC-146, FC-165, `docs/implementation-plan.md` section B1a "Code today", data contract document 06.
- **Tests:**
  - Every workspace `Cargo.toml` names `rusqlite` only in `org-office` and `org-host`.
  - A `syn` scan of `crates/org-office` fails on a `rusqlite` path or an SQL-keyword string literal outside `src/store/sqlite/`. Its allowlist is the old store (`src/store.rs`, `src/store/` except `open.rs`) and the five old files ADR-0018 and FC-165 name. B1b empties the allowlist.
- **Docs:** each decision of section 4 in the document that owns it; ADR-0021 status to accepted; `office-codes.yaml` format in document 06, followed by `python3 docs/design/organization-data/test_schema.py`.
- **Merge 2 and acceptance here.**

### What B1b can then consume

- `org_office::open_store`, `StoreConfig`, `OfficeStore::run(op)`, and `CreateOrganization` and `CreateTeam` as `StoreOp`s.
- `org_store::WriterAuthority` and `JournalHead`, for the host adapter in `apps/agentic-org-office/src/transfer/writer.rs`.
- `Journal::create` for the protected creation path; `rebuild`; `Store::close()` as the place for the clean-stop marker.
- The read port for the directory read.
- The `engine_boundary` allowlist to empty.

## 4. Open design questions

**Q1. How does a non-generic office hold a generic store?** Design open point 1 (`docs/design/office-store.md:877`) recommends boxing the engine behind a supertrait and `&dyn` ports.
Recommendation: keep static dispatch. In `store/open.rs`:
- `trait OfficeEngine: Engine` with a blanket impl, bounded by `Tx: OrganizationRead + OrganizationWrite` and `Snapshot: OrganizationRead`. It grows by one bound per family.
- `enum OfficeStore { Sqlite(Store<SqliteEngine>), Memory(Store<MemoryEngine>) }`, the second arm under `testkit`.
- `trait StoreOp { type Out; fn run<E: OfficeEngine>(self, &Store<E>) -> Self::Out; }` and `OfficeStore::run`.

Adding an operation changes no shared file. Adding a family adds one bound. Adding an engine adds one arm. It needs no forwarding impls for boxed ports.

**Q2. Deferred foreign keys escape the per-operation mark.** `schema.sql` declares most references `DEFERRABLE INITIALLY DEFERRED` (for example lines 19, 93, 95, 1986). SQLite checks those at the outer commit, which is after the journal append. One bad operation would then fail a batch whose entries are already appended, and the design forbids that (`office-store.md:408-411`).
Recommendation: add `CorePort::check()`, called after each operation's apply and before framing. The SQLite engine reads `SQLITE_DBSTATUS_DEFERRED_FKS` through `rusqlite::ffi::sqlite3_db_status`. rusqlite 0.42 has no safe wrapper, so it is one `unsafe` call in `store/sqlite/mod.rs`. Fallback if the spike fails: `PRAGMA foreign_key_check` on the tables the operation touched. Domain rules still live in `decide`; this is a backstop.

**Q3. Rebuild order meets immediate foreign keys.** Rebuild walks paths in byte order (`office-store.md:601-604`), so `state/identity/...` and `state/offices/...` come before `state/organization.yaml`. `offices.organization_id REFERENCES organizations(id)` is immediate (`schema.sql:103`), as are `teams` (73) and `office_codes` (744).
Recommendation: the SQLite engine sets `PRAGMA defer_foreign_keys=ON` at every `begin` and in `replace`, then `check()` before commit. The write path gets the same freedom of file order.

**Q4. Where does the original reply live in the projection?** `mutation_receipts` has `result_ref` and no reply column (`schema.sql:1971-1988`). `CoreRead::receipt` must return the reply without a file read.
Recommendation: a third adapter table, `store_receipt_replies(command_id, reply BLOB)`, filled by the core projector from the receipt file. The design names only `store_cursor` and `store_sources` (`office-store.md:644`); update it. Also: a recorded rejection before the organization exists cannot satisfy `mutation_receipts.organization_id REFERENCES organizations(id)`, so `create_organization` refuses with `Invalid` only.

**Q5. What is the directory generation?** `office-store.md:537-540` says the ordinal of the last entry that changed an organization file. Document 16 says ordinals never become other positions and that each organization command "produces the next generation" (`16-recovery-and-office-continuity.md:57-58, 204-209`). An ordinal is also not recoverable from files after segments are released.
Recommendation: the generation is the high-water of the organization origin stream. Every organization decision takes `uow.next(Stream::Organization)`. Creation takes 1. It is recovered from the journal and its checkpoint.

**Q6. Does `org-store` depend on `org-proto`?** The design's graph shows no such edge (`office-store.md:117`). UUIDv7 syntax is owned by `org_proto::is_uuid_v7` (`crates/org-proto/src/identity.rs:2`).
Recommendation: depend on `org-proto` for that one function instead of copying it. `org-proto` has only `serde` and `serde_json` as dependencies, and `org-work` and `org-mailbox` already depend on it.

**Q7. HR lead's initial status.** A4 writes `status: active` plus `agent_status: paused` (branch `a4-create-organization`, `organization/bootstrap.rs`). Document 04 allows one state field.
Recommendation: `status: active`, no `agent_status`. The agent is not runnable because it has no execution configuration.

**Q8. YAML emitter home.** `org-docs` parses and edits but does not emit.
Recommendation: `organization/records/yaml.rs` in B1a; move it to `org-docs` when B3 needs it a second time.

**Q9. WAL checkpoints.** Design section 8 is B1c reading, but leaving the default puts a drive flush on the writer thread.
Recommendation: B1a sets `wal_autocheckpoint=0` and runs the passive checkpoint from the flusher. The 64 MiB WAL bound as back-pressure waits for B1c.

## 5. Premises the code contradicts

| # | Premise | What the code shows |
| --- | --- | --- |
| P1 | The primitives of `durable.rs` are carried "as they are" (`implementation-plan.md:364-366`) | `publish` calls `org_work::selected_codec` (`durable.rs:241, 252`), `crate::ulid::generate` (265) and `super::crash` hooks (144, 275, 291, 299). `org-store` may depend on no family. Block 2 replaces them with `sha2`, its own limit, a process-and-counter temp name, and the `Disk` seam |
| P2 | `files.rs` goes "on the Linux check's allowlist in place of `durable.rs`" | No such check or allowlist exists at `34c2958b`. `crates/org-platform` has `dirs`, `disk`, `service` and no `fs`. Nothing to edit; record it for X1b |
| P3 | Creation writes organization, HQ office, team, creator, HR lead (`implementation-plan.md:328-331`) | `organizations.organization_repository_id` references `repositories` (`schema.sql:19`). The family must also own `repositories.yaml`, as today's bootstrap writes it (`bootstrap.rs:216-219`) |
| P4 | `WriterAuthority` moves "unchanged in shape" and is called once per flush | The host accepts a confirmation only when `after == p` or `after.follows(p)` (`crates/org-host/src/writer.rs:524`), and `follows` requires `ordinal == before + 1` (`crates/org-proto/src/work.rs:1744`). A group flush advances several ordinals. `JournalPosition` also carries one origin high-water, not one per stream. B1b must change that rule in an A4-owned file |
| P5 | A projector error "undoes to the mark and fails that operation alone" | True for immediate constraints only. See Q2 |
| P6 | Done-when: "a committed write is answered while a flush is held back" | Both organization operations are durable (`office-store.md:335`; ADR-0014 decision 3). Shown with the probe family on both engines |
| P7 | Receipts are "projected into `mutation_receipts`" and that suffices | No reply column. See Q4 |
| P8 | A projector is "a pure function of a path and its bytes" | Team and employee files hold no organization ID (README shared identity contract). The projector is constructed with it |
| P9 | The layout names `store/mod.rs` for the seam | `store.rs` exists until B1b. Seam is `store/open.rs` in B1a |

## 6. Risks

- **R1. Dead code under `-D warnings`.** Nothing in production calls the new org-office modules until B1b. `cargo clippy --workspace --all-targets -- -D warnings` would fail on crate-private items. Mitigation: export the seam, the two commands and the port traits from `lib.rs`, which B1b's composition needs anyway. No `allow(dead_code)`.
- **R2. A build input under `docs/`.** `include_str!` of `schema.sql` makes every contract edit a projection change. Accepted by design open point 5. The 91 tables exist empty.
- **R3. Schema triggers on untested orders.** `organization_remote_reference_insert` and the cycle triggers run during rebuild in path order. The harness covers it; expect fixes in the SQL module, not in the schema.
- **R4. In-process crash model.** It is valid only if `Drop` does nothing durable and the engine holds no unflushed user-space state. One `SIGKILL` test checks the model on SQLite.
- **R5. Size.** Blocks 3 and 5 are the largest. If block 5 exceeds a commit, split the flusher and the change stream from the writer.
- **R6. Two creation paths until B1b.** A4's bootstrap and B1a's `create_organization` write different bytes for the same records (Q7, attribution). B1b removes the first. The lead should tell A4 that B1a's bytes are the target.
- **R7. Timing tests.** The flush hold is driven by a testkit gate, never by sleeping.
- **R8. Linux CI.** `org-office` is built and tested on Ubuntu. The macOS branch in `files.rs` stays the only platform conditional.

## 7. Acceptance map

| Done-when (section B1a) | Evidence |
| --- | --- |
| Organization suite passes against SQLite and memory (ENG-3) | Block 9: `organization_conformance_tests!` twice through `open_store` |
| Rebuild-equivalence harness on both engines | Block 7 (memory), block 9 (SQLite) |
| A crash at each point of the crash table reopens to wholly absent or wholly present, no decision twice (STORE-1) | Block 5 (memory, probe), block 9 (both engines, organization), decision counter, `SIGKILL` test |
| Deleting the projection and reopening rebuilds an equal one (STORE-2) | Block 9: delete `runtime/<office-id>/`, compare dumps |
| STORE-3 | Block 3 header scan; block 9 digest change with unchanged journal and file bytes |
| A committed write is answered while a flush is held back (STORE-4) | Blocks 5 and 9, probe family; durable `create_team` not answered before its flush |
| STORE-5 | Blocks 5 and 9: disk failing, custody closed, reads answer |
| A failed flush stops the office; a failed append fails its batch alone (STORE-8) | Blocks 3, 5 and 9: three flush sites, append error, short append, truncation failure |
| Checklist rows FC-141, FC-146, FC-165 | Block 10, with evidence paths; none marked Done (their scope ends in B1c and later) |

STORE-6 is tested for exact bytes in blocks 2 and 7 but is B1b's criterion. STORE-7 is B3's.

### Critical Files for Implementation

- /Users/vgmello/projects/agentic-org-claude/docs/design/office-store.md
- /Users/vgmello/projects/agentic-org-claude/docs/design/organization-data/schema.sql
- /Users/vgmello/projects/agentic-org-claude/crates/org-office/src/store/acceptance/durable.rs
- /Users/vgmello/projects/agentic-org-claude/crates/org-office/src/store.rs
- /Users/vgmello/projects/agentic-org-claude/crates/org-office/src/organization/bootstrap.rs

## Revision 2 — lead rulings on the independent plan review (2026-11-18). These win over the text above.
F1 Directory generation = the organization stream's high-water (Q5 stands). Mechanism: the engine commit stores the
   stream high-waters with the cursor; CoreRead exposes them, so a read returns the generation of its snapshot. A
   Refusal::Rejected organization command returns its position (the generation does not advance). On rebuild the value
   is set from the journal/checkpoint, never from a file. Suite cases: rejected command leaves the generation unchanged;
   equal after delete-and-rebuild and after segment release.
F2 Every SQLite connection sets SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE, so dropping the engine does no durable work. The
   SIGKILL test has three env-selected points: during apply (open transaction), after append before the projection
   commit, after the committed reply.
F3 After every crash-point reopen, resubmit the same operation id and assert the original reply and an unchanged
   decision counter. A CrashPoint halts and joins the writer and flusher threads before reopen.
F4 Custody rule (one lead owns A4 and B1a; recorded here as the agreed contract; the host change itself lands in B1b):
   a confirmation is accepted when it is the same incarnation, `before` equals the stored head, the ordinal is strictly
   greater or equal-and-identical, and each stream high-water is non-decreasing. FakeCustody enforces exactly this.
F5 A4 adopts the contract bytes now (no agent_status on the HR profile; one status field) — added to the A4 plan. B1a
   owns one shared golden fixture of the creation files that both creation paths test against. Agent profile path:
   state/teams/<team-id>/agents/<employee-id>/profile.md; people likewise under the team.
F6 Q2/Q3 stand, with: PRAGMA defer_foreign_keys=ON issued after BEGIN (spike asserts it survives ROLLBACK TO and is
   off after COMMIT); the deferred counter is zero at every operation boundary (spike asserts zero after a valid
   create_organization); a non-zero check in `replace` runs PRAGMA foreign_key_check to name table and row; a failed
   check() on the write path is an internal error, logged, never a Refusal; conformance case: memory and SQLite agree on
   every scripted command. Confirm rusqlite 0.42 has no safe wrapper before adding the unsafe call.
F7 engine_boundary.rs in B1a asserts only: rusqlite is named in Cargo.toml of org-office and org-host alone, and the new
   B1a paths (organization/{records,ports,projector,features,testkit}, store/open.rs) contain no rusqlite path. The
   workspace SQL scan and its allowlist move to B1b.
F8 No org-store → org-proto edge. org-store checks only that an id is one safe path component; families validate
   UUIDv7. Record in office-store.md and project-structure.md.
F9 Streams follow the data contract: organization, work, message_content, message_state.
F10 Q1 static dispatch accepted; record its costs in office-store.md open point 1; reads get OfficeStore::read with a
   generic read op. B2 starts after merge 2.
F11 Test: a retry of a failed creation with the same id decides afresh. Reply table added to the digest list.
F12 The flusher's passive checkpoint uses its own connection; a long reader only delays it.
F13 The organization family owns state/repositories.yaml; B3 writes through its port. Record in the design.
F14 Merge 1 gets its own checks and fresh review before it goes to v1-integration.
Q7 note: agent_status: paused is already at base (bootstrap.rs:213), not only on the A4 branch.
Status: READY. Starts in slot-2 after K1 commits its crate (root Cargo.toml/Cargo.lock are serial).
