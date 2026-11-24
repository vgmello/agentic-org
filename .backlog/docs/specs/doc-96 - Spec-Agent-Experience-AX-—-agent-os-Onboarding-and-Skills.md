---
id: doc-96
title: 'Spec: Agent Experience (AX) — agent-os, Onboarding, and Skills'
type: specification
created_date: '2026-11-23T03:48:00Z'
updated_date: '2026-11-24T11:34:00Z'
tags:
  - spec
  - ax
  - agents
  - cli
  - skills
  - v1
---
> [!NOTE] Authoritative specification (2026-11-23)
> Establishes **Agent Experience (AX)** as a first-class product discipline parallel to browser UX ([`doc-39`](doc-39%20-%20Spec-App-Shell-and-Navigation-UX.md)).
> - **Authority chain:** Organization Data contracts ([`organization-data README`](../organization-data/doc-72%20-%20Organization-Data-—-README.md)) and ADRs ([`.backlog/decisions/`](../../decisions)) win on records, fields, storage, and security; this spec together with [`doc-27`](../design/doc-27%20-%20Design-Agent-Instructions-Assembly.md), [`doc-28`](../design/doc-28%20-%20Design-agent-os-Command-Line.md), and [`doc-59`](doc-59%20-%20REQ-08-agent-os-CLI-and-Kernel-Primitives.md) governs agent-facing interaction standards and outranks the `principal-ax-designer` persona and project skill.
> - **Delivery tracking:** AX discipline and persona (`TASK-85.1`), Day-1 onboarding, prefix-cached standing instructions, and phase-gated `agent-os` skills (`TASK-85.2`), CLI AX upgrades and multi-runtime AX evals (`TASK-85.3`), and surgical task CLI, server-side `--fields` pruning, and bounded `ConflictDetail` (`TASK-90`).

# Spec · Agent Experience (AX): `agent-os`, onboarding, and skills

Agentic Org serves two first-class users: **people** in the browser (governed by UX specs anchored on [`doc-39`](doc-39%20-%20Spec-App-Shell-and-Navigation-UX.md)) and **headless AI agents** operating through `agent-os` (governed by this specification). Every agent-facing surface—command-line grammar and output formats, launch catalogues, standing instructions, Day-1 orientation, on-demand skills, error and concurrency recovery, and cross-runtime execution—must meet the standards and measurable quality gates below.

## 0. Scope, authority, and the AX vertical

AX governs every surface a managed agent perceives or invokes inside an office-started session:

- **Command-line interface (`agent-os`):** grammar, flags, input bounds, deterministic answer envelopes, `--help`/`--schema`, output pruning, surgical patching, and `session attach` batching ([`doc-28`](../design/doc-28%20-%20Design-agent-os-Command-Line.md), [`doc-59`](doc-59%20-%20REQ-08-agent-os-CLI-and-Kernel-Primitives.md)).
- **Standing instructions & launch catalogue:** prompt assembly order, prefix-cache stability, 10 KiB base-prompt cap, 4 KiB launch-catalogue cap, resolved identity/reporting chain, and `TurnTrigger` untrusted-data fencing ([`doc-27`](../design/doc-27%20-%20Design-Agent-Instructions-Assembly.md)).
- **Day-1 agent onboarding & on-demand skills:** cold-start turn-1 orientation and phase-gated built-in skills served through `agent-os skill list|read` ([`doc-34`](../design/doc-34%20-%20Design-Organization-Processes-and-Standards.md) decision 4, [`doc-32`](../design/doc-32%20-%20Design-Development-Processes-Process-Packages.md) §6).
- **Error recovery & felt safety:** closed `ErrorCode` taxonomy, actionable `error.hint` and `error.retry_with`, bounded `ConflictDetail` deltas on stale `--if-match`, idempotency replay, truthful `Ack` classes, and fail-closed `ADR-0031` authorization ordering.
- **Multi-runtime portability & empirical evals:** uniform headless behavior across `claude -p`, `codex`, and `agy` on macOS and Linux, proved by automated contract checks (`CLI-1`–`CLI-15`), instruction scenarios (`INSTR-1`–`INSTR-23`), and measurable AX metrics.

