# v1: separate acceptance and merge units

Revision19, 2026-11-16. Owner direction replaces “finish the assignment bundle
first.” Root owns architecture, interfaces, acceptance and integration. This
changes delivery boundaries, not selected semantics or security requirements.
The original assignment contract still governs assignment itself.

## Preserve and extract

Preserve branch `codex/v1-assignment-integration`, HEAD `b77cbd68`, all checkpoints,
author sessions, worktrees and stash. Let the active CH-LIFE-1r2 author reach its
bounded safe handoff. Import only its agreed source; do not overwrite current
guidance with its older checkout. No current repair or implementation restarts.

The large branch becomes a preserved source reservoir, not an all-or-nothing
merge candidate. Never merge its whole ancestry to deliver an increment. Use one
active delivery branch/worktree from current `origin/v1-integration`; copy only
the unit's bounded patch with original-source hashes and author validation.
Shared files require hunk selection, not whole-file copying. Record included and
excluded hunks, prerequisites and actual compilation closure. A green reservoir
test is not a passing extracted build. Accept/review/merge the unit before starting
the next active delivery branch; retain the reservoir and blocked checkpoints.

Every row below is a distinct acceptance and merge unit. Internal foundations have
their own tested contracts but **do not count as usable features**. Their tests and
reviews cannot depend on finishing unrelated future functionality. The exact
existing-file inventory, primary unit, source hash and shared-file reservation are
in [v1-delivery-source-map.json](v1-delivery-source-map.json). Allocation is an
extraction inventory, not permission to copy a whole shared file or a readiness
claim. Root owns all shared composition, contract, Cargo and module-registration
hunks; the preserved fullstack author implements bounded extraction/repairs.
The original browser author retains its four-file reservation.

## Integration order

