---
name: code-navigation
description: Navigate source and verify symbols using installed language tools, targeted search and shared project knowledge in Claude Code or Codex.
---

Read AGENTS.md/CLAUDE.md as applicable, active `.agents/agent-kit/DECISIONS.md` entries and
`.agents/agent-kit/PROJECT_MAP.md`. Verify map references against current source.

1. For large-project exploration, use indexed-context and the shared checkout-bound
   lookup tool first. Reuse returned source and narrow follow-ups. Use `rg --files`
   and `rg -n` for known literals, unsupported files or a reported index gap; avoid
   whole-repository dumps. Use project-memory for scoped durable knowledge.
2. Use Repowise `context` for relationships and `symbol` for precise current source.
   Do not add another navigation server or index during adoption or review.
3. Verify semantic correctness with targeted search plus the project's installed
   compiler/type checker: local TypeScript `tsc --noEmit`, Python's configured
   pyright/mypy, Go checks, or the repository's existing equivalent. During a
   read-only review, ask the runner/lead for check evidence if the tool needs writes.
   Do not download packages or start unfamiliar MCP servers during a review.
4. Record definition/caller paths, limits of dynamic resolution, exact commands
   and missing tooling. Missing navigation is not proof that no callers exist.

Both harnesses run the same `.agents/agent-kit/verification.json` checks via
`python3 .agents/agent-kit/runtime/kit_hooks.py check`. Each adapter has native hooks;
Codex hooks require local support and project/hook trust. Its review runner also
explicitly checks before acceptance and after fix commits. Use installed formatters
as needed; a configured hook is not evidence that it ran in this environment.
