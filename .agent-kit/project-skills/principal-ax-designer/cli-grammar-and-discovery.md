# CLI grammar, discovery, pruning, and batching

Reference for the [principal AX designer](SKILL.md). Grounded in `crates/org-command`
(`table.rs`, `help.rs`, `parse.rs`, `shape.rs`, `lib.rs`), `crates/org-agent-cli`,
`doc-28` (`Design: agent-os Command Line`), `doc-59` (`REQ-08`), and `TASK-85.3`/`TASK-90`.
Where `doc-28` or a domain data contract is stricter, that contract wins.

## 1. One grammar, one table

Every command in `agent-os` (and `agentic-org` for signed-in people) is declared once as a
static `Command` row in `CommandTable` (`crates/org-command/src/table.rs`):

```text
agent-os <resource> <verb> [--flag <value>]...
```

- **Zero positional arguments.** A bare token after `<verb>` is refused locally with
  `invalid_request` and `EXIT_USAGE` (`2`) before contacting the office, with a hint naming
  the likely flag. Positional arguments cause argument-order hallucinations across models.
- **Single-word resources and verbs** (lower-case ASCII, at most one hyphen where one word
  cannot carry the meaning, e.g., `task-note`, `dep-add`). Reuse the standard verb set
  everywhere so agents guess right before calling `--help`: `list`, `inspect`, `create`,
  `update`. Introduce a domain verb only when the standard four do not say the action
  (`send`, `ask`, `reply`, `accept`, `withdraw`, `transition`, `assign`, `hire`, `push`).
- **Flag names derive mechanically from `Field.name`:** `snake_case` field names become
  `--kebab-case` flags (`Field::flag()`). Boolean flags take no value (`--required`,
  `--dry-run`, `--durable`). Repeatable scalar list flags (`Type::List` of String/Integer/Enum)
  accept repeated occurrences (`--labels backend --labels security`). Structured objects and
  open JSON fields (`Type::Object`, `Type::Json`) are never squeezed into ad-hoc CLI string
  syntax; pass them via `--json`, `--input-file ... --input-format json|yaml`, or `--stdin`.
- **Single source of truth.** Parser, `--help`, `--schema`, launch catalogue, and
  `org_command::testkit` contract checks all read the same `CommandTable`. Never fork a
  parallel parser or hand-written help string.
- **Alias discipline.** When an ergonomic shorthand exists (`who` -> `directory list`,
  `task note ...` -> `task-note ...` in `crates/org-agent-cli/src/{lib,attach}.rs`),
  normalization must behave identically across one-shot invocation, `--help`, `--schema`,
  and `session attach`. Undocumented or partial aliases confuse cold-start agents.

*Breaks when:* `session attach` itself is a transport-mode entrypoint rather than a single
RPC row (`crates/org-agent-cli/src/lib.rs`); even there, its `--help` and every emitted JSON
line must still obey `doc-28` §2.9 and `envelope_schema()`.

## 2. The 4 KiB `CATALOGUE_LIMIT` economics

`crates/org-command/src/help.rs` enforces `CATALOGUE_LIMIT = 4096` bytes (`CLI-11`). That
cap is deliberate: the launch catalogue sits inside every agent's standing prompt on every
turn (~1,000 tokens).

### Why naive flat catalogues fail at scale

`CommandTable::catalogue()` currently prints one line per command:
`<resource> <verb>: <summary>\n`. At ~60–70 bytes per command, a flat catalogue saturates
around 60–65 commands. With org, team, person, agent, project, role, hr, session, grant,
directory, msg, channel, status, task, checklist, task-note, repo, worktree, process, skill,
and budget already registered across `crates/org-agent-cli/src/*_table.rs`, the catalogue
sits near saturation (~4,044 / 4,096 bytes) before `doc`, `feature`, `milestone`, `release`,
`draft`, and `search` commands land (`TASK-85.3`, `TASK-90`).

### Scaling rules that preserve zero-guesswork discovery