| Unit | What the user can actually do | Dependencies, reuse, remaining work and acceptance |
| --- | --- | --- |
| F0 — guidance baseline | Internal only; no product feature. | Extract current solo workflow/kit and incoming v1 documentation at 6f02ae95 without the code bundle. Catalog/policy/preflight/instruction review. |
| F1a — pure release verifier | Internal only; no signed release claimed. | Reuse org-release/ADR0009 and its exact limits/signature tests. Generated fixture keys remain test-only. Genuine signing setup stays final. |
| F1b-socket — OS admission substrate | Internal only. | Reuse four org-socket confinement/image/example files and two module registrations under ADRs0008/0009; no dependency changes. Actual subprocess ambient-denial, approved FD/listener/thread and kernel-image checks. Unsupported platforms fail-closed. Separate acceptance/merge from coupled consumers. |
| F1b-release — host/web adapters with first consumer | Internal only. | F1a/F1b-socket plus the minimum reviewed GatewayRelease/static-signing contract/helper closure before F4 composition. Preserve unaccepted ace8b29d and its real failed compile; no pulling whole F4 to compile a foundation. Exact file/hunk ownership and independent acceptance required before consumer enablement; release recovery/image/asset tests remain. |
| F2a — source/codec | Internal only; no task writes. | Eight retained org-docs/org-work source/test/vector files plus root module/export hunks under ADR0006r5. Preserve original bytes, duplicate/numeric/resource vectors and legacy semantics; independent Python vectors. No assignment service/UI dependency. Transport-coupled selected_fingerprint test remains with its U5 production consumers; original allocation/hash retained. |
| F2b — durable acceptance journal | Internal only. | F2a; existing office acceptance/origin capture and executable migrations. Crash/replay, protected receipt, publication/sequence and legacy compatibility tests. Bootstrap uses this journal, so this cannot be postponed until daily tasks. |
| F3a — CA escrow adapter | Internal cryptographic library only. | Accepted F2b base; five retained org-pki files plus exact workspace/lock hunks. ADR0007r1 unchanged API/format/primitives. Compile and exercise real production generate/seal/recover/signature and strict refusal paths before expansion; full checks/author/fresh reviews and separate integration. No host/directory/task/F4 dependency. See v1-pki-escrow-contract.md. |
| F3b — worker HQ creation/admission provider | Internal only: actual encrypted create/fault/reopen and matching control port; no host provisioning or normal setup claim. | Accepted F3a/F2b. Active v1-creation-admission-interface.md r1, all four prerequisite conditions. Exact33path grant in source receipt and v1-hq-creation-provider-contract.md. Real early worker compilation/executable phase tests, owner/MAX-rebind/generation assertions, private DER exclusion, Created before admission but Ready only after writer/owner/bootstrap. No task imports or fake host authority. |
| F3c — protected host creation and recovery delivery | Internal composed host producer/delivery prerequisite for U1, separate acceptance/merge. | F3b plus real org-host reservation/account linkage/current-owner/custody/root/epoch, authenticated supervisor create/control/restart and protected recovery delivery/ACK. Human-session-dependent composition follows its F4 prerequisite; preserve original ADR0007/0011, fresh-owner-only authority and existing-HQ proof refusal. REC-1 and physical authentication remain genuine requirements. |
| F4 — sessions/channel lifecycle | Internal until U1. | F1b-socket/F3 and reviewed F1b-release contract/adapter closure as composed; reuse active CH-LIFE-1r2 repair, host auth/listener, gateway handoff and browser sessions. Capacity/ACK/closure, logout/revocation/expiry, cookie/origin/CSRF, foreign-account and endpoint-failure tests. Extract session routing from selected-task dispatch; no task capability advertised just to make sign-in compile. |
| U1 — onboarding, first organization, sign-in/reopen | Establish the first administrator, supply organization/repository/initial-team inputs, enter the real workspace, sign out/in and reopen the same organization after restart. | F1–F4 plus minimum selected directory/bootstrap and existing setup continuation/static auth UI. Finish authorized organization reads and real create/open/session UI adapters. Offline zero-network proof, stable identities/home-team/HR readiness, lost-create-reply exact retry, restart, denial/accessibility/browser evidence and documented manual development exercise. No task adoption/assignment completion prerequisite. Prerequisite (GW-01 r2): task-free office-channel open on the existing CH-LIFE gateway budget for canonical `org.snapshot` workspace entry; F4 deliberately ships no office channel. |
| U2 — teams | Create root/child teams, inspect them and reopen unchanged IDs/TEAM numbers. | U1 and canonical directory/journal. Reuse chart/forms and traced legacy writer facts. Root must first settle two-store authority/cutover/source-position semantics and obtain focused architecture review. No dual writable truths. Test protected owner/HQ auth, UUIDv7/counter/allocation, retry/concurrency, legacy/reimport compatibility and crash/reopen; real browser create/list. |
| U3 — people | Add and maintain people in teams; inspect roster/chart with correct home-team, handle, role and reporting relationships. | U2; reuse employee/profile UI and compatible validation. Implement protected canonical mutations/reads and compatibility, not an independent directory. Test identity/links/lead rules, access, retries/concurrency/reopen and real browser flow. Runnable employees remain explicitly not-ready without execution configuration. |
| F5 — task reads/adoption | Internal migration/read foundation, not daily-work delivery. | F2b/U1/delivered directory; reuse ADR0010 and R1 privacy repair. Preserve original IDs/source/history/provenance, reject private/unclassified current/historical snapshots; owner-bound staging, generation fence, legacy receipts/capture and replay. Unsupported sources stay intact; old-HQ owner proof remains required where applicable. |
| U4 — daily tasks | Create a team's task, open it, change supported facts/state and see truthful history/queues after restart. | U2/U3/F5; reuse task reads, snapshots, journal/receipts and work UI. Implement missing selected creation/state/content and scoped T-number allocation. Preserve owner/actor/scope/unknown source, conflicts/original-operation recovery/private-note separation. Browser create→inspect→change→reopen plus failures/security/recovery. Assignment remains unavailable until U5. |
| U5 — assignment | Independently assign/clear team/person, resolve a stale conflict and recover a lost response without a second write. | U4; reuse existing selected service/native/HTTP/UI and sixteen transitions. Original assignment contract unchanged: authorization/directory, immutable facts, durable events/receipts, complete-state browser/recovery. Review this unit's complete delta, not every previously accepted setup/release file. |
| U6 — one useful Codex runtime | Configure one coding employee and complete a real task in its correct workspace, with visible result, cancellation and attribution. | U3/U4 and U5 where execution needs assignment; reuse org-runtime-codex/runtime, org-supervise, agent-os/spool. Protected credentials, binding, filesystem/process restrictions, cancellation/fencing/recovery and minimum usage/budget/permit enforcement before billable work. Preserve Phase A continuity. Review material interfaces first; real bounded execution/denial/failure/cleanup proof. |
| Later selected v1 scope | Each further capability has its own usable contract/acceptance/merge. | Projects/planning, office operations, broader runtimes and other selected features remain in docs/FEATURES.md. Move a dependency forward only if the earlier unit actually needs it. Partial delivery is not full v1. |
| Final — genuine release | Install/run the authentic signed production release on supported platforms. | All other implementation; genuine Apple signing/public trust, bundle/image/floor and normal setup/service/physical-auth/affected platform verification. Signing is last per v1-release-signing-deferral.md; production verifier remains fail-closed. |

The first usable unit **combines initial organization creation/reopen with onboarding
and sign-in**: current `human/setup.rs` only reports success after provisioning HQ,
running gateway and an active office. A sign-in page without a usable organization
is not delivery. Further organization management is its own later slice when needed.
This is an actual dependency adjustment, not a reason to finish assignment first.

F1/F3/F4 share composition. Extract pure libraries first, then the narrow composed
closure. Bootstrap currently uses `selected_tasks/bootstrap.rs` and `service::owner`.
Recover the smallest shared owner/directory interface with the original author;
review any material change before dependents. Do not import the entire task service
to satisfy an import, advertise unfinished task capabilities, remove journal/auth
checks, or create artificial wrappers solely to lower file counts.

## Exact ownership and useful parallelism

The JSON source map lists every existing changed file and hash, including files
reserved for root-owned hunk extraction. Current channel worker's exact sixteen
paths remain unchanged until safe handoff. Root alone updates delivery metadata.

