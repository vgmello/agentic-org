---
name: plan-review
description: Independently review architecture before detailed planning, or new implementation-plan risks before dispatch; explicit PLAN REVIEW mode only.
---

Use an independent reviewer in a fresh context with the active profile’s
`architecture_review` model/effort when
task-routing's threshold applies. Record stage ARCHITECTURE or IMPLEMENTATION_PLAN.
Large/complex tasks require ARCHITECTURE review before detailed task breakdown;
routine bounded work can stay with the lead. Never implement, delegate or treat
architecture/plan readiness as code acceptance.

Inspect the original request, plan/contract revision, active decisions and focused
source references. For ARCHITECTURE, read the canonical decision record, original
premises/rationale and any predecessor/challenge evidence. Verify requirements,
observed facts and assumptions separately; unsupported claims remain UNVERIFIED.
A failed mechanism does not invalidate the reason it was chosen. Check which valid
constraints the replacement retains, and demand evidence for rejected premises.
Missing essential feasibility evidence means INCOMPLETE. Use architecture-decisions
for the escalation/history protocol. For IMPLEMENTATION_PLAN, focus on new risks
and consistency with the reviewed architecture; do not repeat unchanged analysis.
Check scope, architecture assumptions, interfaces, dependencies, integration order,
ownership, rollback/failure cases and whether acceptance checks
can establish the intended behavior. Prefer the smallest sufficient design.
Identify concrete omissions or risks; do not rewrite the entire plan or invent
stylistic work. Ask for missing material evidence rather than assuming it exists.

Return a concise READY / CHANGES_REQUIRED / INCOMPLETE assessment with the stage,
decision/plan revision, reviewer/session/model/effort, findings tied to premise/plan/
source references and required resolutions. Use observed metadata; never invent IDs.
Missing essential evidence means INCOMPLETE. The lead records architecture review
in the decision record, plan review in the contract, and resolves blockers before
the next stage. Default to one pass and, if needed, one focused recheck; persistent
disagreement returns to the lead for diagnosis.

Do not require a code diff or passing implementation checks before code exists.
Do not return FINAL_REVIEW.json or invoke the final acceptance runner for a plan
review. The eventual final code/output review requires another new agent instance.

In hybrid mode, invoke a fresh Claude code-reviewer with the policy model/effort.
Use skill-routing for the proposed affected paths/risks at phase review; supply
the project reviewer brief and selected architecture/critical-area procedures.
Missing relevant guidance is incomplete coverage, not READY.
In codex-only, use a fresh read-only CLI process so independence and write
permissions do not depend on a native child inheriting the parent’s context or
sandbox. Save a short prompt referencing the original request, decision revision
and relevant source, then run (create the ignored output folder first):

```bash
HINDSIGHT_DISABLED=true AGENT_KIT_MEMORY_ACCESS=review \
codex exec -m gpt-6.2-sol -c 'model_reasoning_effort="xhigh"' \
  -c 'agents.enabled=false' -c 'features.hooks=false' \
  -c 'mcp_servers.hindsight.enabled=false' -c 'approval_policy="never"' -s read-only --json \
  --output-schema .agent-kit/templates/PLAN_REVIEW_SCHEMA.json \
  --output-last-message .agent-kit/logs/plan-review/result.json \
  - < .agent-kit/logs/plan-review/prompt.txt > .agent-kit/logs/plan-review/events.jsonl
```

Use a unique folder per pass; save its observed thread ID and actual completion
status. Do not fork implementation history, use --last, or infer READY from process
exit alone. Missing auth/evidence stays INCOMPLETE. The lead evaluates findings and
records readiness on the actual decision/plan revision; this is not an authenticated
approval service. Codex native architecture-reviewer is available only when the parent already
enforces read-only execution, disables native Hindsight hooks/MCP, and restricts
kit memory to review-phase bounded recall. Otherwise use the fresh CLI above.
Custom roles inherit parent MCP servers; role TOML cannot override that MCP or
permission configuration. Do not infer memory isolation from a read-only shell.
Reference: https://learn.chatgpt.com/docs/agent-configuration/subagents .
