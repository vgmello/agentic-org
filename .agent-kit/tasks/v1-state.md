# v1 implementation checkpoint

Overall objective remains **complete the selected v1 organization-data model in
vertical UI/backend/database slices**, proceeding to other work when a dependency
blocks one slice. A prerequisite delivery is not completion of this objective.

Integration target: `v1-integration` for all v1 features, per the owner's latest
instruction. Review locally before integration; do not wait for a PR. Release
`v1` remains separate. Preserve existing live messaging and user organization data.

Current integration baseline: `4dd45f5ccd616cb5c54900196bd88b56ebe30011`.

## Ownership blocker audit

Three consecutive goal turns retain the same owner-controlled prerequisite.
The latest audit fetched both release and integration refs, found no incoming
release commits or prerequisite changes, and checked the installed coordination
inbox twice with no replies. The remaining vertical slices require edits reserved
by AGENTS.md to UI/server/shared-contract owners. The Codex adapter additionally
requires the Ada/River-owned authentication/environment boundary. The bounded
independent feasibility work above does not resolve either authorization or that
contract. Resume after an explicit owner override or the affected owners' replies
and prerequisite changes. Full v1 completion remains unproven and outstanding.

## Current delivery

- Branch `codex/v1-codex-runtime`, stable contract
  [v1-codex-runtime-contract.md](v1-codex-runtime-contract.md), metrics journal
  `27b1c5f855bf4796a99c9f5e89351e3d`.
- The missing Codex-owned native adapter is blocked before detailed planning.
  [RT-01](../decisions/RT-01-codex-native-runtime.md) records the actual
  Sol Xhigh architecture author's INCOMPLETE assessment. Document 11 explicitly
  requires an unfinished adapter/environment-contract extension; current
  launch/probe inputs and supervisor allowlists do not provide the selected
  connection-auth mechanism. Ada/River own that boundary. No adapter code exists,
  no independent architecture READY is claimed, and the full contract remains.
- Re-ran the existing S0 no-auth/no-model probe on installed Codex 0.169.0:
  sandbox, exact fixture socket, synthetic read/write deny and legacy Git checks
  pass. Empty-thread resume is unavailable, as expected without a real turn.
  Live model-tool isolation, isolated binding and full policy conformance remain
  unverified. The private output is not an office-readiness receipt.
- Additional bounded no-auth/no-model feasibility probes (RT-01 r2) ruled out
  `mcp_servers={}` as a blanket reset and confirmed `:workspace` temporary-root
  inheritance can permit an outside-root write. A controlled empty home plus
  CLI untrusted-project selection removes the synthetic MCP/project layer from
  effective config. A custom non-inheriting profile allows fixture workspace
  and scratch writes and refuses the tested sibling write. These narrow results
  guide the next architecture review; they do not resolve the owner-controlled
  connection-auth boundary or prove live model-tool isolation.

## Previous delivery

- Task-history coverage was reviewed and integrated at `4dd45f5c`; the integration
  tree equals the exact reviewed feature tree. Both local reviews were clean and
  original-author validation accepted. The metrics journal below is accepted and
  closed; the full selected task-history codec and vertical slice remain open.
- Branch `codex/v1-task-history`, stable contract
  [v1-history-coverage-contract.md](v1-history-coverage-contract.md).
- Delivered contract: [v1-history-coverage-contract.md](v1-history-coverage-contract.md),
  Codex-owned `org-work` revision-coverage prerequisite. Existing event/store
  formats remain unchanged. Full vertical integration is still required.
- Accepted metrics journal: `343bdbc564d7483aad95eb7b36fdc9f7`. Original full-codec
  journal `65b3bc1bb55d47658be48f419fa908b4` closed abandoned before implementation:
  numeric hash semantics require a document-owner decision. Its full requirements
  remain outstanding; no narrower result is claimed to satisfy them.
