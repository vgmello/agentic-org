# Agent memory: how runtimes and research systems do it, and what goes wrong

Research for [`docs/design/agent-memory.md`](../design/agent-memory.md). It answers how a permanent agent can keep its identity, its knowledge of the system it works on and what it learns, across many sessions, without filling its context. Written 2026-11-09.

## Evidence legend

- **[doc]**: official documentation, fetched 2026-11-09 (URLs under Sources).
- **[paper]**: the paper's abstract page, fetched 2026-11-09.
- **[not re-fetched]**: cited from the paper body or an earlier reading, not checked today.
- **[3p]**: third-party write-up. A lead, not a fact.
- **[unverified]**: conflicting or missing primary sources. Each says why.
- **[repo]**: this repository (file and line).

## 0. TL;DR

1. **All three runtimes converged on the same shape** [doc]: a small hand-written instruction file loaded every session (CLAUDE.md, AGENTS.md, GEMINI.md), a machine-written memory store loaded as an **index**, with details read **on demand**, and compaction inside a session. Every one caps what it loads (Claude Code: under 200 lines advised, MEMORY.md 200 lines or 25 KB; Codex: 32 KiB; Antigravity: 24 KB per rule file).
2. **Research systems add three ideas worth taking** [paper]: memory tiers (MemGPT/Letta's always-loaded core blocks vs searched archival memory), **background consolidation** (Letta sleep-time agents, Mem0's ADD / UPDATE / DELETE / NOOP, Codex's idle-time extraction) and **validity over time** (Zep/Graphiti invalidates a contradicted fact instead of deleting it).
3. **Memory is an attack surface** [paper]: AgentPoison (over 80% success at under 0.1% poisoned records), MINJA (poisoning through ordinary queries alone) and SpAIware (persistent instructions planted in ChatGPT's memory) all show that what an agent writes to memory, if it later reads it as instructions, is a durable prompt-injection channel. This repo already turned Claude's auto-memory off for exactly that reason [repo: `docs/design/agent-sessions.md`, decision 6].
4. **More context is not better** [3p, doc]: Chroma's "context rot" study (18 models) shows accuracy falling as input grows, even on simple tasks. Anthropic's guidance is "the smallest possible set of high-signal tokens". Claude Code's docs say longer instruction files "reduce adherence".
5. **Memory drifts and flatters** [paper]: PersistBench (2026) found a median 97% failure on memory-induced sycophancy and 53% on cross-domain leakage. Anthropic's long-running-agent harness saw agents declare victory early after reading their own progress notes. Memory has to be checked against the live system, not trusted.

## 1. The runtimes

### Claude Code [doc]
- **Instruction files** load broadest to most specific (managed, user `~/.claude/CLAUDE.md`, project `./CLAUDE.md`, local `CLAUDE.local.md`), all concatenated. Conflicts are resolved "arbitrarily". CLAUDE.md is guidance, not enforcement. "Target under 200 lines per CLAUDE.md file. Longer files consume more context and reduce adherence." Subdirectory files load on demand. `@path` imports nest up to 4 hops and still cost context.
- **AGENTS.md** is read natively when there's no CLAUDE.md (both with a setting).
- **Auto memory**: `~/.claude/projects/<repo>/memory/MEMORY.md` (an index, one line per memory, typed `user`, `feedback`, `project` or `reference`) plus a topic file per memory. Only the first 200 lines or 25 KB of the index load; topic files are read on demand; a `modified` timestamp is stamped in; Claude is told to skip what code or git already say. Machine-local.
- **Compaction** re-injects the root CLAUDE.md, unscoped rules and auto memory from disk; nested and path-scoped files are summarized away; skills are re-injected up to 5k tokens each, 25k total.
- **Hooks**: `SessionStart` (`startup`, `resume`, `clear`, `compact`), `PreCompact` (its output steers the summary; exit 2 blocks it), `PostCompact`, `Stop`, `SessionEnd`. Injected context is capped at 10,000 characters.
- In this repo, office-run Claude sessions load **none** of this: `--setting-sources ""` and `autoMemoryEnabled: false`, because auto-memory "is an agent-writable instruction channel" [repo: `agent-sessions.md` decision 6]. The office composes the instructions itself [repo: decision 8].

### Anthropic API [doc]
- **Memory tool** (`memory_20250818`): client-side file commands under `/memories`. The API adds a protocol ("ALWAYS VIEW YOUR MEMORY DIRECTORY BEFORE DOING ANYTHING ELSE … ASSUME INTERRUPTION"). The docs put path checks, stripping sensitive data, size caps and expiring files that are never read on the handler.
- **Context editing** clears stale tool results; **compaction** summarizes server-side. Anthropic reports memory plus context editing at +39% on an internal agentic-search eval, and 84% fewer tokens on a 100-turn eval with context editing.

### OpenAI Codex CLI [doc]
- **AGENTS.md** chain: global `~/.codex/AGENTS.override.md` or `AGENTS.md`, then one file per directory from the git root to the cwd, closer files winning, cut at `project_doc_max_bytes` (**32 KiB**). Built once per session.
- **Memories**: **off by default** (`[features] memories = true`). Generated **in the background** from idle chats with separate extract and consolidation models; chats that used MCP, web or tool search can be kept out (`disable_on_external_context`). Secrets are redacted, but "not a guarantee". The docs call memories "a recall layer, not a source of rules".

### Gemini CLI and Antigravity [doc, 3p]
- Gemini CLI was replaced by the Antigravity CLI on 2026-06-18 (banner on geminicli.com).
- GEMINI.md loads hierarchically (global, workspace and ancestors, just-in-time on tool access), `@file` imports, `/memory show|reload`. The memory docs now describe three tiers of hand-edited Markdown and advise against repeating a fact across tiers. Whether `save_memory` still exists under that name is **[unverified]**.
- **Antigravity Knowledge Items** [3p, mirror of the docs; the official page returned 404]: extracted automatically from conversations, each with a title, a summary and artifacts. **Summaries are always visible; artifacts are read only when relevant.** Scoped per workspace (a forum report).

**Common pattern:** a small trusted instruction file, a machine-written store loaded as an index with bodies on demand, and caps. AGENTS.md is the shared format.

## 2. Research systems

| System | Mechanism | Problem it solves |
| --- | --- | --- |
| **MemGPT / Letta** (arXiv 2310.08560 [not re-fetched]; Letta docs [doc]) | Core memory blocks in context (label, description, value, character limit, optional read-only, shareable) edited by tool calls; recall (history) and archival (vector) memory searched by tools. Examples use 4–5k characters per block. | A stable persona and working facts inside a fixed window. |
| **Letta sleep-time agents** (arXiv 2504.13171 [paper]) | A background agent owns the memory tools and rewrites the main agent's core memory between turns, possibly on a stronger model. About 5x less test-time compute for equal accuracy. | Consolidation without slowing replies. |
| **Generative Agents** (arXiv 2304.03442 [not re-fetched]) | A memory stream scored by recency (decay) + importance (1–10, model-rated) + relevance; **reflection** when accumulated importance crosses a threshold writes higher-level insights back. | Coherent long-horizon behavior. |
| **Mem0** (arXiv 2504.19413 [paper]) | Extract facts per exchange; compare with similar memories; the model picks ADD / UPDATE / DELETE / NOOP. +26% relative over OpenAI memory on LoCoMo, 91% lower p95 latency, over 90% fewer tokens than full context. | Duplicates and contradictions. |
| **Zep / Graphiti** (arXiv 2501.13956 [paper]) | A temporal knowledge graph; edges carry validity intervals; a contradiction invalidates the old fact instead of deleting it. Up to +18.5% on LongMemEval. | Stale facts, "what was true when". |
| **A-MEM** (arXiv 2502.12110 [paper]) | Zettelkasten notes (description, keywords, tags) with automatic links; new notes can revise linked ones. | Organizing memory without a fixed schema. |
| **LangMem** [doc] | Semantic (collection or profile), episodic (good examples), procedural (refined prompts); written on the hot path or in the background. | Vocabulary and the hot-path vs background choice. |
| **Reflexion** (arXiv 2303.11366 [not re-fetched]) | After a failure, a written self-reflection conditions the next attempt. | Learning from trial and error without training. |
| **Agent Workflow Memory** (arXiv 2409.07429 [paper]) | Reusable workflows induced from past runs. +24.6% relative on Mind2Web, +51.1% on WebArena. | Procedural memory learned from experience. |
| **CoALA** (arXiv 2309.02427 [not re-fetched]) | Working memory (the context) plus long-term episodic, semantic and procedural memory; actions include retrieve and learn. | The shared vocabulary this design uses. |

## 3. Failure modes, with the evidence

| Failure | Evidence | What helps |
| --- | --- | --- |
| **Context rot** | Chroma, 18 models, 2025-07-14 [3p]: accuracy falls as input grows; on LongMemEval a focused ~300-token prompt beat the ~113k-token history. Claude Code: long files "reduce adherence" [doc]. | A small always-loaded core; everything else just in time. |
| **Memory poisoning** | AgentPoison (arXiv 2407.12784) over 80% attack success at under 0.1% poison rate; MINJA (arXiv 2503.03704) through queries alone; SpAIware (2024) persistent instructions in ChatGPT memory [paper, 3p]. | Memory is data, never instructions; provenance; review before anything reaches the instructions; Codex keeps tool- and web-derived chats out of memory. |
| **Stale and contradictory memory** | LongMemEval tests knowledge updates; MemoryAgentBench (arXiv 2507.05257) finds selective forgetting unsolved [paper]; Claude resolves conflicts "arbitrarily" [doc]. | Timestamps, sources, supersede instead of overwrite, re-check against sources. |
| **Unbounded growth** | Every runtime caps its loads (200 lines / 25 KB, 32 KiB, 24 KB) [doc]; the memory tool docs advise expiring unread files [doc]. | Hard caps with forced consolidation. |
| **Secrets and leakage** | PersistBench (arXiv 2602.01146, ICML 2026) median 53% cross-domain leakage [paper]; Codex: redaction "is not a guarantee" [doc]. | Scope memory per agent and project; refuse secret shapes on write. |
| **Sycophancy and false progress** | PersistBench median 97% memory-induced sycophancy; prompt rules alone don't fix it [paper]. Anthropic's long-running harness: agents declared victory early after reading progress notes (2025-11-26) [doc]. | Verify against the live system; structured state the agent can't casually rewrite (Anthropic kept a JSON list where agents only flip a `passes` flag). |
| **Cost** | Every always-loaded token is paid on every turn. Mem0: over 90% fewer tokens than full context [paper]. | Budgets per section; retrieval instead of preloading. |

## 4. Context engineering in practice

- **Preload a small core, fetch the rest just in time.** Keep "lightweight identifiers (file paths, stored queries, web links)" and load what's behind them when needed (Anthropic, 2025-09-29) [doc].
- **Progressive disclosure in three levels** (Agent Skills, 2025-10-16) [doc]: a name and description always loaded, the body when relevant, files on demand. The same shape is Claude's MEMORY.md index, Antigravity's KI summaries and Letta's core vs archival memory.
- **Structured notes and handoffs.** Anthropic's long-running harness: an initializer writes a progress file and a feature list; every session starts by reading git log, progress and the list, works on one feature and commits at the end [doc].
- **Subagents return condensed summaries** of 1,000–2,000 tokens [doc].
- **Budgets in use** [doc]: CLAUDE.md under 200 lines; MEMORY.md 200 lines or 25 KB; Codex's AGENTS.md chain 32 KiB; hook-injected context 10,000 characters; Letta blocks 4–5k characters; the memory tool's `view` truncates over 16k characters.

## 5. Evaluating memory

- **LongMemEval** (arXiv 2410.10813, ICLR 2025) [paper]: 500 questions across information extraction, multi-session reasoning, temporal reasoning, **knowledge updates** and **abstention**. Assistants drop about 30%.
- **LoCoMo** (arXiv 2402.17753) [paper]: very long multi-session dialogues; long-context models and RAG still lag humans.
- **MemoryAgentBench** (arXiv 2507.05257) [paper]: accurate retrieval, test-time learning, long-range understanding, **selective forgetting**.
- **PersistBench** [paper]: leakage and sycophancy caused by memory.
- **For this product** (synthesis, not from a source): a golden Q&A set per agent (owners, contracts, decisions), knowledge-update probes (change a fact, check the old one is retired), abstention probes, poisoning tests (instructions planted in task inputs must never reach the instructions), leakage probes across projects, and tokens per wake.

## 6. What this means for Agentic Org

1. Keep the runtimes' own memory off (Claude's already is; turn off Codex memories and Antigravity's knowledge items explicitly) and give every agent **one office-managed memory**, rendered into each runtime's instructions. One store also lets an agent change runtime and keep its memory.
2. Split memory by **trust**: what the owner approved can be instructions; what the agent wrote is data, fenced and labeled, and never steers the next session's instructions.
3. Split memory by **type** (CoALA): identity, semantic notes, episodic journal, procedural playbooks (this repo already has private processes for those), working handoffs.
4. Load an **index**, not the store. Hard budgets per section.
5. **Pointers over copies.** What the repo, a design or git already says is linked, not memorized.
6. **Provenance, time and supersession** on every note; never silently overwrite.
7. **Consolidate in the background**, schema-validated, off the agent's hot path.
8. **Measure it**: per-agent evals and tokens per wake.

## Sources

Fetched 2026-11-09 unless marked.
- Claude Code memory: https://code.claude.com/docs/en/memory
- Claude Code context window: https://code.claude.com/docs/en/context-window
- Claude Code hooks: https://code.claude.com/docs/en/hooks
- Claude Agent SDK features: https://code.claude.com/docs/en/agent-sdk/claude-code-features ([unverified]: the default `settingSources` differs between pages)
- Anthropic memory tool: https://platform.claude.com/docs/en/agents-and-tools/tool-use/memory-tool
- Anthropic, managing context (2025-09-29): https://claude.com/blog/context-management
- Anthropic, effective context engineering (2025-09-29): https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents
- Anthropic, Agent Skills (2025-10-16): https://www.anthropic.com/engineering/equipping-agents-for-the-real-world-with-agent-skills
- Anthropic, effective harnesses for long-running agents (2025-11-26): https://www.anthropic.com/engineering/effective-harnesses-for-long-running-agents
- Codex AGENTS.md: https://learn.chatgpt.com/docs/agent-configuration/agents-md
- Codex memories: https://learn.chatgpt.com/docs/customization/memories
- Gemini CLI GEMINI.md: https://geminicli.com/docs/cli/gemini-md/ ; memory: https://geminicli.com/docs/tools/memory/
- Antigravity knowledge items (mirror, [3p]): https://antigravity-doc.pages.dev/en/artifacts/knowledge ; scope report: https://discuss.ai.google.dev/t/workspace-knowledge-items-exposed-in-a-new-workspace/122315
- Letta memory blocks: https://docs.letta.com/guides/agents/memory-blocks ; sleep-time compute: https://www.letta.com/blog/sleep-time-compute and arXiv 2504.13171
- MemGPT arXiv 2310.08560; Generative Agents arXiv 2304.03442; Mem0 arXiv 2504.19413; Zep arXiv 2501.13956; A-MEM arXiv 2502.12110; Reflexion arXiv 2303.11366; Agent Workflow Memory arXiv 2409.07429; CoALA arXiv 2309.02427
- LangMem concepts: https://langchain-ai.github.io/langmem/concepts/conceptual_guide/
- Context rot (Chroma, 2025-07-14): https://www.trychroma.com/research/context-rot
- AgentPoison arXiv 2407.12784; MINJA arXiv 2503.03704; SpAIware (2024) [3p]: https://thehackernews.com/2024/09/chatgpt-macos-flaw-couldve-enabled-long.html
- PersistBench arXiv 2602.01146; sycophancy in context arXiv 2509.12517
- LongMemEval arXiv 2410.10813; LoCoMo arXiv 2402.17753; MemoryAgentBench arXiv 2507.05257
