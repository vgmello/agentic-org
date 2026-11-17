# ADR-0006 / TAI-01 — Selected task assignment integration

Revision 5, 2026-11-15. **ACTIVE architecture; implementation acceptance remains pending.** Original author: `01a1dfb4-1d26-7100-9ea4-72b53756ef23`, observed Sol Xhigh; overall ownership remains with the Codex lead. Investigated branch `codex/v1-assignment-integration` at `73a39c58d683b00ad8620d05f7d5ae10fdc17bc9`. Canonical decision location: `docs/adr/0006-selected-task-acceptance.md`. Revision 4 added the same original author’s focused journal-ordinal correction after independent review of revision 3. Revision 5 adds the independently reviewed focused gateway transport dependency amendment; its authentication/custody and full acceptance requirements remain unchanged. Independent focused recheck returned READY on revision 4; see review history. Valid codec, snapshot, authentication, durability and migration premises are retained. Original proposals and feasibility evidence are retained under `.agent-kit/logs/v1-assignment-integration/author/` and `codec-probe/`.

> **Amended by [ADR-0014](0014-acknowledgement-classes-and-agent-os-latency.md)
> (2026-11-17).** Assignment, creation, status and ownership changes keep this ADR's
> rule: no acknowledgement before durable evidence. `work.comment`, `work.watch` and
> `checklist.check` may be acknowledged once their one atomic operation is committed,
> with the flush following within a bounded interval. Their journal envelope may be
> published without a flush; a durable publication first flushes every earlier
> unflushed envelope, and recovery stops at the first missing or incomplete ordinal.

## Decision and delivery boundary

Deliver selected assignment to real, validated selected tasks through the accepting office, authenticated native transport, Rust browser gateway and static Svelte UI. Include task reads, authorized target choices, ownership/assignment queues, durable operation lookup and task history sufficient to use and verify assignment. Support every task type and projectless tasks.

The smallest faithful boundary operates on existing canonical tasks or tasks admitted by an explicit legacy migration. It does not require implementing all task creation, numbering, scheduling, private notes or historical workflow commands. Those remain v1 obligations. For adopted tasks, other mutations must either use the same selected acceptance foundation or return an explicit unsupported result; they cannot bypass history through legacy storage.

This delivery includes missing foundations: trusted selected directory facts, writer admission/fencing, a complete event codec/validator, recoverable task/event/receipt commits, explicit adoption, and production authentication/transport composition. Completing another pure helper, extending only the SQL draft, or connecting the UI to legacy `task.assign` would not satisfy it.

Initially use the verified HQ as accepting writer for already shared selected tasks. Other offices forward to that writer or report pending/unavailable; they do not accept competing revisions. Offline creation and broader handover remain separate requirements, without being weakened by this boundary. A usable existing-instance admission path is required. Fresh-install acceptance additionally requires the protected HQ setup/service-registration work currently unavailable in the executable; test-only database seeding cannot stand in for onboarding.

## Authority, premises and recheck triggers

The documentation authority map and organization-data documents 01–06, 08, 12 and 16 govern this decision. Documents 01/04 distinguish organization membership, home team and execution placement; 02/03 establish project/task ownership and scope; 06 owns identity and numbering; 08 owns attribution; 05 owns task history and accepted writes; 12 owns assignment; 16 owns receipts and continuity. Draft SQL establishes intended constraints, not deployed storage.

| Premise | State and source evidence | Recheck trigger |
| --- | --- | --- |
| TAI-P1: accountable owner, assigned team and employee are independent facts | **VERIFIED requirement.** Document 12 revision 4 and TA-P1/P2/P3: all nullable pairs, explicit complete replacement, exact revision, active same-org desired targets and no home-team constraint. | Owning assignment, employee-status or ownership contract changes. |
| TAI-P2: legacy assignment is incompatible with selected assignment | **VERIFIED observation.** `org-work/src/tasks.rs::assign` is individual-only; absent assignee clears it; `check_expected` uses assignment-aspect conflict handling. Legacy recipients and optional task team do not establish selected employee/owner semantics. TA-P4 remains valid. | Changes to legacy command, conflict or recipient behavior. |
| TAI-P3: reviewed helpers are reusable but establish no acceptance | **VERIFIED observation.** `selected_assignment.rs::plan_assignment` is pure; inspected references are its definition/tests. `history_coverage.rs` consumes already-verified checksums. TA-P5/P6 and TH-P5 explicitly leave persistence/authentication outside their scopes. | Connecting a caller, changing ports, or claiming validated events/accepted writes. |
| TAI-P4: current native storage supplies useful transaction machinery, not selected durability | **VERIFIED observation.** `org-office/src/store.rs::attempt_batch` commits SQL before replies and can rerun attempts. `store/work.rs` stores legacy snapshots/events with immutable-event triggers and ULID checks. It has no selected canonical-file/event/receipt commit path. | Store scheduling, retry, durability or identifier validation changes. |
| TAI-P5: typed assignment decoding after a JSON map is insufficient | **VERIFIED observation.** Native `org-proto/src/wire.rs::Request.params` is a `Value`; gateway `CommandEnvelope` deserializes `params` through `Value`/`from_value`. Duplicate keys can disappear before TA's strict visitor runs. | Raw-envelope decoding or command schema changes. |
| TAI-P6: authentication primitives exist but the production vertical path is incomplete | **VERIFIED observation.** Office host-channel authorization checks grants, channel/session/office/org and live owner state. The host control service dispatches hello/status; production web startup refuses to open a listener. A working production session/grant/browser composition is **UNVERIFIED**. | Implemented host operations, authenticated handoff and real production-path evidence. |
| TAI-P7: selected target facts need an authoritative consistent source | **VERIFIED gap in inspected path.** Legacy `OrgView` provides a per-attempt organization snapshot, but not the selected team/employee/status/home-team model and mappings required here. Proposed selected directory admission and generation barrier are **UNVERIFIED mechanisms**. | Directory importer, organization mutation ordering or fact-provider implementation. |
| TAI-P8: new codec 2 can preserve numeric tokens without rounding | **VERIFIED only for a bounded disposable corpus:** 12 valid cross-language outputs and 11 rejected malformed/duplicate inputs. Full bounded production validation remains **UNVERIFIED**. TH-P4 remains disproved for the previous numeric convention; schema-1 compatibility is not repaired. | Full codec/header/hash/budget vectors or retained-source checks contradict the mechanism. |
| TAI-P9: writer fields do not prove selected fencing | **VERIFIED limitation.** Legacy owner-office/generation fields and work-origin metadata exist; the inspected assignment/load path does not establish the proposed selected writer admission and handover protocol. That mechanism is **UNVERIFIED**. | Writer startup, restore, epoch or handover implementation. |
| TAI-P10: production UI is deliberately unavailable today | **VERIFIED observation.** `lib/work/index.ts` exports unavailable operations; capability gating prevents mock fallback. `TaskSheet.svelte` implements the legacy single-assignee interaction. Existing task browser journeys use the e2e mode. | Production feature adapter, capability negotiation or test-server composition changes. |
| TAI-P11: migration must supply genuinely missing facts rather than infer them | **VERIFIED requirement; concrete mappings UNVERIFIED.** Documents 06/08/12 require opaque IDs, provenance and preserved history. Legacy task/team/member/number fields do not establish every selected relationship. | A concrete import manifest, mapping rule or source dataset is introduced. |
| TAI-P12: journal commit ordinals and event-origin positions are distinct | **SOURCE-VERIFIED distinction:** legacy append allocates one position per reference and rejected/no-op receipts allocate none. **SELECTED mechanism; UNVERIFIED production correctness:** one immutable envelope per durable transaction, independent commit ordinal, frozen exact mapping to unchanged work origins. Amend document 16 as below; retain complete recovery and platform full-flush obligations. | Crash/reopen, full-flush, replay or publication checks contradict the mechanism. |
| TAI-P13: source-preserving snapshot edits retain unrelated bytes | **SOURCE-VERIFIED parser markers; UNVERIFIED production editor.** Pinned saphyr-parser 0.3.3 uses character offsets and half-open spans; quoted scalar spans include trailing trivia, requiring the bounded closing-quote lexer specified below. | Unicode/CRLF/comments/extension/body preservation or parser guard checks contradict the editing mechanism. |