1. **Prefer progressive resource-level grouping over cryptic abbreviations.** Truncating
   summaries to cryptic fragments increases CLI usage errors (`exit_code == 2`). Grouping
   verbs per resource (e.g., `task: create, list, inspect, update, transition, assign, comment, watch, unwatch, history, deps, dep-add, dep-remove, labels — Manage tasks, deps, and labels`
   or compact two-tier resource lines + `agent-os <resource> --help`) compresses ~70+
   commands from ~4,050 bytes to ~1.2–2.0 KiB while keeping every resource and verb name
   explicit and searchable.
2. **Never silently omit commands from the catalogue.** An agent cannot guess a resource it
   never saw named. Every active resource and verb must remain discoverable either directly
   in the catalogue or via one deterministic `<resource> --help` hop.
3. **Byte-identical across OSes (`CLI-11`).** `CommandTable::new` sorts rows by
   `(resource, verb)`. Catalogue and schema snapshots must be byte-identical on macOS and
   Linux and fail CI on unintended drift.
4. **Never load schemas at launch.** Full JSON Schemas for 70+ commands would burn tens of
   thousands of tokens upfront—the exact anti-pattern `REQ-08` CLI-first architecture avoids.

## 3. Three-tier `--help`, `--schema`, and concrete examples

Every discovery call runs locally from `CommandTable` with zero office round-trip and works
even without an active run binding (`doc-28` §2.8):

| Call | Output format | Purpose |
| --- | --- | --- |
| `agent-os --help` | Plain text | Lists all resources and their verbs, plus the next help command |
| `agent-os <resource> --help` | Plain text | Lists every verb of `<resource>` with its one-line summary |
| `agent-os <resource> <verb> --help` | Plain text | Lists flags (marking `Required.`), structured fields, `content_field`, common options, **and a concrete invocation example** |
| `agent-os <resource> <verb> --schema` | JSON envelope (`Format::Json` default) | Emits `command`, `call`, `kind`, draft 2020-12 `input` and `output` JSON Schemas, `content_field`, declared `errors`, and `ack` |

- **`Command.example` belongs in `--help` (`TASK-85.3`).** Every row already carries a
  contract-validated `Example { input, output }` checked by `CommandTable::new` against
  both the parser and schema (`table.rs` lines 481–489). Rendering a runnable example from
  `Command.example.input` inside `<resource> <verb> --help` turns `--help` from a flag list
  into a one-shot copy-and-adapt template, cutting first-try syntax errors dramatically.
- **`--help` wins wherever parsed as a flag.** Even if other flags on the command line are
  missing or invalid, `--help` prints help and exits `0`—except when it appears as the value
  of a value-taking flag (`--body --help` sets `body` to `"--help"`).

## 4. Shell-escaping immunity (`doc-28` §2.2, `CLI-7`)

Headless agents invoke `agent-os` through POSIX shells. Multiline Markdown, backticks,
single/double quotes, `$VAR`, and YAML front matter break inline shell quoting constantly.

- **One declared `content_field` per text-heavy command** (e.g., `body` on `task create`,
  `task update`, `task comment`, `msg send`, `msg reply`, `process create|update`; `text` on
  `task-note add|update`). Passing `--input-file <path>` or `--stdin` feeds raw UTF-8 bytes
  directly into `content_field` with zero escaping.
- **Structured payloads via `--input-format json|yaml`.** Multi-question `msg ask` payloads
  or complex updates use `--input-file ask.yaml --input-format yaml` or `--stdin --input-format json`.
- **Hard read bounds (`parse.rs`, `office.rs`).** `Sources` reads at most `max_bytes + 1`:
  `MAX_INPUT_BYTES = 1 MiB` (1,048,576 bytes) for text/JSON, `MAX_YAML_INPUT_BYTES = 64 KiB`
  (65,536 bytes) for YAML. Oversized inputs fail locally with `invalid_request` and the limit
  in `error.hint`.
- **Never read stdin implicitly (`CLI-8`).** No command reads stdin unless `--stdin` is
  explicitly passed; no command ever prompts for TTY input.
- **The office never opens caller-named paths.** `agent-os` reads `--input-file` inside the
  agent's own isolation and sends the bytes in the RPC payload.

## 5. Token-saving reads: `--fields`, `--format`, pagination, and filters

