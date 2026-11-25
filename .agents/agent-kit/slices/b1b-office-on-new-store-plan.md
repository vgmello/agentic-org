# B1b plan: the office runs on the new store

Base: `v1` at `74865317`. Repository root: `/Users/vgmello/projects/agentic-org-claude`. Paths below are relative to that root. The critical files at the end are absolute.

This was read-only planning. Nothing was built or run. Claims marked **(not verified)** were inferred from reading and not checked by running anything.

## 0. Summary

- **What B1b does.**
  - The office opens its store through `open_store`. HQ creation writes journal entry 1, which also holds the CA records.
  - The office's native directory read (`org.snapshot`, `who`, `inbox.list`) and `team.create` answer from the new projection.
  - The old store is deleted: `store.rs` and `store/**` except `store/open*` and `store/sqlite/**`, about 27,600 lines. The SQL outside it goes too.
  - Everything else the office served answers `unsupported`: work, messaging, status, process ownership, operation lookup, subscribe, and the other organization commands.
- **Delivery shape.** Seven blocks and two merge points:
  - **M1** (blocks 1–2) is green on its own. It switches off what only the old tables served, while organization data still uses the old store.
  - **M2** (blocks 3–7) is the switch. Inside it, blocks 4, 5 and 6 are green only per crate. The workspace, real-process and e2e gates run at the end of block 6 and at M2.
- **Dependencies.** No new third-party crate.
  - `org-office` gains `syn` as a dev-dependency. `=2.0.119` with `full` and `visit` is already locked and used by `apps/agentic-org-office`.
  - `org-office` drops `org-work`, `org-sync`, `org-sync-proto` and, if Q2b is taken, `org-mailbox` from its dependencies. The writer confirms each with clippy.
- **Contract changes.**
  - `org_proto::ErrorCode::Unsupported` is added.
  - The writer-control wire goes to version 2: per-stream high-waters, an organization data root instead of the public and protected roots, and a UUIDv7 journal incarnation.
  - `HqCreationV1` loses `public_root`.
  - The host's custody rule becomes ruling F4.
- **Owner-visible consequence.** An organization created on today's store (A4) does not open after B1b. Nothing is migrated (design §6, §12). It must be created again (risk R1).
- **Contradictions.** Seventeen places where the requirement text or design disagree with the code are listed in section 5. Four of them change design text: the CA key going into the journal, the clean-stop marker's owner, the escrow file's owner, and the wire command IDs.
- **Parallel split.** Worth it for block 4 (org-office) running beside block 5 (host and wire), in separate worktrees. Everything else is one writer.

## 1. What B1b delivers and what it does not

**Delivers:**
- **Opening.** `Office` opens through `open_store`:
  - host custody is checked at open and confirmed once per flush, through the real host adapter and the F4 rule;
  - the projection is replayed, or rebuilt when missing or stale;
  - the clean-stop marker is written on an orderly stop and removed at open.
- **Creation.** HQ creation writes entry 1, which holds the organization family's ten files plus the CA escrow package and the wrapped CA key record. Recovery-code release reads the same public record as today.
- **Native calls on the new projection.** `org.snapshot`, `who`, `inbox.list` and `msg.bind`'s directory lookup, plus `team.create`.
- **Deletion.** The old store, the SQL outside it, and the worker features only it served, which now answer `unsupported`.
- **Guards.** ENG-1 narrowed to `src/store/sqlite/` by a workspace `syn` scan with an empty allowlist. ENG-2: office tests run on the memory engine through `open_store`. STORE-6 tested on the production creation path.
- **Records.** FEATURES rows and docs updated.
- **Golden follow-up.** The comparison is run against A4 once (block 1). It is then replaced by a production-path golden test (block 4).

**Does not deliver:**
| Item | Who takes it |
| --- | --- |
| Work calls | B4 |
| Messaging, status lines, `subscribe` live events | B6 |
| Process-ownership records | C3 |
| Other organization commands; `operation.get` for org commands; sibling-name rules on `team.create` | B2 |
| Organization replication (satellites) | S1 |
| Benchmarks, flush interval, WAL bound | B1c |
| Wire command and operation IDs moved to UUIDv7 | The separate ID slice in the milestone log |
| Browser views | Out of scope (owner direction) |

## 2. File ownership

### Created

| Path | Block |
| --- | --- |
| `crates/org-office/src/office_local/{mod.rs, records.rs, projector.rs, testkit.rs}` (Q2) | 3 |
| `crates/org-office/src/organization/snapshot.rs`: records to `org_proto::OrgSnapshot`, pure, no store | 3 |
| `crates/org-office/src/organization/features/create_organization/production_bytes.rs` (replaces `bootstrap_bytes.rs`) | 4 |
| `crates/org-office/src/office/tests/{lifecycle.rs, creation.rs, store.rs}` (rewrites; names indicative) | 4 |
| `apps/agentic-org-office/src/directory.rs`: `MemberDirectory` over `Office::org_snapshot` (Q2b) | 6 |

### Edited

| Path | Block | Note |
| --- | --- | --- |
| `crates/org-office/src/organization/features/create_organization/bootstrap_bytes.rs` | 1 | Test setup only |
| `crates/org-proto/src/error.rs` | 1 | `Unsupported` |
| `apps/agentic-org-office/src/{lib.rs, serve.rs, msg_calls.rs, live.rs, inbox/hint.rs}`, `serve/tests/*` | 2, 6 | |
| `apps/agentic-org-office/tests/launch.rs`, `tests/launch/**` | 2, 6, 7 | |
| `apps/agent-os/tests/office.rs`, `apps/agent-os/README.md` | 2 | |
| `crates/org-office/src/store/open.rs`, `store/open/operations.rs`, `store/open/tests.rs` | 3, 4 | |
| `crates/org-office/src/organization/mod.rs` | 3, 4 | |
| `crates/org-office/src/{lib.rs, office.rs, office/hq_creation.rs, hq_creation.rs, error.rs, office_file.rs}` | 4 | |
| `crates/org-office/Cargo.toml` | 4, 7 | Drop deps; add `syn` dev-dep |
| `crates/org-office/tests/engine_boundary.rs` | 7 | |
| `crates/org-proto/src/work.rs` (Q8: delete `WriterIdentity` and `JournalPosition` once unused) | 5 | |
| `crates/org-office-launch/src/{writer.rs, contract types for HqCreationV1}` (exact file not verified) | 5 | |
| `crates/org-host/src/{writer.rs, provisioning.rs}` and their tests | 5 | |
| `crates/org-host/src/{enc1_process_tests.rs, web_process_tests/organizations.rs}` | 7 | |
| `apps/agentic-org-office/src/{hq_creation.rs, transfer.rs, transfer/writer.rs, transfer/recovery.rs, inbox.rs, test_support.rs, startup.rs (only if the transfer order needs it)}` | 6 | |
| Docs | 7 | See block 7 |

### Deleted (line counts at `74865317`)

**`crates/org-office/src/store/` (block 4):**
- `store.rs` (1,686)
- `store/acceptance.rs` and `store/acceptance/**` (4,179)
- `store/hq_creation.rs` (430)
- `store/messages.rs` (778)
- `store/org.rs` and `store/org/**` (9,492)
- `store/origin.rs` and `store/origin/**` (1,207)
- `store/owned_processes.rs` (228)
- `store/status.rs` (360)
- `store/sync.rs` (89)
- `store/work.rs` and `store/work/**` (9,176)
- Total: about 27,625 lines.
- `store/open.rs` becomes `store/mod.rs`, plus `store/open/**` moved under it (B1a P9). Optional; the writer may keep the `open` name if the move only churns imports.

**Other `crates/org-office/src` files (block 4):**
- `messaging.rs` (881); `OfficeDirectory`'s logic moves to the worker (Q2b)
- `status.rs` (405)
- `owned_processes.rs` and `owned_processes/**` (424)
- `organization/bootstrap.rs` (265) and `organization/bootstrap/tests.rs` (446)
- `organization/admission.rs` (105)
- `organization/canonical_directory.rs` (146)
- `organization/activity.rs` (185)
- `organization/apply.rs` (41); its only caller is replication
- `organization/status.rs` (209)
- `office/tests/work.rs` and `office/tests/work/current.rs` (598)
- `office/tests/instance.rs` (737): the instance-binding cases are rewritten into `lifecycle.rs`
- `organization/features/create_organization/bootstrap_bytes.rs` (226)
- `ulid.rs` (97), if nothing remains after the deletions **(not verified)**

**`apps/agentic-org-office` (block 2):**
- `src/owned_recovery.rs` (164)
- `src/owned_recovery/tests.rs` (206)
- `tests/owned_recovery.rs` (140)

### Kept, though the old store implemented it

`crates/org-office/src/rules/**`, `employees/**`, and `organization/{structure, people, policy, view, directory}.rs` stay. They are pure rules over `OrgView`, with `rules/memory.rs` as their in-memory implementation. B2 deletes or replaces them; it owns the many-to-many membership model (Q15).

