# v1 implementation checkpoint

Broader target: the selected organization-data model in vertical UI/backend/database slices. A prerequisite or domain-only delivery does not complete v1.

Current owner direction (2026-11-16, plan17): preserve active bounded repairs through safe handoff, then extract smaller **separate acceptance and merge units** from the existing reservoir. Deliver onboarding/sign-in with its necessary first-organization creation/reopen, teams, people, daily tasks, assignment and one useful Codex runtime in dependency order. Internal foundations do not count as usable deliveries. No entire-assignment-bundle-first gate; the assignment contract itself is unchanged for its eventual unit. See [current delivery boundaries and exact source inventory](v1-incremental-delivery.md). Signing setup/authentic signed-release verification remains last; security enforcement and other production requirements remain intact.

Integrate reviewed features into `v1-integration`; review locally without waiting for a PR. Release `v1` is separate. Codex owns every repository area and uses local kit workers/reviewers. Do not message or wait for Ada, offline developers or an external reviewer. Preserve genuinely blocked work and continue independent features. Preserve live CLI/spool and user organization data.

## Current work

Integration base: `73a39c58d683b00ad8620d05f7d5ae10fdc17bc9`.
Branch: `codex/v1-assignment-integration`.
[Vertical acceptance contract](v1-assignment-integration-contract.md);
[assessment](v1-assignment-integration-assessment.json).

[ADR-0006 / TAI-01](../../docs/adr/0006-selected-task-acceptance.md) revision 5 remains active, including the focused independently reviewed private gateway transport amendment. The original author identified a genuinely missing HQ escrow format prerequisite; [ADR-0007 / REC-ESC-1](../../docs/adr/0007-hq-creation-escrow.md) revision 1 now has fresh independent architecture READY and a bounded exact-version primitive probe. [Implementation plan revision 7](v1-assignment-integration-plan.md) preserves the complete contract and adds that dependency order; REC-1 and production recovery remain open.

Implementation checkpoints include codecs/snapshot preservation, recoverable journal acceptance, protected writer admission/resume, registry sessions with grant lifecycle, selected native and Rust HTTP adapters, bounded authorized task/history/content reads, and a static task page using the production browser adapter. Focused writer (4), browser/session/client (177), auth (6) and large-page tests passed with their recorded scopes. These do not prove protected setup/adoption or the complete real-service browser journey. Current broader check failures still require repair; workspace/kit checks and both fresh code reviews/original-author validation remain. No vertical assignment feature is accepted as delivered. The same original Sol/Xhigh author continues protected setup/public escrow projection. [ADR-0008 / GW-SBX-MAC-1](../../docs/adr/0008-gateway-macos-sandbox.md) now separately authorizes macOS sandbox implementation after independent READY and disposable actual enforcement/thread/host probes; Linux remains unavailable pending its required reviewed adapter. Neither decision is code or full-feature acceptance. Private live handles/source receipts: `.agent-kit/logs/v1-assignment-integration/state.json`; metric journal `52fcb69bdb0d4986bc8e320a0ced903c` remains open.

Current integration checkpoint (2026-11-16): [ADR-0009](../../docs/adr/0009-complete-release-verification.md) revision 2 is independently ARCHITECTURE READY and active after correcting the shared UI file/directory budget. The lead retains overall architecture, contracts, dependency order and acceptance. The original fullstack author owns production integration; the separate browser author owns only selected browser tests/configuration. The isolated release-library author has completed the imported pure `org-release` library, with exact hashes and main-checkout focused fmt/19 tests/Clippy passing. Actual consumer manifests preserve an acyclic normal/build dependency graph; the gateway does not import privileged host code. Current capability catalog and parent packet cover the public build-trust and kernel-image adapter paths.