**What AX never overrides:**

- AX adds no record, field, or storage rule outside the owning [organization-data contracts](../organization-data/doc-72%20-%20Organization-Data-—-README.md).
- AX never widens authorization or substitutes CLI validation for office enforcement ([non-negotiable 9](../doc-7%20-%20Architecture-Non-Negotiables-and-Who-Decides-What.md), [`ADR-0029`](../../decisions/decision-29%20-%20ADR-0029-Only-someone-who-may-hire-can-hire-there-is-no-approval.md), [`ADR-0031`](../../decisions/decision-31%20-%20ADR-0031-Any-employee-changes-tasks-a-private-note-is-read-by-its-author-and-by-people.md)).
- AX follows [Architecture Rules](../doc-8%20-%20Architecture-Rules.md) and [Architecture Non-Negotiables](../doc-7%20-%20Architecture-Non-Negotiables-and-Who-Decides-What.md): logic lives in libraries (`org-command`, `org-agent-cli`, `org-office-sessions`, `org-process`), binaries (`apps/agent-os`) stay thin, and planned runtime/OS variation sits behind consumer-owned adapters.

## 1. Discovery tiers and context-window budgets

Context-window bytes are paid on every turn by every running agent. Every capability, rule, and schema must be placed in the lowest-overhead tier that still prevents cold-start guesswork:

| Tier | Surface | Hard budget / bound | What belongs here | What is prohibited here |
| --- | --- | --- | --- | --- |
| **1. Standing base prompt** | Built-in instructions (`instructions.rs`) | **`<= 10,240` UTF-8 bytes** (`BUILTIN_INSTRUCTIONS_MAX_BYTES`, ~2,500 tokens) | Agent situation (turns, no terminal watcher, ephemeral memory), authority precedence & untrusted-data fencing, core `agent-os` mental model (`resource verb --flag`, JSON envelope, `--help`/`--schema`, `skill list\|read`), stop & cost discipline, amended `ADR-0025` sub-agent lifetime | Worked multi-step recipes, full schemas, role/team/project specifics, vendor-specific instructions, unbuilt commands |
| **2. Launch catalogue** | Generated from live `CommandTable` (`help.rs`) | **`<= 4,096` UTF-8 bytes** (`CATALOGUE_LIMIT`, ~1,000 tokens) | Complete capability map naming every active resource and verb so agents know what exists without guessing; scales via progressive resource-level grouping as domains grow | JSON Schemas, cryptic truncated fragments, unbuilt rows, OS-dependent ordering |
| **3. On-demand skills** | `agent-os skill list` & `skill read --name <key>` (`org-process`) | ~1 focused page per skill | Day-1 basics, phase-gated task guides (`tasks-create`, `tasks-execute`, `tasks-finalize`), domain workflows (messaging/asks, channels, code/worktrees, hiring/directory, processes/standards, budgets, errors/recovery, batch/attach) | Stale commands or flags, JSON field names posed as CLI flags, conflicting overrides of must-follow org standards |
| **4. Just-in-time introspection** | `agent-os [--help \| <res> --help \| <res> <verb> --help \| --schema]` | Local-only, zero office round-trip | Exact flags, requiredness, enums, `content_field`, common options, concrete `Command.example` usage line, draft 2020-12 input/output JSON Schema, declared `errors`, `ack` class | Silent omission of examples, office calls, binding requirement |
| **5. Omitted / prohibited** | — | `0` bytes | Nothing exposed for unbuilt features, internal wire-only names, raw SQL/storage paths, or MCP wrappers for product primitives (`CLI-12`) | MCP tools duplicating `agent-os` primitives; Claude-only slash commands or `.claude/` assumptions |