### Overlaps with other work

- **B2** extends `organization/**`, `store/open/operations.rs` and `store/sqlite/organization.rs`. Start B2 after M2, or freeze those files to B1b during blocks 3–4.
- **A5** will touch `apps/agentic-org-office/src/serve.rs` and `lib.rs` (blocks 2 and 6). Sequence A5's office-side edits after M1 and before or after block 6, never concurrently.
- **O1** may touch `crates/org-host/src/catalog.rs` and the supervisor. B1b does not need `catalog.rs`, because the intent file name `hq-creation.v1.json` is kept.
- **Docs:** `docs/FEATURES.md`, `docs/implementation-plan.md`, `docs/design/office-host-boundary.md` and `docs/design/office-store.md` are edited in blocks 2 and 7 only.

## 3. Blocks, in dependency order

Every test uses `tempfile::tempdir()`. No test reads a default location. Writers commit after every block and run only the quick checks listed. "Slow gate" means what the lead runs.

### Block 1 · The golden comparison against A4, and `unsupported`

- **Files.** `crates/org-office/src/organization/features/create_organization/bootstrap_bytes.rs` (test setup only) and `crates/org-proto/src/error.rs`.
- **Golden comparison.**
  - Cause of "HQ provisioning requires recovery" **(inferred from reading, not run)**: `OWNER` (`"01ARZ3NDEKTSV4RRFFQ69G5FAV"`, `bootstrap_bytes.rs:22`) is not the creator's employee ID. `Binding::validate` (`crates/org-office/src/hq_creation.rs:14-20`) refuses unless `bootstrap.creator.employee_id == owner_member_id`.
  - Fix: set the owner to `creation_command().creator.employee_id`. The `Owner` authority returns the same.
  - Run `cargo test -p org-office todays_bootstrap -- --ignored`. Replace the ignore reason with the exact remaining difference list, or remove the ignore if the list is empty.
  - Remaining differences are not fixed in the old bootstrap: B1b deletes it in block 4. Their only consequence is R1, because A4-created organizations are not migrated.
- **`ErrorCode::Unsupported`** (`"unsupported"`). Doc: "the protocol has this call; this office does not serve it yet". `serde(other)` keeps older peers decoding it as `Unknown`. No UI change: the UI never calls the office socket. Its own `'unsupported'` status in `apps/agentic-org-ui/src/lib/platform/http/client.ts:53` is a host-operation outcome, not this code.
- **Tests that would fail without it.** The un-ignored or updated golden test, and a round trip of `ErrorCode::Unsupported`.
- **Quick checks.** `cargo test -p org-proto`; `cargo test -p org-office bootstrap_bytes -- --include-ignored`; clippy on both.
- **Slow gate.** Folded into block 2's.

### Block 2 · The worker stops serving what only the old tables served

The office still keeps organization data on the old store.

- **Files.**
  - `apps/agentic-org-office/src/{lib.rs, serve.rs, msg_calls.rs, live.rs, inbox/hint.rs}`
  - Delete the `owned_recovery` files listed in section 2.
  - `apps/agentic-org-office/serve/tests/{work.rs, inbox*.rs}` as needed.
  - `tests/launch.rs`, `tests/launch/activation/owner.rs`, `tests/launch/inbox_hint.rs`
  - `apps/agent-os/tests/office.rs`, `apps/agent-os/README.md`
  - `docs/FEATURES.md`
- **Contracts.**
  - `serve::OfficePorts`:
    - `mailbox: Option<Arc<Mailbox>>`. `None` answers `msg.send`, `msg.inbox`, `msg.read` and `msg.wait` with `unsupported`. `msg.bind` still binds through `directory`.
    - `feed: Option<Box<dyn OrgFeed>>`. `None` answers `subscribe` with `unsupported`.
    - The `work`, `status` and `operation_get` closures return `Ok(Err(ApiError::new(ErrorCode::Unsupported, ...)))` from composition. The serve logic is unchanged.
  - The serve-layer tests that use `org_mailbox::testkit` in-memory stores keep running with `Some(..)`. They test the serve layer, not the old tables, so they are kept.
- **Behavior.** In `lib.rs`, composition stops calling these:
  - `office.mailbox`, `work_call`, `set_status`, `presence_after_status`
  - `org_head`, `org_can_resume_from`, `org_changes_after`
  - the owned-process API
  
  `who_with_status` becomes `who` with an empty runtime view: everyone offline, no status line. That keeps the directory read working.
- **Tests.**
  - Each unplugged call answers `unsupported` through the real worker. This replaces the message, status, hire-and-work cases in `tests/launch/activation/owner.rs`; the hire uses `member.add_agent`, which keeps working on old organization tables until block 4.
  - `agent-os` message commands against a named office exit with the office-refusal exit code and print no body. `who` still works.
  - Test functions whose subject was the removed storage are deleted: `owned_recovery`, and the SQL-mailbox real-process cases. No `#[ignore]` is added.
- **FEATURES rows:**
  - FC-081 drops "SQL-backed office mailbox … is wired" and becomes In progress or Planned, gap B6.
  - FC-034, FC-036, FC-039 to FC-055 (work rows citing office persistence), FC-102, FC-042, FC-028, FC-145: one sentence each saying the office answers `unsupported` until B4, B6, C3 or S1. Each row is checked against its own text **(not verified row by row)**.
  - FC-072 stays Done. Its scope is the file-queue transport, which does not use the office (P10). Only the README's "Through an office" paragraph changes.
- **Quick checks.** `cargo test -p agentic-org-office --lib`; `cargo clippy -p agentic-org-office -p agent-os --all-targets`.
- **Slow gate.** Workspace clippy and test, including `apps/agentic-org-office/tests/launch*` and `apps/agent-os/tests/office.rs`; mocked e2e; kit check.
- **Merge point M1** (blocks 1–2).

### Block 3 · org-office additions the switch needs

Both engines; no production caller yet.

- **Files.** `office_local/**`, `organization/snapshot.rs`, `organization/mod.rs`, `store/open.rs`, `store/open/operations.rs`, `store/open/tests.rs`, `lib.rs` (exports, so no dead code), `organization/testkit.rs` (a CA fixture).
- **Contracts.**
  - **Office-local family** (Q1, Q2, Q3). Paths:
    - `state/security/ca-escrow.v1.bin`: the exact ADR-0007 package, raw bytes, published to Git.
    - `local/offices/<office-id>/security/ca-key.v1.json`: `{schema_version:1, organization_id, office_id, operation_id, escrow_version:1, public_key (hex, 65 bytes), package_sha256, wrapped_ca_key (hex)}`. Exact field names are the writer's; one JSON object, fixed field order.
    - `local/offices/<office-id>/clean-stop`, owned so that rebuild never fails on it (P6).
    - The projector writes no rows (probe pattern) and `validate` checks nothing across families.
    - `office_local::read_ca(files: &org_store::Files, office) -> Result<Option<CaRecords>, _>` reads both files. It is used at open and by the public-record read; neither is a hot read path (design §7 rule, recorded in Q2).
  - `CreateOrganizationOp` gains `ca: CaMaterial { public_key: [u8;65], package: Vec<u8>, wrapped_ca_key: Vec<u8>, operation_id: String }`.
    - `run` calls `create_organization::decide`, then appends the two CA file changes with `.create` (exclusive). They land in the same entry.
    - The operation's fingerprint is recomputed over the organization command's fingerprint plus SHA-256 of the CA bytes. A retry with regenerated material conflicts; a retry with the retained material matches.
  - `DirectorySnapshot: ReadOp<Out = org_proto::OrgSnapshot>` (via `organization::snapshot::project(read) -> OrgSnapshot`). Mapping:
    - `teams`: from `Team`. `owner_team_id = parent_team_id`, `lead_member_id = lead_employee_id`, `version = meta.revision`.
    - `members`: from `Employee`.
      - `kind` and `name` map directly.
      - `owner = (id == owner member)`; the owner member is passed in, taken from the creation binding (Q10).
      - `agent: None` and `office_id: None` for agents, so the HQ inbox source never nags about HR (`apps/agentic-org-office/src/inbox/org_source.rs:31-41`).
      - `reports_to_member_id`, `created_at` from meta, `version`.
    - `memberships`: one per employee for its home team, with `title_in_team`.
    - `projects`, `project_teams`, `areas`: empty.
    - `position = Position { epoch: 1, seq: generation }` (Q16).
  - `TeamCreateWire: StoreOp<Out = Result<Result<CommandAccepted, ApiError>, StoreError>>`.
    - Input: the wire's `TeamCreateParams` and the grant-bound member.
    - `command_id` (an uppercase ULID, `crates/org-proto/src/org.rs:793-797`) maps to the store operation `Id` by lowercasing (Q4). A non-UUIDv7 `team_id` is `invalid_request`.
    - `owner_team_id = None` means the parent is the top-level team.
    - Authorization (Q9): the member must be an active person who leads the parent team or one above it; otherwise `not_authorized`.
    - Answers:
      - A reply is `CommandAccepted { status: Committed, position: Some(Position{1, generation}) }`.
      - `TeamRejected { ParentArchived }` is `conflict`.
      - A missing parent is `not_found`, pre-checked by a read inside `decide`.
      - Other `Refusal::Invalid` is `invalid_request` with the fixed reason.
  - **Clean-stop marker** (Q3).
    - `open_store` reads `local/offices/<office-id>/clean-stop` before `Store::open`, removes it, and syncs the directory. `OfficeStore::stopped_cleanly() -> bool` reports whether it was there.
    - `OfficeStore::close()` writes it (`sync_all` plus a directory sync) after `Store::close()` and the engine checkpoint succeed.
    - A fatal store, a drop, or a failed close never writes it.
  - `OfficeStore::fatal() -> Option<StoreError>`: `status().stopped`.
