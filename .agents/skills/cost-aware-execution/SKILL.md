---
name: cost-aware-execution
description: Keep context, delegation, caching and review budgets efficient under the selected profile; optimize accepted work rather than token price.
---

Use task-routing and `.agents/agent-kit/policy/model-routing.json`. Load long benchmark
reports only when revisiting policy; do not paste them into every agent context.
A routing profile is a starting policy, not a measured project optimum.

- Plan in the existing lead context. A tiny task needs a short plan, not another
  planning agent. Delegate useful independent slices; handoff can cost more than
  direct work. Familiar implementation requires every evidenced criterion.
- Lookup helpers only extract bounded facts. They never implement or accept work.
  Use direct Repowise/source searches when a separate agent would add overhead.
- Sol reviews independently and fixes confirmed issues in that SAME session.
  Disputed findings return to the author. After repairs use a fresh review; stop
  when clean, findings repeat or the explicit budget is exhausted. Default three
  rounds, hard maximum five. Never drop final acceptance gates to claim savings.
- Final review focuses on original acceptance criteria, observable outputs and
  repair regressions. Use the profile's required effort; high-risk work cannot be
  silently downgraded. Same-model reviewers can share blind spots even in fresh
  contexts: retain meaningful independent tests and output evidence.

Context reuse retains knowledge; prompt caching reuses computation for a matching
prefix. Neither guarantees the other. There is no cache transfer between providers
or different models. Keep model/effort stable during a review/fix pair, resume by
explicit ID, and preserve original author context through validation. Compaction
can remove intent: save the contract, rationale, blockers and next step on disk.

Keep reusable instructions concise/stable and load skills only for the active
phase. Use bounded memory excerpts and source references, not transcripts, lockfiles
or raw logs. A per-result bound is not a whole-session budget. Stop retrieving once
answered; revalidate existing evidence instead of rediscovering every file. Use
project-memory's growth audit and explicit post-compaction intent recovery.

Provider defaults remain enabled. No synthetic keepalive/prewarm calls or automatic
TTL changes. In hybrid mode, inspect actual Claude main/subagent cache behavior;
main-session /usage does not include every child. Longer TTL can cost more to write.
Codex same-session counters are cumulative; the runner subtracts the review snapshot
for the resumed fix. Fresh review/final sessions are separate totals. Preserve raw
reports; missing counters, resets and ambiguous zeros stay null. Recheck semantics
when upgrading the CLI. Changing sandbox/schema may affect caching; do not grant
review write access to chase cache hits.

Use process-improvement for whole-task measurement: planning, workers, lookups,
all reviews, repairs, failures, checks and human time. Sol-only usage is a subset,
never another charge on top of task totals. Actual bills, API-equivalent estimates
and subscription allowance are different quantities. Cache rate, output tokens or
comment count alone cannot select the winner. Review one predeclared hypothesis
with quality guardrails and rollback; never auto-promote policy from incomplete data.
