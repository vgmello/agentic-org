# Errors, optimistic concurrency, and felt safety for agents

Reference for the [principal AX designer](SKILL.md). Grounded in `doc-28` §2.3–2.7,
`crates/org-proto/src/error.rs`, `crates/org-command/src/{lib,envelope,office,shape}.rs`,
`crates/org-agent-cli/src/{attach,office,dispatch}.rs`, `ADR-0014`, `ADR-0020`, `ADR-0029`,
`ADR-0031`, `TASK-85.3`, and `TASK-90`.

## 1. One deterministic envelope and three exit codes

Every `agent-os` invocation communicates outcome through two orthogonal channels that must
never disagree:

| Exit code | `ok` | Meaning | Office contacted? |
| --- | --- | --- | --- |
| `0` | `true` | Command completed or reached a valid waiting/async `meta.status` (or `--help` / `--schema` printed) | Yes (except `--help`, `--schema`, `--version`) |
| `1` | `false` | Office refusal, authorization failure, conflict, timeout, transport error, or output schema violation | Attempted or failed at runtime |
| `2` (`EXIT_USAGE`) | `false` | CLI syntax/parse error (unknown flag, positional arg, missing required flag, oversized/invalid input, conflicting flag+JSON) | **No** (nor is `KeySource` called) |

### Strict `envelope_schema()` conformance everywhere

Every JSON output (success or failure) MUST validate against `org_command::envelope_schema()`
(`crates/org-command/src/envelope.rs`):
- Exact top-level keys in fixed order: `ok`, `data`, `meta`, `error` (`additionalProperties: false`).
- On `ok: true`: `meta.status` is required; `error` is `null`.
- On `ok: false`: `data` is `null`; `error` is an object with required `code`, `message`,
  `hint`, plus optional `retry_with` (and `conflict` when `TASK-90` lands bounded
  `ConflictDetail`).
- **No bypasses in `session attach` (`TASK-85.3`).** Pre-dispatch parse/bind error envelopes
  in `crates/org-agent-cli/src/attach.rs` must include `"data": null` and `"meta": {}` via
  the shared envelope builder—never hand-rolled partial JSON missing `data` or `meta`.
- **Failures ignore `--format compact` and `--format yaml`:** every error prints one line of
  JSON envelope so scripts and agents parse failures with one rule.

## 2. Closed `ErrorCode` taxonomy, actionable `hint`, and `retry_with`

`crates/org-proto/src/error.rs` defines the closed lower-case `snake_case` `ErrorCode` enum
shared by `agent-os`, the office wire protocol, and the browser API. Agents branch on
`error.code`; never invent ad-hoc error strings outside `ErrorCode`.

### Rules for `error.message`, `error.hint`, and `error.retry_with`

| Code | What `hint` must tell the agent | What `retry_with` carries |
| --- | --- | --- |
| `invalid_request` | Exact valid flags, enum values, limits, or conflicting input fields (`flags_hint` / `resources_hint` / `verbs_hint`) | Absent (or corrected flag when unambiguous) |
| `not_authorized` | Plain statement of refusal and who may perform the action (e.g., `"You don't have access to hire. Ask your team lead."`, `ADR-0029`) | **Always absent** (retrying cannot succeed) |
| `not_bound` | Binding missing/expired; run inside an office-started session with `AGENT_OS_SESSION_TOKEN` | Always absent |
| `not_found` | Which ID/key/reference was not found and which `list` command discovers valid ones | Always absent |
| `conflict` | Whether to retry with the new revision or inspect changed sections first (`TASK-90`) | `{"if-match": <current_revision>}` |
| `checklist_incomplete` | Names the unchecked required steps and whether to complete them or supply closure justification | Absent or required flag name |
| `operation_reused` | States that this `--idempotency-key` was already used with different input or principal; mint a new key | Always absent (never suggest reusing the same key) |
| `too_large` | Exact byte/character limit (`1 MiB` input, `64 KiB` YAML, etc.) and how to trim or split | Always absent |
| `rate_limited` | Delay window (`retry_after_ms`) before retrying | Absent (or timing guidance) |
| `unavailable` / `not_ready` | Wait briefly and repeat once with the exact same idempotency key; if it fails twice, record on the task and move on (`INSTR-15`) | `{"idempotency-key": "<same_key>"}` (injected automatically by `org-command::run` for writes!) |
| `unsupported` | This office build does not serve the command/flag yet; do not retry | Always absent |

**Anti-pattern to kill:** Generic hints like `"Retry."`, `"Check input."`, or `"Error"` fail
Law 4. Every refusal handler must emit a concrete next action or an explicit stop signal.
Never leak host filesystem paths outside the caller's view, SQL statements, or stack traces
into `message` or `hint`.

## 3. Optimistic concurrency (`--if-match`) and smart bounded `ConflictDetail`

When multiple agents or people collaborate on a task or structure record, stale writes are
normal—not exceptional. Poor conflict responses cause the worst context-window waste and
data-loss loops in multi-agent systems.

### Contract (`doc-28` §2.7, `CLI-6`, `TASK-90`)

- Revisioned write commands declare `IfMatch::Required` (or `IfMatch::Optional` where only
  certain sub-modes mutate an existing record).
- Ordinal checklist mutations (`checklist check|edit|remove|move` with 1-based `--section`
  and `--index`) **must require `--if-match <expected_revision>`** so concurrent inserts,
  reorders, or removals can never target the wrong step. Stable UUIDv7 `--step-id` remains
  valid without positional drift.