- **Tests (both engines).**
  - The office-local suite: exact bytes of both CA files (STORE-6); an extra unowned file under `local/offices/<id>/` fails rebuild with its path; rebuild-equivalence with creation plus CA.
  - The snapshot mapping on the golden creation: generation 1, HR with `agent: None`, the creator as `owner`.
  - `team.create` via the wire op:
    - a retry with the same `command_id` gets the same answer;
    - another payload with that `command_id` is `operation_reused`/`invalid_request`, per the op's conflict mapping;
    - `ParentArchived` is `conflict`; an unknown parent is `not_found`; a non-lead person is `not_authorized`.
  - The marker: written by `close`; absent after a drop or a crash point; removed at the next open and reported once.
  - A creation retry with regenerated CA bytes conflicts; with the same bytes it gets the original reply.
- **Quick checks.** `cargo test -p org-office` and `cargo test -p org-office --features testkit`; `cargo clippy -p org-office --all-targets --features testkit -- -D warnings`.
- **Slow gate.** Workspace clippy and test. Green: additive.

### Block 4 · `Office` on the new store; the old store deleted

org-office only. May run in parallel with block 5.

- **Files.** `crates/org-office/src/{office.rs, office/hq_creation.rs, hq_creation.rs, lib.rs, error.rs, office_file.rs, Cargo.toml}`, the deletions in section 2, the new tests, and `organization/mod.rs`.
- **Contracts.** These are the interface block 6 codes against; signatures matter. Names are indicative.
  ```rust
  pub struct DataRoots { pub durable: PathBuf, pub runtime: PathBuf }           // absolute; <data-root>/<org-id>/{durable,runtime}
  pub struct Custody { pub identity: org_store::OfficeIdentity,
                       pub authority: Arc<dyn org_store::WriterAuthority> }
  impl Office {
    pub fn provision_hq(folder: impl Into<PathBuf>, new: &NewOffice, key: Key, binding: &hq_creation::Binding,
        roots: DataRoots, custody: Custody,
        material: impl FnOnce(Option<LocalRecord>) -> Result<LocalRecord, OfficeError>) -> Result<Office, OfficeError>;
    pub fn open(folder: impl Into<PathBuf>, key: Option<Key>, instance: &str,
        roots: DataRoots, custody: Custody) -> Result<Office, OfficeError>;
    pub fn discard(folder: impl Into<PathBuf>, office_id: &str) -> Result<(), OfficeError>;
    pub fn close(self) -> Result<(), OfficeError>;          // orderly: store close, checkpoint, clean-stop marker
    pub fn fatal(&self) -> Option<String>;                  // the store stopped (flush or append failure), fixed text plus path
    pub fn stopped_cleanly(&self) -> bool;
    pub fn org_snapshot(&self) -> Result<OrgSnapshot, OfficeError>;
    pub fn who(&self, params: &WhoParams) -> Result<Result<WhoResult, ApiError>, OfficeError>;
    pub fn submit_org(&self, member: &str, command: OrgCommand) -> Result<Result<CommandAccepted, ApiError>, OfficeError>;
    pub fn is_hq(&self) -> Result<bool, OfficeError>;      // replaces is_replication_hq
    pub fn hq_creation_record(&self) -> Result<Option<LocalRecord>, OfficeError>;
    pub fn hq_creation_public_record(&self) -> Result<Option<PublicRecord>, OfficeError>;
    pub fn secrets_key(&self) -> Option<&Key>; pub fn info(&self) -> &OfficeInfo; pub fn folder(&self) -> &Path;
    #[cfg(any(test, feature = "testkit"))]
    pub fn for_test(spec: TestOffice) -> Office;            // temp folder and roots, FakeCustody, EngineChoice (Memory by default), fixed clock
  }
  ```
  - `submit_org` answers `team.create` through `TeamCreateWire`. Every other `OrgCommand` is `unsupported` (B2).
  - `org_operation_get`, `org_activity`, replication, snapshot export and staging, `seed_organization_for_test`, `bind_selected_owner`, `install_selected_acceptance`, `finish_selected_bootstrap`, `validate_hq_creation_admission`, `commit_hq_creation` and `verify_hq_creation_binding` are deleted, as is the `OrgStore` impl (`office.rs:200-216`).
  - `org_office::{WriterAuthority, AcceptanceConfig, AcceptanceIdentity, JournalPosition, OwnerAuthority, OwnerAccount, OfficeMessageStore, OfficeDirectory, OwnedProcess*, presence_after_status, OrganizationSnapshot*}` leave `lib.rs`.
- **Opening** (design §4 "Opening"):
  1. Take `office.lock` in `<home>/offices/<office-id>/`; this lock is the office lock (Q6b).
  2. Read `office.json` and the intent (`hq-creation.v1.json`, content `version: 2`; Q7).
  3. Refuse a folder holding `org.db` or `office.db`, or with no intent, with `OfficeError::Invalid("this office was made by an older build")`. The host maps this to `storage_error` (Q6).
  4. Check that the intent's roots and IDs equal `roots` and `custody.identity`.
  5. Run `open_store` with `Opening::Existing`.
  6. Read the projection: the organization exists, its ID equals `office.json`'s, and this office is HQ.
  7. Read the CA records with `office_local::read_ca`.