For U1 the fullstack author initially owns existing source-map U1/F3/F4 files
serially, especially `crates/org-host/src/{bootstrap.rs,human/setup.rs,instance/setup.rs}`,
`apps/agentic-org-web/src/{service.rs,sessions/http.rs}` and
`crates/org-office/src/selected_tasks/bootstrap.rs`. Root assigns exact shared
contract/registration hunks in the extraction receipt before any write.

After the create/open/session DTO is agreed, the preserved browser author may own
only `apps/agentic-org-ui/src/lib/create-flow.ts`, its test, onboarding
`welcome/+page.svelte` and `create/+page.svelte`, new
`src/lib/features/onboarding/{gateway.ts,types.ts,state.ts,state.test.ts}`,
`e2e-onboarding/{onboarding.spec.ts,support.ts}` and
`playwright.onboarding.config.ts`. Shared platform/session/shell glue stays
serial with root/fullstack author until explicit handoff. No UI dispatch now.

For U2 the reserved backend additions are
`crates/org-office/src/organization/create_team.rs`,
`crates/org-proto/src/selected_organization.rs`,
`crates/org-client/src/selected_organization.rs`,
`apps/agentic-org-web/src/teams.rs`; journal/directory/fence/registration changes
are serial shared work after reviewed cutover. UI additions are
`apps/agentic-org-ui/src/lib/features/team-directory/{TeamCreateForm.svelte,TeamDirectory.svelte,gateway.ts,types.ts,create-state.ts,create-state.test.ts}`
and `e2e-directory/teams.spec.ts`. Existing chart composition stays reserved.

U3 reserves `crates/org-office/src/organization/people.rs` and
`apps/agentic-org-web/src/people.rs`, with reviewed DTO/client additions serially.
Exact existing employee UI files and U4/U6 new source files are enumerated before
those units' dispatch; this roadmap is no blanket write grant or premature API.
U5 reuses the exact source map and preserved four-file browser reservation.

At most two concurrent writers **including root**, separate worktrees, agreed
interfaces/disjoint actual paths. Root coordinates/verifies without source edits
when two delegates write. Default one author during extraction; use two only when
an agreed backend/UI contract makes the overlap useful. Preserve author contexts,
never create a replacement tree or duplicate routine discovery.

## Checks, local reviews, acceptance and exercise

Each unit gets its own stable contract, base/commit, reuse manifest, assessment,
current packet/preflight and source-bound evidence. Keep full required Rust
fmt/workspace Clippy/workspace tests for Rust units; app type/tests/build and
affected browser/accessibility/CSP for app units; kit/catalog checks as affected.
Use targeted regressions while repairing, then one coherent required pass at the
unit's freeze. Pure library extraction does not require the entire assignment
browser journey. Shared changes revalidate affected consumers meaningfully.

Each unit needs fresh independent review of its **complete delta**, exact original-
author validation and a separate fresh final exact-commit code/output review under
the installed kit. Old broad INCOMPLETE reports remain preserved and confirmed
findings travel with the affected unit. They are not approvals; unchanged reservoir
files are not repeatedly rereviewed. Track remaining orphan/excluded hunks in the
map until selected scope is fully allocated. Root merges accepted units --no-ff
into v1-integration and pushes it. No release-v1/main merge/push. Checklist rows
remain incomplete until their full stated scope passes, even if a sub-scope ships.

F1–F5 use temporary-root unit/real subprocess fixtures and are explicitly internal.
For U1 acceptance, provide a documented foreground disposable **development/test**
launch path using actual static UI and supervised Rust services, generated local
admission fixtures and user-entered bootstrap inputs. No seeded production accounts.
The existing ignored assignment driver auto-creates HQ/imports tasks: it is reusable
evidence, not manual onboarding delivery. Reopen the same disposable root to prove
persistence. Keep genuine production release/image/privilege enforcement unchanged.
Do not invent a runnable command before the launcher exists. Publish exact working
commands with accepted U1. U2–U6 reuse that actual authenticated development instance,
not a mock backend or hidden state seeding. Signing-dependent production exercise
stays final; no unsigned-production path or generated production trust key.

Execution update: focused IMPLEMENTATION_PLAN review returned READY, no findings,
for plan17 SHA e7a8cba3b44a5d668ff40a3220a20de8c05f82f41b432b8880b738f9078be3e9
and the 324-path inventory. Actual fresh session01a1e42a-626e-7360-9256-d07176c5f8ef,
Sol/Xhigh, terminal0; ordered ACK/current preflight verified by root. This readiness
and the status-only update do not approve code. Material domain/security interface
changes still require focused architecture review before dependents.

The channel author reached a safe source-frozen handoff: all fourteen production
hashes remain unchanged; the two executable fixtures are repaired and root ran
22 real v3_activation tests successfully (0 failed/ignored; 40 filtered). These
are preserved reservoir receipts, not F4 extraction or acceptance.

F0 was accepted after checks, two confirmed documentation repairs, author intent
and separate fresh final review, then merged --no-ff and pushed to v1-integration
at cad1057b09fa8b105dfb843d0eaed42d24861d58. It delivers guidance only.
F1a was accepted at cdfed316 and merged/pushed as cd777450 after the genuine
strict-object R1 repair, full current checks, retained author intent and clean fresh
initial/separate final reviews. Original failed report and macro/source-guard
regression remain preserved; no verification waiver. Pure library only.