- When `expected_revision < current_revision`, the office returns `ErrorCode::Conflict`
  carrying:
  1. **`retry_with: {"if-match": <current_revision>}`** (standard `CLI-6` contract).
  2. **Bounded hybrid `ConflictDetail` (`error.conflict`):**
     - `current_revision` and `expected_revision`
     - `events_since`: enriched deltas (`task_updated`, `checklist_step_*`, status/assignment
       changes) capped strictly at **`<= 20` events** AND **`<= 8 KiB` serialized JSON**
     - `events_truncated` (boolean) and `omitted_events` (count) when the cap trips
     - `changed_sections`: compact list of affected areas (e.g., `["checklist:work"]`,
       `["labels"]`, `["body_markdown"]`)
     - `reread_recommended` (boolean): `false` when the intervening changes did not touch
       what the caller is mutating and events are untruncated; `true` when overlapping
       sections changed or events were truncated
     - `current_summary`: compact metadata snapshot (status, revision, assignee, checklist
       counts)—**never inlining full `body_markdown` or comment bodies**
- **Why this creates felt safety:** If agent A added a label while agent B checked an
  acceptance step, agent B sees `changed_sections: ["labels"]`, `reread_recommended: false`,
  and `retry_with: {"if-match": 5}`, and retries cleanly in one call without pulling 15 KiB
  of task markdown back into context. If both edited the same checklist section,
  `reread_recommended: true` tells agent B to run `checklist list --task <ref>` before
  retrying—preventing blind clobbering.

## 4. Idempotency keys and acknowledgement classes (`ADR-0014`)

- **Auto-minted keys (`org-command/src/lib.rs`):** For every write where `!dry_run` and no
  `--idempotency-key` was passed, `org-command` mints one via `KeySource` (UUIDv7 in
  `org-agent-cli`) after CLI parsing succeeds and before calling `Office`. Both success and
  failure envelopes report `meta.operation_id`.
- **Safe replay (`CLI-5`):** Same caller + same key + same input returns the original answer
  without re-applying the mutation. Same key + different input fails with `operation_reused`.
  Passing `--dry-run` with `--idempotency-key` fails locally with `invalid_request` (`CLI-4`).
- **Truthful durability (`Ack::Committed` vs `Ack::Durable`):**
  - `Ack::Committed`: accepted into the office's serialized writer/journal fast path
    (messaging, status line, task comments, checklist checks, watches; single-digit ms).
  - `Ack::Durable`: flushed to disk (durable-class mutations, or any write called with
    `--durable`).
  - `meta.ack` always states truthfully which class was achieved. Never mark an unpersisted
    or dry-run operation as committed (`--dry-run` emits `meta.dry_run: true`, mints no IDs
    or revisions, and undoes projection effects cleanly).

## 5. Async waits, `meta.status`, and clean suspension (`ADR-0020`)

No `agent-os` command may hold its caller longer than the short hold (`90 s` unless
configured; `non-negotiable 18`). Long waits return `ok: true` with an explicit `meta.status`:

| `meta.status` | Meaning | Agent action |
| --- | --- | --- |
| `completed` | Finished (every read, every completed write, answered ask) | Continue |
| `waiting` | Short hold elapsed; ask still open (`data` gives ask ID & `remaining_ms`) | Do other unblocked work, or call `msg wait --ask <id>` |
| `timed_out` | Non-required ask expired; `data` includes defaults if provided | Proceed with defaults or without answer; record on task |
| `withdrawn` | Ask was withdrawn | Proceed without it |
| `suspended` | Required ask still unanswered after wait; office will pause and resume this session with the answer | Record progress on task, wind down cleanly, end turn (`INSTR-2`) |
| `gate_required` | Reserved for future explicit approvals (never returned by `hr hire` in v1, `ADR-0029`) | Continue other work; outcome arrives as message |

**Status alignment rule (`TASK-85.3`):** Never let `data.status` contradict or shadow
`meta.status` (e.g., `data.status: "required_pending"` while `meta.status: "completed"`).
Keep `meta.status` authoritative (`suspended`, `waiting`, `timed_out`, `withdrawn`,
`completed`) so agents branch on one field across every command.

## 6. Fail-closed `ADR-0031` security ordering and zero silent drops

1. **Authorization runs first—always.** On every office call, verify the run binding and
   caller authority (`require_active_employee`, scope/team/hire checks) **before** looking
   up the record, validating `--if-match`, or assembling `ConflictDetail`. An unauthorized
   caller MUST receive `not_authorized` with zero existence oracle and zero conflict
   metadata (`TASK-90` AC #4).
2. **Private task notes never leak.** Under `ADR-0031` (`TASK-50`), private task notes are
   visible only to their author and authorized people—never to another agent. Private notes
   and their metadata MUST NEVER appear in `events_since`, `task history`, `ConflictDetail`,
   or shared Git exports.
3. **Output contract guard (`shape::conform`).** `org-command` strips undeclared fields
   (and `testkit` fails builds on undeclared fields so the guard is never abused) and fails
   closed with `unavailable` if the office omits a required field or returns the wrong type—
   explicitly warning on writes that the mutation may have already committed and reporting
   `meta.operation_id` so the agent inspects before retrying.
4. **Never leak secrets in errors or debug logs.** `Binding::Debug` redacts
   `AGENT_OS_SESSION_TOKEN` (`crates/org-agent-cli/src/office.rs`); error messages and
   hints never echo tokens, keys, or wrapped credential payloads.
