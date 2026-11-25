# Fixed tooling stack and deployment

Kit 0.10.0 uses **Repowise 0.61.0 as its only code index** in both profiles. This decision is made
for adoption: the agent configures the project, rather than asking the user to
select infrastructure. RTK is excluded; commands execute directly.

## The shipped stack

| Responsibility | Tool and behavior |
| --- | --- |
| Code discovery and relationships | Repowise 0.61.0 through `starter_kit_context.lookup` on the shared MCP gateway, shared by Claude and Codex |
| Focused source access | Repowise `symbol`/`context`, followed by bounded `rg`/source reads for exact literals and graph gaps |
| Type correctness and formatting | The project's existing compiler/type checker and formatter, selected from its actual manifests/CI by the adopting agent |
| Persistent knowledge | Hindsight 1.2.1 semantic recall through `starter_kit_context.recall` and `kit_memory.py`; reviewed Git records, DECISIONS.md and scoped maps remain authoritative |
| Task continuity | Task-state records and the kit's metadata checkpoint hooks; the author session stays alive through review |
| Command output | `kit_hooks.py`: direct execution, original exit status, bounded diagnostics and full private logs |
| Planning and implementation | The installed profile assigns all roles/efforts: Claude families in hybrid, Sol in Codex-only; helpers only extract facts |
| Review and acceptance | Sol High fresh review/same-session fix (the routing policy's `review.model`: gpt-6.1-sol or gpt-6.2-sol), original-author validation, then a fresh final review: Sol in Codex-only, Opus in hybrid |
| Cache reuse | Native provider caching and recorded usage; no keepalive calls or caching proxy |

Hindsight runs locally with embedded pg0 and local CPU embedding/reranking, without
model API keys. No second code index, command rewriter or additional navigation MCP
server is installed. See [Hindsight setup and health](HINDSIGHT.md). Existing application dependencies remain authoritative.
The shared retrieval tool is structural navigation, not a claim of typed LSP parity;
compiler/check evidence is required for consequential refactors. Unsupported graph
coverage calls for bounded source inspection and explicit evidence, not a new tool
selection or a declaration that there are no callers.

## Why Repowise

Separate search, relationship and symbol lookups let agents discover locations
without automatically loading large source bundles. Its history/rationale support
also fits long-running projects. This is an engineering choice, not proof that it
wins every workload. On our Django sample, initial indexing took 235 seconds and
an initial update 37 seconds; unchanged-source fingerprints avoid repeating that
update on every lookup. Changes still incur refresh work.

[Repowise 0.61.0](https://github.com/repowise-dev/repowise/tree/v0.61.0) is
AGPL-3.0-or-later. Its maintainer's
[benchmark](https://github.com/repowise-dev/repowise/blob/v0.61.0/docs/BENCHMARKS.md)
reported reduced model output on specific tasks; that is not a demonstrated
reduction in this application's total bill. Measure complete accepted tasks.

## Deploy into a project

Use macOS, Linux or WSL, with Bash, Git, ripgrep and Python 3.12 including venv/pip.
The kit's core scripts still support Python 3.10+, but the managed index runtime is
Python 3.12. Docker with Linux containers and BuildKit is also required for Hindsight.
The adopting agent installs missing prerequisites using the host's
established package workflow. Package download access is required on first setup.
Codex needs its usual installation, login and model access. Claude is required only
for hybrid; Codex-only requires no Anthropic account or allowance.

From the starter-kit checkout:

```bash
bash install.sh --project /absolute/path/to/project --mode codex-only
```

The adopting agent reads one real function/class and identifies its source file.
Then, from the target project's Git root, it runs:

```bash
python3 .agents/agent-kit/runtime/kit_context.py setup --query REAL_SYMBOL --expect-path path/to/source.py
python3 .agents/agent-kit/runtime/kit_memory.py setup
python3 .agents/agent-kit/runtime/kit_doctor.py --project . --live --checks --json
```

The two arguments are project evidence, not infrastructure choices. For example,
Django uses `--query QuerySet --expect-path django/db/models/query.py`. The command:

1. Merges Codex `.codex/config.toml` and, only in hybrid, Claude `.mcp.json`, preserving
   unrelated entries and backing up deliberate changes. Conflicting managed entries
   block instead of being silently replaced.
2. Installs the fixed provider into ignored `.agents/agent-kit/tools/repowise-0.61.0/`, using
   `pip --require-hashes` and the shipped transitive dependency lock. It checks the
   installed packages and records the resolved environment. No global profile or
   application dependency manifest is changed.
3. Builds or refreshes this checkout's index with paid/model-based prose generation,
   editor setup, hook installation and workspace discovery disabled.
4. Starts the exact configured MCP launcher, initializes it, lists its two bounded retrieval tools,
   and proves that a lookup finds the expected source file. An empty result fails.
5. Writes `.agents/agent-kit/logs/context/setup.json` with `CONTEXT_READY` or `BLOCKED`.
   Repeat setup reuses a matching managed environment; failed installation remains
   visibly incomplete and can be retried.

The normal command needs no backend or executable selection. `--executable` exists
only for controlled tests or an explicitly managed deployment; it skips dependency
provisioning and records that limitation. Do not use it for a fresh default adoption.
For a pre-existing Repowise index, inspect its version/ownership, then the adopting
agent may pass `--adopt-existing`. Never copy tool environments, index caches or
`.agents/agent-kit/logs` between worktrees. Prepare each worktree separately.

After setup, restart/trust the project configuration in the selected harness(es) and verify
the real tool is available. Claude exposes `mcp__starter_kit_context__lookup`;
Codex lists `lookup` under `starter_kit_context`. Use:

```json
{"repo_root":"/absolute/current/checkout","kind":"search","query":"REAL_SYMBOL"}
```

`CONTEXT_READY` proves the local tool/MCP setup. It does not prove client trust,
model login, sufficient application checks or a completed live review. Finish
ADOPT.md, record those separate states, and resolve actual access blockers without
inventing successful model calls.

## Runtime use and freshness

Kinds are `search`, `context`, `symbol` and `why`. `symbol` accepts returned IDs such
as `src/api.py::handler` or `src/api.py:12-30`; `context` supplies relationships.
`why` supplies recorded history/rationale for review against accepted decisions.
The same connection exposes `recall` for scoped reviewed memory. No model-based
`ask`, arbitrary shell, source-edit, memory-write or approval tool is exposed.
The gateway does write local derived caches/logs.

The fast initial build supplies core structure and essential Git metadata, not
every expensive history feature. Updates use `--index-only --working-tree` so
staged, unstaged and nonignored new files are included. Provider telemetry/network
features are disabled for index operations; setup downloads packages separately.
Repowise can recover its own savings estimates from local same-repo transcript
markers; those estimates are not our billing evidence.

Each checkout owns its index. The launcher resolves the actual Git root, and every
request supplies that root. Before lookup, a fingerprint of HEAD, index entries and
tracked/nonignored source contents determines whether to reuse the last successful
refresh. Changed source triggers an update. Failures invalidate earlier success;
edits during refresh/retrieval reject the result. The gateway serializes requests.
Do not run another independent index writer against the same cache.

Fingerprinting is bounded to 20 seconds. Ignored dependencies, symlink targets,
external services and environment changes are outside that fingerprint. Submodules
and special/linked directory layouts need explicit project handling. Static graphs
remain partial, so empty caller lists do not prove dead code and inferred affected
tests never replace configured acceptance checks.

Results default to 12,000 characters, with a truncation flag and full local output
path. Narrow follow-ups before reading the full result. Refresh/query commands have
a 600-second default budget each; initial setup runs before model work. The agent
may adjust an evidenced project budget and matching client timeout when needed.
No unsupported or failed index is reported as ready.

## Memory and output

Use `project-memory` for scoped recall, source-backed proposals and explicit lead
promotion into versioned knowledge. Changed source flags a record for review;
matching hashes do not establish semantic truth. Keep governing decisions and
subsystem maps curated, and record task intent/next steps before handoff/compaction.
Hooks save metadata and pointers; they cannot reconstruct unwritten reasoning.
Fresh reviewers receive project standards/current source rather than inherited
review conclusions. Claude and Sol do not share a model cache.

Recall requires an affected `--path`, ranks relevant accepted records and returns
short discovery excerpts within 4,000 characters including the JSON envelope by
default. Full rationale stays in the referenced record; read applicable constraints
before implementation. Omitted records have a next-page offset. Run `kit_memory.py
audit` after curation; the doctor also warns about oversized guidance/maps/decisions
and current task records. These alarms never truncate active requirements. Load
only the current task and relevant full records, not the archive. See the
[efficiency audit](EFFICIENCY_AUDIT.md) for what is enforced versus agent guidance.

Verification executes the configured command arrays directly. Success output stays
out of model context; failures show a bounded tail and a full private log path under
`.agents/agent-kit/logs/commands/`. Receipts preserve exit status and log byte counts. There
is no output-rewriting wrapper or LLM summarizer in the acceptance path.

## Release validation

From the kit checkout, `python3 scripts/context_smoke.py` exercises fresh dependency
installation, setup repetition, a real MCP lookup, dirty edits/renames, new/deleted
files and a separate linked worktree. CI runs this on Linux and macOS with Python
3.12, separately from offline script tests. No model account is needed for these
checks. The application's authenticated workflow and total savings still need its
own acceptance pilot.

Maintainers regenerate the dependency lock from `repowise.in` with
`uv pip compile --universal --python-version 3.11 --generate-hashes --no-header --no-annotate`.
Revalidate the provider and deployment tests before changing versions. Agents do
not load the dependency lock into conversation context; package installation reads it.
