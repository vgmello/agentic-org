# F2b — durable acceptance journal and legacy capture

Separate INTERNAL acceptance/merge unit of reviewed plan17, following accepted
F2a and the owner's upstream documentation reconciliation. Base integration:
0fdb9723375dcf56a8887aaa22eee159d6c4c3a4. Root owns architecture, contract,
interfaces, dependency order, metadata and integration. This journal is required
by protected bootstrap; it does not deliver onboarding, task CRUD or assignment.
First usable U1 remains actual onboarding/sign-in/create/reopen/restart.

Preserve original fullstack author01a1dfb4-1d26-7100-9ea4-72b53756ef23 and
immutable source b77cbd6876d72f76cc3b7f1d5fa3c3d96f424c09, every checkpoint,
worktree/branch, user original-checkout dirty files/stash, installed CLI/spool and
live organization data. No restart or whole-reservoir merge. The immutable 324-row
inventory's twenty-one F2b paths are source/provenance references, not permission
to copy all later-dependent changes from their shared files.

Reuse scoped retained implementation in org-office:
- store/acceptance.rs and acceptance/{crash,durable,rows,schema,tests}.rs;
- store/origin.rs and origin/tests.rs;
- store/work/{assembly,call,tests}.rs and tests/{assembly,call,metadata}.rs;
- store/tests/opener_{build,build_pins,build_tests,inventory,scope,sources}.rs;
- office/tests/selected_legacy.rs.
For existing files select only this journal/capture/compatibility unit's hunks;
preserve unrelated baseline behavior/tests and defer later-consumer hunks with
their source/hash and explicit coverage dependency. New pure journal files may
be reused exactly when actual compilation closure permits it. No test loss.

Root assigns the retained original author sole source-write ownership after a
bounded closure handoff: those twenty-one paths and the minimum journal/schema,
legacy capture and module/test registration hunks in org-office src/{store,lib,
office}.rs, src/store/work.rs and src/office/tests.rs. Necessary org-office
Cargo.toml/Cargo.lock/workspace Cargo feature hunks require actual dependency
evidence and explicit root agreement before mutation. Root pauses tracked writes
during author implementation; root alone then updates this contract's assessment,
reuse receipt, current plan/state, scoped FEATURES/FEATURE-AREAS evidence and
catalog evidence only if actual manifest/source changes require it. One writer
is preferable for coupled journal/storage/capture hunks; no ready independent UI
DTO or useful second source assignment. Never exceed two disjoint worktree writers.

Acceptance:
1. Compile this journal/legacy unit on the actual accepted base using F2a and
existing storage/runtime contracts, without selected task service/protocol/UI,
host/web/bootstrap/session composition or other unaccepted source. No source
cycles. Follow ADR0006r5 and owning organization-data history/authority contracts;
draft SQL does not establish deployed schema. Review any material newly necessary
contract/storage/security mechanism before dependent implementation.
2. Preserve durable operation identity/original-result replay, frozen source/event/
receipt inputs, protected state, publication/ordinal ordering, startup recovery and
crash/refusal semantics. Never fall back to unjournaled writes after failed startup.
Preserve legacy captured-write/replay compatibility and selected-state fencing;
do not enable dual writable truths, assert current snapshots prove history, or
confuse integrity with authority/classification. No production capability advertised.
3. Preserve strict SQLCipher opener/build/source guard enforcement. Add only exact
legitimate new call-site/schema inventories and required pins, supported by actual
source. Do not import future opener entries, weaken guards, change dependency pins,
waive failures or relax assertions/statuses to obtain green checks.
4. Run meaningful retained journal/crash/replay/receipt/sequence and legacy-origin/
compatibility tests against temporary roots, plus focused compiler/Clippy/fmt and
the unchanged enforcement contract. Record all failures, extraction deltas and
deferred coverage. Never operate on user organizations or installed live state.
5. Freeze complete unit; pass workspace fmt, all-target Clippy -D warnings,
workspace tests, explicit configured kit checks and normal structural commit hook.
Refresh actual capability/skill packets/preflights and fixed checkout index; keep
checks/source/provenance hashes and original-author intent for the exact tree.
6. Fresh independent complete-unit review, retained original-author validation,
substantive root integration intent and separate fresh final exact-commit review
must yield current installed ACCEPTED. Collect every attempt/honest unknown usage,
close journal before branch switch, then --no-ff merge/push ONLY v1-integration.

Full production security, durability, tests and independent gates remain intact.
Genuine release-signing setup/authentic production release verification stays LAST;
no unsigned-production bypass, generated production trust or signing questions now.
F3 protected HQ/escrow then F4 sessions/composition and usable U1 follow; the full
selected v1 and later teams/people/tasks/assignment/Codex runtime remain incomplete.