Actual temporary protected bootstrap, HQ provisioning, canonical directory import, task adoption and supervised Rust/static UI now run together. Fixture6 complete unchanged Chromium journey passed1/1 (50.8s), including lossless large snapshot/history chunks, all sixteen assignment transitions, independent clearing, queue filters, conflict/reapply, accepted-response loss/reload with the same operation ID, original-result replay after later edits, scope denials, logout propagation/saved session revocation, accessibility and CSP/errors. Production fixes repaired the earlier event digest, sheet containment, wrong-office refusal mapping and transient sign-in contrast defects. Root verified exact test/config and18artifact hashes (seven masked screenshots), protected shutdown and cleanup. This remains an explicit debug-image/temp-custody fixture, not authentic release/setup/service evidence. WebKit ordinary-pointer failure independently reproduces on an unstyled generated button; no forced-click workaround or WebKit pass is claimed. Foreign-account privacy and independent-session account-wide revocation still need mapped meaningful actual tests. Private source-bound receipts are under `.agent-kit/logs/v1-assignment-integration/browser-parallel/runtime-attempt-5/`.

Release-store archive/pin/recovery mechanics now have14 focused tests at their captured checkpoint; runtime admission and production setup/service/recovery, supported-platform evidence, current full checks, fresh local reviews and original-author validation remain open. [Production-readiness clarification](v1-assignment-production-readiness.md) preserves those gates; generated credentials, debug image admission, library tests and architecture READY do not establish authentic production signing or a shipped vertical feature.

[Incremental delivery order](v1-incremental-delivery.md) governs separate accepted merges, not merely worker assignments. Existing broad check/review receipts remain historical evidence on the reservoir; every extracted unit needs its own current verification, original-author validation and fresh independent/final reviews.

## Accepted checkpoints

- Solo workflow correction: feature `effdb4e0`, integration `73a39c58`. AGENTS.md, branch process, roles and catalog aligned; all kit checks, both fresh local reviews and author validation accepted. Journal `2fa7c6b3abc64129a9528c00b4bb1c0f` closed.
- Complete-pair assignment domain: feature `339d01a0`, integration `1ded3a88`. Exact revision, explicit nullable pair, active same-org desired targets and old/new payload; legacy APIs unchanged. All 225 org-work tests, workspace and kit/UI checks passed; both local reviews and author validation accepted. Journal `081df96c0d874a85914acd41a2fb55bd` closed. [Domain contract](v1-task-assignment-contract.md), [TA-01](../decisions/TA-01-explicit-task-assignment.md), private evidence `.agent-kit/logs/v1-task-assignment/`.
- History metadata coverage: feature `43604eb3`, integration `4dd45f5c`; checks/reviews/author accepted, journal `343bdbc564d7483aad95eb7b36fdc9f7` closed. [Coverage contract](v1-history-coverage-contract.md), [TH-01](../decisions/TH-01-task-history-validation.md). Metadata coverage is not full payload/hash verification. [Full history contract](v1-task-history-contract.md) remains open; ADR-0006 now selects the new codec prerequisite.

## Parked runtime work

`codex/v1-codex-runtime` contains no adapter implementation. Preserve uncommitted task/decision records in stash `3d07327070a89a20004b5dd242e3c871e863369e`; never drop it. Private evidence: `.agent-kit/logs/v1-codex-runtime/`. The selected authentication/environment boundary needs local implementation review. Offline owner agreement no longer blocks it.

## Selected-model coverage

Requirements below are not claims that draft SQL or legacy APIs ship them. Each delivery needs actual UI/API/library/storage evidence and local review; preserve or explicitly migrate existing behavior.

