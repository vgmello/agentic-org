# F2b: focused interface and extraction handoff

Revision 2, 2026-11-16. **IMPLEMENTATION_PLAN review pending.** Root owns the
overall architecture, stable F2b contract, interfaces and integration. This is a
bounded closure clarification under plan17/ADR0006r5, not a new task service or
replacement journal mechanism. No source writes before this focused review.

## Source evidence and retained premises

- JI-1 VERIFIED source: at immutable b77cbd6876d72f76cc3b7f1d5fa3c3d96f424c09,
  store/acceptance.rs:25 imports only JournalPosition and WriterIdentity from the
  otherwise future selected_tasks protocol. Those two records and the two
  JournalPosition methods are exactly selected_tasks.rs:520–569. Current accepted
  base 0fdb9723 already exposes org_proto::work and office depends on org-proto.
  No selected task protocol/service exists on this base. Recheck if more imports
  appear or fields/serialization/validation change.
- JI-2 VERIFIED source: WriterAuthority, AcceptanceConfig and AcceptanceIdentity
  already live in private store/acceptance.rs:23–49. The future selected module
  merely re-exports them. Office's retained install_selected_acceptance checks
  organization/office identity and HQ role before installation. Journal::checkpoint
  refuses absent authority in non-test builds; cfg(test) alone permits None.
  The production composition must still authenticate the host lease. A public
  Rust DTO/trait is neither a new HTTP route nor proof of actor authority.
- JI-3 VERIFIED source: the six new acceptance files otherwise use F2a/existing
  crate dependencies. Their owner-directory test at tests.rs:671–729 crosses a
  later consumer boundary; its run_job exhaustion/original-receipt assertions
  remain mandatory here. The selected_legacy integration and assignment-service
  SIGKILL harness also depend on later adoption/directory/assignment consumers.
- JI-4 CORRECTED source: schema7's complete table inventory is required by row
  capture/schema hashing. Current instance.rs aliases OFFICE_SCHEMA.create as v6
  and treats7 as future. The retained b77 repair is NOT a genuine v6 fixture: it
  combines v5 SQL with owned_processes and admit=None, retaining v5's mandatory
  status/expiry fields. Accepted 0fdb9723 office.rs:71–80 instead uses work::admit_v6
  and status.rs:14–21 permits current-task-only rows with NULL status/expiry.
  Freeze or reconstruct the actual accepted v6 schema/admission in instance.rs,
  independently of the schema7 create function. Verify its pre-migration version,
  schema constraints and representative valid v6 rows before exercising migration.
  Assert those rows survive schema7 and another reopen. Preserve true v1–v5
  fixtures, existing metadata migration controls and current+1 refusal; do not
  relabel draft SQL or the retained b77 approximation as a genuine v6 fixture.
- JI-5 VERIFIED source: durable.rs requires only the workspace-pinned libc for
  O_NOFOLLOW and macOS F_FULLFSYNC. No raw_value/org-docs/zeroize/new package is
  needed. All six allocated opener/build/native guard-file differences belong
  to later consumers; leave those files unchanged in F2b.
- JI-6 UNVERIFIED until implementation: actual compiler/guard/platform/test closure.
  The original author's read-only handoff is source-derived, not a passed build.
  Missing essential evidence or a conflicting premise returns to root first.

Source handoff: same retained author01a1dfb4-1d26-7100-9ea4-72b53756ef23,
536.059s READONLY, actual eight-source ACK/preflight0, all1404 inputs unchanged.
Private evidence: .agent-kit/logs/v1-durable-journal/original-fullstack-author/
closure-1/{result,execution,controller-capture-receipt}.json. Root inspected the
exact two records, config/authority signatures and non-test authority refusal.
The child Repowise lock EPERM is retained; actual Rust LSP resolved writer
definitions/references. No source, test, Cargo or guard mutation occurred.

## Proposed exact interface placement

1. Add only the exact retained WriterIdentity, JournalPosition and
   JournalPosition::{valid,follows} definitions to existing org-proto/src/work.rs.
   Preserve all fields, serde representation/attributes and validation behavior;
   no selected command/decoder module, route or alternative duplicate type.
   When selected_tasks is later extracted, replace its duplicate definitions with
   re-exports of these canonical work types, preserving its public import paths.
   Future host/office consumers must use the same nominal types; record that
   integration requirement with the later unit before dispatch.
2. Retarget acceptance.rs's import to org_proto::work. Directly re-export
   AcceptanceConfig, AcceptanceIdentity, JournalPosition and WriterAuthority from
   org-office/lib.rs's private store::acceptance. Office's retained install method
   uses the crate export instead of the unavailable selected module. Preserve
   existing scope/HQ checks, failure fencing, non-test authority requirement and
   the sole writer's ordered installation barrier. No automatic installation or
   protected host/session/bootstrap implementation is added or claimed.