Additional hard I/O bounds (`crates/org-command/src/parse.rs`):
- `--input-file`, `--stdin`, and `--json`: `<= 1,048,576` bytes (`MAX_INPUT_BYTES`, 1 MiB); `Sources` reads at most `max_bytes + 1`.
- `--input-format yaml`: `<= 65,536` bytes (`MAX_YAML_INPUT_BYTES`, 64 KiB).
- List pagination: default `20`, valid range `1..=200` (`DEFAULT_LIMIT`, `MAX_LIMIT`).
- Overridable organization processes/standards injection: capped at `24 KiB` with truncation notice (`FC-112`, `doc-34` §3.2); mandatory (`overridable: false`) documents are included in full and fail closed on mandatory budget overflow.

## 2. Command-line ergonomics and token-saving reads

### 2.1 Single-table grammar and self-discovery

- Every command follows `agent-os <resource> <verb> [--flag <value>]...` with **zero positional arguments** and lower-case `--kebab-case` flags derived mechanically from `Field.name` (`doc-28` §2.1).
- Standard verbs (`list`, `inspect`, `create`, `update`) are reused consistently across resources; domain-specific verbs are added only when those four cannot express the action.
- Every command is declared once in `CommandTable` (`crates/org-command/src/table.rs`). Parser, three-tier `--help`, `--schema`, launch catalogue, and `org_command::testkit` contract checks derive from that table alone.
- `<resource> <verb> --help` MUST render a concrete, contract-validated example derived from `Command.example.input` so agents can invoke complex commands on the first try (`TASK-85.3`).
- Explicit ergonomic aliases (such as `who` → `directory list` and `task note` → `task-note`) MUST normalize identically across one-shot CLI execution, `--help`, `--schema`, and `session attach` (`TASK-85.3`).
- Text-heavy commands declare exactly one `content_field` accepting raw UTF-8 via `--input-file <path>` or `--stdin` with zero shell escaping (`CLI-7`); structured inputs use `--json` or `--input-format json|yaml`. No command ever prompts or reads stdin unless `--stdin` is passed (`CLI-8`).

### 2.2 Token-saving reads, pruning, and surgical mutations

- **Formats:** `--format json` (default, single-line deterministic envelope), `--format yaml` (envelope with block scalars for readable prose), and `--format compact` (`data` only on success—tab-separated rows for lists, `name: value` for records—appending a final `meta: <one-line-json>` line only when `meta.status != completed` or `meta.next_cursor` is present; failures ALWAYS print the JSON envelope).
- **Client- and server-side `--fields` pruning (`TASK-90`):** `org-command` projects requested fields client-side (`shape::project`) AND passes requested top-level field roots on `org_command::Request` so heavy composite reads (`tasks.inspect`) skip loading unrequested sections (`body_markdown`, `comments`, `watchers`, `dependencies`, `history`) server-side while `shape::conform` validates returned sections strictly.
- **Query sizing and high-signal filters (`TASK-85.3`):** High-volume list commands (`task list`) expose `total` and `--count` so agents check scope size before pulling large result sets into context, plus targeted filters (`--exclude-status`, `--unassigned`, `--modified-file <substring>`, `--labels`, `--query`, `--sort`) that eliminate client-side scanning loops.
- **Surgical updates without whole-document re-reads (`TASK-90`):**
  - `task update` supports field-level `PatchField` patching and `--append-body` (mutually exclusive with full `--body` replacement) alongside additive/subtractive list flags (`--add-labels`, `--remove-labels`, `--add-modified-files`, etc.).
  - `checklist list --task <ref> [--section acceptance|dod|work|all] [--unchecked]` fetches only needed checklist steps.
  - `checklist add` supports `--section` and repeatable `--text`; `checklist check|edit|remove|move` support 1-based `--section`/`--index` (including batch check) **only when `--if-match <revision>` is supplied**, alongside stable UUIDv7 `--step-id`. Mutations return compact receipts (mutated step + per-section progress counts) rather than full task dumps.