TA-01 remains active for pure assignment semantics. TH-01 r2 remains active for coverage only. Their previous review acceptance does not extend to this vertical architecture. The lead owns shared contracts and documentation; resolving these mechanisms requires engineering and independent review, not approval from offline owners.

## Assignment application and public interfaces

Keep selected application policy in `org-work`, with a narrow selected command/read API and persistence/fact ports owned by that capability. The office adapter supplies trusted state and implements acceptance. Retain `plan_assignment` as the assignment decision inside that application path. Do not stretch legacy `StoredItem` into a selected task by adding one optional field: it lacks the required selected owner, organization identity, assignment pair and attribution model, and its storage assumes ULIDs.

Propose an additive typed operation named `task.set_assignment`, with a separately negotiated selected-assignment capability. Preserve `task.assign` and its existing wire semantics. The selected operation has target organization/task identity outside its command object; the command contains exactly `expected_revision`, `assigned_team_id` and `assignee_id`. Both nullable fields are mandatory. Operation identity and authenticated principal are envelope metadata, never task-controlled actor fields.

The HTTP surface gets an explicit assignment endpoint, task/history reads, authorized target choices and receipt lookup documented in the gateway contract. It is not a generic native-command tunnel. Add a gateway contract revision/capability before enabling it; current API/version types are not evidence that the endpoint ships. Use the same operation ID through browser, gateway, native command and receipt, including any documented mapping to the legacy envelope name `change_id`.

Two transport adaptations are necessary:

- Decode the new command from preserved raw JSON at both native and HTTP boundaries. Reject duplicate, missing and unknown keys before conversion to `Value`, including duplicate wrapper fields. Keep legacy decoding behavior compatible. Passing a map into TA's visitor cannot meet strictness.
- The domain supports positive signed-64-bit revisions; browser numbers cannot represent that entire range. Use canonical decimal strings for revisions in the new browser DTO and convert losslessly to native `i64`. Native TA commands and canonical events retain their selected integer types. Document and test the adapter, including the signed maximum and overflow. A lossless browser JSON-number implementation is an alternative requiring equivalent evidence; narrowing the domain to JavaScript-safe integers is not acceptable.

Preserve opaque task/team/employee IDs exactly. Existing gateway identifier restrictions and native ULID storage checks cannot become selected ID policy. Version the new route validators to admit selected IDs safely, including UUIDv7 IDs. Resolve IDs through trusted registries; never concatenate unchecked user strings into filesystem paths. Existing IDs incompatible with the documented literal filename layout need a reviewed reversible path-encoding contract or explicit staging, without renaming their identities.

Read DTOs expose accountable owner separately from both assignment fields, immutable reporter, exact revision and explicit history coverage. Queue filters distinguish owned backlog, assigned-team work and individual work. Return the committed event identity/revision and origin-office information needed for outcome reconciliation. Conflicts return an authorized current revision/snapshot without mutating either assignment field.

## Authentication, facts, identity and writer admission

Browser credentials terminate at the Rust browser service and host authentication boundary. Preserve host key custody and authenticated listener/channel handoff. Add a capability-scoped native-client entry point over a host-approved channel; do not reuse an agent binding or automatic launcher as human authorization. The web app receives neither office database keys nor unrestricted host authority.

Preserve the authenticated subject through to the accepting service. The current temporary app-member representation alone is insufficient for account-bound receipt replay. An owner event has `actor_kind: owner` and null employee ID; its stable account binding remains protected authentication metadata linked to the receipt. A managed employee retains its injected employee identity. Never manufacture an employee from an owner name, profile flag or browser body.

