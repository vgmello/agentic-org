---
name: task-routing
description: Plan new work, assess evidence and assign the installed profile's model and effort to each implementation slice.
---

The lead owns one overall plan. Before substantive planning, use process-improvement
to register the stable request/acceptance contract, separately from the evolving
plan. Workers refine assigned slices; they do not restart top-level planning.
Read `.agent-kit/policy/model-routing.json` and use cost-aware-execution. The active
mode is explicit: hybrid uses Claude planning/workers and Sol review; codex-only
uses Sol throughout. Never silently switch profiles because a model is unavailable.

Copy TASK_ASSESSMENT.json from `.agent-kit/templates` to `.agent-kit/tasks/TASK-assessment.json`.
Give source/contract/check evidence for every known fact. Unknown stays null. Run:

```bash
python3 .agent-kit/runtime/kit_policy.py --assessment .agent-kit/tasks/TASK-assessment.json
```

Save the result with the slice contract. Code maps facts to model/effort; the lead
remains accountable for the evidence's truth. Use returned planner/worker/review
values, not remembered provider tiers. Architecture review uses architecture_review.
Familiar implementation requires ALL seven facts: a working source analogue,
precise behavior/failure cases, localized interfaces without design work, meaningful
deterministic checks, low security/data/concurrency risk, no unexplained failed
repair, and resolved ambiguity. Missing facts escalate rather than qualify by default.

1. Scope the request with relevant decisions/source. Use bounded-lookup only when
   a focused extraction will save context; direct known-symbol queries need no agent.
2. Plan in the lead's existing context with the returned planning effort. If that
   context cannot supply the required model/effort, use a correctly configured
   planning agent. Small tasks need a short plan, not a committee.
3. For large/complex tasks, use architecture-decisions and independent plan-review
   at stage ARCHITECTURE BEFORE detailed breakdown. Preserve premise IDs, evidence,
   rationale, alternatives and checks. Record READY on the decision revision;
   unresolved feasibility or blocking findings prevent affected implementation.
4. Write ordered, end-to-end slices with acceptance checks, ownership, dependencies,
   exact checkout/base refs, expected files, model/effort, escalation and budget.
   Link reviewed decisions and premise IDs. The lead owns shared interfaces and
   integration; workers refine local steps without separate reviews of every step.
5. Review detailed planning only when it introduces new security/data/concurrency
   risk, cross-repo dependencies, unresolved ambiguity or material design/interface
   changes. Use a fresh independent plan-review at IMPLEMENTATION_PLAN. A changed
   architecture returns to step 3. Otherwise record why another pass is unnecessary.
   One pass plus one focused recheck is the default; disagreement returns to the
   lead for diagnosis instead of an unbounded review loop.
6. Dispatch only ready slices. Use fullstack-engineer for implementation delegates
   with explicit returned model/effort; verify effective settings in the harness.
   Use skill-routing to select the project's role brief and relevant skills, and
   require the recipient's preflight. Match actual paths and semantic risks; a
   critical-area rule cannot qualify as low-risk familiar implementation. Missing
   coverage returns to project-capabilities for evidence-backed adaptation.
   One worker is the default, at most two independent writers in separate worktrees.
   Direct lead work is preferable when handoff overhead exceeds the benefit.
   A full-stack owner may coordinate independent implementation delegates after
   agreeing interfaces/ownership and the lead's task-wide budget. Keep shallow
   delegation; the owner integrates and validates the entire feature.
7. Unexpected design work, scope/interface changes, or an unproductive repair
   returns to the lead. A missing/invalid architectural premise pauses affected and
   dependent work. Use architecture-decisions to preserve valid constraints, review
   a material replacement and update plans/routing/checks before resuming.

Keep original author sessions through intent validation. Use focused worker
contexts and resume related fixes; preserve intent/checkpoints before compaction.
New reviewers inspect source independently before earlier findings. Neither a plan
nor a worker's successful stop hook establishes final implementation acceptance.
Use review-cycle, including the profile's separate final code/output review.

A Codex native child inherits parent permissions; role text is not a separate
sandbox. Use the wrapper's read-only fresh exec processes for independent code
reviews and plan-review's read-only command for architecture review. Architecture
and planning readiness remain recorded agent decisions, not authenticated gates.

For Codex worker dispatch, pass the selected `model` and `reasoning_effort` with
`fork_turns="none"` when the native spawn tool exposes those fields. Supply the
bounded contract and evidence references, not the whole lead transcript. Some
clients hide overrides: then use a fresh `codex exec` with explicit `-m` and
`-c 'model_reasoning_effort="SELECTED_EFFORT"'`, `-s workspace-write`,
`-c 'agents.enabled=false'`, JSON events and a saved output file. Retain the actual
thread ID and resume that exact writer for intent validation. Never assert that
prompt text changed a role's High default to Medium/Xhigh. Verify the effective
settings; unavailable required effort stays blocked. CLI worker time/usage belongs
in the whole-task journal, including any failed launch.