### 2.3 High-frequency batching (`session attach`) and latency

- `agent-os session attach` (`doc-28` §2.9, `CLI-13`, `ADR-0014` decision 5) binds once to the office socket, reads one JSON object per line of stdin (`{"command":"...","input":{...},"options":{...}}`), and emits one single-line JSON envelope per line of stdout. A failing line emits a schema-conformant error envelope and never stops subsequent lines.
- Common commands (`doc-28` §2.11, [non-negotiable 18](../doc-7%20-%20Architecture-Non-Negotiables-and-Who-Decides-What.md)) answer in single-digit milliseconds one-shot and hold the `ADR-0014` target (`1 ms` median / `2 ms` p95 on the reference Mac) inside `session attach`. No common command loads schemas, reads extra files, or performs unnecessary round-trips.

## 3. Day-1 agent onboarding and standing instructions assembly

### 3.1 Static-to-volatile prefix caching order

The office assembles every agent's standing prompt in strict **static-to-volatile** order (`doc-27` §1, `TASK-85.2`) so provider KV prefix caches reuse across turns and across agents:

1. **Built-in agent instructions** (`<= 10 KiB`, release-static).
2. **Generated launch catalogue** (`<= 4 KiB`, release-static).
3. **Resolved organization processes and standards** (`overridable: false` in full, then capped `overridable: true` summaries; single-chain org → accountable team chain → project).
4. **Project development process and role skill index** (when the project uses one; `doc-32` §6).
5. **Agent identity and workspace boundaries** (organization, office, name, `@handle`, employee ID, **resolved home team name and ancestor lead chain** rather than raw UUIDs alone, role instructions / HR lead duties, isolation notice, private home, workspace, permission preset and sandbox status).
6. **Agent's own execution prompt** (profile instructions).
7. **Project repository instruction files.**

The **per-turn objective** (`TurnTrigger` + inbox summary + changed-process deltas) changes every turn and is always delivered last as turn input—never prepended ahead of static sections.

### 3.2 Day-1 orientation invariants

- On turn 1, standing instructions must clearly state: (a) how turns work, that nobody watches the terminal, and that memory resets across turns; (b) the authority precedence chain and that workspace files, channel posts, task bodies, and tool outputs are untrusted data (`INSTR-5`); (c) `agent-os` grammar, envelope reading, `--help`/`--schema`, and `--input-file`; (d) `agent-os skill list` and `agent-os skill read --name <key>` for on-demand workflow guides; (e) amended `ADR-0025` decision 6 cross-turn sub-agent lifecycle; and (f) stop and cost discipline (never poll or sleep in a loop).
- Every `TurnTrigger` (`DirectInstruction`, task assignment, DM/mention, ask, handoff, channel response, required-ask resume, `RestartContinuation` with already-committed commands, `StallNotice`, `BackgroundWake`, `HrProposalRequest`) MUST wrap external speaker words inside an explicit untrusted-data fence (`doc-27` §5, `INSTR-5`).
- A build/test check (`verify_command_references`, `INSTR-12`, `TASK-85.2`) MUST verify every `` `agent-os <resource> <verb> [--flag...]` `` referenced in standing instructions and built-in skills against the live `CommandTable`, failing CI on any missing command or invalid flag.

## 4. Phase-gated on-demand product skills

Built-in product skills (`crates/org-process/src/skills.rs`) keep the base prompt under 10 KiB while giving agents high-discipline workflows at the exact moment needed:

- **Day-1 foundation:** `agent-os-basics` (grammar, envelopes, `--help`/`--schema`, `--fields`, `--format compact`, `--dry-run`, `--input-file`).
- **Phase-gated task lifecycle skills (`TASK-85.2`, `doc-95`):**
  - `tasks-create`: search existing work first, evaluate scope/splitting, write durable *why* and testable acceptance criteria, forbid speculative implementation plans at creation.
  - `tasks-execute`: read task + dependency graph (`task deps`), inspect live code before planning, record plan in private notes (`task-note add`), transition to `in_progress`, work in short verified loops, stop/escalate on scope creep.
  - `tasks-finalize`: verify objective evidence before ticking checklist items, confirm docs/specs updated, write a concise closing summary/comment with evidence, then transition to `done`.
