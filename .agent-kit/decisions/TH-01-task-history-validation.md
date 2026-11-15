# TH-01: selected task history validation prerequisites

- Status: ACTIVE for revision coverage only, revision 2, 2026-11-15.
- Baseline: `884856559b946ac17129088b2416e40d2d0c1bc4`.
- Scope: `org-work`, no protocol or persistence trait changes.
- Original architecture analysis: Sol Xhigh session
  `01a1dddb-0073-7493-949b-00192bc8415d` (read-only CLI).
- Current delivery: [coverage contract](../tasks/v1-history-coverage-contract.md).
- Overall request remains complete v1 vertically; this prerequisite does not
  complete task history or change the required UI/API/storage behavior.

## Premises and preserved requirements

| ID | Requirement / fact | Evidence and state | Recheck trigger |
| --- | --- | --- | --- |
| TH-P1 | Selected event identity is organization/task/revision; IDs remain opaque. | VERIFIED: documents 05 task activity history and 06 identity. | Changes to owning contracts. |
| TH-P2 | Coverage is contiguous from creation or an explicit migration baseline through the snapshot/tombstone revision. | VERIFIED: document 05 and document 12 migration boundary. | Import/baseline semantics change. |
| TH-P3 | Baselines never prove earlier history complete; unknown later tags must survive. | VERIFIED: documents 05/12. | New selected change/baseline schemas. |
| TH-P4 | Existing Rust canonical JSON is portable for arbitrary Python-produced numbers. | DISPROVED: original author source/probe evidence; exponent formatting and large-integer precision differ. | Numeric contract and a tested compatible codec exist. |
| TH-P5 | Header coverage can be validated independently of payload numeric encoding. | VERIFIED by separation of document 05 coverage/checksum obligations. Caller must independently verify checksums; coverage never establishes content integrity. | Public API begins accepting/validating event payloads. |
| TH-P6 | Codex owns org-work and can add pure domain behavior without editing another owner's area. | VERIFIED: AGENTS.md, org-work Cargo.toml/lib.rs. | Source ownership or persistence traits change. |

## Decision, revision 1

Proposed a complete pure selected-event codec/hash/coverage validator. Feasibility
analysis found TH-P4 disproved and no complete numeric canonicalization rule.
That delivery is blocked pending the owning document's numeric decision; no code
was implemented. Preserve the original requirements and interoperability vectors.
Do not reuse the old helper unchanged, ban floats, round large integers, substitute
JCS, or silently adopt lexical-number preservation as a new contract.

## Decision, revision 2

Proceed only with the independent **revision coverage** capability (TH-P5). The
public function consumes expected organization/task identity, current revision,
an explicitly selected creation/migration origin and event headers. Headers carry
event and filename revisions, change tag and a checksum already verified by the
caller. Return coverage facts or a structured error; never a validated-event or
publication/persistence receipt. The API documentation must make that limitation
visible at the entry point and result type.

Check every header before deduplicating; reject invalid identity/revision/digest
syntax or inconsistent claims. Compare same-identity checksums and header facts
for idempotent retries; conflicts fail. Sort unique revisions, require the declared
origin marker and contiguous coverage through the supplied current revision. Do
not infer a baseline from a gap. Unknown later change tags remain admissible.
Migration coverage always says earlier history is unavailable, even if its baseline
revision happens to be 1. Enforce the positive signed-SQLite-integer revision range
and avoid arithmetic at integer boundaries that can overflow.

Implementation stays pure and deterministic with existing standard-library
collections. No new crate/domain dependency or persistence abstraction is needed.
At crate level org-work's existing dependencies/consumers remain unchanged; no
coupling edge is added. Callers will adapt validated import metadata later.

Rejected alternatives: block all independent work on the numeric question;
misrepresent an unverified header as a validated event; trust input order; treat a
matching operation ID as event identity; infer migration baselines; silently accept
conflicting duplicates. These fail TH-P2/P3/P5 or the owner's proceed-when-blocked
instruction. This bounded prerequisite is useful without changing existing codecs.

Limits: no payload hash computation/verification, actor/schema/assignment or
timestamp validation, state reconstruction, filesystem path resolution, task
authorization, atomic journal/SQL writes or UI. An importer must satisfy those
separate obligations before accepting/publishing a projection. Opaque checksums
are used only to compare already-verified content identities. The full selected
history task and all vertical layers remain outstanding.

Validation: out-of-order input, opaque IDs, matching/conflicting duplicates,
cross-organization/task and filename mismatch, empty/gapped/truncated/extra
history, creation and explicit baselines, later baseline attempts, unknown tags,
invalid checksum syntax and signed-integer boundaries. Run workspace Rust/kit
checks and independent exact-commit local reviews before integration.

## Architecture review

Revision 2: READY, independent Sol Xhigh read-only CLI session
`01a1dde5-0b21-75b0-817a-a812ebab7683`. No blocking findings; identity, explicit
baselines, duplicate/conflict checks and numeric-policy separation were verified
against documents 05/12 and the current skill packet. Lead accepts readiness for
detailed planning and implementation of coverage only. Re-review any expansion
into event payload codecs, new import/store interfaces or resolution of the
numeric contract. Full event validation remains blocked under revision 1.