3. Keep JournalPosition's anchor position0 distinct from positive transaction
   ordinals. Its valid/follows helpers are structural checks; they do not establish
   authority, complete origin mapping or durable publication by themselves.

These are necessary shared-record/export placements for the already selected
journal, not a change of delivery objective, wire fields or security mechanism.
The stable registered contract remains unchanged; this separately reviewed handoff
concretizes root's interface ownership and grants the two extra test/DTO paths.
If review finds a material architectural replacement is required, pause and return
to architecture-decisions before dependent implementation; do not claim readiness.

## Source ownership and integration

Root assigns the same author sole source writes in the delivery checkout on
codex/v1-durable-journal from0fdb9723, after READY. Root pauses tracked writes.
Exact paths:
- org-office/src/store/acceptance.rs and acceptance/{crash,durable,rows,schema,tests}.rs;
- new org-office/src/store/acceptance/tests/crash_reopen.rs (storage-only harness);
- org-office/src/store/origin.rs and origin/tests.rs;
- org-office/src/store/work/{assembly,call,tests}.rs and tests/{assembly,call,metadata}.rs;
- org-office/src/{store,lib,office}.rs and office/tests/instance.rs;
- org-proto/src/work.rs; org-office/Cargo.toml and Cargo.lock (libc-list hunk only).
All crate paths are under crates/. Existing files get only scoped hunks. Six
guard files, work.rs module root, office/tests.rs and selected_legacy stay unchanged.
Root owns task/feature/catalog metadata after source handoff; no concurrent writer.
Retain immutable inventory/hash allocations even for deferred hunks. No extra
paths/dependencies or invented adapter without a concrete compiler/source receipt
and root's explicit bounded handoff. No offline messaging or user-state mutations.

## Required checks and falsification

- Preserve exact legacy event allocation/put order, receipts and unadopted behavior;
  maintain adopted-state SQL/capture fences with unadopted controls. Install failure
  must fence later writes; absence of production authority must not enable legacy
  fallback for an installed/failed selected journal.
- Implement a real storage-only SIGKILL/reopen counterpart at all ten existing
  crash hooks. Exercise actual journal publication/projection/recovery, zero/one/
  multiple origin mappings and original receipts. Later assignment-service harness
  stays preserved for U5; storage tests do not replace service/browser coverage.
- Preserve actual run_job ordinal exhaustion and original-receipt readability;
  move only owner-directory assertions to their F3/F5 consumer. Preserve raw source
  and coverage location/hash; never drop that validation.
- Verify genuine v1–v6→7 migration/reopen and future-version refusal. In the
  assigned instance.rs path, independently freeze/reconstruct accepted 0fdb9723
  v6 (including work-key admission and the nullable status/current-task shape).
  Before upgrade assert user_version6, relevant sqlite_master/table constraints,
  and valid current-task-only, status-only and combined rows; verify representative
  row contents plus instance/incarnation metadata after upgrade and again after
  reopen. Do not alias the evolving OFFICE_SCHEMA.create or use v5 SQL plus one
  table as v6. Preserve the v1–v5 metadata migration loop and add equivalent v6
  coverage here or the assigned metadata.rs. Actual malformed/future-schema
  refusal controls remain; no fixture relabeling, schema8/HQ creation, new task
  command or unaccepted service import.
- Actual focused compiler/journal/origin/legacy tests, unchanged SQLCipher guard,
  platform flush behavior and focused Clippy/fmt must pass after extraction. Then
  full workspace fmt/Clippy/tests, explicit kit checks/normal hook, original-author
  exact-tree validation, fresh initial and separate final current ACCEPTED gate.
- Keep root's single architecture/integration owner, at most two disjoint worktree
  writers (one here), preserved contexts/worktrees/checkpoints and signing LAST.
  Failed premises or unverifiable guards/coverage stop affected work, not relax it.

Review only these new interface/coverage boundaries for consistency with active
ADR0006r5, ADR0002 and owning05/06/16; do not repeat the unchanged oversized bundle
or plan17 review. READY permits bounded implementation, never feature acceptance.

## Focused review history

Revision1 review CHANGES_REQUIRED, fresh read-only session
01a1e546-f737-74e1-926f-cfd395e590d6, requested Sol Xhigh, actual exit0
389.951s, artifacts interface-review-1/. No implementation acceptance claimed.
The concrete P2 v6-fixture mismatch is accepted and corrected in JI-4 and the
required checks above. Shared records/exports and crash-test separation required
no architectural replacement. One focused same-session recheck is pending on
revision2; source writes remain paused until actual READY.