Proposed initial mutation policy is the live authenticated organization owner path, matching the available host authority guard. Other principals require an explicit selected-task mutation capability; receiving assignment, owning a team, leading a subtree or contributing to a project grants none implicitly. Managed agents without that capability are denied by the new operation and retain their separate legacy contract. This initial policy is a reviewable product choice, not a claim that document 12 selected a complete authorization matrix. Broader required actor access enlarges this boundary before implementation.

Each request checks channel binding, grant scope/expiry, session revocation and live authorization. Reads and receipt lookup have corresponding actor/scope checks. Authentication failures do not disclose task existence or another actor's receipt.

Admit selected organization/team/employee/project records into a validated directory generation. Resolve desired targets from one generation, with task state, project ownership and authorization facts held consistent through acceptance. Directory changes must pass the same ordering barrier or invalidate preparation before its commit. Separate lookups followed by a later write are insufficient. The old consistent organization snapshot is a useful pattern, not proof that the selected facts exist. Preserve inactive historical references; only requested non-null targets receive current active/same-org checks.

The accepting office verifies trusted organization/HQ identity, office identity, authority epoch, restore/checkpoint state and exclusive writer ownership. A local process lock prevents local overlap; it does not fence a copied root on another machine. Handover requires verified old-writer shutdown or a verified machine fence before authority advances. Missing authority evidence stops acceptance. Higher observed epochs fence stale writers; restored offices never promote themselves. Remote requests cannot nominate a writer or epoch. Assignment does not transfer authority.

## Recoverable acceptance and receipt protocol

Keep one office writer and one work-source allocation boundary. Add a dedicated acceptance job to the existing writer rather than placing filesystem effects in a rerunnable `TxnFn` or introducing a second task writer. Preparation may be retried while effect-free; once a durable envelope is published, only its frozen projection may be retried.

At this architectural level the acceptance sequence is:

1. Authenticate/admit the request, verify writer authority, and check the protected receipt for its operation ID and immutable actor/request binding.
2. A matching terminal receipt returns its original authorized outcome before current target/revision validation. Another actor, scope or fingerprint conflicts. An uncertain existing operation is reconciled before effects repeat.
3. For a new operation, read one validated task/directory generation; authorize; check exact revision; invoke the pure assignment decision. Equal-pair requests still validate desired targets and produce one revision/event. Preserve owner, project, canonical path, ID, number, reporter and creation time.
4. Freeze the complete resulting task snapshot, fully attributed event, protected receipt/outcome, source sequence/identity, authority evidence and projection mutations. A rejected admitted command can freeze a receipt-only outcome; it creates no task revision/event.
5. Publish that full transaction through the durable journal boundary. Then materialize canonical snapshot/event and protected receipt/result files and project SQL idempotently. After the journal commit, SQL or file failure cannot turn the operation into a definite rejection or rerun domain effects. Hold further affected work until recovery; a lost response is reconciled by the original operation ID.
6. Return completion only when durable source evidence is verified and the accepted revision is coherently available through the read path. Hourly Git publication remains asynchronous.

The receipt fingerprint includes versioned command kind, organization/task scope, expected revision and exact desired pair. Actor binding is separate and immutable. Stable owner-account identity permits authorized recovery after session renewal; session identity is not the business idempotency key. Keep completed/rejected outcomes stable and protected. Align admission/retention with the existing gateway's documented retry window, and persist expiry/tombstone evidence deliberately. Cache deletion must not roll sequence, receipt or admission high-water state backward.

### Immutable acceptance source and sequencing

