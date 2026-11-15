# Task history revision coverage

Owner request remains complete selected v1 organization-data vertically across
UI/backend/database; if blocked, proceed with other work. Review locally and merge
finished features into `v1-integration`, without waiting for a PR.

The full selected-history validator is blocked on document 05's unresolved numeric
canonicalization contract. This independent prerequisite checks revision coverage
in Codex-owned `crates/org-work` without choosing a hash policy. It does not
complete task history, its vertical slice or v1. Preserve those requirements in
`v1-state.md`; do not reinterpret coverage as event integrity or authenticity.

Scope: pure metadata-only coverage validation, no I/O, new persistence traits,
wire changes, SQL, UI or changes to existing command/event codecs. Authorities are
document 05 Task activity history and document 12 migration/history baselines.
Inputs provide expected organization/task/current revision, explicit creation or
migration origin, and event headers including filename revision and a checksum
that the importing layer must have independently verified against its payload.

Acceptance:

1. Validate every header's organization/task and filename/event revision against
   expected identity; revisions are positive and within SQLite's signed integer
   domain. Opaque existing IDs are preserved.
2. Identical `(organization, task, revision, checksum)` retries are idempotent;
   conflicting same-identity checksums fail. Invalid checksum syntax and
   inconsistent header claims for an identical retry fail deterministically.
3. Creation history requires `created` at revision 1. A migration history requires
   an explicitly declared positive baseline revision and `history_baseline` at
   that revision, always preserving the fact earlier history is unavailable.
   A later baseline cannot conceal missing events. Unknown later tags are allowed.
4. Accept out-of-order input after checking all headers; deduplicate retries and
   require contiguous revisions from the declared origin through the current task
   or tombstone revision. Empty input, gaps, extra later events, mismatched final
   revision and invalid identity/headers fail without overflow or mutation.
5. Return explicit revision-coverage information. API documentation states that
   payload hashes, schema/actor/assignment validation, reconstructed state,
   authorization, persistence and import/publication acceptance are separate.
   No caller can mistake this result for a validated or persisted event.
6. Meaningful tests cover those positive/negative cases and integer boundaries.
   Run required Rust workspace and kit checks; obtain fresh independent local
   review, original-author validation and fresh configured final review before
   merging the exact reviewed feature into `v1-integration`.
