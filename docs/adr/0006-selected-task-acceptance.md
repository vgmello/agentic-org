# ADR-0006 · Task changes are accepted through one recoverable office journal

Status: accepted. Implementation pending. The data contract owns the record shapes:
[document 03](../design/organization-data/03-features-and-work.md) the task file,
[document 05](../design/organization-data/05-operational-data.md#task-event-codec) the
event codec and resource profile, [document 12](../design/organization-data/12-task-assignment-and-history.md)
assignment, and [document 16](../design/organization-data/16-recovery-and-office-continuity.md)
the office's acceptance journal. The [office store design](../design/office-store.md)
and [ADR-0021](0021-office-store-architecture.md) own how the journal is written. This record owns how a task change travels from a
person or agent to a durable, recoverable outcome, using assignment as the first
complete path. How fast each operation is acknowledged is owned by
[ADR-0014](0014-acknowledgement-classes-and-agent-os-latency.md).

## Decision and delivery boundary

Assignment is delivered end to end on real tasks: accepting office, authenticated
native transport, Rust web process and static Svelte UI. The delivery includes task
reads, authorized target choices, ownership and assignment queues, durable
operation lookup, and task history sufficient to use and verify assignment. It
supports every task type and projectless tasks.

Every other change to a task uses the same acceptance path or returns an explicit
unsupported result. No change bypasses task history.

The delivery includes the foundations it needs: trusted directory facts, writer
admission and fencing, a complete event codec and validator, recoverable
task/event/receipt commits, and production authentication and transport. A pure
helper, a draft-schema change or a UI wired to an older call does not satisfy it.

The HQ office is the accepting writer for shared tasks. Other offices forward to it
or report pending or unavailable; they never accept competing revisions. A task
created in another office is accepted by that office under the same rules. Fresh
installation uses the protected HQ setup and service registration; test-only
database seeding never stands in for onboarding.

## Assignment application and public interfaces

TA-01, the pure assignment decision of document 12, stays the decision inside this
path. Policy and its persistence and fact ports live in `org-work`; the office
adapter supplies trusted state and implements acceptance.

The typed operation is `task.set_assignment`. It has target organization and task
identity outside its command object; the command contains exactly
`expected_revision`, `assigned_team_id` and `assignee_id`. Both nullable fields are
mandatory. Operation identity and the authenticated principal are envelope
metadata, never task-controlled actor fields.

The HTTP surface has an explicit assignment endpoint, task and history reads,
authorized target choices and receipt lookup, documented in the
[web API](../design/web-api-v1.md). It is not a generic native-command tunnel. The
same operation ID travels through browser, web, native command and receipt.

Two transport rules apply:

- The command is decoded from preserved raw JSON at both native and HTTP
  boundaries. Duplicate, missing and unknown keys are rejected before conversion to
  a generic value, including duplicate wrapper fields.
- Revisions are positive signed 64-bit integers. The browser DTO carries them as
  canonical decimal strings and the web process converts them losslessly to `i64`.
  Native commands and canonical events keep integer types. The signed maximum and
  overflow are tested. Narrowing the domain to JavaScript-safe integers is not
  acceptable.

Task, team and employee IDs are UUIDv7 and are preserved exactly. They are
resolved through trusted registries; unchecked strings are never concatenated into
filesystem paths.

Read DTOs expose the accountable owner separately from both assignment fields,
the immutable reporter, the exact revision and explicit history coverage. Queue
filters distinguish owned backlog, assigned-team work and individual work. The
response returns the committed event identity and revision and the origin office
needed for outcome reconciliation. A conflict returns an authorized current
revision and snapshot without changing either assignment field.

## Authentication, facts and writer admission

Browser credentials end at the web process and the host's authentication boundary.
Each person's request reaches the office with that person's principal and a
host-issued grant ([ADR-0012](0012-office-channel-lifecycle.md)). An agent binding
is never used as human authorization. The web process holds no office keys and no
host authority.

The authenticated subject is preserved to the accepting service. An owner event
has `actor_kind: owner` and a null employee ID; its stable account binding is
protected authentication metadata linked to the receipt. A managed employee keeps
its injected employee identity. An employee is never manufactured from an owner
name, a profile field or a browser body.

The initial mutation policy is the live authenticated organization owner. Other
principals need an explicit task-mutation capability; receiving assignment, owning
a team, leading a subtree or contributing to a project grants none implicitly.
Each request checks channel binding, grant scope and expiry, session revocation and
live authorization. Reads and receipt lookup have matching actor and scope checks.
Authentication failures do not disclose whether a task exists or another actor's
receipt.

Organization, team, employee and project records are admitted into a validated
directory generation. Desired targets are resolved from one generation, and task
state, project ownership and authorization facts stay consistent through
acceptance. Directory changes pass the same ordering barrier or invalidate a
preparation before its commit. Inactive historical references are preserved; only
requested non-null targets get current active and same-organization checks.

The accepting office verifies the organization and HQ identity, its own office
identity, the authority epoch, restore and checkpoint state and exclusive writer
ownership. A local process lock prevents local overlap; it does not fence a copied
root on another machine. Handover requires a verified shutdown of the old writer or
a verified machine fence before authority advances. Missing authority evidence
stops acceptance. Higher observed epochs fence stale writers; a restored office
never promotes itself. A remote request cannot nominate a writer or an epoch.
Assignment does not transfer authority.

## Recoverable acceptance and receipts

There is one office writer and one work-source allocation boundary. Acceptance is a
dedicated job on that writer, not a filesystem effect inside a rerunnable
transaction callback, and not a second task writer. Preparation may be retried
while it has no effect; once its journal entry is appended, only that entry's files
and projection may be replayed.

1. Authenticate and admit the request, verify writer authority, and check the
   protected receipt for its operation ID and immutable actor and request binding.
2. A matching terminal receipt returns its original authorized outcome before any
   current target or revision validation. Another actor, scope or fingerprint
   conflicts. An uncertain existing operation is reconciled before effects repeat.
3. For a new operation, read one validated task and directory generation,
   authorize, check the exact revision, and run the assignment decision. A request
   for the current pair still validates its targets and produces one revision and
   event. Owner, project, canonical path, ID, reference, reporter and creation time
   are unchanged.
4. Freeze the outcome as file changes: the complete resulting task file, the fully
   attributed event file and the protected receipt, with the source positions taken.
   The entry never holds projection rows; the projection is derived from those files.
   An admitted command that is rejected freezes a receipt-only entry and creates no
   task revision or event.
5. Append that entry to the office's acceptance journal and project it into SQL in the
   same writer batch. The task, event and receipt files are written after the flush
   that covers the entry. After the append, an SQL or file failure cannot turn the
   operation into a rejection or rerun its domain effects. Further affected work
   waits for recovery; a lost response is reconciled by the original operation ID.
6. Report completion only when the journal entry is committed in the operation's
   class and the accepted revision is readable through the read path. Git
   publication is asynchronous.

The receipt fingerprint covers the command kind and version, organization and task
scope, expected revision and the exact desired pair (document 12, fingerprint
version 1). Actor binding is separate and immutable. A stable owner-account
identity permits authorized recovery after session renewal; the session is not the
idempotency key. Completed and rejected outcomes stay stable and protected.
Admission and retention match the web process's documented retry window, and
expiry evidence is kept deliberately. Deleting a cache never rolls sequence,
receipt or admission high-water state backward.

Committed-class task activity (`work.comment`, `work.watch`, `checklist.check` and
task creation) uses the same path and is acknowledged once its one atomic operation
is committed, with the flush following within the bounded interval of ADR-0014.
Its entry is appended without a flush; the group flush that answers a durable
operation covers every earlier entry, and recovery truncates a torn tail only above
the head the host custody last confirmed. Assignment, status, ownership and every other change are
durable-class.

### Journal commit ordinals are separate from event positions

Document 16 defines the acceptance journal, its layout and its anchor. These
rules apply:

- A journal ordinal orders accepted entries, including receipt-only ones. It is neither a work-event allocator nor a replication
  position. Each published transaction consumes exactly one ordinal, whatever its
  event count. Read-only receipt replay and refusals that create no durable record
  consume neither an ordinal nor event positions.
- Each entry records the work-origin office, incarnation and high-water before
  and after, and the exact ordered origin entries it allocated. For `n` entries
  after high-water `h`, positions are exactly `h+1 … h+n`; for zero entries,
  `after = before`. No event is manufactured for a receipt.
- The office writer prepares both counters and all changes together. Task
  revisions, work-origin positions, client and feed positions and journal ordinals
  stay distinct. Replication consumes event origins unchanged; receipt-only
  transactions leave no event-sequence holes. The message allocator is separate.

A new office's journal starts at ordinal 1 from an anchor written at creation. A
restart or projection rebuild continues the same journal incarnation and verified
prefix, and recovers the cursor and origin high-water from the durable sources,
never from SQL defaults. Replay validates journal continuity and each recorded
mapping, projects frozen changes and both cursors atomically, skips an already
applied transaction whose identity and digest match, and never reruns a domain
decision or allocates a replacement position. A verified restore may start a new
journal incarnation only with the predecessor head's identity and digest, watermark
evidence and writer fencing. Journal rotation never rotates work or message
origins.

**Example.** From an anchor with work incarnation `I` and event high-water 40:

| Journal ordinal | Frozen outcome | Origin mapping | Event high-water |
| --- | --- | --- | --- |
| 1 | Assignment succeeds | `(I,41)` | 40 → 41 |
| 2 | Admitted stale-revision rejection; protected receipt only | Empty | 41 → 41 |
| 3 | A change that produces three events | `(I,42)`, `(I,43)`, `(I,44)` | 41 → 44 |

A crash before transaction 3 is published establishes nothing; an uncertain
publication is reconciled by scanning. A crash after publication but before files
and SQL finish replays the three frozen events and the original receipt exactly
once, ending at ordinal 3 and high-water 44.

**Exhaustion.** Arithmetic is checked before publication. When event positions are
exhausted, a zero-event exhaustion rejection is still recorded if journal capacity
remains. When journal ordinals are exhausted, new durable transactions stop before
any effect and receipt lookups stay available. Nothing wraps, resets or
automatically creates an incarnation.

Publication appends the entry to the journal's current segment; one group flush,
the macOS full flush, makes every entry up to it durable. A failed flush is fatal and
never retried in the same process. Immutable blobs and the anchor are durable before
any entry that references them. Heads, checkpoints, SQL and the canonical files are
derived from the entries.

## Canonical task state, events and snapshots

Task files and event files are at document 03's canonical paths. Assignment
changes neither location nor reference. Replicated files are validated inputs, not
a second command authority.

Each accepted revision has a full event as document 05 defines it: one codec
(`canonical-json/2`), the complete old and new pair for assignment, a trusted actor
and time, the operation ID, and the SHA-256 payload hash. The current task body,
fields and unmodified extensions are preserved without lossy parsing. A rebuilt
projection is accepted only after validating event schemas, hashes,
filename/identity agreement, duplicates, contiguous coverage and the final snapshot
or tombstone revision. Unknown change tags are kept and shown as unknown changes.
Missing events require recovery; no baseline is inferred. Events are kept through
closure and tombstoning. Public history contains no private-note body, ID or author,
agent memory or credentials.

The assignment command fingerprint is computed over the validated typed command,
never over the incoming JSON text: equivalent string escapes give the same
fingerprint, and fractional or exponent spellings of a revision are invalid.

Resource limits are the `selected-task-resources/1` profile, defined across [document 03](../design/organization-data/03-features-and-work.md) (task snapshots), [document 05](../design/organization-data/05-operational-data.md) (events and public blobs) and [document 16](../design/organization-data/16-recovery-and-office-continuity.md) (journal manifests). Native
`MAX_FRAME_BYTES` and HTTP `MAX_BUSINESS_BODY_BYTES` stay at 1 MiB; large history
reads and content use byte-bounded pages and authenticated content-reference
chunks of at most 256 KiB raw bytes per request or response.

### Source-preserving snapshot editing

`org-docs` provides an additive source-preserving editor. The task adapter may
change only `revision`, `assigned_team_id`, `assignee_id`, `updated_at`,
`updated_by_kind` and `updated_by_id`. It keeps the complete original Markdown
bytes, front-matter offsets, decoded root keys and scalar source spans, reuses the
existing duplicate-key, alias, anchor, tag, merge, root-mapping, numeric and
structural guards, and never serializes the whole value tree.

The editable representation is a block-root mapping whose six mutable entries are
present simple single-line entries with nonempty scalar tokens: plain,
single-quoted or double-quoted. Flow-root mappings, implicit empty values, complex
keys, multiline mutable scalars and other ambiguous forms are refused. Parser
markers count characters with half-open spans; the editor maps them to UTF-8 byte
offsets over the exact front-matter substring and preserves BOM, fences and line
endings. Because the parser's quoted-scalar span includes trailing whitespace and
comments, a bounded single-line quote lexer finds the actual closing quote and the
suffix is kept. Only the six value tokens are replaced; the result is reparsed
under the same guards. Every slice outside them, including the body, stays byte
identical.

Created and baseline events carry the exact public Markdown, inline when the
budget allows and otherwise as a checksum-addressed public blob published with its
event (document 05). Protected journal blobs are separate.

## Browser and UI

The UI is a task-assignment feature module with an injected production transport,
scoped to account, instance, organization and task. A possible dispatch that did
not answer becomes unresolved and is reconciled by lookup, never resubmitted under
a fresh operation ID. Only the non-secret operation identity is kept for resume.

The task sheet shows accountable owner, assigned team and individual separately.
Each clear action submits the full desired pair. Active choices come from
authorized server facts; historical and inactive labels stay visible. The writer is
authoritative over browser validation. The UI has loading, submitting, unresolved,
conflict, rejected or error, and completed states. A conflict keeps the person's
pending selection and requires an explicit new intent against the refreshed
revision. An unresolved assignment is never shown as accepted. A change of scope
discards stale responses. History distinguishes loading or error,
earlier-unavailable and unknown changes, and shows recorded reporter and actors.
Controls have accessible labels, keyboard operation and announced focus, error and
status. Tailwind Plus-derived UI stays inside `apps/agentic-org-ui`.

## Dependency direction

Policy and ports are in `org-work`; storage, journal and directory adapters in
`org-office`; typed public DTOs in `org-proto`; scoped transport in `org-client`;
host composition in `org-host`; browser adaptation in `agentic-org-web`. The
browser never imports native storage, and the web app does not depend on the office
app. No domain-to-office or domain-to-host dependency and no app-to-app source edge
exists. The web process uses `org-socket` framing and `org-office-launch` descriptor
transfer for host-control browser sessions privately; `org-client` is the office
RPC adapter. The host owns password verification, sessions and grant minting and
revocation.

## Rejected alternatives

Treating draft SQL or coverage headers as durable validation; appending a journal
inside a retryable closure; accepting with SQL followed by independent file
writes; relying on Git publication for acknowledgement; parallel source
allocators; production mocks; trusting target facts from the UI; and a separate
task service or database, which would add writer and identity coordination
without removing the durable-commit obligation.

## Acceptance

- Codec vectors agree across an explicit reference and Rust for integers beyond
  u64 and JavaScript precision, fractional and exponent forms, negative zero,
  Unicode, null and default expansion and nested unknown fields.
- Source-preserving edits handle Unicode before spans, CRLF and BOM, escaped and
  quoted keys, trailing comments, nested same-name keys, numeric extension
  spellings and body lookalikes; untouched slices are exact and the intended facts
  reparse. Unsupported forms are refused.
- Strict raw native and HTTP envelopes: duplicate keys at every level,
  missing/null/type/unknown fields, UUIDv7 IDs and the full signed revision range.
  All sixteen current/desired assignment-state combinations, equal-pair events and
  independent clearing are tested.
- Assignment raced with another task change and with target deactivation or
  directory replacement gives an exact conflict or one consistent accepted decision,
  with no change to owner, reporter, scope, reference or body.
- A retry after later changes, deactivation, response loss and restart obtains the
  original authorized outcome without an extra revision or event. Another account,
  scope or fingerprint can neither replay nor inspect it. Revoked, expired and
  wrong-channel grants and agents without the capability are denied.
- Failures injected around the journal append, the group flush, the SQL commit,
  file materialization, read publication and response recover the same accepted
  outcome after reopening, from organization data plus the retained journal and
  protected receipts. The platform flush contract is verified separately from
  in-process fault tests.
- Concurrent writers, copied roots, old epochs, missing checkpoints and an
  interrupted restore never yield a duplicate accepted task or revision or a reused
  source identity, and do not disturb message watermarks.
- The built static UI runs against the real web, host and office path in temporary
  roots with test credentials: all four assignment states, queues, history,
  conflicts, unresolved lookup and reload, authorization, keyboard and
  accessibility, with UI evidence. Mock-backed browser tests do not qualify.

## Consequences

- One writer and one journal per office carry every task change, so every accepted
  change is recoverable and replayable without rerunning decisions.
- After the first accepted write, the durable format is fixed for that data root:
  an older executable is never started against it and an earlier checkpoint is
  never restored over acknowledged changes. Because nothing has shipped, the format
  may still change before v1 by replacing it, not by adapting data.