The reviewer’s finding is confirmed. [origin.rs::append](../../crates/org-office/src/store/origin.rs#L189) rejects empty references and advances by `refs.len()`. [engine.rs](../../crates/org-work/src/engine.rs#L175) stores eligible rejected receipts without recording events; `write` also permits successful legacy no-ops without events, feed rows or positions.

**Select a distinct journal commit ordinal.** It orders durable acceptance transactions, including receipt-only transactions; it is neither another work-event allocator nor a replication position. Reusing `work_feed.seq` would have the same receipt-only gap.

The TAI-P12 distinction is source-verified; production acceptance/recovery remains unimplemented and unverified.

### Journal identity and chain

Amend document 16 to distinguish:

- **Work-origin stream:** existing `(office_id, origin_incarnation, origin_seq)` identities and event high-water.
- **Acceptance journal:** `(organization_id, office_id, journal_kind, journal_incarnation, commit_ordinal)`.

Use `journal_kind: work-acceptance` and this protected layout:

`recovery/offices/<office-id>/journals/work-acceptance/<journal-incarnation>/transactions/<commit-ordinal>.json`

Ordinals are canonical decimal positive signed-64-bit integers, beginning at 1. Each newly published frozen transaction consumes exactly one ordinal, regardless of event count. Read-only receipt replay and refusals that create no durable record consume neither ordinal nor event positions.

A separate immutable `anchor.json` establishes the journal’s initial state. It is not transaction zero. Transaction 1 references the anchor digest; subsequent transactions reference the preceding ordinal and complete envelope digest. Hash identity, predecessor, frozen mutations, receipts and origin mapping under the selected versioned journal codec.

Filename and envelope identity must agree. Conflicting duplicates, forks, missing predecessors or incompatible codecs stop recovery. Head files and SQL cursors remain projections.

### Frozen mapping to existing event origins

Each envelope records:

- Existing work-origin office/incarnation and high-water **before and after**.
- Ordered, exact allocated origin entries, including original sequence, target/generation/version, feed sequence, organization position and timestamp.
- Frozen snapshots/events, allocation mutations, protected receipts/results and their original client/feed positions.

For `n` origin entries and prior high-water `h`, require positions exactly `h+1 … h+n`, with `after=h+n`. For zero entries, require `after=before` and an empty mapping. Never call legacy `append` with empty references or manufacture an event for a receipt.

Preserve the actual `self.puts` allocation order; do not reconstruct it from sorted target identifiers. Task revisions, work-origin positions, client/feed positions and journal ordinals remain distinct.

The existing office writer prepares both counters and all mutations together. Publication of one frozen envelope makes that complete mapping authoritative. No participating legacy work mutation bypasses this boundary, and no parallel allocator is introduced. Existing replication continues consuming unchanged event origins; receipt-only transactions introduce no event-sequence holes.

### Initialization, restore and replay

Before installation, quiesce/fence writers and capture consistent retained counters, origin logs, snapshots, receipts and import evidence. Persist a journal incarnation and durable anchor referencing that evidence, active work incarnation and existing high-water. Start the journal ordinal at 1 without restarting the event allocator or retroactively inventing transaction identities. Missing historical evidence remains explicitly unavailable; the anchor cannot certify fabricated coverage. Adoption gates activate only after their required validation and durable installation.

Ordinary restart or projection rebuild continues the same journal incarnation and verified prefix. Recover the journal cursor and origin high-water from retained authoritative sources, not disposable SQL defaults.

Replay first validates journal continuity and each recorded before/after mapping against the preceding authoritative state. Project frozen mutations and both projection cursors atomically in SQL. Already applied transactions require matching identity/digest and are skipped; inconsistent projections require reconciliation. Never rerun domain decisions or allocate replacement event positions. Completion still requires coherent verified task/event/receipt reads.

A verified restore/cutover may establish a new journal incarnation only with retained predecessor-head identity/digest, imported watermark evidence and writer fencing. Missing continuity stops acceptance. Event-incarnation rotation follows the existing restore contract independently; journal rotation alone never rotates work or message origins. Retain old incarnations and watermarks.

### Concrete vectors

**1. One event, rejection, then multi-event legacy acceptance.** Starting from a verified anchor with work incarnation `I` and event high-water 40:

| Journal ordinal | Frozen outcome | Origin mapping | Event high-water |
| --- | --- | --- | --- |
| 1 | Selected assignment succeeds | `(I,41)` | 40 → 41 |
| 2 | Admitted stale-revision rejection; protected receipt only | Empty | 41 → 41 |
| 3 | Legacy command succeeds with three events | `(I,42)`, `(I,43)`, `(I,44)` | 41 → 44 |

The legacy targets are eligible non-adopted records. Transaction 3 retains their exact ordered target/version mappings. Replication sees contiguous events 41–44, while the journal has three transactions.

**2. Crash at transaction 3.** Before verified publication, no acceptance is established; uncertain publication requires scanning/reconciliation. After durable publication but before SQL/files finish, replay installs the frozen three events and original receipt exactly once, ending at journal ordinal 3/event high-water 44. It never calls the legacy decision again. An already projected transaction is verified and skipped.

**3. Exhaustion.** Check arithmetic before publication. At event high-water `i64::MAX−1`, a two-event proposal cannot allocate either event; if journal capacity remains, persist a zero-event exhaustion rejection. Event exhaustion does not prevent otherwise valid receipt-only transactions. Journal-ordinal exhaustion stops new durable transactions before effects; existing receipt lookups remain available. Never wrap, reset or automatically create an incarnation to escape exhaustion. Continuation requires the explicit verified continuity procedure above.


Publication retains the full-envelope guarantees: durable temporary-file flush, atomic non-overwriting installation and parent-directory durable flush, including ADR-0002’s macOS full-flush requirement. Required immutable blobs and the anchor become durable before a referencing transaction. Specify journal schema/codec independently of task events; use codec-2 normalization omitting only the designated top-level envelope checksum.

Preparation runs on the existing writer outside rerunnable callbacks. Freeze the complete outcome before irreversible publication. After publication retry only projection of frozen mutations, never domain effects. Completion requires verified contiguous journal/source evidence and coherent task/event/receipt reads. Post-commit projection failure means recovery/unresolved response, not rejection; Git publication remains asynchronous. Head/index files, SQL and any retained journal.jsonl export are derived. Preserve the separate message allocator and all high-water marks. Protected account-bound receipts, consistent directory generation and verified writer fencing remain required.

## Canonical task state and history

Selected public snapshots remain at document 03's canonical paths:

- Project task: `projects/<project-id>/work/tasks/<task-id>.md`.
- Projectless task: its owning team's nested team path followed by `work/tasks/<task-id>.md`.
- Event: `work-history/tasks/<task-id>/<revision>.json`.

Assignment changes neither location nor number. The selected directory retains home-team, project-owner and execution-placement facts separately. Raw replicated files are validated/imported inputs, not a second command authority.

Each accepted revision has a full event with fixed `task_kind: task`, organization/task/revision identity, schema, trusted actor/time, optional operation ID and document 05's complete old/new pair. Expand required null/default fields and compute the selected SHA-256 payload hash. Preserve current task body, fields and unmodified extensions without lossy parsing. `org-docs` offers strict bounded front-matter parsing, but its `serde_json::Value` representation does not establish arbitrary numeric or extension round-trip fidelity. Reuse its public parser only where suitable; any needed lossless extension belongs behind a reviewed compatible API, with original source retained.

Validate event schemas, actual hashes, filename/identity agreement, duplicates, contiguous coverage and final snapshot/tombstone revision before accepting an imported/rebuilt projection. Apply TH's coverage helper only after independent payload verification. Retain unknown future change tags and their content; show them as unknown changes. A migration baseline always leaves the earlier period unavailable, including a baseline at revision 1. Missing middle events require recovery, not an inferred baseline.

Keep events through closure/tombstoning and retain reporter, final revision and last assignment pair in tombstone metadata. Public history contains no private-note body, ID or author association, agent memory or credentials. Legacy shared `CommentKind::Note` remains shared; migration cannot retroactively make it private.

## Selected event schema and codec 2

Select an explicit new codec, rather than redefining schema 1. New task events use `schema_version: 2`, identifying **task-event JSON codec 2**. Task snapshots retain their independently specified schema version. SHA-256 and organization/task/revision event identity remain unchanged.

The normative codec-2 rules are:

1. Accept one complete JSON value, with only JSON whitespace surrounding it. Reject invalid UTF-8, a leading BOM, malformed syntax, trailing content and unpaired surrogates. Valid escaped surrogate pairs decode to their Unicode scalar.
2. Decode object keys before checking uniqueness. Reject duplicate decoded keys at every depth, including escaped equivalents. Sort keys lexicographically by Unicode scalar values, with shorter prefixes first. Apply no Unicode normalization. Preserve array order.
3. Emit compact UTF-8 with no separator whitespace or trailing newline. Strings and keys use `\"`, `\\`, and the short escapes `\b`, `\f`, `\n`, `\r`, `\t`. Other U+0000–U+001F characters use lowercase `\u00xx`. Emit every other scalar directly, including U+2028/U+2029 and supplementary characters. Do not escape `/` or apply HTML escaping.
4. Preserve each number’s exact original ASCII token matching:
   `-?(0|[1-9][0-9]*)(\.[0-9]+)?([eE][+-]?[0-9]+)?`.
   Preserve exponent case, signs, permitted zeros, mantissa digits and negative zero. Perform no floating-point conversion or numeric normalization. Emit booleans and null in lowercase.
5. Numerically equivalent spellings intentionally identify different hashed content: `1`, `1.0`, `1e0` and `1E+000` remain distinct. Replacing one at an accepted event identity conflicts.
6. Compute `payload_sha256` over the canonical UTF-8 event object with **only its top-level `payload_sha256` member omitted**. Nested members with that name remain included. The stored checksum is required and contains exactly 64 lowercase hexadecimal characters.

Before hashing, producers materialize the complete document-05 envelope: schema version, organization/task identity, fixed `task_kind: task`, revision, occurred time, actor kind/ID, operation ID, change object and source metadata. Optional actor/operation IDs are explicit nulls where applicable; absent producer-side source metadata becomes `{}`. Admitted event files must contain these materialized members; verification does not silently repair missing fields.

Headers receive typed validation independently of generic payload numbers. Revision must be a positive signed-64-bit integer token, without fraction or exponent, and agree with the filename; schema version is exactly integer `2`. Validate identity, attribution and timestamps under their owning contracts. New events have actual known attribution; authorized imports retain genuine unknown provenance. Assignment payloads require both nullable keys in both pairs.

Unknown change tags and extension members remain in the lossless representation and checksum. Retention does not authorize private-note, memory or credential leakage.

### Command identity is separate

Assignment commands remain strict typed complete-pair commands. Native revisions are positive `i64`; browser revisions use canonical decimal strings converted losslessly. Fractional/exponent revision spellings are invalid, even when mathematically integral.

Fingerprint the validated typed command using a versioned deterministic encoding of method, scope, task, integer revision and decoded nullable IDs. Do not fingerprint incoming lexical JSON or use event numeric-token identity as command identity. Preserve the independent immutable actor/account binding.

Thus equivalent string escapes yield the same command fingerprint; invalid revision spellings cannot bypass retries. Reusing an operation ID with changed typed intent or actor conflicts. Matching receipts return the original authorized outcome before fresh revision/target validation.

## Source, projection and compatibility amendments

Amend document 05, document 12’s import references, draft SQL/schema coverage and validators together. Version-aware projection must retain canonical change/source-metadata text and extensions losslessly. Generic `Value`, SQLite numeric extraction/reserialization and browser number parsing cannot regenerate authoritative payloads.

Retain schema-1 bytes, hashes and identities unchanged. Admission dispatches through explicitly supported schema/codec validators. Unsupported prior selected codecs remain retained/staged with explicit **unverified/unavailable** coverage; they cannot establish verified history or authorize adoption. Do not overwrite them, reinterpret them as codec 2, or fabricate a baseline concealing known missing history.

Legacy WorkEvents newly wrapped as selected schema-2 events preserve the entire original object and numeric tokens in `source_metadata.legacy_event`, including original change, attribution, `auto`, `cause` and `org_position`. Capture source tokens before lossy decoding. This wrapping never replaces an already accepted selected event at its identity.

## Versioned resource profile

Use **`selected-task-resources/1`**, with MiB meaning 1,048,576 bytes:

| Resource | Admission limit |
| --- | --- |
| Event input and canonical output | 16 MiB each |
| JSON container nesting | 64; root container counts as 1 |
| JSON nodes | 200,000, counting every value/container and object key |
| Individual decoded string/key or raw number token | 8 MiB |
| Journal manifest | 16 MiB; same nesting/node/token limits |
| Immutable blob/chunk | 64 MiB |
| Snapshot front matter | Existing 64 KiB, depth 16, 10,000 parser-event limits |
| Snapshot body | 8 MiB |

Add **64 MiB aggregate referenced blob bytes and 1,024 blob references per manifest**. Otherwise a bounded manifest could reference arbitrarily expensive payloads. Count repeated references toward the aggregate; verify actual lengths and hashes. Larger imports remain intact in staging.

Enforce budgets during scanning/emission, before excessive allocation or commit, using checked counters. Return structured resource errors without payload disclosure, truncation or partial admission. These limits constrain bytes/work, not numeric magnitude.

Record the profile identifier in events, manifests and adoption metadata; do not add a seventh mutable snapshot property. Recovery retains the recorded profile and its decoder. A newer deployment must not silently apply smaller limits to acknowledged records; unsupported profiles require explicit recovery handling.

**Transport adjustment:** native `MAX_FRAME_BYTES` and HTTP `MAX_BUSINESS_BODY_BYTES` are both **1 MiB** (`org-proto/src/lib.rs`, `agentic-org-web/src/body.rs`). Retain those limits. Use small completion DTOs, byte-bounded history pages and authenticated content-reference chunk operations carrying at most **256 KiB raw bytes** per request/response; enforce the 1 MiB fully encoded limit, including envelopes/base64. Preserve existing queue/time bounds. Large imports and reads therefore require this transport capability rather than larger legacy frames.

## Source-preserving snapshot editing

Use an additive source-preserving API in [org-docs](../../crates/org-docs/src/lib.rs#L207). Its syntax machinery remains domain-independent; the selected adapter permits changes only to:

`revision`, `assigned_team_id`, `assignee_id`, `updated_at`, `updated_by_kind`, `updated_by_id`.

Retain the complete original Markdown bytes, front-matter offsets, decoded root keys and scalar source spans. Reuse existing duplicate-key, alias/anchor/tag/merge, root-mapping, numeric and structural guards. Never serialize the complete `Value` tree.

The admitted editing representation is a block-root mapping whose six mutable entries are present, simple single-line entries with nonempty scalar tokens. Plain, single-quoted and double-quoted values are supported. Stage flow-root mappings, implicit empty values, explicit complex-key entries, multiline mutable scalars and other ambiguous representations. Nested extension structures remain permissible under existing guards.

**Marker conversion is established by source:** pinned `saphyr-parser 0.3.3` defines `Marker.index()` in characters and `Span` as inclusive-start/exclusive-end. Its scanner advances indices per character, including both CRLF characters. Build a character-index-to-UTF-8-byte-boundary table over the exact front-matter substring passed to the parser, then add its original document byte offset. Preserve BOM, fences and original line endings.

**One necessary correction:** `scanner.rs::scan_flow_scalar` consumes trailing whitespace/comments before recording its end marker. Replacing an entire quoted-scalar event span could therefore delete comments. Within that parser-identified span, use a bounded single-line quote lexer—handling doubled single quotes and double-quote escapes—to find the actual closing quote. Preserve the suffix unchanged. Plain scalar spans use the scanner’s recorded content end. Any inconsistent or ambiguous boundary stages the representation.

Replace only the six value-token ranges, using decimal revision, literal null and YAML-safe quoted strings with escaped line breaks. Reconstruct from unchanged source slices, then reparse under the same guards and validate selected facts.

Acceptance requires intended values to decode correctly and **every nonmutable slice, including body bytes, to remain identical**. Unknown extension number spellings therefore survive even when the parser’s temporary `Value` approximation would differ. Existing out-of-range/unsafe parser rejections remain staged.

## Exact snapshots in events and recovery

Schema-2 created/baseline events retain typed known facts plus an explicitly specified exact public-Markdown representation: inline string when budgets permit, otherwise a checksum-addressed immutable blob descriptor containing encoding, length, digest and profile.

This avoids lossy YAML-extension conversion. A body at 8 MiB plus front matter already exceeds the single-string limit; JSON escaping can also exceed the event-output limit. Blob selection must account for both.

Public snapshot blobs belong to the document-05 history contract and publish with their referencing events. Journal payload blobs remain protected operational sources. Required blobs become durable before manifest publication. Admit only verified public snapshot content; private or unclassified source remains protected staging. Preserve original legacy sources independently.


## Migration and legacy preservation

Adoption is explicit, staged and recoverable. Inventory original task snapshots, events, IDs, number namespaces, project/team relationships and unmapped features; preserve them before installing selected state. A reviewed manifest supplies missing accountable owner, project ownership, employee identity/home team and scope mappings from actual evidence. Do not reinterpret the legacy optional task team automatically as either selected owner or assigned team, choose the first membership, or convert an owner pseudo-recipient into an invented employee.

Keep original task IDs, event revisions/order, timestamps and actors. Map bug/request records to selected task types with `task_kind: task`. For imported events retain the entire original `WorkEvent`, including `auto`, `cause` and `org_position`, in `source_metadata.legacy_event`, with its original change. Historical individual assignments do not imply a historical team. Set current assigned team to null only where the source lacks an explicit assigned-team fact, as document 12 requires.

Preserve genuine unknown provenance. A snapshot-only source receives an explicit baseline at its existing revision and import time. Partial surviving history is retained and reported incomplete; do not conceal gaps. Unmapped assignments, workflows, fields or relationships remain staged and visible as migration work, without resetting or discarding them.

Readable-number mapping must follow document 06. Do not strip B/REQ prefixes, merge conflicting counters, renumber records or allocate a new number during assignment. Ambiguous mappings/collisions block adoption of the affected record while independent records can proceed.

Install a durable adoption registry and format/version gate atomically with imported state. Fence every legacy write path capable of touching adopted IDs, including cascades, Undo and replication imports. Non-adopted legacy commands keep their contract. An adopted task accessed through an incompatible legacy mutator gets an explicit migration/version result, not silently changed semantics. Stop/fence or upgrade other known writers before adoption; a local registry cannot control an old remote binary. Retain original legacy data for recovery and inspection. Messaging cutover remains separate: no installed CLI replacement or live spool reset is part of this feature.

## Startup and actual adoption path

Implement startup prerequisites within the delivery boundary:

- Fresh installation uses actual protected HQ bootstrap: establish the owner account in host-private storage, create/register HQ identity and protected key custody, register/start the service, and complete authenticated worker/browser handoff. Bootstrap credentials and outcomes never enter the work journal. Expose assignment capability only after recovery and authorization readiness.
- Existing instances use a host-owned, owner-authenticated admission/adoption operation exposed through the human interface. Verify registered instance/HQ identity, account authority, keys, writer fencing and retained source high-water; stage a mapping manifest and preserved sources; validate directory/task/history; then atomically install adoption gates and recoverable imported state. Missing authority enters protected recovery/bootstrap, never inferred authorization from imported profile flags.
- Reject incompatible legacy writes to adopted IDs. Preserve unmapped records and unsupported history in staging. Do not reset source state, fabricate capabilities or require unrelated task creation.

Retain r1’s initial owner mutation policy and distinct agent identities; assignment itself grants no capability. No absent owner or external approval is a prerequisite.


## Browser and UI responsibility

Keep static SvelteKit deployment and Rust server authority. Build a selected task-assignment feature module with an injected production transport, scoped to account, instance, organization and task. Do not turn the existing global legacy `workApi` into a mock-backed selected service.

Reuse the generic operation client's immutable-intent/operation-ID behavior where appropriate: a possible dispatch becomes unresolved and is reconciled by lookup, never resubmitted under a fresh ID. The legacy sender currently creates a new ID per invocation and is unsuitable for that recovery path. Retain only non-secret operation identity needed for resume, not a browser command outbox.

The sheet displays accountable owner, assigned team and individual separately. Each clear action submits the full desired pair. Active choices come from authorized server facts; historical/inactive labels remain visible. Browser validation improves feedback; the writer remains authoritative.

Provide explicit loading, submitting, unresolved, conflict, rejected/error and completed states. Preserve the user's pending selection during a conflict and require an explicit new intent against the refreshed revision. Do not optimistically present an unresolved assignment as accepted. Scope changes invalidate stale responses and cannot attach an old result to a different task/account. History distinguishes loading/error, baseline/earlier-unavailable and unknown future changes. Attribution shows recorded reporter/actors rather than treating the latest assignee as the historical fixer.

Use accessible labels, keyboard operation, focus/error/status announcements and responsive layouts. Tailwind Plus-derived UI stays inside `apps/agentic-org-ui`. Existing legacy e2e tests and production-unavailable checks remain useful but cannot prove this production vertical flow.

## Dependency direction and coupling

Source direction remains apps composing libraries: selected policy/ports in `org-work`; storage/journal/directory adapters in `org-office`; typed public DTOs in `org-proto`; scoped transport in `org-client`; protected host composition in `org-host`; browser adaptation in `agentic-org-web`. The browser never imports native storage, and the web app does not depend on the office app. No domain-to-office/host dependency or app-to-app source edge is introduced.

Measured graph: all workspace packages, distinct normal project-package dependencies including target-specific normal dependencies; excludes third-party, standard-library, dev/build and runtime communication edges. Proposed changes below are estimates, not implemented measurements.

| Unit | Current Ca/Ce/I | Proposed Ca/Ce/I | Consequence |
| --- | --- | --- | --- |
| org-proto | 11 / 0 / 0 | unchanged | High consumer blast radius; additive versioned contracts and unchanged legacy decoding. |
| org-work | 2 / 1 / 0.33 | unchanged | No persistence/host dependency; ports and policy stay cohesive. |
| org-office | 1 / 5 / 0.83 | 1 / 6 / 0.86 | Proposed dependency on public `org-docs` parser API; Ce 6 needs review. |
| org-docs | 1 / 0 / 0 | 2 / 0 / 0 | Parser remains domain-independent; compatibility matters to org-process. |
| org-client | 1 / 3 / 0.75 | 2 / 3 / 0.60 | New web consumer of a capability-scoped transport API. |
| org-host | 1 / 4 / 0.80 | unchanged | Authentication/handoff stays in the existing host library. |
| agentic-org-web | 0 / 2 / 1 | 0 / 5 / 1 | org-client plus private org-socket/org-office-launch transport edges; no storage implementation dependency. |
| agentic-org-office | 0 / 7 / 1 | unchanged | Existing composition root remains at Ce 7; avoid further convenience dependencies. |

Revision 5 focused amendment: the web composition privately consumes `org-socket` framing/descriptor-liveness primitives and `org-office-launch` bounded descriptor-transfer contracts for host-control browser sessions. `org-client` remains the approved office RPC adapter. Moving this browser-session control into org-client now would add a public session API and a new client-to-host-control edge without an actual shared consumer. The host still owns password verification, sessions and grant mint/revocation; web receives no office/CA keys or ambient host-root authority. Existing exact serde 1.0.229, zeroize 1.9.1 and hmac 0.12.1 are direct web dependencies for typed ingress, controlled secret buffers and maintained proof verification, not a new crypto protocol/version. No path-based office-socket fallback is authorized. All deadlines, strict framing, revocation/cleanup and actual service acceptance remain obligatory.

Lead checked the actual normal-project-package graph after the amendment: web dependencies are org-client, org-host-control, org-office-launch, org-proto and org-socket; no source cycle. org-socket Ca changes 3→4, Ce remains0; org-office-launch Ca4→5, Ce remains1 (I0.20→0.1667). These are bounded observed counts for this checkpoint, separate from external crypto dependencies and runtime channels. Other table rows retain the prior architecture estimates. Evidence: `.agent-kit/logs/v1-assignment-integration/gateway-dependency-review/source-graph.json`. This assessment does not infer behavior from imports or compile readiness.

The new parser edge avoids duplicating a YAML parser; lossless suitability must still be demonstrated. The new client edge avoids a second native protocol implementation. Runtime browser-to-office interaction is measured separately from these source edges. No new package is justified merely to improve scores. Additional dependencies, parser incompatibility or a stability inversion return this decision to review.

## Foundation sequence, alternatives and verification

Architectural dependency order, without work dispatch:

1. Amend owning contracts to the selected codec 2, resource profile, exact snapshots/public blobs, raw-preserving command envelopes, browser revisions/ID handling, initial authorization and immutable protected journal layout, following independent architecture READY on this revision.
2. Establish usable host authentication/handoff and selected directory/writer admission. Include protected HQ bootstrap/service registration if the intended delivery must support a fresh installation.
3. Implement complete codecs, explicit import/adoption, one durable work-source allocator, frozen journal acceptance, receipts, replay and canonical/SQL projection. Preserve participating legacy semantics and source evidence.
4. Connect selected assignment and authorized reads/lookup through typed native and gateway interfaces.
5. Connect the production UI and prove the complete flow and failure boundaries. Intermediate foundations are checkpoints, not selected-assignment acceptance.

Rejected alternatives are wiring legacy assignment as selected v1; trusting UI target facts; treating draft SQL or coverage headers as durable validation; appending a journal inside a retryable closure; accepting with SQL followed by independent file writes; relying on Git publication for ACK; running parallel source allocators; and enabling production mocks. Each violates an identified premise. A separate selected service/database would add writer, identity and migration coordination without removing the same durable-commit obligation; the existing office writer is the smaller boundary.

Concrete feasibility and falsification checks for implementation:

- Codec-2 vectors must agree across the explicit reference and Rust for large integers beyond u64 and JavaScript precision, fractional/exponent forms, negative zero, Unicode, null/default expansion and nested unknown fields. Retained known events must verify unchanged. Failure reopens TAI-P8 rather than weakening inputs.
- Exercise source-preserving edits with Unicode before spans, CRLF/BOM, escaped/quoted keys, trailing comments, nested same-name keys, numeric extension spellings and body lookalikes. Assert exact untouched slices and reparsed intended facts. Unsupported representations remain staged.
- Exercise strict raw native/HTTP envelopes, duplicate keys at every relevant level, missing/null/type/unknown fields, opaque IDs and full signed revision boundaries. Test all sixteen current/desired assignment-state combinations, equal-pair events and independent clearing.
- Race assignment with another task change and with target deactivation/directory replacement. Require exact conflicts or one consistent accepted decision, with no owner/reporter/scope/number/body changes. Historical inactive references and cross-home-team desired assignments remain valid under their respective rules.
- Retry after later task changes, target deactivation, response loss and restart. Obtain the original authorized outcome without an extra revision/event. Another account, scope or fingerprint cannot replay or inspect it. Test revoked/expired/wrong-channel grants and denied agent capabilities.
- Inject failures around envelope write/flush/publication/directory flush, SQL commit, task/event/receipt materialization, read publication and response. Reopen/rebuild disposable projections from published organization data plus unpublished journals and protected receipts; recover the same accepted outcome. In-process fault tests alone do not prove power-loss durability; verify the platform flush contract separately.
- Challenge concurrent writers, copied roots, old epochs, missing checkpoints and interrupted restore. No duplicate accepted task/revision or reused source identity is allowed. Legacy and selected work must share the reviewed allocation boundary without disturbing message watermarks.
- Use migration fixtures containing number collisions, owner pseudo-recipients, missing ownership/home-team facts, unknown attribution, full legacy events, partial history, baselines, tombstones and unselected workflow data. Adoption either preserves verified mappings or stages the record; old writers cannot mutate adopted IDs.
- Run the built static UI against the real Rust gateway/host/office path in temporary roots with test credentials. Verify all four assignment states, queues, history, conflicts, unresolved lookup/reload, authorization, keyboard/accessibility and UI evidence. Vite e2e mocks or a generic gateway harness alone do not qualify.

After architecture review and implementation, run required Rust formatting/clippy/workspace tests, app type/tests/build and relevant browser checks, plus `.agent-kit/runtime/kit_hooks.py check`. Preserve original-author intent validation, fresh local review/fixes and separate final review of the exact commit. None of those execution checks was run during this design-only investigation.

Rollback before adoption can disable the capability. After accepted selected writes, pause mutation and retain compatible reads, canonical files, journals, receipts, adoption gates and watermarks. Do not restart an old writer against adopted data or restore an earlier checkpoint over acknowledged changes. Use compatible replay/forward repair, or a separately reviewed reverse migration preserving later events and outcomes. Executable-only rollback is insufficient after the durable format changes.

## Architecture review history

- Revision 3: fresh read-only Sol Xhigh session `01a1dfeb-1e44-78d3-b1f9-a75054e8092d` returned **CHANGES_REQUIRED** for TAI-P12: one envelope cannot reuse event positions for zero/one/multi-event transactions. Actual model/effort were independently observed by the lead from matching turn metadata. No application code was accepted.
- Revision 4: original author `01a1dfb4-1d26-7100-9ea4-72b53756ef23` confirmed the source finding and separated immutable journal commit ordinals from preserved legacy event origins. The vectors above resolve the mapping and retain all other valid requirements. The same independent reviewer returned **READY**, with no remaining findings. Architecture permits detailed planning; it is not final code or v1 acceptance.
- Disposable evidence: codec probe matched 12 valid outputs and rejected 11 malformed/duplicate inputs; snapshot span probe edited/reparsed six fixtures and rejected five unsupported/invalid forms. These bounded corpora establish no full production codec, editor, authorization or durability acceptance.

Review evidence: `.agent-kit/logs/v1-assignment-integration/architecture-review/recheck/result.json`; reviewed proposal SHA-256 `e69d4ce9f75078556af3f1b0706d017bc2016ee68c5ca66afda7efba99ffe681`. The archived proposal preserves the exact reviewed bytes before this status/history update.

- Revision 5: original fullstack author reported the actual additional private web socket/launch and existing-version serde/HMAC/zeroize edges. Lead preserved P1–P13, reviewed actual manifests/lock and proposed the bounded composition amendment. Fresh read-only Sol/Xhigh-assigned architecture reviewer `01a1e0d9-3260-7db2-ac08-9c7dc609c2ba` returned **READY** with no findings; actual model/effort was not independently attested by its environment. Exact reviewed amendment is archived as `gateway-dependency-review/proposal-r5.md`, SHA-256 `c18047d6bc2cb87d7a6222e0f07c61c0b068917f17c03096d5b1ad104cb53323`; base r4 bytes/hash and result are retained beside it. Lead activated r5. This is architecture only; code, original-author/final review and production evidence remain open.