Actual F1b extraction failed because host runtime/platform source also needs the
GatewayRelease/private static-signing/identifier helper closure. Preserve unaccepted
ace8b29d on codex/v1-release-adapters; no source or author context discarded. The
independent socket unit was accepted at d6dbf8ee after confirmed Linux cfg repair,
full checks/retained-author intent and clean fresh initial/separate final reviews;
merged/pushed as ac20f099. Real macOS example/all69tests and Linux cross-Clippy pass;
raw blocked report and sandbox failures remain retained. No platform/runtime waiver.
F2a was accepted at 2fe7c3d3 and merged/pushed as 1b911da2: eight immutable
source/test/vector imports, two minimal registration/export hunks, unchanged
Cargo/lock, full current gates, retained-author exact-tree validation and clean
fresh initial/separate final reviews. The fingerprint integration test stays
preserved with its actual U5 consumers. Pure source/integrity helpers only.

The owner's pinned upstream documentation batch at 6f02ae95 was accepted in normal
merge a11edaf4 and integrated/pushed at 0fdb9723. Most F0 document bytes already
matched it, but equivalent bytes did not establish ancestry; that is now corrected.
Current solo/local-review/security rules, incoming requirements/audit and accepted
evidence remain. Eight configured checks, normal hook, fresh initial/root intent/
separate final passed, with no feature-status change.

Current F2b on codex/v1-durable-journal starts from 0fdb9723; bootstrap depends on
this journal. Its stable contract is registered before detailed planning. Root
owns architecture/interfaces/integration, retained fullstack author assesses the
bounded minimum closure first, then owns the agreed source extraction alone.
Root pauses source writes during implementation. Existing shared test/source files
need hunk selection; actual compilation, guard maintenance and acceptance remain
unproven. No ready independent DTO/second writer or broad plan review is needed.
This boundary refinement follows actual compiler dependencies and changes no
selected architecture, security policy or U1 acceptance. A new broad plan review
is unnecessary; material contract changes still receive focused review first.
Neither internal unit is usable onboarding or full v1 completion.

F2b focused closure checkpoint: the original fullstack author completed read-only
source assessment (536.059s). Interface revision2 independently READY after one
focused v6-fixture correction/recheck, session01a1e546-f737-74e1-926f-cfd395e590d6.
Exact22-path handoff is v1-durable-journal-interface.md/reuse.json; root owns
architecture/integration and pauses tracked writes while that one author extracts.
No second independent writer is ready across this shared writer/capture boundary.
No guard changes, future service imports or source acceptance claimed.

F2b implementation checkpoint: exact22path extraction/current original-author
ready checkpoint consumed by root; all focused compiler/Clippy/fmt/guard and
journal/legacy/migration/protocol checks pass. Actual28SIGKILL cases cover10hooks.
Root rejected a literal-fixture substitution and required real SQLCipher TEXT
subtype RED/GREEN preservation through publication/reopen/rebuild. Two private
test-only selected entrypoints must regain production compilation with real
F3/F5/U5 consumers. Both timed-out CLI executions and raw failures remain; author
saved its ready source/check receipt before the second timeout. Complete source
freeze/workspace/kit/original-author/initial/final review acceptance still pending.

F2b full-workspace checkpoint: fmt/Clippy pass, only two office migration-test
failures remain (609office passes/2existingignored). Author processes are terminal;
root takes messaging.rs/status.rs tests only, making24unit source paths. Replace
the fake downgraded-current v2 fixture with independent released v2 SQL and prove
status/migration/reopen; use current schema version in the v1 message migration.
No production migration or reviewed interface change. Required gates repeat on the
new freeze; retained-author exact-tree validation and both fresh reviews remain.
This bounded serial repair is faster than a new writer handoff and does not change
usable delivery order or acceptance. See reuse.json/root_migration_test_handoff.

F2b07572fd5 full gates pass3020/0/5existingignored plus8kit/hook; retained author
validated frozen tree. Fresh initial review identified missing recovery reflush
and native exact replay consuming ordinal/clock (including exhaustion refusal).
The runner rejected absolute finding paths; preserve BLOCKED run1284422c and both
real issues. Root grants retained author7file repair scope in reuse record and
pauses tracked writes. No independent second writer across shared journal boundary.
Production durability/idempotency gates remain unchanged; final current full gates,
author validation and fresh initial/final acceptance are still required.

F2b bounded repair handoff: original author terminal0/784.55s, F1 recovery flush
fixed with actual RED/GREEN,30acceptance passes,32SIGKILL cases across12hooks and
strict flush-failure fencing. All nonowned1413inputs and6guards unchanged. Both
new native success/refusal replay tests remain RED;103existing work passes.
Opaque TxnFn needs a typed producer for safe MAX lookup. The exact public Rust
trait/shared prefix and private FIFO probe proposal has focused independent
architecture review before additional ownership/dependent code; original journal
architecture and pre-decision exhaustion policy remain. No source writer active.

