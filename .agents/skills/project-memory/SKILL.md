---
name: project-memory
description: Recover task intent, retrieve scoped project knowledge and capture evidence-backed updates for lead review across Claude and Codex sessions.
---

At task start, read the small project map and applicable active decisions, then the
current task checkpoint if resuming. Do not glob/read the knowledge or tasks archive
into context. Prefer `mcp__starter_kit_context__recall` in Claude or `recall` under
`starter_kit_context` in Codex. Supply `repo_root`, a specific `query`, affected
`paths` and `phase` (`implementation` or `review`). This uses the existing MCP
connection outside the shell sandbox and is bounded to 4,000 characters. It cannot
write knowledge or source. A missing tool requires setup-health/context setup and
client restart/trust; do not relax the review sandbox.

For shell-capable lead sessions, the equivalent discovery command is:

```bash
python3 .agent-kit/runtime/kit_memory.py recall --query "Specific decision or constraint needed for this task" --path src/area
```

Recall requires a path and defaults to **4,000 characters for the complete JSON
response**, not tokens. Hindsight semantically retrieves approved committed records
with local CPU models, no model API key. Returned IDs resolve to current Git
records; provider text is never authoritative. On outage, `DEGRADED` explicitly
marks local fallback; ask the lead to run setup-health. Semantic discovery can
omit relevant constraints: always read the applicable active decision ledger/ADRs.
Omit `--query` for local paginated recall. This ranks governing decisions first, then closer source scope
and recent lead review. It returns short excerpts and record paths, not full
rationales. Read the relevant full records before relying on their constraints;
an excerpt can omit a qualification. `omitted_records`/`next_offset` make omissions
visible: narrow the path or repeat with `--offset N` to inspect the next page when
needed. Restart paging after editing memory. `--path .` is a deliberate cross-project
lookup, never the default. Multiple paths cover a cross-cutting change. Recency is
not authority and does not resolve conflicting decisions.

The lead owns shared knowledge. Workers propose reusable facts, decisions or map
updates with rationale and current source references using the KNOWLEDGE.json
template, then run `kit_memory.py propose --input PATH`. Pending proposals stay
private under `.agent-kit/logs/memory/proposals/`; they are not governing decisions.

After validating intent and source, the lead runs:

```bash
python3 .agent-kit/runtime/kit_memory.py promote --input PROPOSAL_PATH --reviewer-session ACTUAL_SESSION_ID --rationale "Why this is correct and useful"
```

Promotion rejects changed source evidence and creates a versionable accepted record
in `.agent-kit/knowledge/`. Commit it with the relevant work, then run
`python3 .agent-kit/runtime/kit_memory.py sync`. Repeat if `remaining_records` is
nonzero; uncommitted records are never synced. Curate governing choices
in DECISIONS.md, update PROJECT_MAP.md or a focused `.agent-kit/maps/<area>.md` capsule,
and explicitly mark superseded accepted JSON records `SUPERSEDED`. Do not accumulate
contradictory active records. The recorded reviewer is a declaration, not proof of
identity; semantic correctness remains the lead's responsibility.

Recall checks source hashes. NEEDS_REVIEW means revalidate the claim; a changed file
does not automatically invalidate a design decision. SOURCE_UNCHANGED only checks
the recorded evidence. External contracts, dependencies and environment can change.
Existing project ADRs remain authoritative; index-mined history is proposed evidence,
not an automatic replacement for accepted decisions.

For architecture, use architecture-decisions and ARCHITECTURE_DECISION.md. The
canonical versioned record lives in the existing ADR directory, otherwise
`.agent-kit/decisions/`; the ledger, maps and task contracts link to it. Preserve
premise IDs, evidence/state, original rationale, challenge history and replacement
links. Accepted knowledge about that decision includes the decision file itself
and relevant current implementation files in `sources`; changing its status/premises
then flags recall for revalidation even if application code is unchanged.
Do not duplicate the full rationale in memory
JSON. A new architecture reviewer reads applicable premises and rationale up front;
independence from the author does not mean discarding governing project knowledge.

Before handoff, interruption, compaction and acceptance, update the task-state file
with intent, constraints, current revisions, failed checks and the next step. The
SessionStart/PreCompact hook records task pointers and a revision; it cannot capture
unwritten reasoning. After each substantive accepted slice, record useful knowledge
or note that no durable update was needed. Never copy complete transcripts or logs.

Keep the root map within roughly 4,000 characters, the active decision ledger within
8,000, one current task checkpoint within 6,000 and a subsystem capsule within 8,000.
These are growth alarms, not grounds to drop requirements. Keep global invariants
in the ledger, link scoped decisions/capsules, and move superseded/completed history
to linked archives. Preserve active constraints, unresolved failures and next steps.
Do not copy the same rationale into a ledger, map, task record and accepted record;
choose its canonical home and link it elsewhere. After memory changes run:

```bash
python3 .agent-kit/runtime/kit_memory.py audit
```

The doctor also reports these soft-limit warnings. Neither command truncates files
or deletes knowledge. Budgets measure characters; actual session context also
includes roles, skill/tool definitions and history. Inspect the harness context
display when growing or compacting a session. Load subsystem capsules only on
demand. Do not enable automatic
unreviewed shared-agent memory writes. Fresh reviewers may read governing project
knowledge and the original contract using `--phase review` (decisions/maps only).
The Codex review wrapper enforces this phase for memory commands and prevents
promotion/sync; manual Claude review must apply the same rule. Inspect code before prior review findings
or author acceptance notes. Prompt caching, session context, source indexes and
durable project memory serve different purposes. See the installed HINDSIGHT.md
reference for deployment, recovery, subscriptions and measurement limits.