- **Creation:**
  1. Validate the request (today's `validate_creation_request`, minus `public_root`).
  2. Take the lock.
  3. Freeze intent v2 (`instance, operation, owner, office_id, office_name, org_id, org_name, created_at, durable_root, runtime_root, bootstrap`) with the atomic write of `office_file.rs:162-176`.
  4. Use `Opening::Create(AnchorFacts{zeros, no predecessor})` if the journal incarnation folder (`org_store::journal_dir`) is absent; otherwise use `Existing`.
  5. `prior` is the CA records if present. `frozen = material(prior)`. Its operation and organization must match.
  6. Run `CreateOrganizationOp { operation_id: lowercase(binding.operation_id), here: identity, command: from PreparedV1 + office name, ca }`. It is durable.
  7. Write `office.json` last.
  8. A creation that fails before entry 1 leaves the intent and the journal anchor. A retry with the same intent resumes; another request is refused (`OfficeExists`).
  9. Nothing under `durable/` is ever removed.
  10. The `workspace/` folder is no longer made (Q14).
- **`discard`.** A folder with `office.json` or an intent is `OfficeExists`. A folder with neither is emptied and removed. Database checks go: there are none.
- **Errors.** `From<org_store::StoreError>` for `OfficeError` keeps paths in the cause (logged) and out of `Display`.
- **Tests that would fail without the change.** Office level; memory by default, a SQLite subset via `TestOffice.engine`.
  - `production_bytes.rs`: `provision_hq` with a fixed clock writes exactly `creation_files()` plus the two CA files under `durable/` (STORE-6, production path). This replaces the A4 comparison.
  - A foreign `state/organization.yaml` planted before creation stops creation, and the error's cause names the path (STORE-6, second half).
  - Deleting `runtime/<office-id>/` after creation and a team: reopen rebuilds an equal `dump()` and an equal `org_snapshot`.
  - Drop as kill: `team.create` answered, the `Office` dropped (not closed), reopen has the team; `stopped_cleanly()` is false. `close()` then reopen gives true.
  - A creation retry after a crash point (store crash hooks via `Testing`) yields the same CA public key and one entry 1, and `material` is called with `Some(prior)`.
  - Lock, refusal and discard cases from today's `office/tests.rs`, kept where their subject still exists: lock wait, `office.json` last, link refusal, foreign entries.
  - A legacy folder holding `org.db` is refused.
  - `hq_creation_public_record` returns the escrow bytes and matches `ca-key.v1.json`'s `package_sha256`. ENC-5 in-process: the projection file contains neither the raw nor the wrapped key bytes.
  - `fatal()` reports a flush error injected through `Testing.faults`.
- **Order inside the block.** 4a builds the new `Office` and its tests while the old modules are still present. Clippy then shows dead code in the old store, so 4a is a commit and not a gate. 4b deletes the old store (`git rm`) and fixes what is left.
- **Quick checks.** `cargo test -p org-office --features testkit`; `cargo clippy -p org-office --all-targets --features testkit -- -D warnings`.
- **Slow gate.** org-office only. The workspace does not compile until block 6, because `apps/agentic-org-office` uses the removed API.

### Block 5 · Host custody for the new journal; writer wire v2

May run in parallel with block 4.

- **Files.** `crates/org-office-launch/src/writer.rs`; `crates/org-office-launch` create-line types (`HqCreationV1`); `crates/org-proto/src/work.rs` (Q8); `crates/org-host/src/writer.rs`, `writer/tests.rs`, `provisioning.rs`, `provisioning/tests.rs`.
- **Wire, `org_office_launch::writer`, `VERSION = 2`:**
  - `Identity { instance_id, organization_id, office_id, journal_incarnation, writer_epoch: u64, machine_id }`
  - `Head { ordinal: u64, sha256: String(hex64), high_waters: HighWaters { organization, work, message_content, message_state: u64 } }`
  - `Admission { version, identity, durable_root, runtime_root, owner_account, owner_member }`
  - `Request` and `Response` carry `Head`.
  - `HqCreationV1` drops `public_root` and its `version` becomes 2. The data roots travel only in the writer admission.
- **Host behavior:**
  - **Reservation:**
    - `journal_incarnation = ids::uuid_v7()`, not `ids::generate()` (`crates/org-host/src/writer.rs:386`; P1).
    - `machine_id` stays the 64-hex device value (`writer.rs:384`), which is already a valid store `Id`.
    - `writer_epoch = 1`.
    - Roots: `<instance-root>/<org-id>/durable` and `<instance-root>/<org-id>/runtime`, created `0700` and required empty at first reservation. This replaces `provisioning.rs:638-669`.
  - **`Confirm`** follows F4:
    - same identity;
    - `before == stored`;
    - `after.ordinal > before.ordinal`, or equal with the same hash;
    - every high-water ≥ the stored one;
    - `before = None` requires `after.ordinal == 0`.
    - This replaces `writer.rs:519-526`.
  - The record's `schema_version` becomes 3. Older records are refused (pre-v1, no migration).
  - `Request::Owner` stays served. The worker stops calling it (Q10).
- **Tests that would fail without it:**
  - a confirm advancing several ordinals is accepted;
  - an equal and identical confirm is accepted;
  - a lower high-water is refused;
  - a `before` mismatch is refused;
  - a reservation mints a UUIDv7 incarnation and the contract roots;
  - a retried reservation with other roots is refused;
  - a provisioning test with a fake worker passes the new roots.
- **Quick checks.** `cargo test -p org-office-launch -p org-proto`; `cargo test -p org-host --lib writer provisioning`; clippy on those.
- **Slow gate.** Those crates only. Real-process host tests need block 6.

### Block 6 · Worker composition on the new office

Needs blocks 4 and 5. The workspace compiles again.

- **Files.** `apps/agentic-org-office/src/{lib.rs, hq_creation.rs, transfer.rs, transfer/writer.rs, transfer/recovery.rs, inbox.rs, serve.rs (SubmitFn takes the member), directory.rs (new), test_support.rs}`, and the unit tests and `tests/launch/**` fixtures needed to compile and pass.
- **Contracts.**
  - `transfer::writer` implements `org_store::WriterAuthority` over wire v2. `CustodyError` keeps fixed text; a closed channel fails closed. `install(stream, instance, office_id) -> (Custody, Admission)` only receives and checks the admission; it no longer touches `Office`.
  - `SubmitFn = Box<dyn Fn(&str /* member */, OrgCommand) -> ...>`. `serve.rs` passes `binding.member()` for grant-bound callers (`serve.rs:1113-1134`).
- **Behavior.**
  - **Create** (`create_and_serve`):
    1. Receive the writer control first (`control.transfer()`, then `transfer::install_initial`). The host already sends it right after the handoff (`crates/org-host/src/supervisor/channels.rs:399-410`).
    2. Run `hq_creation::prepare(..., roots, custody)`, then emit `Created`.
  - **Start** (`open_and_serve`): receive the writer control first, then run `Office::open`. A start with no writer control is refused (every office now needs custody).
  - **Serving:** `serve_office` stops calling `initial_writer_admission`. Its stop predicate adds `office.fatal().is_some()`, which logs the fixed text and exits 1 so the host restarts.
  - **Shutdown:** `Arc::try_unwrap(office)` then `office.close()`. If other references remain, log it and drop (unclean, marker absent).
  - **Wiring:** inbox and recovery-public use `is_hq`. `msg.bind` uses the new `directory.rs` over `org_snapshot`, with `org_office::directory::resolve_member`.
- **Shared fake custody.** `test_support::FakeWriter` serves checkpoint and confirm with the F4 rule over a socket pair. It is extracted from `tests/launch/activation.rs:985-1040`. Real-process launch tests use it to act as the host.
- **Tests that would fail without it:**
  - the adapter sends one `Confirm` per flush (count against `FakeWriter` while committing three `team.create` concurrently);
  - a create and a restart through the real worker binary with `FakeWriter`;
  - `team.create` on a grant channel, answered, then `SIGKILL`; the restart shows the team (done-when 2);
  - a recovery-public read returns the escrow;
  - start without writer control is refused;
  - an injected fatal store exits 1.
- **Quick checks.** `cargo build --workspace`; `cargo test -p agentic-org-office --lib`; clippy on the touched crates.
- **Slow gate.** Workspace clippy and test, real-process tests, the real-instance smoke below, mocked e2e.
- **Earliest production-path verification is here.** On a temp instance: `agentic-org setup`, sign in, create an organization in the browser. Restart the service. Delete `<instance-root>/<org-id>/runtime/<office-id>/`, restart, and the office is active. Run `apps/agentic-org-ui/e2e-real/first-slice.spec.ts` and `cargo test -p org-host web_process_tests::organizations`.

### Block 7 · Tests on the new layout, guards and records

- **Files.** `crates/org-host/src/{enc1_process_tests.rs, web_process_tests/organizations.rs, provisioning/tests.rs}`, `crates/org-office/tests/engine_boundary.rs`, `crates/org-office/Cargo.toml` (`syn` dev-dep), the remaining `apps/agentic-org-office/tests/launch/**`, and these docs:
  - `docs/FEATURES.md`: FC-141, FC-146, FC-165, FC-018, FC-016, FC-162, FC-142
  - `docs/implementation-plan.md`: B1b "Code today", plus a B2 note on `rules/`
  - `docs/design/office-store.md`: §2, §11 done, open points 1 and 7 decided, Q2
  - `docs/adr/0021-office-store-architecture.md`: consequences
  - `docs/adr/0007-hq-creation-escrow.md`: step 2 "office database" becomes "protected local record"
  - `docs/design/office-host-boundary.md`: §HQ creation context, §5 storage, "What the code does today"
  - `docs/design/organization-data/README.md`: line 87
  - `docs/design/organization-data/01-organization-structure.md`: lines 178-182
  - `docs/design/create-organization.md`: lines 154-156
  - `docs/design/standalone-server.md`: line 199 (P15)
  - `apps/agent-os/README.md`
- **Tests.**
  - `engine_boundary.rs` gains the workspace `syn` scan:
    - any `rusqlite` path, or an SQL-keyword string literal, in `crates/org-office` outside `src/store/sqlite/` fails;
    - the allowlist is empty;
    - the manifest check stays: `org-office` and `org-host` only.
    
    This is ENG-1 for the office.
  - `enc1_process_tests.rs`:
    - read the wrapped key from `local/offices/<office-id>/security/ca-key.v1.json`, not from SQLite;
    - assert `runtime/<office-id>/office.db` is plain SQLite (header bytes, no `rusqlite`) and holds neither key form;
    - the key-less restart still reaches `active`.
  - `web_process_tests/organizations.rs`:
    - the files root ends with `<org-id>/durable/state`;
    - new case: create, stop, delete `runtime/<office-id>/`, restart; the organization is listed and the office is `active`;
    - the recovery-release test passes unchanged except for paths.
- **Quick checks.** `cargo test -p org-office --test engine_boundary`; `cargo test -p org-host --lib provisioning`. The writer does not run docs checks.
- **Slow gate.** Everything: workspace clippy and test, real-process, mocked and real-instance e2e, kit check.
- **Merge point M2 and acceptance here.**

### Parallel split

- **Writer A:** blocks 3 then 4, in worktree A. Owns `crates/org-office/**`.
- **Writer B:** block 5, in worktree B. Owns `crates/org-host/**`, `crates/org-office-launch/**` and `crates/org-proto/src/work.rs`.
- No shared file. Block 6 needs both and is done by one of them after rebasing.
- Blocks 1, 2, 6 and 7 are single-writer.
- The saving is about one session of wall time. The risk is interface drift, which the signatures in blocks 4 and 5 bound.

## 4. Open design questions

- **Q1 · The wrapped CA key in the journal.**
  - Document 01 (lines 179-182) says the usable key is "excluded from … all selected journals/history". Design §11 makes it a protected record through the store. ADR-0007 step 3 wants it in one initial transaction with the organization.
  - **Recommendation:** entry 1 carries the wrapped key (never the raw one) at `local/offices/<id>/security/ca-key.v1.json`. The journal sits under `durable/recovery/`, which is never Git. Amend document 01 to "excluded from organization Git, copies, exports and projections; the protected journal holds it wrapped until released".
- **Q2 · Who owns the escrow, the CA record and the marker.** B1a's family owns neither path (`records.rs:822-860`), and rebuild fails on unowned files (`crates/org-store/src/rebuild.rs:38-44, 103-112`).
  - **Recommendation:** a row-free `office_local` family. CA reads go through `org_store::Files` at open and for the recovery-public read, never on hot paths.
  - **Q2b:** move `OfficeDirectory` into the worker (`apps/agentic-org-office/src/directory.rs`). `org-office` then drops `org-mailbox`.
- **Q3 · The clean-stop marker.**
  - **Recommendation:** the design's place, `local/offices/<office-id>/clean-stop`, owned by `office_local`.
  - Removed and the directory synced before `Store::open`. Written after `Store::close` and the checkpoint, with `sync_all` and a directory sync, not a full flush: a lost marker only means "unclean". Reported as `stopped_cleanly()` for B4.
- **Q4 · Wire command IDs.** They are uppercase ULIDs (`org.rs:793-797`, `hq_creation.rs:19`). Store IDs must be lowercase path components (`change.rs:114-119`; design §5).
  - **Recommendation:** lowercase them deterministically in the adapter, recorded in the receipt's kind docs. The ID slice moves the wire to UUIDv7 later.
- **Q5 · `unsupported`.** **Recommendation:** add `ErrorCode::Unsupported` rather than reuse `Unavailable`, which invites retries.
- **Q6 · Legacy and A4-era offices.**
  - **Recommendation:** refuse with a fixed message; no migration. **Owner to confirm** (R1).
  - **Q6b:** the existing `office.lock` is the design's "office lock"; host custody fences across machines. No new lock file.
- **Q7 · The intent file.** **Recommendation:** keep `hq-creation.v1.json`'s name, so `crates/org-host/src/catalog.rs:273` is unchanged. Content becomes version 2 with the data roots. Discard and start verify against it.
- **Q8 · Home of the writer wire.** **Recommendation:** move `WriterIdentity`/`JournalPosition` out of `org_proto::work` into `org_office_launch::writer` as v2 types. The channel is the private host–worker wire, not the public protocol.
- **Q9 · `team.create` authorization before B2.** **Recommendation:** a grant-bound, active person who leads the parent team or one above it. The creator qualifies. B2 replaces this.
- **Q10 · The owner.**
  - **Recommendation:** the snapshot's `owner` flag comes from the creation binding's owner member, frozen in the intent.
  - The worker stops calling `OwnerAuthority::verify_live`. The host keeps answering `Request::Owner` until A5 or B2 decide whether to use or delete it.
- **Q11 · `subscribe`.** **Recommendation:** `unsupported`. No subscriber exists (FC-036). Organization `org.changed` events cannot be produced from file changes without B2's change mapping.
- **Q12 · `operation.get` for org commands.** **Recommendation:** `unsupported` until B2 maps store receipts.
- **Q13 · A fatal store.** **Recommendation:** the worker exits 1 after logging. The host's restart policy applies.
- **Q14 · The `workspace/` folder.** **Recommendation:** stop making it. No reader was found outside creation and discard **(not verified exhaustively)**.
- **Q15 · `rules/`, `employees/`, and `organization/{structure,people,policy,view}`.** **Recommendation:** keep them for B2. Delete only what loses every caller: `apply.rs`, `canonical_directory.rs`, `activity.rs`, `status.rs`, `bootstrap*`, `admission.rs`.
- **Q16 · `Position` from a generation.** **Recommendation:** `epoch: 1, seq: generation`, documented in `organization/snapshot.rs`. Whether clients treat `epoch` specially is **not verified**.

## 5. Premises the code contradicts

| # | Requirement or design says | Code shows |
| --- | --- | --- |
| P1 | The host custody adapter can feed `org_store::OfficeIdentity` | `OfficeIdentity` needs safe lowercase path components (`crates/org-store/src/change.rs:114-119`, `custody.rs:16-23`). The host mints `journal_incarnation` with `ids::generate()`, an uppercase ULID (`crates/org-host/src/writer.rs:386`, `ids.rs:21`). `writer_epoch` is `i64` (`crates/org-proto/src/work.rs:1708`) against `u64`. Provisioning passes empty strings that `reserve` overwrites (`provisioning.rs:660-667`) |
| P2 | Host confirms once per flush (F4) | The rule still requires `after.follows(p)`, ordinal + 1 (`crates/org-host/src/writer.rs:519-526`; `org-proto/src/work.rs:1741`). The wire carries one `origin_high_water` (`work.rs:1716-1722`) |
| P3 | Paths `<data-root>/<org-id>/durable/state/…` | Host passes `public_root = <instance-root>/durable/state/<org-id>` and `protected_root = <instance-root>/durable/authority/<office-id>` (`provisioning.rs:638-669`), plus `HqCreationV1.public_root` (`:587`). Escrow at `<public_root>/security/ca-escrow.v1.bin` (`office/hq_creation.rs:362, 383-401`) |
| P4 | Calls "answer `unsupported`" | No such error code (`crates/org-proto/src/error.rs:19-120`). The closest pattern today is `Unavailable` (`apps/agentic-org-office/src/serve.rs:1019-1027`) |
| P5 | `team.create` answers through the new store | The wire `command_id` is an uppercase ULID (`org.rs:793-797`). Store operation IDs must be lowercase (design §5) |
| P6 | Clean-stop marker under `local/offices/<office-id>/` (design open point 7) | Rebuild walks that prefix and fails on a file no family owns (`rebuild.rs:38-44, 103-112`) |
| P7 | CA escrow under `state/security/` (document 01:151) | No family owns it (`organization/records.rs:822-860`), so a rebuild after B1b would fail |
| P8 | CA key "excluded from all selected journals" (document 01:179-182) | Design §11 and ADR-0007 step 3 put it in the creation transaction, which is entry 1 |
| P9 | Design §2 and §11 delete `messaging.rs` with messaging | `OfficeDirectory` (`messaging.rs:31-32, 137-147`) also serves `msg.bind`'s identity lookup and `who` binding, which is not message storage |
| P10 | FC-072 goes dark | FC-072's scope is the file-queue transport (`docs/FEATURES.md:182`), which does not use the office (`apps/agent-os/README.md:39-50`). Only the office route goes dark; FC-081 (`FEATURES.md:193`) does go dark |
| P11 | "The commands A5 exercises keep working" | A5 is not built: the implementation plan's A5 "Code today" and the milestone log list it as next. The meaning is the native calls A5 will use: `org.snapshot`, `who`, `team.create` |
| P12 | Orderly stop writes the marker | The worker drops `Office` (`apps/agentic-org-office/src/lib.rs:551-552`). A `Store` drop is a kill by design (`crates/org-store/src/store.rs:612-627`). An explicit `close` is needed |
| P13 | B1a plan: org-office has a `syn` dev-dependency | `crates/org-office/Cargo.toml` has none. F7 moved the scan to B1b |
| P14 | Golden comparison follow-up | Cause **(inferred, not run)**: `OWNER` ≠ creator ID (`bootstrap_bytes.rs:22`, `hq_creation.rs:14-20`). The comparison's subject, the old bootstrap, is deleted in block 4 |
| P15 | One layout | `standalone-server.md:199` lists `durable/journal/`. The store writes `durable/recovery/offices/<office-id>/journals/acceptance/<incarnation>/` (`office-store.md:15`; `crates/org-store/src/journal.rs:530-536`) |
| P16 | ENG-1: SQL only in the store adapters (ADR-0018:63) | `crates/org-host/src/enc1_process_tests.rs:171-190` opens the office's databases with `rusqlite` |
| P17 | Plan "Code today": custody "called twice per write and once per read" | Partly verified. A read calls `checkpoint` through `verify_authority` (`store/acceptance.rs:167-180, 413-419`); a write runs `selected_read` then `confirm` (`:141-163, :420-430`). The adapter also serves `Owner` (`transfer/writer.rs:79-98`). B1a's `Store::open` already does the checkpoint and confirm at open (`org-store/src/store.rs:355-368, 433-444`), so B1b's custody work is the adapter and the host rule, not store code |

The other "Code today" claims are verified:
- `office.rs` opens `org.db` and `office.db` and writes `meta` (`office.rs:399-428, 813-822, 843-858`).
- The creation record is in `office/hq_creation.rs` and `store/hq_creation.rs`. `crates/org-office/src/hq_creation.rs` (`Binding`, `LocalRecord`, `PublicRecord`) is a third file the text omits.
- `lib.rs` wires the mailbox to the old tables (`apps/agentic-org-office/src/lib.rs:396`, `messaging.rs:44-63`).

## 6. Risks

- **R1 · The owner's existing instance.** An organization created through A4 has `org.db` and `office.db` and no journal. After B1b its office is refused. **Mitigation:** tell the owner before M2; reset the instance or create the organization again; no migration (pre-v1).
- **R2 · M2 is not green per block.** Blocks 4 to 6 break the workspace build until block 6. **Mitigation:** the commit boundaries are stated, the lead's gates run at the end of block 6 and at M2, and the parallel split shortens the red window.
- **R3 · Host writer records.** Schema 3 refuses records left by earlier test or real instances. Covered by R1.
- **R4 · Test churn.** Real-process tests rely on `Office::create` with legacy IDs (`apps/agentic-org-office/tests/launch*.rs`, `transfer/tests.rs`, `tests/transfer_composition.rs`, `apps/agent-os/tests/office.rs`). **Mitigation:** `Office::for_test` and `test_support::FakeWriter` come first in blocks 4 and 6.
- **R5 · Wrapped CA bytes in journal segments** until release. ENC-5's scope and the enc1 scan change (Q1).
- **R6 · Dead code under `-D warnings`** during 4a and after deletions. Remove items rather than allow them.
- **R7 · Kill timing.** `SIGKILL` cases wait on the reply and on store status, never on sleeps.
- **R8 · Durable creation now waits for the full flush and host confirmation.** Real-process deadlines may need slack on macOS **(not measured)**.
- **R9 · Concurrent increments.** B2 and A5 overlap (section 2); coordinate.
- **R10 · Inbox regression.** If HR mapped with an agent profile, a phantom "agent not placed" Problem would appear. Covered by `agent: None` (block 3 test).

## 7. Acceptance map

| Done-when (B1b) | Block and test |
| --- | --- |
| Organization created through the host; deleting `runtime/<office-id>/` and restarting rebuilds with no loss | Block 4: in-process delete-and-reopen, equal `dump()` and `org_snapshot`, both engines. Block 7: `web_process_tests::organizations` create, stop, delete runtime, restart, still active and listed. Block 6: real-instance smoke |
| A committed organization write survives killing the office right after its answer | Block 4: drop-as-kill after `team.create`. Block 6: real worker `SIGKILL` after the `team.create` answer, restart shows the team |
| Files at the contract's paths under `durable/state/` | Block 4: `production_bytes.rs`, exact `creation_files()` plus CA files under `<org-id>/durable/`. Blocks 5 and 7: host provisioning and web-process tests assert `<instance-root>/<org-id>/durable/state/organization.yaml` |
| `rusqlite` and SQL in `org-office` only under `src/store/sqlite/` (ENG-1) | Block 7: `engine_boundary.rs` `syn` scan, empty allowlist |
| The office's tests pass on the memory engine through `open_store` (ENG-2) | Block 4: `Office::for_test` defaults to `EngineChoice::Memory`; the office suites run on memory with a SQLite subset |
| STORE-6 | Block 3: exact CA-file bytes. Block 4: exact creation bytes on the production path, and a foreign file stops creation naming the path |
| Recovery-code delivery unchanged | Block 6: recovery-public read. Block 7: `actual_recovery_code_is_released_once_natively_and_no_credential_reaches_http_or_a_plain_file` passes |
| Calls only the old tables served answer `unsupported`; tests deleted or rewritten, never disabled | Block 2: real-worker `unsupported` cases. Block 4: other `OrgCommand`s. The block 2 and 7 diffs add no `#[ignore]` |
| FEATURES rows updated | Blocks 2 and 7 |
| Golden follow-up | Block 1: run against A4. Block 4: replaced by the production-path test |

## 8. Inventory of today's consumers of the old store

"Store" means `crates/org-office/src/store.rs` and `store/**`, excluding `store/open*` and `store/sqlite/**`.

**`crates/org-office/src/office.rs`:**
- `OFFICE_SCHEMA` and `OfficeStores` (`:74-115, :172-176, :786-802`): deleted. Replaced by `OfficeStore`.
- `create*` and `build`, with `put_meta`/`meta`: deleted. Creation goes through `provision_hq`, and `for_test` serves tests.
- `open*`: becomes the new `open`.
- `discard`: rewritten without database checks.
- `OrgStore` impl, `bind_replication_*`, `replication_*`, `export_*`/`stage_replication_snapshot`: deleted (S1).
- `org_snapshot`: new store, via `DirectorySnapshot`.
- `who`: new store.
- `submit_org`: new store for `team.create`; every other command is `unsupported`.
- `org_operation_get`: `unsupported` (B2).
- `org_head`, `org_can_resume_from`, `org_changes_after`: deleted. `subscribe` is `unsupported`.
- `org_activity`: deleted.
- `org_change_token`, `wake_followers`, `wait_for_org_change`: kept only if still used after block 6; otherwise deleted.
- `seed_organization_for_test`: deleted. `for_test` replaces it.
- `work_call`, `work_activity`: deleted; the worker answers `unsupported` (B4).
- `bind_selected_owner`, `install_selected_acceptance`: deleted (Q10).
- `local_store`, `org_store`: deleted.

**`crates/org-office/src/office/hq_creation.rs` and `crates/org-office/src/hq_creation.rs`:**
- `Intent`: becomes v2 with the roots.
- `provision_hq_for_instance`: becomes `provision_hq` on the new store.
- `verify_completed_history`, `verify_retained_binding`'s SQL, `validate_public_root`, `publish_public`: deleted.
- `verify_public`: becomes a check of the escrow file against `package_sha256`.
- `retained_hq_creation_binding`: reads the intent only.
- `validate_hq_creation_root`, `validate_hq_creation_admission`, `commit_hq_creation`, `verify_hq_creation_binding`: deleted.
- `hq_creation_record` and `hq_creation_public_record`: read through `office_local::read_ca`.
- `Binding`: loses `public_root`.

**`organization/*`:**
- `bootstrap.rs` (selected SQL) and its tests: deleted. Creation is entry 1.
- `admission.rs` (`OwnerAuthority`, `selected_owner_authority` SQL): deleted (Q10).
- `canonical_directory.rs`, `activity.rs`, `apply.rs`, `status.rs`: deleted.
- `directory.rs`: kept. It works on `OrgSnapshot`.
- `structure`, `people`, `policy`, `view`: kept for B2.
- `features/create_organization/bootstrap_bytes.rs`: replaced.

**Messaging and status:**
- `messaging.rs` (`OfficeMessageStore` over `store::messages` and `origin`; `mailbox()`; the SQL conformance tests): deleted; `unsupported` (B6). `OfficeDirectory`'s logic moves to `apps/agentic-org-office/src/directory.rs`, over the new snapshot.
- `status.rs` (`set_status`, `who_with_status`, `set_current`) and its tests: deleted; `status.set` is `unsupported` (B6). `who` has no status lines.

**Owned processes:** `owned_processes.rs` and its tests are deleted (C3). The worker's `owned_recovery.rs`, its tests and `tests/owned_recovery.rs` are deleted in block 2.

**`processes/`:** a README only; no consumer.

**`employees/`:** used only by `rules/decide.rs`; no store use. Kept.

**`rules/`:**
- `OrgCommand::parse` is kept; `serve.rs` and the `agent-os` tests use it.
- `decide` and `memory` are kept for B2.
- `store/org.rs`'s `OrgView` implementation is deleted with the store.

**`apps/agentic-org-office/**`:**
- `lib.rs` ports:
  - `work`, `status`, `operation_get` → `unsupported`
  - `snapshot`, `who`, `submit` → new store
  - `feed` → `None`
  - `mailbox` → `None`
  - `directory` → new
  - `register_inbox` → `is_hq`
  - shutdown → `close`
- `hq_creation.rs`: `prepare` and `verify_existing` use the new signatures.
- `transfer/writer.rs`: now implements the `org_store::WriterAuthority` adapter; the selected acceptance and owner installation are deleted.
- `transfer/recovery.rs`: uses `is_hq` and `hq_creation_public_record`.
- `inbox.rs`: `org_snapshot`, new store.
- `owned_recovery.rs`: deleted.
- `serve.rs`: the optional ports, and `SubmitFn` takes the member.
- Tests:
  - `tests/launch.rs`, `launch/activation*.rs`, `launch/inbox_hint.rs`, `src/hq_creation/tests.rs`, `src/transfer/tests.rs`, `src/tests/transfer_composition.rs`: rewritten on `FakeWriter` and `for_test`.
  - Satellite and replication cases (`bind_replication_satellite`) are deleted (S1).
  - Message, status and work cases become `unsupported` assertions.

**Other crates:**
- `apps/agent-os/tests/office.rs`: `Office::create` becomes the real create path with a fake writer. Message cases assert `unsupported`; `who` stays.
- `crates/org-host/src/enc1_process_tests.rs` (`rusqlite` on `office.db`/`org.db`, `local_hq_creation`): rewritten to file reads.
- `crates/org-host/src/provisioning/tests.rs:393-401, 583-598` and `web_process_tests/organizations.rs:199, 448, 961`: new paths.
- `org-sync`: `OrgStore` was implemented by `Office`. The implementation is deleted and the crate stays parked (S1).
- `org-mailbox` testkit: was used by `messaging.rs` tests. Those tests are deleted; the serve tests keep using it.

**org-office internal tests:**
- `office/tests.rs`: rewritten as lifecycle tests.
- `office/tests/instance.rs`: folded into lifecycle.
- `office/tests/work*`: deleted.
- `office/hq_creation/tests.rs`: rewritten as `creation.rs`.
- `store/**` tests: deleted with the store.
- `store/open/tests*`: kept.

## 9. Critical files

- /Users/vgmello/projects/agentic-org-claude/crates/org-office/src/office.rs
- /Users/vgmello/projects/agentic-org-claude/crates/org-office/src/office/hq_creation.rs
- /Users/vgmello/projects/agentic-org-claude/crates/org-office/src/store/open.rs
- /Users/vgmello/projects/agentic-org-claude/apps/agentic-org-office/src/lib.rs
- /Users/vgmello/projects/agentic-org-claude/crates/org-host/src/writer.rs
- /Users/vgmello/projects/agentic-org-claude/apps/agentic-org-office/src/transfer/writer.rs
- /Users/vgmello/projects/agentic-org-claude/crates/org-host/src/provisioning.rs

## Revision 1 — lead review (2026-11-18). These win over the text above.

Drafted by a read-only Opus planning agent; lead spot-checked P1 (`org-host/src/writer.rs:387` `ids::generate()`), P2 (`writer.rs:519-526` single-step `follows`), P4 (`org-proto/src/error.rs` has no `Unsupported`), P6/P7 (`org-store/src/rebuild.rs:38-46` refuses unowned paths), P10 (`docs/FEATURES.md` FC-072 is file-queue scope), P14 (`bootstrap_bytes.rs:22` OWNER vs `hq_creation.rs:14-20`). All hold.

- L1 · Q4 accepted for B1b: `org_store::Id` checks only a safe path component (`crates/org-store/src/change.rs:108-121`), so a lowercased Crockford ULID is a valid, bijective operation ID. The reviewer should check whether document 16's receipt contract requires UUIDv7 operation IDs; if so this is a recorded deviation until the ID slice.
- L2 · Q1, Q2, Q3, Q5, Q7, Q8, Q9-Q16: recommendations accepted as the starting position, subject to the architecture review.
- L3 · Q6 / R1 (existing A4-created organizations refused, no migration): pending owner confirmation before M2. Blocks 1-3 do not depend on it.
- L4 · Q1 amends data-contract document 01 (wrapped CA key in the protected journal until release). Data-contract change: confirm with the other lead/owner before block 7's doc edit; block 3 implements the design §11 / ADR-0007 reading meanwhile.
- L5 · Parallel split (blocks 4 ‖ 5) accepted only after block 4's public signatures and block 5's wire v2 types are frozen in this file by the lead (post architecture review).
- L6 · Routing (kit_policy, `.agents/agent-kit/tasks/b1b-assessment.json`): writers opus/high (fullstack-engineer), architecture review opus/high, per-block review gpt-6.1-sol/high, final review opus/high.
- L7 · OWNER 2026-11-18: L3 confirmed: old (A4-era) organizations are refused, no migration; the owner recreates them. L4 confirmed: amend document 01 so the wrapped CA key may sit in the protected journal until release.

## Revision 2 — lead rulings on the architecture review (2026-11-18). These win over everything above.

Architecture review: Claude architecture-reviewer, claude-opus-5-5 (effort per role), stage ARCHITECTURE, verdict CHANGES_REQUIRED, 3 High, 10 Medium, 8 Low. Lead verified H2 (`crates/org-store/src/journal.rs:568-602`: folders then `anchor.json`), H3 (`crates/org-proto/src/org.rs:818-836`), M3 (`crates/org-office/src/office_file.rs:21,165-170` fixed temp name + `create_new`).

### Block order (M9) and writers
New order: **1 → 2 (M1 merge) → 3 → 4a → 5 → 6 → 4b → 7 (M2 merge)**.
- 4a builds the new `Office` beside the old store (no deletions). 6 switches the worker; the production-path smoke runs at the end of 6. 4b is the mechanical deletion, gated green. The workspace compiles after every block; only real-process tests may be red between 5 and 6.
- Writers: A = 3 then 4a (owns `crates/org-office/**`); B = 5, may start right after M1 (owns `crates/org-host/**`, `crates/org-office-launch/**`). 6, 4b, 7 single writer. Q8/L3: the `org_proto::work::{WriterIdentity, JournalPosition}` deletion moves from 5 to 4b.

### Rulings
- **H1** accepted. `provision_hq` waits for materialization (`OfficeStore::sync()` or equivalent) after `CreateOrganizationOp` and before writing `office.json` or emitting `Created`. Test: create with materialization delayed by `Testing.faults`, then `hq_creation_public_record` is not answered before the files exist.
- **H2** accepted. `Opening::Create` iff `<journal_dir>/anchor.json` is absent. Crash test between the folder syncs and the anchor write; the retry completes.
- **H3** accepted. `team.create` with `owner_team_id: None` answers `invalid_request` ("a top-level team arrives with B2"); never reinterpreted. Test.
- **M1** accepted. Opening an HQ refuses when the escrow or key record is missing, the escrow's SHA-256 differs from `package_sha256`, or public key / organization ID / operation ID differ. One test each.
- **M2** accepted, marker stays at the design's `local/offices/<office-id>/clean-stop` (owned by the office-local family). Read before open; removed and its directory synced only after `Store::open` succeeds and before the first submit. Content: `{journal_incarnation, head_ordinal}`; a mismatch with the opened journal counts as unclean.
- **M3** accepted. One single-file publish primitive exposed from `org_store::files` (unique temp name, full flush, exclusive rename or replace, directory sync, bounded read refusing links). Used by the intent and `office.json`. Update `docs/design/linux-host.md:106,120,588,718` in block 7.
- **M4** accepted. Real-host test (block 7): create, `team.create`, stop, delete `runtime/<office-id>/`, restart; assert `org.snapshot` equal to before, the same `team.create` `command_id` returns the original answer, recovery-public read still returns the escrow. The in-process `dump()` equality runs on SQLite (memory always rebuilds).
- **M5** accepted. Block 6 through `FakeWriter`, block 7 through the real host: (a) a flipped byte at or below the confirmed head refuses and names the ordinal; (b) an older segment copy (journal behind the host checkpoint) is refused; (c) a closed custody channel fails closed.
- **M6** accepted. Owner approved the doc 01 amendment (L7). Doc 01 and `.agents/agent-kit/project-skills/hq-creation-provider/SKILL.md` are updated in **block 3**, before code relies on it. Recorded invariant (doc 01 + office-store §10): the outbox, Git publication and any export never ship `local/**` or other protected-tree file changes; S1 inherits a test. Security tests: no file under `durable/state/**` holds the wrapped key bytes; no file under `durable/` or `runtime/` holds the raw PKCS#8.
- **M7** ruling replaces L1/Q4: no lowercased ULIDs on disk. The wire ULID maps deterministically to a UUIDv7 operation ID (48-bit timestamp kept; version/variant bits set; the 80 random bits truncated to 74). The mapping lives in the office's wire adapter with its own tests (stable, valid UUIDv7, same ULID → same ID). The ID slice later removes the mapping when the wire carries UUIDv7. Note in document 06 / FC-142.
- **M8** accepted. Every office requires custody: remove the `selected_writer_required` launch flag and the `selected-writer.required` folder marker (or make them always true, writer's choice, documented). The host refuses to start an office with no valid schema-3 writer record, reports the reason, and does not restart-loop. `crates/org-host/src/supervisor/**` and `catalog.rs:270-276` join block 5's ownership.
- **M10** accepted. `local/offices/<office-id>/security/ca-key.v1.json` is one JSON object, keys in this order, lowercase hex, no whitespace, trailing newline: `{"schema_version":1,"organization_id":…,"office_id":…,"operation_id":…,"escrow_version":1,"public_key":<130 hex>,"package_sha256":<64 hex>,"wrapped_ca_key":<hex>}`. Written into document 01 in block 3.
- **L1** accepted: `TeamCreateWire`/`DirectorySnapshot` stay thin; wire mapping in `Office::submit_org`/`Office::org_snapshot`; the pre-B2 authorization rule is owned by the organization family and checked inside `decide`.
- **L2** accepted: the organization family owns `state/security/ca-escrow.v1.bin`; the office-local family owns only `local/offices/<office-id>/…` (CA key record, clean-stop marker).
- **L4** accepted: add `'unsupported'` to `apps/agentic-org-ui/src/lib/proto.ts` `API_ERROR_CODES` (library mirror only, no page change) in block 1.
- **L5** accepted: duplicate `command_id` with another payload expects `invalid_request` only.
- **L6** accepted: FC-072 stays Done; its row says why B1b does not affect it.
- **L7** accepted: record that per-person grants are the only owner gate for organization commands after B1b.
- **L8** accepted: assert confirms ≤ flushes and confirms < operations, not an exact count.
- Info (C-track): unowned files under `local/offices/<id>/agents/<employee-id>/home/` would fail rebuild; noted for C2/C3.

### Frozen interfaces for the parallel split
Block 4a's `Office` signatures in section 3 block 4 stand, with: `provision_hq` waits for materialization (H1); `submit_org(&self, member: &str, command: OrgCommand)`. Block 5's wire v2 types in section 3 block 5 stand, with the M8 launch changes. Changes to either need the lead.

### Recheck (same reviewer, 2026-11-18): three blocking items, resolved here

- **R2-B1 · compile window.** Corrected: `apps/agentic-org-office` does not compile on writer B's branch from the end of block 5 until block 6 (block 5 removes `HqCreationV1.public_root` read at `apps/agentic-org-office/src/lib.rs:209`, replaces the writer wire types used by `transfer/writer.rs`, and changes the launch line). 4a and 5 stay on their own branches; nothing is integrated between 5 and 6; block 6 starts on a branch that merges both and is the first workspace gate after M1. Writer B's checks stay per crate. 4a ‖ 5 stays.
- **R2-B2 · M8 narrowed.** The host's reservation marker `<root>/office-state/offices/<id>/selected-writer.required` (`crates/org-host/src/writer.rs:253-272`) **stays**: `catalog::unfinished_folders` (`crates/org-host/src/catalog.rs:270-276`) relies on it so start-up discard never deletes a reserved create. M8 removes only the launch line's `selected_writer_required` flag (every office requires custody). New block 5 test: reserve custody, kill the worker before the intent, restart the host; the folder is not discarded, the key is kept, and the create retry completes.
- **R2-B3 · operation-ID forms (refines M7).**
  - One function in org-office, `organization::operation_id::from_wire_ulid(&str) -> Result<org_store::Id, _>`, used by both `Office::submit_org` (`team.create` `command_id`) and `provision_hq` (`HqCreationV1.operation_id`).
  - Bits: UUIDv7 = `unix_ts_ms` (48, the ULID's timestamp) ‖ `ver` 0b0111 ‖ `rand_a` (12) ‖ `var` 0b10 ‖ `rand_b` (62). With the ULID's 80 random bits r79..r0 (big-endian): `rand_a` = r79..r68, `rand_b` = r67..r6; r5..r0 are dropped. Lowercase hyphenated text. Golden-vector tests pin at least three ULID→UUIDv7 pairs (incl. all-zero and all-ones random parts), plus: the result is a valid UUIDv7 and the same ULID always maps to the same ID.
  - On disk everything holds the mapped UUIDv7: receipts, the journal entry's operation, `ca-key.v1.json.operation_id`. The intent and `office.json` keep the launch binding's ULID as received (they record the request).
  - `hq_creation::LocalRecord::new` (`crates/org-office/src/hq_creation.rs:61-66`) validates its `operation_id` as UUIDv7 (the on-disk form). M1's check at open compares `from_wire_ulid(intent.operation)` with the record's value.

### Status
READY (lead, 2026-11-18) after the recheck's three blocking items were resolved above. Their resolution follows the reviewer's stated fixes (B1 option a, B2 keep-marker + test, B3 single mapping with pinned bits); no further pass.

### Block 5 receipt notes (lead, 2026-11-19)
- Done at 01ba06a6 (worktree-b1b-block5). Wire v2 types as reported in the block 5 receipt; `wire::confirms` and `check()` helpers are what block 6's adapter and `FakeWriter` reuse.
- Block 6 must take the admission first, use `Admission::check`, and compare the admission's organization and roots with the intent.
- Block 4b: delete the legacy plain-ULID launch-line profile (id_profile None) in `org-office-launch`; the host can no longer start such offices.
- Block 7: `crates/org-host/src/web_process_tests/organizations.rs:199,448,961` move to the `<org-id>/durable/state` layout.

### Block 3 receipt notes (lead, 2026-11-19)
- Done at 6eb839ec, 30d84c32, ca91e481 (worktree-b1b-block3). 4a uses `organization::wire::team_create(member, params, |req| store.run(CreateTeamOp{..}))`, `DirectorySnapshot { owner }`, `office_local::read_ca` + `CaRecords::check` (M1), `OfficeStore::{stopped_cleanly, fatal, close}`.
- Moved to 4a/6: `hq_creation::LocalRecord::new` validates a UUIDv7 `operation_id` once creation switches (today's live path still passes the wire ULID).
- The CA records' detail is in document 16 ("The HQ's CA records"), summarized in document 01: document 01 is a skill instruction source limited to 24,000 bytes.
- Escrow framing check in `organization::records::ca_escrow_public_key` duplicates org-pki's framing on purpose (no new dependency).
- Block 5 review (Opus medium): fixes sent back to writer B (provisioning roots test, transient custody read not fatal, create gate also matches owner). Recorded: per-organization roots must be empty at first reservation, so a second office of one organization on one instance is refused; the emptiness rule must become per office (or the roots office-scoped) before milestone 3. Block 6: the worker's `Admission::check` use should also check the owner account's form.

### Block 4a receipt notes (lead, 2026-11-19)
- Done at 90b9a07b (worktree-b1b-block3): `org_office::journal_office::{Office, DataRoots, Custody, TestOffice}` beside the old Office; `org_store::files::{publish_one, read_one}` (M3); `LocalRecord::new` requires UUIDv7 (old callers use `LocalRecord::legacy`, deleted in 4b).
- Known: 5 `apps/agentic-org-office` `hq_creation::tests` fail until block 6 (worker still passes the wire ULID).
- Block 6 switches the worker: import `journal_office::{Office, DataRoots, Custody}`; create via `provision_hq(folder, &new, key, &binding, roots, custody, material)` with the material closure building `LocalRecord::new(org, from_wire_ulid(&binding.operation_id)?.to_string(), ..)` and comparing prior.operation_id with the mapped ID; start via `Office::open(folder, key, instance, roots, custody)`; `submit_org(binding.member(), cmd)`; `who(params)`; `is_hq()`; stop predicate adds `office.fatal().is_some()`; shutdown `Arc::try_unwrap` + `close()`; discard via `journal_office::Office::discard` (UUIDv7 IDs only). Provision/open refuse missing roots (the host creates them, block 5).
- Block 4b: delete the old Office and store, rename `journal_office::Office` to `org_office::Office`, move the folder helpers out of the old `office.rs`, delete `LocalRecord::legacy` and `Binding.public_root`, drop the escrow exclusion in the old bootstrap comparison.

### Reuse pointers from agentic-org-da (2026-11-19, checked against origin/v1 9536d845)
- Block 6/7 (writer B, org-host): (1) `home.rs` `check_socket_paths` must also check `<run>/r/<16 hex>` (Broker::bind, `supervisor/channels.rs:118`); re-apply by hand from origin/river/host-launch-v3-life `ba4439a2` with its test (ok = limit-24, long = limit-23). (3) handoff/ready reads in `channels.rs` set SO_RCVTIMEO once; a byte-trickling worker can pass the 5 s deadline: reset per byte (reference origin/river/host-launch-v3 `rendezvous.rs:196`, test `a_trickled_receipt_still_ends_at_the_deadline`). Low severity.
- Separate small change after B1b: org-docs BUG-194/195/196 from origin/claude/pensive-mayer-2fyf8x `8e8c907d` (unreviewed; BUG-195 changes field-path format: recheck org-command/org-process vectors; BUG-196 matches parser text: make it robust). Needs its own fresh review.
- B6: D7 snooze race (origin/park/remy/org-mailbox-2b `66c94fc2`, `6aa09ae9`) if in-memory snooze timers stay; origin/codex/triage-sql ideas only; activity cursor rules BUG-197/198 (pensive-mayer `eea88721`) restated on rebuilt activity reads (also B4).
- Later: origin/river/activity-recency (when a consumer exists, FC-036); origin/rory/p6-write-checks `org_process::check_write` for L12 after C5.
- Block 4a accepted at a62de4a4. 4b: drop the duplicate `OLDER_BUILD_ENTRIES` vs old `OFFICE_ENTRIES` when the old office goes. Block 6: `discard` returns `OfficeExists` for a folder holding the custody marker (incl. a create refused before its intent).