Native replay interface rev1 READY210.591s, fresh session01a1e58f. Root activated
v1-native-replay-interface.md with all source-linked obligations and correct
Arc adapter testkit.rs. Five minimum org-work source additions extend F2b24->29
without changing immutable allocation or architecture. Ten exact source paths go
to the same original author; root pauses tracked writes. This coupled queue/reply/
authorization repair has no useful second writer. F1 remains preserved, both native
RED regressions must become GREEN, all final gates remain. No usable unit change.

F2b native repair handoff: original author completed817.716s/exit0. Root verified
29 source hashes, six unchanged guards,1404 nonowned baseline inputs and all named
logs. Both retained native retry regressions are GREEN; MAX/fresh authorization,
read-only probe and failed-cleanup fencing, Arc/default/cascade compatibility,
duplicate replay and reopen checks pass. Journal30tests/32SIGKILLvectors/12hooks
retain recovery-reflush repair. Root owns current freeze; no concurrent writer.
Full current checks and NEW initial/retained-author/fresh final acceptance remain
open. No usable feature or production host/signing claim; U1 stays first delivery.


### 2026-11-16 F3 source-derived handoff

F2b accepted at d55e3c25, merged/pushed v1-integration eacb505d after3036 current tests, original-author intent and fresh initial/separate final reviews. The owner independent-delivery policy c5123221 is merged in this working branch and applies now. Preserved original-author F3 intake completed exit0 with11-source actual ACK/preflight; it identified genuine PreparedV1/UUID, selected-owner/directory and F4 session dependency boundaries. Whole F3 is not imported.

Concrete split: original fullstack author01a1dfb4-1d26-7100-9ea4-72b53756ef23 writes ONLY the five org-pki files plus exact Cargo.toml/Cargo.lock hunks under v1-pki-escrow-contract.md. Lead owns architecture/integration and does read-only shared-boundary analysis concurrently; no second source writer until interfaces are agreed. The writer must first compile/exercise the real cryptographic library against its declared existing dependencies. Prior author contexts/worktrees/source reservoir and every deferred F3/F5 owner/generation test remain. Root updates only declared metadata after the writer's safe handoff. F3a has its own acceptance/journal/merge; the broader F3 discovery contract/journal is preserved as superseded, not accepted.

Next shared decision: creation/bootstrap owns PreparedV1 and UUID validation; writer admission needs live owner plus canonical directory without a task-service dependency. Inspect actual retained/current contracts, preserve field/validation/authority semantics and obtain focused material-interface review before dependent implementation. F4/native recovery composition follows its genuine prerequisites; no extra placeholder package, unsigned production bypass or fake setup success. U1 remains the first usable onboarding/sign-in/create/reopen/restart delivery.


## F3b dispatch checkpoint — plan19

F3a source065a0824 independently accepted and merged/pushed0d4035d7, including owner c5123221 delivery policy. Stable F3b contract v1-hq-creation-provider-contract.md, late-registration journalc3b9754b90bf477da91d0d9c6a6ddeda (focused architecture/author refinement began earlier; not backdated). Canonical creation/organization boundary v1-creation-admission-interface.md revision1 preserves the exact reviewed candidate and all4mandatoryconditions. Retained original fullstack author01a1dfb4 owns ONLY33explicit source paths/hunks recorded in .agent-kit/logs/v1-hq-escrow/f3b-before-writer-1.json, in separate dedicated checkout agentic-org-delivery-worker, branchcodex/v1-hq-creation-provider, base0d4035d780e108b90dcf461998d0f97c4b993ac7. Root owns overall architecture/acceptance/integration and serial metadata; read-only while author writes. One source writer: creation/launch/admission phase ordering shares files and no independent ready writer would reduce the critical path. Browser/release/channel author contexts remain reserved; no dispatch against unsettled interfaces. At most2writers remains the cap.

Early proof is actual agentic-org-office all-target compilation, then real temporary executable create/fault/reopen through the declared launch/control port and encrypted persistence. Test supervisor is an internal fixture, never production host ownership/custody or human sign-in. Source guard extensions/new test paths require exact reassignment, not blanket extraction. Preserve F2b repairs, original owner/MAX/counter/generation assertions and all source reservoirs. Full production v1 and original gates stay intact. Next integrate this provider, then F3c/F4 real consumers, then U1 first usable onboarding/sign-in/create/reopen/restart; no oversized bundle gate or signing setup before the rest.


F3b safe handoff: original author completed38granted source paths, including the exact five-path socket/fixture extension. Root verified all38source hashes,1406distinct nonowned inputs (including all five root-owned metadata inputs) and six unchanged guards. Actual executable tests pass2/2 from lead environment after child sandbox bind refusal: Created→SIGKILL→same-create→admit→Ready→reopen preserves package/checkpoint; wrong scope/account/owner, EOF and missing control never bind/Ready. Provider-only internal evidence; production host authority and usable U1 remain separate. Root serially owns frozen checks/review/integration now; no second ready independent source writer or new scope.

F3b current full suite found one released-v6 comparison failure because the new empty local_hq_creation table was included among old tables. After original-author terminal handoff, root takes only crates/org-office/src/office/tests/instance.rs: explicit new-table absence/empty assertions and exact table exclusion from prior-row comparison, preserving every legacy value. No production/source-guard change, no parallel writer. New total39unit source paths; all38author hashes remain frozen. Re-run meaningful migration and complete current gates before normal review.