| Document | Area | Remaining assessment/delivery |
| --- | --- | --- |
| 01 | Organization, teams, roles, offices, repositories, structure content | Selected revision/repository/provenance migration and UI. |
| 02 | Projects, releases, milestones, integration branches | Explicit migration from existing schema-3 JSON bodies. |
| 03 | Features, unified typed tasks, supporting documents | Selected ownership/type/assignment migration from StoredItem. |
| 04 | Employee home team, handles, reporting, placement | Identity-preserving migration and UI. |
| 05 | Shared operational records, history, private notes | Full accepted-write/history validation; separate authorized private storage. |
| 06 | Stable IDs, counters, authority journal | Preserve opaque IDs/numbers; allocation/recovery evidence. |
| 07 | Deployment attempts and snapshots | Runtime/API/UI assessment and implementation. |
| 08 | Attribution, lifecycle, ownership, relationships | Enforce invariants in every affected slice. |
| 09 | Release content, flags, changelogs | Runtime/API/UI assessment and implementation. |
| 10 | Immutable usage/prices/corrections/coverage | Recoverable ledger; runtime usage alone is insufficient. |
| 11 | Profiles, routing, attempts, permits, fencing | Runtime/API/UI; local contract/security review. |
| 12 | Assignment and truthful history import | Current vertical contract; queues and actual complete-pair acceptance. |
| 13 | Private notes pointer | Document 05 owns the single contract. |
| 14 | Providers, pools, credential bindings | Runtime/API/UI assessment and implementation. |
| 15 | Messaging/chat, delivery recovery | Preserve pending asks/replies/triage through explicit cutover. |
| 16 | Receipts/checkpoints/cleanup/office continuity | Durable evidence at every affected boundary; no destructive reset. |
| 17 | Budgets/extensions/holds/stops | Runtime/API/UI and enforcement evidence. |

## Resume discipline

Finish each unit's exact contract, required checks, fresh local reviews and original-author validation. Close its metrics before integration. Start the next active delivery from updated `v1-integration`; preserve the unmerged reservoir and all original contexts. If genuinely blocked, preserve the checkpoint and continue independent work. Do not mark overall v1 complete until every selected requirement and vertical acceptance is verified.

Checkpoint 2026-11-16 02:38UTC: root verified actual disposable launchd adapter1PASS and cleanup, current mainpacket9 actual15sourceACK/preflight0, and focused independent ADR0010r1 ARCHITECTURE READY for conservative baseline1 legacy adoption. Original author is implementing dependent owning docs/import/tests; not migration acceptance. An extended actual native protected bootstrap/foreign-account/two-session-revocation fixture failed101 after revocation with Unavailable versus expected NotAuthorized, stable553Rust/Cargo sources; same author received exact failure and holds released. Bounded Linux pinnedTokio feasibility passed but no Linux production profile/adapter was selected. Browser original context is doing a read-only minimal extra-auth fixture handoff; no new source ownership yet. Full assignment and all final production/check/review gates remain incomplete. Evidence: `.agent-kit/logs/v1-assignment-integration/{implementation/lead-actual-setup-service-test-1.json,implementation/lead-native-auth-fixture-receipt-1.json,legacy-adoption-profile-review/activation.json,parallel-execution-checkpoint-3.json}`.

### 2026-11-16 current parallel checkpoint

Root remains sole architecture/plan/acceptance/integration owner. Plan9/parentpacket10 actual17-source ACK/preflight0 and browserpacket4 actual9-source ACK5/preflight0 are verified. Original main author preserved; browser helper completed/froze its fourfiles after scoped checks0 in a separate worktree. Actual native repaired privacy/account-revocation fixture passed21.875s with all618 source hashes stable. The complete extended Chromium run failed at foreign linked operation lookup403 versus required404; later privacy/logout/revocation branches were not reached. Root verified17 sanitized artifacts and protectedExit0/completeownedcleanup. Original backend author owns the repair; no assertion/status relaxation. ADR0011 protected recovery delivery received independent architectureREADY and is now being implemented, not accepted. Current office security inventory4 failures remain until precise reviewed source/build maintenance. Full production/current checks and independent initial/original-author/final exact-commit gates remain open; feature unaccepted/unmerged. See parallel-execution-checkpoint-3.json and acceptance-audit-draft.json for exact scoped evidence.
