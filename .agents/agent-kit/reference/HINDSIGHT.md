# Local semantic project memory

The fixed memory backend is **Hindsight 1.2.1**, alongside Repowise 0.61.0.
Repowise indexes code. Hindsight discovers reviewed project decisions, maps and
learnings. Git records and their source evidence remain authoritative.

## Deployment and subscriptions

Both Codex-only and hybrid use the same project bank and bounded `recall` tool
on the existing `starter_kit_context` MCP connection. The CLI remains available
for lead-owned setup, sync and diagnostics.
Codex keeps its existing ChatGPT/Codex login; Claude Code keeps its Claude login.
The memory service needs **no model API key or paid model calls**. It does not use
either subscription for memory processing. Reading recalled text consumes normal
agent context; CPU, RAM and disk still have a cost. Saved login checks cannot prove
account entitlement or remaining allowance.

The adopting agent installs/starts Docker using the supported host workflow, then:

```bash
python3 .agents/agent-kit/runtime/kit_memory.py setup
python3 .agents/agent-kit/runtime/kit_memory.py status
python3 .agents/agent-kit/runtime/kit_doctor.py --project . --live --checks --json
```

Linux uses Docker Engine with BuildKit; macOS and WSL use Docker Desktop with Linux
containers. First setup needs registry, Python package and Hugging Face download
access, several GB of free disk and sufficient Docker memory for PostgreSQL and two
small ONNX models. No GPU is required. The agent provisions the fixed image and
discovers its loopback port automatically. Docker installation/host startup may
require user intervention on managed machines; an unavailable daemon is a blocker.
Do not replace it with a remote or paid backend implicitly.

The image is built from the pinned upstream API-only image digest. Python additions
are hash-pinned to the upstream release lock; embedding and reranker artifacts are
pinned to immutable Hugging Face revisions in `policy/hindsight/runtime.json`.
The small CPU profile avoids the full image's unused GPU/framework dependencies.
It uses **embedded PostgreSQL through pg0**, stored in a named Docker volume, rather
than a separately administered database. The API publishes only on `127.0.0.1`.
One service is reused per runtime policy on a Docker host; repositories have
distinct banks derived from their normalized origin URL (Git common directory
when there is no origin). Linked worktrees share a bank but filter against their
own committed records. This is local single-user tooling, not a multi-tenant server.

`llm_provider=none` selects chunk retention, disables observations/consolidation
and reflection, and uses local ONNX embedding/reranking. Models are downloaded at
build time; the running service uses offline model paths. No Claude/Codex auth
files, API keys or project source tree are mounted in the container. The existing
kit MCP gateway exposes bounded recall only; Hindsight's own MCP endpoint and
retention/reflection tools remain disabled. No automatic transcript imports.

## Normal use

Prefer the `starter_kit_context.recall` MCP tool in both harnesses. It runs outside
the shell sandbox, so restricted reviewers need no Docker or network exception.
Pass `repo_root`, a bounded `query`, explicit `paths`, and `phase` (`implementation`
or `review`). Codex forwards the review/pilot environment into this server; a
review environment forces governing-only recall even if the requested phase differs.
The tool cannot promote/sync memory or change source. Restart/trust the updated
MCP configuration after upgrading. The live doctor probes this exact connection.

The lead reviews proposals, promotes useful records and commits them with the
work. Then sync the derived semantic index:

```bash
python3 .agents/agent-kit/runtime/kit_memory.py sync
python3 .agents/agent-kit/runtime/kit_memory.py recall --query "Why are invoice amounts stored as cents?" --path src/billing
```

Only unchanged, committed `ACCEPTED` JSON records enter Hindsight. Their short
summary, rationale and source paths are retained; source bodies and private logs
are not. Content-addressed IDs make repeated sync idempotent. At most 100 new
records are uploaded per call; repeat if `remaining_records` is nonzero. No blanket
history import is performed.

Recall has a 1,500-token provider cap and a **4,000-character complete response**
cap by default. Explicit paths are required. Provider text is never inserted into
context: returned IDs resolve to current approved Git records in this checkout.
Deleted, superseded, uncommitted, foreign and changed record revisions are excluded.
Source drift is labeled `NEEDS_REVIEW`; it does not erase the original rationale.
Read the full record and applicable decisions before relying on an excerpt.
Semantic recall is discovery, not an exhaustive governing-decision list. Path-only
recall remains available with pagination; active architecture constraints must
always be read separately.

Fresh reviewers use `--phase review`: only governing decision/map records are
eligible. The Codex review wrapper also sets `AGENT_KIT_MEMORY_ACCESS=review` for
review and same-session repair. Review sessions cannot promote or sync memory.
Manual Claude reviewers must follow the same skill instructions. These are local
workflow guards, not authenticated identity or a security boundary against an agent
that deliberately bypasses the wrapper.

## Health, recovery and upgrades

Setup retains, semantically recalls and deletes a probe in a disposable bank, then
syncs approved records. Repeat setup reuses the managed service and preserves data.
`kit_memory.py status` checks current version/profile and availability. The
`setup-health` skill runs the doctor with `--live`, repeating real code and memory
probes. `--checks` also runs the configured, source-bound project checks. Receipts
are in `.agents/agent-kit/logs/health/`; no paid model calls are made.

When Hindsight is unavailable, recall reports `DEGRADED` and returns bounded local
memory. This is useful continuity, not a healthy semantic service. Setup/doctor
remain failed until recovery. Check Docker, inspect the named service logs and
rerun setup. Never delete a volume to make a health check pass. Container restart
preserves pg0 data. Git records can rebuild this derived index on a new host.

A changed runtime policy creates a new versioned service/volume and resyncs Git
records. Old services/data are retained; retire them deliberately only after all
dependent checkouts have migrated. Setup receipts bind to the exact checkout,
config and policy. Rerun setup after cloning, moving or upgrading a checkout; do
not copy receipts or hand-edit project IDs/ports. Existing banks with an unrelated
mission or changed profile block setup rather than being overwritten.

## Efficiency evidence and the later pilot

`kit_memory.py usage` aggregates memory request counts, latency, transport size,
returned characters and failures. Verified no-LLM operations record zero LLM
tokens; unknown observations stay unknown. Dollar cost stays unknown because
these events exclude CPU/storage and the agents' own consumption.

Use the installed `HINDSIGHT_PILOT.md` procedure later in the application repo.
It compares local scoped memory against semantic memory using the same approved
corpus, stable tasks, model settings and acceptance gates, including failed work,
review repairs and cold setup cost. A successful service probe proves integration,
not improved task quality or subscription savings. No paid pilot is part of this
installation.

Upstream: [Hindsight v1.2.1](https://github.com/vectorize-io/hindsight/tree/v1.2.1)
(MIT), including its `hindsight-api/hindsight_api/config.py` no-LLM configuration.