F3b b8ff17f9 passed3062Rust/0fail/5existingignored, all8configuredapp/kit checks and normal structural hook. Fresh initial review a52ec1c8302643ccaa086c004a7b679e found3real P1 gaps: missing selected creation intent/bootstrap can skip genesis before Ready; admission may redirect frozen public_root; a self-consistent channel may bind another original owner. Run is BLOCKED because report uses absolute paths; preserve it unchanged and collect it in the same late-registration journal. No acceptance/integration. Root confirmed source predicates and returns exactly8existing paths to the same original author: crates/org-office/src/office/hq_creation.rs; crates/org-office/src/office.rs; crates/org-office/src/organization/admission.rs; crates/org-office/src/organization/bootstrap.rs; crates/org-office/src/office/hq_creation/tests.rs; crates/org-office/src/organization/bootstrap/tests.rs; apps/agentic-org-office/src/transfer/writer.rs; apps/agentic-org-office/tests/launch/v3_activation.rs. All other source/guards/metadata stay frozen; root tracked read-only during repair. Enforce existing immutable request/root/owner contract in owning domain/adapter and add actual executable regressions. Preserve legacy behavior and current interfaces; escalate a necessary public/wire/trait change before implementing it. No new architecture, wider import or test waiver. Root reruns structural early/current full gates, then NEW fresh initial, exact original-author intent and separate final before integration. No independent second source writer across this startup/admission boundary.


F3b repair checkpoint: actual root executable RED on unchanged b8ff17f9 proves all4 new cases emit false Ready (old2PASS). Focused architecture correction revision2 e175277e92697977d5d80973f23055162eb2e1dd0d9458436e91934d5ca88e36, reviewed Sol Xhigh session01a1e670-edd5-7013-9541-79b5cbe1a879, READY after correcting legacy qualification. Activate additive rejection-only Office::validate_hq_creation_admission(&self, config: &AcceptanceConfig, owner_member: &str) -> Result<(), OfficeError> before installation; no granting authority/private exports/new schema/wire/trait. Require coherent intent/private binding whenever creation receipts remain; matching PreparedV1 mandatory for selected UUIDv7 creation, receipt-bearing plain legacy bootstrap=None stays compatible. Independently enforce frozen root in install and immutable creation member against live authority at bind; preserve FIFO/MAX/revocation behavior. Existing canonical creation/admission architecture remains active. Detailed evidence .agent-kit/logs/v1-hq-escrow/admission-validation-review-1/root-activation.json. Exactly8 prior-granted paths return to original author for product repair; root tracked readonly, no second writer across coupled startup/admission. Four new executable refusal assertions remain frozen. Root GREEN/earlystructural/fullcurrentgates/normalcommit and NEWinitial/exactauthor/freshfinal remain before acceptance/merge.


F3b review2 safe handoff: 0f850ef0 found one additional real P1: completed exact creation retry could recreate lost org.db, resetting native history. Same-session fix adds pre-initialization committed-storage validation and regression for advanced-state valid retry plus missing/preschema/empty organization storage, retaining all CA/private/local/intent/marker/public bytes. Eight configured checks pass; child runtime socket tests report knownEPERM, not product evidence. Runner BLOCKED at normal structural commit gate (15-path provisioning function), HEAD00bf remains, all raw artifacts preserved; no acceptance. Root verified1441nonowned/sixguards and takes ONLY office/hq_creation.rs for a private helper preserving the exact new check/order. Reviewer test and FEATURES changes remain intact, original author context preserved, no concurrent writer. Recheck structural early, actual6 executable cases and fullcurrentgates, normal checkpoint and NEWfreshinitial/exactauthor/separatefinal required. No usableU1 or signing claim.


## Post-F3b handoff — plan20 (2026-11-17)

F3b accepted source4abca318 is merged/pushed v1-integration55c7d762 with identical reviewed tree04735578. Current3074Rust/zero failures/five existing ignored, app checks/tests/build, original-author validation, fresh initial and separate final review all passed; accepted journalc3b9754b and immutable archived receipts remain in v1-hq-escrow. This is internal provider acceptance, not FC-016/U1/fullv1 completion. The whole-feature checklist remains In progress.

Next stable contract v1-protected-host-creation-contract.md was registered before substantial planning in journalbf2fe737a41c4207b6b7a03f566f9723. Branchcodex/v1-protected-host-creation starts exact origin/v1-integration55c in the existing dedicated delivery checkout. Root remains architecture/acceptance/integration owner. Retained original fullstack author01a1dfb4-1d26-7100-9ea4-72b53756ef23 is running a bounded read-only source closure handoff for protected host producer/recovery and its genuine F4 session/release prerequisites. Source inventory is not a grant; no production source writer yet. Root owns only checklist/task metadata now. Shared auth/provisioning/supervisor paths make this handoff serial; do not add a second overlapping writer. Exact path/hunk ownership and integration order will follow the source-bound handoff, preserving original architecture and independent-delivery requirements.