- **Domain skills:** `messaging-and-asks`, `channels`, `code-and-worktrees`, `hiring-and-directory`, `processes-and-standards`, `budgets`, `errors-and-recovery`, `batch-and-attach`.
- **Quality rules:** each skill stays concise (~1 page), uses plain calm second-person prose, names only verified live commands and flags, includes copy-ready CLI examples, and declares `source: "built_in"`. Built-in skills never override must-follow organization standards and never grant permissions (`doc-32` §6).

## 5. Errors, optimistic concurrency, and felt safety for agents

### 5.1 Deterministic envelopes, exit codes, and hints

- Every JSON output—including pre-dispatch errors in `agent-os session attach` (`crates/org-agent-cli/src/attach.rs`, `TASK-85.3`)—MUST validate strictly against `org_command::envelope_schema()` (`{ok, data, meta, error}`).
- Exit codes are fixed and unambiguous: `0` when `ok: true` (or `--help`/`--schema`/`--version`), `1` when `ok: false` from office/runtime refusal or schema mismatch, `2` (`EXIT_USAGE`) when the CLI line itself failed parsing before contacting the office or calling `KeySource`.
- `error.code` uses the closed lower-case `org_proto::ErrorCode` enum. `error.message` states what failed in one sentence without leaking SQL, stack traces, or out-of-scope paths. `error.hint` states the exact next action or stop condition (never bare `"Retry."`). `error.retry_with` carries the exact flag map to change on retry (e.g., `{"if-match": <rev>}`, or auto-injected `{"idempotency-key": "<key>"}` on `unavailable`/`not_ready`), and is absent when retrying cannot succeed (`not_authorized`, `unsupported`, `operation_reused`).
- `meta.status` (`completed`, `waiting`, `timed_out`, `withdrawn`, `suspended`, `gate_required`) is authoritative across all commands; `msg ask` and `msg wait` MUST NOT contradict `meta.status` with drifted `data.status` names (`TASK-85.3`).

### 5.2 Smart bounded `ConflictDetail` and guarded ordinal mutations (`TASK-90`)

- Stale `expected_revision < current_revision` on revisioned task or ordinal-checklist writes returns `ErrorCode::Conflict` with BOTH:
  - `error.retry_with: {"if-match": <current_revision>}` (`CLI-6`); and
  - bounded hybrid `ConflictDetail`: `events_since` capped at **`<= 20` events** and **`<= 8 KiB` serialized JSON**, `events_truncated`, `omitted_events`, `changed_sections`, `reread_recommended`, and compact `current_summary` (never inlining `body_markdown` or comment bodies).
- Enriched `task_updated` and `checklist_step_*` deltas allow an agent to distinguish non-overlapping edits (`reread_recommended: false` → safe to retry with updated `--if-match`) from overlapping edits or truncated deltas (`reread_recommended: true` → re-inspect affected section first).
- Ordinal checklist mutations (`--section`/`--index`) MUST refuse execution unless `--if-match` is provided; UUIDv7 `--step-id` addressing remains valid without positional drift.

### 5.3 Idempotency, durability, fail-closed security, and zero silent drops

