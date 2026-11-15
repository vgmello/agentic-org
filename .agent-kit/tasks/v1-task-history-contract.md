# Selected task history: validation prerequisite

Owner request: read `docs/design/organization-data` and complete v1 in vertical
slices across UI, backend and database. Use local review and merge completed work
into `v1-integration`, the shared integration branch for all v1 features.

This disjoint delivery implements the Codex-owned work-library prerequisite for
the selected task activity history. It does not complete the task-history vertical
slice or v1. UI, durable file/journal storage, SQL projection and API integration
remain explicitly outstanding and require their AGENTS.md area owners (or an
explicit owner override). No existing command, event codec or database is migrated.

Authority: document 05, Task activity history; document 12, migration/history
baseline semantics; document 08, stable identity and attribution. Existing v1 IDs
remain opaque. Code scope is `crates/org-work/` plus task/review evidence in
`.agent-kit/`. No existing public protocol or persistence trait changes.

Acceptance:

1. A pure library validates selected schema-1 task events, their fixed task kind,
   positive revision, actor tuple, nonempty change tag, object metadata and the
   explicit old/new assignment pair. Unknown change tags/payloads are retained.
2. Canonical event JSON uses sorted object keys, compact UTF-8 JSON, explicit
   optional nulls/default empty metadata and a SHA-256 excluding the checksum.
   Import verifies the actual checksum. Source-backed interoperability vectors
   cover the draft Python event producer, Unicode, nulls and nested objects.
3. Replay validates organization/task identity, filename revision, contiguous
   coverage and the current snapshot/tombstone revision. Full history begins with
   `created` revision 1; snapshot-only migration may begin at an explicit
   `history_baseline`, without claiming earlier history complete. Identical
   retries are idempotent; conflicting same-identity content is rejected.
4. Malformed, tampered, missing-middle, conflicting and mismatched histories are
   rejected deterministically. Historical assignment IDs are not revalidated
   against present-day membership. Existing work behavior and codecs remain valid.
5. Run repository-required Rust checks and kit checks. Obtain independent local
   diff review, original-author validation and the configured fresh final review
   for the exact feature commit before merging to `v1-integration`.

Full-slice follow-up: accepting writer and durable journal commit task/events/
idempotency receipt recoverably; import/projection and immutable SQL storage;
authorized activity API; UI including unknown change and unavailable history
states. Do not label the capability shipped before those layers are verified.