Everything printed on stdout lands directly in the model's context window. Design reads so
agents fetch only the exact slice they need:

- **`--format json|yaml|compact`.**
  - `json` (default): single-line deterministic envelope, stable field order, ideal for pipes
    and `jq`.
  - `yaml`: envelope with block scalars for readable long Markdown/prose bodies.
  - `compact`: strips the envelope on success (`data` only—tab-separated rows for lists,
    `name: value` for single records). Appends a final `meta: <one-line-json>` line **only**
    when `meta.status != completed` or `meta.next_cursor` is present. Exact byte guarantee:
    any field named `meta` or value starting with `meta:` is JSON-quoted so `^meta: ` is
    unambiguous. Failures ALWAYS emit the full JSON envelope in every format.
- **`--fields <a,b.c>` — client AND server-side pruning (`TASK-90`).**
  - `org-command` already projects output via `shape::project` (`lib.rs` lines 173–177),
    preserving declared field order.
  - For heavy composite records like `task inspect` (`body_markdown`, `checklist`, `comments`,
    `depends_on_tasks`, `dependent_tasks`, `history`), client-side pruning alone still wastes
    office/DB work and wire bandwidth. Carry requested `--fields` roots on `org_command::Request`
    and mark prunable sections `Field::optional` in `TASK_INSPECT_OUTPUT` so the office skips
    loading unrequested sections server-side while `shape::conform` stays strict.
- **Bounded pagination + cheap sizing (`TASK-85.3`).**
  - Lists default to `--limit 20`, capped at `MAX_LIMIT = 200`, continuing via `--cursor`
    from `meta.next_cursor`. Out-of-range limits fail locally with `invalid_request`.
  - Expose `total` and `--count` on high-volume lists (`task list`) so an agent can check
    scope size before pulling 50 tasks into context.
  - Provide high-signal filters that eliminate client-side loops: `--exclude-status` (e.g.,
    exclude `done,cancelled`), `--unassigned`, and `--modified-file <substring>` (find prior
    tasks touching a file path).

## 6. Surgical patching without whole-document re-reads (`TASK-90`)

The single most common context-window anti-pattern in agent task workflows is:
`task inspect` (full body + 30 comments + full history) -> tick 1 checklist step -> get full
task back -> repeat 8 times.

- **Field-level `PatchField` updates:** `task update` patches only supplied fields and
  supports additive/subtractive list flags (`--add-labels`, `--remove-labels`,
  `--add-modified-files`) plus `--append-body` (mutually exclusive with full `--body`
  replacement) so agents append findings without re-reading or re-sending the entire body.
- **Surgical checklist reads & writes:** `checklist list --task <ref> [--section acceptance|dod|work|all] [--unchecked]`
  returns only the checklist steps needed. `checklist add` accepts `--section` and repeatable
  `--text`. `checklist check|edit|remove|move` accept 1-based `--section`/`--index` (including
  batch check) **only when `--if-match <revision>` is provided**, alongside stable UUIDv7
  `--step-id`. Mutations return compact receipts (mutated step + per-section progress counts)
  rather than dumping the entire task.

## 7. High-frequency batching via `agent-os session attach`

One-shot commands pay OS process spawn + socket bind per invocation. When an agent runs
multiple checks or updates in one shell turn (e.g., inbox triage + task status + checklist
updates), `agent-os session attach` (`doc-28` §2.9, `CLI-13`, `ADR-0014` decision 5):

- Binds once to the office socket, then reads one JSON object per line of stdin
  (`{"command":"msg inbox","input":{},"options":{"limit":5}}`) and writes one single-line
  JSON envelope per line of stdout.
- Answers every line independently: one malformed or refused line emits an error envelope
  and never aborts subsequent lines.
- Holds the latency target of `ADR-0014` (`1 ms` median, `2 ms` p95 on the reference Mac;
  common commands listed in `doc-28` §2.11 never perform schema loads, extra round-trips, or
  unnecessary flushes).
- Every emitted error envelope—including pre-dispatch parse/bind errors in
  `crates/org-agent-cli/src/attach.rs`—MUST conform strictly to `envelope_schema()` with all
  four top-level keys (`ok`, `data`, `meta`, `error`).