The local setup failure was a missing checkout-local ignored Hindsight setup receipt; actual setup retain/recall/delete passed with zero model calls. Full live doctor now passes code lookup, memory storage/retrieval, memory MCP and saved-login probes; this is setup health, not product acceptance or entitlement. Exposed code MCP remains bound original checkout; use actual-delivery-root local gateway rather than retargeting it. All production code and preserved contexts remain intact.


Plan20 activation: source-derived F3c needs native F4 auth/lifetime in the same real composition; independent review01a1e6ef-b253-7853-85fd-24d579b41d67 READY activated native-host-boundary revision1 under all5conditions. Compose native auth/bootstrap/provision/writer/recovery plus actual first-consumer host release adapter, deferring private gateway_binding/browser startup to real browser F4/U1. This is the same internal F3c outcome with genuine dependencies, not U1 acceptance. No new atomic ordering design: retained owner already precedes Writers::reserve; pending SQL operation is not authority. Root corrects the intake's alleged ordering contradiction. Exact93candidate paths/hunks/provenance are canonical source grant, not permission for whole reservoir replacement.

Writer handoff: exact original fullstack author01a1dfb4 writes only active native grant in dedicated delivery checkout; root product-readonly while writer runs. Root keeps overall architecture/interfaces/acceptance/integration and executes sandbox-blocked genuine native/socket fixtures. No independent second writer is ready across overlapping auth/host/launch/recovery composition. First safe handoff requires actual host all-target compile and exact real native fixture closure; use --ignored/--exact and actual built worker, never zero-test output. Then current full checks, author intent and separate fresh local reviews before independent merge; browser/gateway sessions and built UI/user-entered onboarding remain next usable U1 closure.


Native safehandoff1: originalauthor completed exit0/980.638s with87changed source files (43existing/44new),27launchcodec tests passing, fmt/diff checks and exact native/build pins passing. Host/worker compilation remains incomplete. Root verified checkpoint2 SHA f205b94d2f26533d212a474a6a66b76a05ac17d13c0a339c2449b774168c433b: all93owned hashes current and1401nonowned inputs unchanged. Extend exact grant by3paths only: frozen supervisor/channels/registry.rs and registry/tests.rs plus worker src/lib.rs only Arc::clone(&office) into Channels::new. This supplies the already reviewed private registry/real existing Office handle, with no new authority/wire design, so no repeated architecture review. Root reassessed host manifest and lock actual delta:7required archive transitive packages, existing158package identities/checksums unchanged; refresh only2catalog evidence entries. Preserve all failed compile/merge logs and exactsourceguard; resume SAMEauthor with fresh packet after this safe handoff. No native runtime/fullfeature acceptance yet.

### F3c native checkpoint, 2026-11-17T01:21:21.753531+00:00

Root actual protected native bootstrap/create/retry/recovery-metadata/worker-restart fixture PASS: one executed test, zero failed or ignored, 14.522 seconds, 1494 source inputs unchanged. Evidence: `.agent-kit/logs/v1-protected-host-creation/root-native-producer-proof-1/result.json`. Unaccepted/unmerged; full workspace checks, original-author validation and fresh reviews remain. Sole retained original author now closes Clippy warnings in existing 96-file native grant. Lead remains product-readonly while the writer runs; serial work avoids overlapping native authority changes. Browser onboarding, real physical authentication, full recovery and authentic signed-release verification are not established by this fixture.

Root verified terminal native author turn3: 96 owned hashes and 1400 unchanged nonowned inputs. Scoped Clippy (normal/test-support), formatting and 15 focused tests pass. Native proof2 passed after cleanup. Full workspace Clippy found two inherited GatewayFirstLine test initializers referencing the deferred release field; root removed only those impossible fields, preserving all assertions/current wire. Continue full gates and review; no architecture or acceptance boundary changed.

Full workspace Clippy now passes. Workspace tests exposed inactive registry account.verify returning Unavailable instead of existing NotAuthorized-before-params contract; root restored refusal only when Authentication is absent, preserving active sign-in. Existing CLI refusal regression now also names session renew/end/grant/inspect and office.open_channel. This localized compatibility repair changes no authority/interface premise; root owns the extra CLI integration test. Full gates3 use --no-fail-fast to gather any other workspace failures in one run.

Checkpoint before acceptance: full gates3 finished with 15 worker launch failures sharing two stale readiness expectations and one build-inventory refusal of approved host build dependencies. Root updated only those exact readiness objects to require office-channels/1 (ADR0012), preserving every assertion. Build-helper closure needs precise inventory adaptation, remaining full current gates, original-author validation and fresh reviews. This checkpoint is intentionally unaccepted and must not merge yet. Root owns the additional parent v3_activation.rs fixture.


F3c root repair handoff: checkpoint1ce54471 preserves native implementation. Strict worker readiness regressions now pass27/27, and the precise build inventory passes15/15 plus the real workspace source fence. Root owns the bounded compatibility/test-inventory paths in v1-protected-host-creation-root-repairs.md, preserving the original96-path native grant and author context. No concurrent writer is active; coupled composition makes these repairs serial. Current full gates, fresh whole-diff review, original-author validation and separate fresh final remain before this internal unit merges. FC012/014 are In progress with native evidence; U1 remains the first usable browser delivery and signing remains last.


