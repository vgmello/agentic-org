# Current-branch feature checklist reconciliation

2026-11-16, Codex. Source checkpoint:
`b77cbd6876d72f76cc3b7f1d5fa3c3d96f424c09` on
`codex/v1-assignment-integration`. This is a documentation reconciliation, not
implementation acceptance or a new product release. It supplements the earlier
[first-five](2026-11-16-first-five-features.md) and
[remaining-58](2026-11-16-remaining-feature-audit.md) audits at their recorded
revisions; their historical results are not rewritten as current results.

Reviewed all 158 existing checklist row scopes against their audit/evidence
references and the changes since that baseline. Refreshed 25 rows where current
source materially changes the evidence or the remaining-work description. Added
two previously accepted library foundations. No incomplete product row was
promoted to Done. Unchanged rows retain their explicitly dated audit evidence;
this is not a fresh whole-product source/test execution.

The resulting checklist has 160 rows: **11 Done, 66 In progress, 42 Planned,
1 Blocked, 40 Deferred, 0 Needs verification**. Done includes explicitly bounded
internal libraries and narrow capabilities; it does not mean eleven usable
end-to-end product deliveries or a completion percentage.

## Completed library foundations

These are missing inventory entries for already accepted work, not new product
scope or a subdivision that declares the assignment/history feature complete.
Both integration commits are verified ancestors of the current local and remote
`v1-integration` refs and this preservation HEAD.

| Row | Exact completed scope | Verified evidence and exclusions |
| --- | --- | --- |
| FC-159 | Pure assignment command decoding and decision planning. | Integration `1ded3a88`, [contract](../../.agent-kit/tasks/v1-task-assignment-contract.md), [decision API](../../crates/org-work/src/selected_assignment.rs), [18 unchanged public-API tests](../../crates/org-work/tests/selected_assignment.rs). Current source adds a separate retry-fingerprint API before the original declarations; the accepted planner/decoder and test file are unchanged. That added fingerprint API is excluded from this Done scope. No authorization, storage, receipts, HTTP/UI or full assignment claim; those remain FC-041. |
| FC-160 | Header-only task revision-history coverage. | Integration `4dd45f5c`, [contract](../../.agent-kit/tasks/v1-history-coverage-contract.md), [coverage API](../../crates/org-work/src/history_coverage.rs), [coverage tests](../../crates/org-work/tests/history_coverage.rs). Both source and tests match the accepted integration byte-for-byte. Checks supplied identity/revision/digest-header claims, duplicate consistency, gaps and explicit migration baseline. It does not calculate payload hashes, validate authenticity, persist history or complete FC-042. |

The existing exact-feature review handoffs for these two commits report accepted
scoped work with original-author validation and separate fresh final review. Their
local receipts remain under `.agent-kit/reviews/8449c5dc7e4041aa92dc2b46c4a3bc05/`
and `.agent-kit/reviews/37a849d94cc74fc99165be47f9713344/`, respectively. This
inventory edit neither recreates nor expands those historical approvals.

## Bootstrap and access

FC-011/012/013/014/015/016/017/018/019/022/153/154/155 now distinguish actual
implementation from the earlier unavailable composition:

- [Protected reverse bootstrap](../../crates/org-host/src/bootstrap.rs),
  [resumable setup](../../crates/org-host/src/human/setup.rs),
  [continuation records](../../crates/org-host/src/instance/setup.rs) and
  [service registration](../../crates/org-host/src/human/setup/service.rs) exist.
  Setup retains its original operation IDs and release pin, provisions HQ and
  checks local service readiness. Missing genuine trust/platform prerequisites
  still refuse normal production setup.
- [Host authentication](../../crates/org-host/src/service/authentication.rs) and
  [browser-session HTTP composition](../../apps/agentic-org-web/src/sessions/http.rs)
  implement real account verification, connection-bound sessions, sign-in,
  refresh, logout and revocation. This is broader than fake-backed session UI,
  but does not complete every domain adapter or organization-switching flow.
- [Host recovery](../../crates/org-host/src/recovery.rs) and
  [terminal recovery composition](../../crates/org-host/src/human/recovery.rs)
  implement escrow-related status/delivery/export/ACK paths. Fresh OS presence,
  protected custody and genuine platform verification remain distinct from
  generated test adapters. REC-1 HQ-loss recovery remains unsupported.