- Non-dry-run writes automatically receive a UUIDv7 `--idempotency-key` from `KeySource` when omitted and report it in `meta.operation_id` on both success and failure (`CLI-5`). Repeating the same key with different input fails with `operation_reused`. `--dry-run` + `--idempotency-key` fails with `invalid_request` (`CLI-4`).
- `meta.ack` reports `committed` vs `durable` truthfully (`ADR-0014`).
- **Fail-closed `ADR-0031` ordering (`TASK-90`):** office authorization (`require_active_employee` and scope/team checks) MUST run before entity lookup and before constructing `ConflictDetail`. Unauthorized callers receive `not_authorized` with zero existence oracle and zero conflict metadata. Private task notes MUST NEVER appear in `events_since`, task history, or conflict summaries.
- **Fail-closed `--dry-run` (`TASK-95`):** every `Kind::Write` command MUST either forward `dry_run: true` to a server-side rollback check or refuse with `ErrorCode::Unsupported` before invoking any office RPC (`msg`, `channel`, `status`, `task`, `checklist`, `task-note`, `repo`, `worktree`, `hr`, `process`), never performing a live mutation while stamping `meta.dry_run: true`.
- **Zero silent drops:** duplicate keys in `--json`/YAML and conflicting flag + `--json` values fail with `invalid_request`; `shape::conform` preserves all declared output fields (including `process inspect --why` `chain`/`winner` and `--offset` chunking fields `total_bytes`/`next_offset`/`parent`, `TASK-95`) and rejects missing required fields or wrong types with `unavailable`, `meta.operation_id`, and an explicit write-uncertainty warning (`crates/org-command/src/lib.rs`).

## 6. Runtime and model portability

- `agent-os` runs identically across all three v1 native runtime adapters (`FC-101`):
  - **Claude Code** (`crates/org-agent-runtime-claude`, `claude -p --input-format stream-json --output-format stream-json`)
  - **Codex** (`crates/org-agent-runtime-codex`, `codex app-server --stdio`)
  - **Antigravity** (`crates/org-agent-runtime-agy`, `agy --input-format stream-json --output-format stream-json`)
- **Zero MCP product primitives (`CLI-12`):** launch MCP config is empty; no product capability is exposed as an MCP tool.
- **Zero vendor-specific prompt assumptions:** built-in instructions and product skills never reference Claude slash commands, `.claude/` files, or single-vendor features. Transport quirks (such as Codex `shell_environment_policy` preserving baseline POSIX `PATH`/`HOME`/`TMPDIR`/`LANG`/`TERM` alongside `ctx.office_env_names` while filtering ambient secrets, Claude `--system-prompt-snapshot`, or Agy U+2028/U+2029 line splitting) and runtime-neutral `ToolCapability::{Read, Search, Edit, Shell, Subagent}` translations (`cap:*` presets, safe word-bounded `CommandPrefix("agent-os")` with shell-metacharacter fail-closed rejection in `ReadOnly`/`WorkspaceEdits`, and normalized sub-agent lifecycle events, `TASK-94`) are isolated inside `crates/org-agent-runtime*` and `crates/org-office-sessions`.
- **Cross-OS determinism (`CLI-11`):** launch catalogue, `--help`, and `--schema` outputs are byte-identical on macOS and Linux.
- **Model portability (`doc-27` §8):** behavioral evals must pass not only on flagship models, but also on the cost-effective models organizations run in production.

## 7. AX acceptance criteria and measurable quality gates (`AX`)

**Discovery and token budgets**
- **AX-1** · Built-in standing agent instructions (`BUILTIN_INSTRUCTIONS_MAX_BYTES`) stay `<= 10,240` UTF-8 bytes (`INSTR-12`), enforced by automated tests.
- **AX-2** · The generated launch catalogue stays `<= 4,096` UTF-8 bytes (`CATALOGUE_LIMIT`, `CLI-11`) with headroom as new domains (`doc`, `feature`, `milestone`, `release`, `draft`, `search`) land, using progressive resource-level discovery rather than silently dropping resources or truncating to cryptic fragments.
- **AX-3** · All four discovery tiers exist locally with zero MCP product primitives (`CLI-12`): standing prompt, launch catalogue, on-demand `agent-os skill list|read`, and offline `--help` / `--schema` (valid draft 2020-12 JSON Schema, `CLI-3`).
- **AX-4** · Input reads remain strictly bounded (`MAX_INPUT_BYTES = 1 MiB`, `MAX_YAML_INPUT_BYTES = 64 KiB`), list pages remain bounded (`1..=200`, default `20`), and overridable org-process injection remains capped (`24 KiB`) while mandatory org standards fail closed on overflow.
- **AX-5** · Launch catalogue and `--schema` snapshots are byte-identical on macOS and Linux (`CLI-11`).

