---
name: indexed-context
description: Use the fixed Repowise index for checkout-bound discovery, bounded source retrieval and working-tree refresh in Claude and Codex.
---

For large maintained projects, query the configured index before broad exploratory
Grep/Read loops. A known small file or exact literal can still go directly to source.
Use Repowise 0.61.0 only; do not install or propose a second index as an adoption choice.
Use `mcp__starter_kit_context__lookup` in Claude, or the `lookup` tool under
`starter_kit_context` in Codex (the displayed tool name may differ):

```json
{"repo_root":"/absolute/current/worktree","kind":"search","query":"authentication backend selection"}
```

The same connection exposes `recall` for reviewed project memory; follow
project-memory for its query, path and review-phase rules. It cannot retain memory
or edit source.

Kinds: search (discovery), context (file relationships), symbol (target source), why
(Repowise's recorded rationale). Inspect `.agent-kit/context-tools.json`.
Shell-capable agents can use `python3 .agent-kit/runtime/kit_context.py query --query TEXT`.

- Prepare before starting agent work. The gateway reuses a successful refresh only
  when HEAD, index and current tracked/nonignored source fingerprints match;
  otherwise it refreshes, including uncommitted and new files with Repowise's
  working-tree option. It rejects edits during refresh/retrieval and serializes its own
  requests and rejects mismatched checkout identities and untested versions.
  Do not run a second independent index writer against the same cache.
- A completed refresh is not proof of complete call-graph coverage. Inspect
  provider warnings, unresolved edges and language limits. Empty callers do not
  prove dead code. Use compiler/check evidence for semantic refactoring and
  source search for configuration, templates and dynamic behavior the graph misses.
- Results default to 12,000 characters, with an explicit truncation flag and a
  full local output path. Narrow the question before retrieving that whole file.
  Character counts are not model tokens or demonstrated dollar savings.
- Read the returned source once. Repowise context cards contain relationships and
  may need a focused symbol lookup. Verify
  relevant omitted or changed lines before editing; do not routinely re-read every
  full file after a useful result. Return concise paths and evidence to the lead.
- The shared index does not share conversation history. Every fresh Sol/Opus review
  gets its own evidence and must not inherit earlier findings. Never suppress a
  snippet because another agent has seen it.
- Missing/stale/degraded tools return an error. Fall back to bounded source search,
  report the gap, and repair setup; do not claim indexed coverage. Inferred impacted
  tests guide exploration and never replace the configured acceptance checks.

During adoption, run `python3 .agent-kit/runtime/kit_context.py setup --query REAL_SYMBOL
--expect-path path/to/source.py` after reading that project symbol. This installs the
fixed provider and hash-pinned dependencies, connects the selected profile's clients and proves the
configured MCP can locate the source. The agent supplies project facts; the user
does not choose infrastructure. See `.agent-kit/reference/CONTEXT_TOOLS.md`.