- [Shared verifier](../../crates/org-release/README.md),
  [host release adapters](../../crates/org-host/src/release.rs) and
  [release tests](../../crates/org-host/src/release/tests.rs) cover exact signed
  manifests, directory/tar.gz inputs, pins and verification failures. They do not
  implement downloads or supply genuine production publisher/platform trust.

General account administration/ownership grants (FC-153) stays Planned. Real
first-account bootstrap/credential reads and fixture-only account mutation
helpers do not implement a production account-management API.

Historical disposable real-service Chromium evidence is linked in the
[assignment checkpoint](2026-11-16-assignment-branch-checkpoint.md). It proves
scoped functional behavior with generated debug admission, not authentic signed
production setup, all platforms, a current full-source freeze or fresh complete
review approval. Current channel lifecycle repairs are not imported into this
snapshot. Full supported-platform release acceptance (FC-024) stays Blocked.

## Directory and bootstrap

FC-028/029/031/139/142 use the newer
[selected bootstrap](../../crates/org-office/src/selected_tasks/bootstrap.rs),
[bootstrap regression](../../crates/org-office/src/selected_tasks/bootstrap/tests.rs),
[directory admission](../../crates/org-office/src/selected_tasks/directory.rs) and
[versioned input](../../crates/org-proto/src/selected_bootstrap.rs):

- Canonical organization, remote repository/state branch, HQ, initial team and
  HR profile are materialized. Initial team entity/counter/allocation share one
  frozen journal transaction; new identities use UUIDv7.
- HR has a stable handle, explicit valid home team and paused/not-ready execution.
  There is no fabricated execution configuration or running-agent claim.
- A protected owner account is not an employee profile. Bootstrap creates no
  teamless owner employee; legacy owner-person/membership representations still
  need an explicit compatibility migration.

This establishes partial implementation, not complete selected team/people
creation/editing, general counters, repository/project associations or usable
organization onboarding. Existing-HQ protected first-owner registration is still
a separate missing prerequisite; fresh-HQ bootstrap cannot prove it.

## Tasks and durability

FC-040/041/042/053/141/143/146 now reference:

- [Selected assignment acceptance](../../crates/org-office/src/selected_tasks/service.rs),
  [dedicated journal](../../crates/org-office/src/store/acceptance.rs),
  [selected reads](../../crates/org-office/src/selected_tasks/read.rs) and
  [lossless source adapter](../../crates/org-docs/src/source.rs).
- [Adoption/staging](../../crates/org-office/src/selected_tasks/adoption.rs),
  [legacy source and public-provenance policy](../../crates/org-office/src/selected_tasks/adoption/legacy_import.rs)
  and [current/historical privacy regressions](../../crates/org-office/src/selected_tasks/service/tests/adoption_snapshot_privacy.rs).

These implement recoverable selected work/bootstrap publication, protected
account-bound original outcomes, exact source/history preservation, explicit
mapping/staging and rejection of private/unclassified public snapshots. They do
not complete selected task creation/state editing, every domain migration,
existing-install admission or complete assignment delivery. Rejecting private
data from shared history does not implement author-private notes (FC-053).

The [prior scoped repair checkpoint](2026-11-16-assignment-branch-checkpoint.md)
records ten privacy regressions and sixty selected-task tests passing. Those
receipts are preserved historical evidence, not tests rerun by this documentation
change. Broad independent reviews of the preserved branch remained INCOMPLETE;
no full feature acceptance is inferred from them.

## Validation and handoff

This edit changes only feature inventory/category summaries and this evidence
record. Validate unique contiguous IDs, checkbox/status consistency, one category
per row, row/category/global count agreement, local links and catalog/preflight.
Do not rerun unrelated Rust/application suites for this documentation-only edit.
The new counts do not change selected scope, deferred engines, security or release
gates. Genuine signing setup and authentic signed-release verification remain
last; existing fail-closed release enforcement remains intact.

The side conversation cannot use independent subagents. Its documentation commit
is a preservation checkpoint, not a new independent/final acceptance verdict or
an integration merge. The main lead retains the normal local review gates before
integration. Main-thread implementation, plan edits, active workers and all
original contexts remain untouched.
