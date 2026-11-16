# v1: separate acceptance and merge units

Revision17, 2026-11-16. Owner direction replaces “finish the assignment bundle
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
| F2a — source/codec | Internal only; no task writes. | Existing org-docs source editing and org-work codec/history under ADR0006. Preserve original bytes, duplicate/numeric/resource vectors and legacy semantics. No assignment service/UI dependency. |
| F2b — durable acceptance journal | Internal only. | F2a; existing office acceptance/origin capture and executable migrations. Crash/replay, protected receipt, publication/sequence and legacy compatibility tests. Bootstrap uses this journal, so this cannot be postponed until daily tasks. |
| F3 — protected HQ state/escrow | Internal only; no normal setup/recovery claim. | F2b; reuse org-pki, HQ creation, launch recovery/writer and host custody/provisioning under ADRs0007/0011. Exact-key retry, protected fresh-owner reservation, escrow/delivery ACK, interruptions/reopen and secret exclusion. Existing-HQ first-owner proof stays paused; REC-1 and physical authentication remain their actual requirements. |
| F4 — sessions/channel lifecycle | Internal until U1. | F1b-socket/F3 and reviewed F1b-release contract/adapter closure as composed; reuse active CH-LIFE-1r2 repair, host auth/listener, gateway handoff and browser sessions. Capacity/ACK/closure, logout/revocation/expiry, cookie/origin/CSRF, foreign-account and endpoint-failure tests. Extract session routing from selected-task dispatch; no task capability advertised just to make sign-in compile. |
| U1 — onboarding, first organization, sign-in/reopen | Establish the first administrator, supply organization/repository/initial-team inputs, enter the real workspace, sign out/in and reopen the same organization after restart. | F1–F4 plus minimum selected directory/bootstrap and existing setup continuation/static auth UI. Finish authorized organization reads and real create/open/session UI adapters. Offline zero-network proof, stable identities/home-team/HR readiness, lost-create-reply exact retry, restart, denial/accessibility/browser evidence and documented manual development exercise. No task adoption/assignment completion prerequisite. |
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
current independent acceptance unit is codex/v1-socket-admission from cd777450:
root owns four immutable source imports plus exactly two socket registrations and
metadata; retained fullstack author validates intent/repairs. No useful second
source slice exists, so one writer; read-only validation may overlap frozen checks.
This boundary refinement follows actual compiler dependencies and changes no
selected architecture, security policy or U1 acceptance. A new broad plan review
is unnecessary; material contract changes still receive focused review first.
Neither internal unit is usable onboarding or full v1 completion.
