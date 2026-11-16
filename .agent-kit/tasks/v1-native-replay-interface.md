# F2b native original-result lookup — interface addendum 1

Root owns architecture, the stable F2b acceptance contract, integration and this
bounded grant. ADR-0006 revision 5, ADR-0002 and owning data contracts 05/06/16
remain selected. This repairs a missing typed producer; it does not replace the
journal or deliver a usable feature. The original revision-2 extraction interface
and immutable source allocation remain intact.

Evidence: native WorkService success/refusal retries currently publish a new
ordinal and raise the clock. The opaque `Arc<TxnFn>` cannot safely expose a saved
result at exhausted ordinal without running arbitrary callback/decision code.
Both ordinary regression tests remain enabled and RED. Generic pre-decision
exhaustion guarantees must remain unchanged.

Focused independent architecture review is READY: session
`01a1e58f-384f-7261-809c-73fa88e7dba7`, requested Sol Xhigh, 210.591s completed 0.
Its seven-source actual ACK/preflight 0 and pinned source inspection are retained
in `.agent-kit/logs/v1-durable-journal/native-replay-interface-review-1/`.
Candidate revision 1 SHA256:
`3f1e499097b50934ed149a791eea52099858afffbdb3649b88a8d06b79869c09`.
Author proposal SHA256:
`db8bffd81573115924aac4ec7ee9a0bff0298ad4f0392ea207d673509c262aed`.
The readiness result and every source-linked obligation are adopted below;
notably Arc forwarding lives in `testkit.rs`, not `ports.rs`. This is architecture
readiness only; tests and fresh initial/author/final code acceptance remain open.

## Boundary and exact source ownership

The retained original fullstack author
`01a1dfb4-1d26-7100-9ea4-72b53756ef23` receives sole source ownership of:

| Exact path | Authorized change |
| --- | --- |
| `crates/org-work/src/ports.rs` | Add default `transact_command(&self, caller: &Caller, call: &WorkCall, f: Arc<TxnFn>) -> Result<TxnOutput, StoreError>` forwarding to existing `transact`. |
| `crates/org-work/src/testkit.rs` | Forward the new method in the existing `Arc<T>` WorkStore adapter; no unrelated testkit rewrite. |
| `crates/org-work/src/service.rs` | Bind identical immutable caller/command clones to both typed dispatch and its existing transaction closure. |
| `crates/org-work/src/engine.rs` | Extract and reuse the existing app/member authorization and hash-bound original-result lookup prefix as a shared read-only helper. |
| `crates/org-work/src/lib.rs` | Export that helper for the existing native consumer; no other public contract change. |
| `crates/org-office/src/store.rs` | Add one private typed-native replay-probe FIFO barrier/variant carrying its original work job. Preserve existing Batch/Work behavior. |
| `crates/org-office/src/store/work/call.rs` | Share typed value/error/reply slots between the private probe and fallback; complete exactly once after cleanup. |
| `crates/org-office/src/store/work/assembly.rs` | Implement typed dispatch with fresh org snapshot and original-result lookup. Both probe Found and fallback Replayed return persisted high-water; no clock UPSERT. |
| `crates/org-office/src/store/acceptance.rs` | Run only the explicit typed read-only probe after live authority/cursor/origin/artifact checks. Preserve F1 reflush repair and generic MAX guards. |
| `crates/org-office/src/store/work/tests/assembly.rs` | Preserve both RED regressions and add meaningful actual-native boundary coverage below. |

`work/metadata.rs`, Cargo/lock, source guards, wire DTOs, credentials, production
host/session/service/UI composition and signing remain outside this grant. Any
additional source or material interface change returns to root first. Five new
org-work paths extend the unit source closure from 24 to 29; they do not rewrite
the original 21-path provenance allocation or authorize whole-reservoir imports.

## Required behavior

1. WorkService is the matching typed caller/command/closure producer; the method
   signature alone does not prove the binding. Document and exercise this path.
   Default-store dispatch, Arc forwarding, generic transactions and cascades retain
   their established contracts and dependency direction; no new crate edge.
2. Shared lookup preserves the exact existing app restriction, fresh member
   authorization, hash and error precedence. Found includes original Replayed and
   immediate authorization/hash refusals. Only Miss permits ordinary decision.
   Never call arbitrary TxnFn or domain decision code as a replay probe.
3. Use the same sole writer and FIFO barrier. Verify readiness and live protected
   authority, SQL cursor, origin and artifacts before lookup. The probe is
   effect-free/read-only; complete transaction cleanup before replying. A probe
   error or panic is never Miss. Uncertain integrity or failed rollback fences
   subsequent work. Typed policy refusals preserve their actual error semantics.
4. A found result consumes no ordinal/origin sequence, clock write or publication.
   An unrelated write/reopen cannot alter its original answer. At MAX, a miss
   enters the unchanged generic exhaustion guard before job.attempt. No unjournaled
   fallback, second writer/allocator, accepted-decision rerun or permission bypass.
   A concurrent duplicate miss that becomes fallback Replayed also performs no
   clock UPSERT/publication. Resolve value/error slots once and expose success
   only after successful cleanup.

## Acceptance and handoff

- Real native saved success/refusal: unchanged and advanced clocks, exact answer,
  unchanged ordinal/origin/persisted clock and retained artifact bytes/count/mtime.
- Native success/refusal replay at coherently injected MAX; new write refuses
  before decision, then original replay remains available. Preserve the existing
  raw selected-write and generic run_job pre-decision MAX assertions unchanged.
- Fresh protected authority and caller revocation, app/member rules and hash
  mismatch, read-only probe, error/panic/rollback fencing; no saved value on failed
  cleanup. Storage fixtures establish no real production host admission.
- Arc native override, default-store fallback, generic/cascade compatibility,
  duplicate miss/replay race, unrelated accepted write and reopen. Reuse meaningful
  domain/oracle checks rather than adding tests that merely mirror the helper.
- Retain the completed F1 recovery repair, all 30 acceptance tests/32 SIGKILL
  vectors/12 hooks, byte/device/inode/mtime evidence and six unchanged guards.
- Scoped current domain/native/journal/origin/writer checks, Clippy/fmt/guard;
  concise source/hash/check handoff. Root then freezes and repeats full workspace,
  kit/hook gates, original-author final-tree validation and new fresh independent
  initial plus separate final review. Collect the blocked absolute-path report;
  do not rewrite or waive it. Installed ACCEPTED is required before integration.

Root pauses tracked writes during the author assignment. No second independent
writer is useful across this coupled queue/reply/authorization boundary. Preserve
every checkpoint, all author contexts, worktrees, dirty original checkout, stash,
installed live CLI/spool and organization data. Tests use disposable roots.
Genuine release signing remains last; no production security requirement changes.