**CLI grammar, help, pruning, and batching**
- **AX-6** · Every `agent-os` command is declared once in `CommandTable` with named `--kebab-case` flags, zero positional arguments, and exit code `2` + valid-flag hint before contacting the office on usage errors (`CLI-1`).
- **AX-7** · `agent-os <resource> <verb> --help` renders a contract-validated concrete invocation example from `Command.example.input` alongside flags, structured input, `content_field`, and options (`TASK-85.3`).
- **AX-8** · Pre-normalized aliases (`who`, `task note`) behave consistently across one-shot invocation, `--help`, `--schema`, and `session attach` (`TASK-85.3`).
- **AX-9** · `--fields` prunes composite task inspection server-side (`tasks.inspect` skips unrequested sections while `shape::conform` stays strict), `--format compact` omits the envelope unless `meta.status != completed` or `meta.next_cursor` is present, and `task list` exposes `total`, `--count`, `--exclude-status`, `--unassigned`, and `--modified-file` (`TASK-85.3`, `TASK-90`).
- **AX-10** · `task update` supports field-level `PatchField` and `--append-body`, `checklist list` supports `--section` and `--unchecked`, and `session attach` executes JSONL batches with `ADR-0014` latency targets (`1 ms` p50 / `2 ms` p95 on reference Mac; `CLI-13`, `CLI-15`).

**Day-1 onboarding, prefix caching, and skills**
- **AX-11** · Standing instructions assembly strictly follows `doc-27` §1 static-to-volatile prefix caching order (built-in instructions → command catalogue → org processes → project dev process → agent identity/boundaries → agent profile prompt → repo files, with per-turn objective last as turn input; `TASK-85.2`).
- **AX-12** · Standing instructions resolve the agent's real home team name and ancestor lead chain (not raw IDs alone), explicitly point agents to `agent-os skill list|read`, reflect amended `ADR-0025` decision 6 cross-turn sub-agent lifetime, and fence external speaker input as untrusted data on every `TurnTrigger` (`INSTR-5`, `TASK-85.2`).
- **AX-13** · Built-in product skills (`crates/org-process/src/skills.rs`) cover Day-1 orientation (`agent-os-basics`), phase-gated task workflows (`tasks-create`, `tasks-execute`, `tasks-finalize`), and every active `agent-os` domain (`messaging-and-asks`, `channels`, `code-and-worktrees`, `hiring-and-directory`, `processes-and-standards`, `budgets`, `errors-and-recovery`, `batch-and-attach`) in concise ~1-page guides (`TASK-85.2`).
- **AX-14** · Mechanical verification (`verify_command_references`) validates BOTH commands (`<resource> <verb>`) AND CLI `--flags` cited in standing instructions and every built-in skill against the live `CommandTable`, failing CI on any drift (`INSTR-12`, `TASK-85.2`).

