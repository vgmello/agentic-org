# v1 implementation checkpoint

Overall objective remains **complete the selected v1 organization-data model in
vertical UI/backend/database slices**, proceeding to other work when a dependency
blocks one slice. A prerequisite delivery is not completion of this objective.

Integration target: `v1-integration` for all v1 features, per the owner's latest
instruction. Review locally before integration; do not wait for a PR. Release
`v1` remains separate. Preserve existing live messaging and user organization data.

Current integration baseline: `4dd45f5ccd616cb5c54900196bd88b56ebe30011`.

## Current delivery

- User directed moving to another feature when blocked. Current branch:
  `codex/v1-task-assignment`; stable contract
  [v1-task-assignment-contract.md](v1-task-assignment-contract.md).
- Implement the Codex-owned selected complete-pair assignment domain behavior:
  explicit nullable fields, exact revision, active same-organization targets and
  old/new event payload. Existing legacy commands/store interfaces stay intact.
  API, durable commit and UI integration remain required follow-up layers, not
  implied by a pure domain result.
- Metrics journal `081df96c0d874a85914acd41a2fb55bd`; actual author session
  `01a1dee0-f635-7b11-8050-b8d7113b45f7`, observed Sol Xhigh. TA-01 r1 received
  fresh independent architecture READY. The same author implemented the module
  and 18 public-API tests; all 225 org-work tests, targeted Clippy and formatting
  pass. Child-sandbox sccache failures were resolved with process-local wrapper
  bypasses, with no repository configuration change. Full workspace formatting,
  Clippy with denied warnings and tests also pass. Exact-source local review and
  configured kit checks remain the acceptance steps; this is a checkpoint, not a
  receipt or a claim that task assignment is available through the product.

## Parked work and previous delivery

- Native runtime work is preserved on `codex/v1-codex-runtime`; its uncommitted
  task/decision records are in stash `3d07327070a89a20004b5dd242e3c871e863369e`.
  Private evidence remains in `.agent-kit/logs/v1-codex-runtime/`. Do not drop the
  stash or claim the adapter implemented. Ada/River's selected authentication/
  environment boundary remains a prerequisite. This does not block assignment.
- Task-history metadata coverage was reviewed and integrated at `4dd45f5c`
  (feature `43604eb3`); Rust workspace and kit/UI checks passed, both local reviews
  were clean and original-author validation accepted. Journal
  `343bdbc564d7483aad95eb7b36fdc9f7` is accepted and closed; exact-source
  historical acceptance remains in the review/journal artifacts and Git.
- The full [selected-history contract](v1-task-history-contract.md) remains open:
  portable numeric hashing requires an owning-contract decision. TH-01 preserves
  that distinction. Domain revision coverage is not full history or v1 completion.

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