- Architecture/routing: conservative Sol Xhigh. [TH-01 revision 2](../decisions/TH-01-task-history-validation.md)
  scopes independent coverage only; fresh architecture review is READY. The
  original Sol Xhigh architecture-author session implemented that contract. Its
  186 existing and 21 new org-work tests, formatting and targeted Clippy passed.
  Full workspace Rust formatting, Clippy and tests and the kit checks (including
  UI check/tests/build) passed. The workspace source-inventory guard required
  staging new Rust files before its successful rerun. Exact-source local review
  and integration evidence live in ignored check/review artifacts and Git; this
  checkpoint is not an acceptance receipt.
- The full [selected-history contract](v1-task-history-contract.md) remains
  outstanding, including portable hashes and vertical API/storage/UI delivery.
- UI/server/shared-contract owners were contacted about coordinated vertical
  work. No owner response observed yet. A scope clarification is pending with
  the user; do not infer authorization from elapsed time.

## Selected-model coverage to implement and verify

These are requirements, not claims that draft SQL or existing legacy APIs ship
them. Each completed feature needs actual UI/API/library/storage evidence and
local review. Existing behavior must be preserved or explicitly migrated.

| Owning document | Delivery area | Current evidence / next assessment |
| --- | --- | --- |
| 01 | Organization, teams, roles, offices, repositories and structure content | Existing Organization/Team protocol predates selected revision/repository/provenance fields; foundation migration and UI assessment required. |
| 02 | Projects, releases, milestones and repository integration branches | Existing org store uses schema-3 JSON bodies; selected model requires explicit migration, not replacement by draft SQL. |
| 03 | Features, unified typed tasks and supporting documents | Existing StoredItem separates task/bug fields and lacks assigned team; selected ownership and typed-task migration required. |
| 04 | Employee home team, handles, reporting and placement | Preserve identities; selected identity/placement and UI assessment required. |
| 05 | Operational data, task history and author-only private notes | Legacy immutable work_events exist; selected history validator is current prerequisite. Private notes require separate durable storage and authorization, never shared comments. |
| 06 | Stable identity, readable counters and authority journal | Preserve opaque legacy IDs and numbers; selected allocation/recovery assessment required. |
| 07 | Deployment attempt history and commit snapshots | Runtime/API/UI coverage assessment required. |
| 08 | Attribution, lifecycle, ownership and typed relationships | Cross-cutting invariants must accompany every affected slice. |
| 09 | Explicit release content, flags, product/technical changelogs | Runtime/API/UI coverage assessment required. |
| 10 | Immutable usage ledger, prices, corrections and coverage | Existing runtime usage is not proof of a persisted/recoverable ledger. |
| 11 | Model profiles, routing, attempts, permits and execution fencing | Runtime/API/UI coverage assessment required; contracts and host custody remain owner-governed. |
| 12 | Complete team/person assignment and truthful history migration | Current single-assignee implementation differs; selected full-pair command and queues outstanding. |
| 13 | Private notes compatibility pointer | Document 05 is authoritative; no duplicate implementation contract. |
| 14 | Providers, connection pools and local credential bindings | Runtime/API/UI coverage assessment required. |
| 15 | Messaging/chat and delivery recovery | Preserve Phase A pending asks, replies and triage; selected service cutover requires explicit recovery evidence. |
| 16 | Durable receipts, checkpoints, cleanup and office continuity | Assess with every durable/runtime boundary; never erase state based on draft disposable-cache claims. |
| 17 | Budgets, extensions, holds and stop controls | Runtime/API/UI and enforcement coverage assessment required. |

## Resume discipline

Finish the current exact contract's implementation, required checks, independent
reviews and author validation. Close metrics while on its feature branch, then
merge the reviewed feature into `v1-integration`. Record remaining layers as
outstanding. Start the next feature from the updated integration branch. Do not
mark the overall goal complete until every selected requirement is implemented
and its vertical acceptance evidence is verified.
