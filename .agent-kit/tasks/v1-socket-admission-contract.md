# F1b-socket: narrow OS admission substrate

Separate internal acceptance/merge unit in reviewed plan17, from accepted
origin/v1-integration cd77745070633341637523d00147eca5ac53b17c. Root owns
architecture/integration, exact extraction and root glue; retain original fullstack
author01a1dfb4-1d26-7100-9ea4-72b53756ef23 and immutable b77cbd68 source.
No usable product feature, gateway/service startup or production signing acceptance.

Reuse exactly crates/org-socket/src/confinement.rs,
src/confinement/macos.rs, src/image.rs and
examples/gateway_confinement.rs. Root selects only pub mod confinement and
pub mod image registrations in crates/org-socket/src/lib.rs; baseline rights and
peer implementation/export remain unchanged. No new crate/feature/dependency or
Cargo/lock delta. Preserve source hashes plus any explicit original-author repair
overlay; do not substitute current hashes for immutable provenance.

Metadata scope: this contract, assessment/reuse receipt, truthful current unit and
F0/F1a/blocked-host status in v1-incremental-delivery.md/v1-state.md, narrowly
relevant docs/FEATURES.md and FEATURE-AREAS.md evidence/status wording. No complete
feature Done claim. Catalog only if actual evidence requires it, preserving rules.

Actual F1b host/web adapters require F4 consumer/private signing glue; rejected
extraction ace8b29d and its failed compiler evidence stay preserved, unmerged and
unaccepted. No source reset/restart or omission of those obligations. They land
with their first consumer under a separately scoped closure. This substrate may
land independently without a new architectural mechanism or interface redesign.

Acceptance:

1. Exact four retained source files compile on this integration base without any
   other unaccepted source. libc-only org-socket dependency stays unchanged; no
   unsafe gateway/web code or domain/host/storage dependency enters the leaf.
   ADR0008 and ADR0009 selected OS image/confinement contracts remain authoritative;
   material interface/security/premise changes return to root for focused review
   BEFORE dependent implementation. No broad assignment-bundle review.
2. Main/never-threaded/one-thread and exact descriptor preconditions, fixed
   parameterized macOS policy, deny ambient files/process/exec/connect/bind,
   approved rendezvous and inherited listener/FD traffic, inherited thread policy
   and parent kernel observations retain genuine enforcement. The actual Rust
   subprocess example must pass on this macOS host with disposable roots. No
   mocks, success constants, widened policy, waiver or simulated platform success.
3. Kernel loaded-image APIs bind actual process incarnation/audit token, fail
   closed on errors/unsupported platforms and remain distinct from publisher/
   account/org authorization. Retain existing tests and meaningful negative cases.
   Genuine signing, supported-platform production and first-consumer image/secret
   handoff checks remain with their later owning acceptance units.
4. Scope-check original file hashes and exact2-line lib registration first. Run
   targeted socket tests/Clippy, actual example and unchanged source guard during
   iteration. Freeze complete unit then require cargo fmt --check, full locked
   workspace Clippy --all-targets -Dwarnings/workspace tests, explicit configured
   kit checks and normal structural commit hook without policy/check changes.
   Tests never write to user organizations, live spool, services or credentials.
5. Fresh independent complete-unit review, retained original-author intent and
   separate fresh final exact-commit review; installed ACCEPTED/current status,
   collect all failures/reviews in unit journal, then root noff merge/push only
   v1-integration. Preserve all authors/checkpoints/worktrees/stash/concurrent docs;
   never release-v1/main. Genuine signing/authentic signed production verification
   stays FINAL; no unsigned production bypass.

One source writer for this five-file coupled extraction. Read-only original-author
validation may overlap frozen checks; no useful independent second source slice or
ready UI contract exists here. Root pauses tracked writes if author repair needed;
maximum two writers with separate worktrees remains unchanged for future parallelism.