F3c fresh initial review087e4e8a completed with confirmed writer findings R1(P1): lost committed writer credential can be recreated on pending provisioning retry, and R2(P2): final marker pathname is visible before initialization, stranding retries after interruption. Parser rejected absolute finding paths; preserve raw report/BLOCKED result, not acceptance. Root source inspection confirms both. Existing custody refusal/identity-preservation and durable retry contracts stay unchanged; atomic no-clobber publication and explicit lost-custody refusal are local implementation repairs, with no new schema, public wire or architecture premise. Resume exact reviewer01a1e747-bc1a-7e33-ac24-63f2230bfb65 for bounded same-session repairs under SolXhigh in the same dedicated checkout. Exact ownership: writer.rs, writer/tests.rs, provisioning/tests.rs only. Root pauses product writes; no second writer is useful across this shared writer boundary. Require RED/GREEN missing-credential and marker-interruption regressions, original identity/checkpoint/custody preservation, early structural check and focused Clippy/tests, then root actual native/full current gates and ordinary repair checkpoint. A NEW fresh initial run, exact original fullstack-author validation and separate fresh final remain before integration; preserve all work and failed logs.


F3c same-session writer repair safe handoff: reviewer01a1e747 completed terminal0/1062.620sec. Root verified exact3 owned hashes and1492 unchanged nonowned inputs, read every product/test diff, actual RED cases, Darwin interruption-test adaptation, final12PASS/0FAIL focused tests, both all-target Clippy configurations and formatting. R1 now refuses new custody when retained office/public/protected state exists and refuses replacement device allocation while another writer record survives. R2 flushes a private complete stage before exclusive atomic publication; unsafe final entries are never clobbered. Actual write-limit/SIGSTOP/SIGKILL and post-publication-kill cases retain original admission/checkpoint; old assertions remain. No new schema/wire/dependency/premise. Root owns source again, saves ordinary unaccepted checkpoint before full gates5 (including all3 actual native fixtures) to exercise the staged structural gate early. NEWfreshinitial, exact retained original-author validation and separatefreshfinal remain before merge. All failures/earlier receipts stay archived.


F3c gates5 completed full workspace tests with one failure: actual_host_connection_pair_signs_in_and_eof_invalidates_registry_session hit its test-only8sec socket read watchdog during parallel debug Argon2 work; the exact unchanged compiled test passed alone. All new custody/marker regressions and three actual native fixtures passed. Root now owns only authentication/tests.rs for a bounded30sec sign-in test watchdog shared by its two real sign-in behavioral fixtures. Every response/identity/renew/logout/EOF assertion, real hashing parameters and production deadlines remain unchanged; this is not a sign-in latency/SLO acceptance claim. No new tests, ignores, global serialization or weaker authentication. After focused actual tests and normal checkpoint, rerun full current gates6 and required fresh initial/origin/final reviews. Root remains sole writer; unchanged contracts need no architecture re-review. Preserve gates5 failure and isolated original-source evidence.


F3c separate fresh finalfdbb8911 found F1(P2): HostDb::create protects writer custody but deletes recovery.<operation> if the registry is lost after durable Pending storage and before writer reservation. Root current-source inspection confirms the deletion path; legacy leftover cleanup fixture wrongly includes recovery.Abc123 as disposable. This is an implementation preservation defect under unchanged ADR0007/0011, not a new architecture premise. Exact final reviewer01a1e799-77aa-7f11-b39c-e66aadf9e4b9 owns the same-session bounded repair in hostdb.rs, hostdb/tests.rs and provisioning/tests.rs only. Refuse before any sidecar/credential cleanup when recovery custody survives, including malformed retained records; preserve registry key, unrelated credentials, all sidecar bytes and original pending identity/code. Add actual pre-writer loss preservation RED/GREEN evidence, retain legitimate fresh/uncommitted TLS/registry cleanup, and correct only the obsolete recovery-deletion fixture expectation. No test ignores, broadening, new wire/schema/dependency, real signing/auth setup or workaround. Root pauses tracked writes during repair. Focused/full current checks, NEWfreshinitial, exact original-author validation and separatefreshfinal remain before merge; preserve failed final/earlier author acceptance as historical, not current acceptance.

F3c final-review recovery repair reached safe handoff: exact reviewer01a1e799 completed terminal0/476.551sec. Root read all three source/test diffs and actual RED/GREEN evidence, verified owned hashes and1492 unchanged nonowned inputs. Retained recovery namespace now refuses before sidecar or credential cleanup, including malformed custody; cold file-backed production Pending before writer reservation retains original IDs/code/key/bytes. Original two regressions fail, current two pass; related97PASS/0FAIL/1existing native ignored, both all-target Clippy configurations, formatting and live structural health pass. Only the obsolete disposable recovery entry was removed; legitimate uncommitted registry/TLS/temp cleanup remains covered. Root owns source again and records an ordinary unaccepted checkpoint, then full gates7, NEWfreshinitialXhigh, retained original-author intent2 and separatefreshfinal2. No new architecture/wire/schema/dependency or usable U1/signing claim.