**Errors, optimistic concurrency, and felt safety**
- **AX-15** · Every error response—including `session attach` pre-dispatch parse/bind errors (`attach.rs`) and `--format compact` failures—validates strictly against `envelope_schema()` (`{ok, data, meta, error}`) with exact exit codes `0`/`1`/`2` (`CLI-2`, `TASK-85.3`).
- **AX-16** · Every refusal carries a closed `ErrorCode`, a clear one-sentence `error.hint` naming the next action or who may act (never bare `"Retry."`), and `error.retry_with` whenever repeating with changed flags can succeed; `meta.status` and ask outcome statuses are aligned with zero contradiction (`TASK-85.3`).
- **AX-17** · Stale `--if-match` conflicts return `ErrorCode::Conflict` with `retry_with: {"if-match": <current_revision>}` and bounded `ConflictDetail` (`<= 20` events, `<= 8 KiB` serialized JSON, `events_truncated`, `omitted_events`, `changed_sections`, `reread_recommended`, compact `current_summary` with zero inlined `body_markdown` or comment bodies; `CLI-6`, `TASK-90`).
- **AX-18** · Ordinal checklist mutations (`--section`/`--index`) require `--if-match` and fail closed without it, while UUIDv7 `--step-id` remains supported; non-dry-run writes carry auto-minted or caller-supplied `--idempotency-key` (`CLI-4`, `CLI-5`), report truthful `meta.ack`, and never silently drop fields or coerce duplicate/conflicting keys (`TASK-90`).
- **AX-19** · Office authorization runs before entity lookup and conflict-delta construction (`ADR-0031`), returning `not_authorized` with zero existence oracle and zero conflict metadata; private task notes never appear in shared events, history, or `ConflictDetail` (`TASK-90`).

**Runtime portability and empirical AX quality gates**
- **AX-20** · **Cold-start orientation gate:** On standard Day-1 cold-start eval tasks, a newly started agent reaches its first useful domain action within **`<= 2` orientation calls** and loads the relevant phase skill when entering task work (`INSTR-23`).
- **AX-21** · **CLI syntax/usage error gate:** Across the standard AX evaluation suite, the CLI usage error rate (`exit_code == 2` / total `agent-os` calls) stays **`<= 5%`**.
- **AX-22** · **Self-healing retry-to-recovery gate:** Across injected `conflict`, `invalid_request`, and `unavailable` error drills, agents recover cleanly on the immediately following command at a rate of **`>= 90%`** without human intervention (`INSTR-4`, `INSTR-15`).
- **AX-23** · **Token-overhead gate:** Automated payload benchmarks verify measurable byte reduction on pruned reads (`--fields`, `--format compact`, `--count`) and bounded conflict recovery (`<= 8 KiB` vs full `task inspect`), alongside verified static-first prefix ordering.
- **AX-24** · **Multi-runtime parity gate:** `INSTR-1`–`INSTR-23` and core AX evals pass across `claude -p`, `codex`, and `agy` using the same shared standing instructions and built-in skills, separating contract-test, real-office CLI, and live-model eval receipts honestly (`TASK-85.3`, `FC-101`).

## 8. Delivery tracking and open questions

| Slice | Backlog task | Scope covered |
| --- | --- | --- |
| AX 1/3 — Discipline, persona & spec | `TASK-85.1` | `principal-ax-designer` (Claude + Codex + project skill + 4 references), this spec (`doc-96`), governance & authority-map registration |
| AX 2/3 — Onboarding & phase-gated skills | `TASK-85.2` | `AX-1`, `AX-11`–`AX-14`, `INSTR-12` flag verification, `crates/org-office-sessions/src/instructions.rs`, `crates/org-process/src/skills.rs` |
| AX 3/3 — CLI upgrades & multi-runtime evals | `TASK-85.3` | `AX-2`, `AX-7`–`AX-9`, `AX-15`–`AX-16`, `AX-20`–`AX-24`, `org-command` / `org-agent-cli` fixes, `claude`/`codex`/`agy` AX eval harness |
| Surgical task CLI & bounded conflict deltas | `TASK-90` | `AX-2`, `AX-9`–`AX-10`, `AX-17`–`AX-19`, server-side `--fields` pruning, `checklist list` / `--index` + `--if-match`, `ConflictDetail`, `ADR-0031` ordering |

**Open questions:**
- **Quantitative gate calibration:** The initial thresholds in `AX-20`–`AX-22` (`<= 2` orientation calls, `<= 5%` usage error rate, `>= 90%` next-turn recovery) are baseline v1 targets to be calibrated against the first live `claude`/`codex`/`agy` benchmark run in `TASK-85.3`—especially on cost-effective models (`doc-27` §8).
