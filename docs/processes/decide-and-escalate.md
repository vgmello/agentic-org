---
process: decide-and-escalate     # the match key for overrides, unique per level and scope
name: Decide, consult, escalate
kind: process
level: organization          # organization | team | project | agent (private)
version: 1
owner: ada (Staff Engineer)
status: active
summary: Decide routine calls yourself; when unsure, reach consensus with a fresh peer; escalate only critical or unresolved decisions, with options and a recommendation.
applies_to: every agent
overridable: true             # a team or project may tighten it (e.g. add to the critical list)
---

> The user's standard procedure (2026-11-09). Kept in this repository (`docs/processes/`) until the organization's handbook repository exists.

# Decide, consult, escalate

How every agent makes decisions. The goal: work keeps moving, people are asked only what truly needs them.

## 1. Decide yourself
- Routine and reversible calls are yours: use your best judgment and act. Don't ask "should I?".
- Write down what you decided and why, where the work is (the task, the branch's commit message, your journal).

## 2. When you're unsure, consult a peer
- Start a fresh peer (a subagent or another agent) and give it the question and the facts, not your conclusion.
- Bounce the options until you agree on what's best. Then act on it, and record the decision, the options you weighed and the consult's view.

## 3. Escalate only when it's needed
Go to your lead (or, at the top, the owner) only when:
- **there's no consensus** after the consult; or
- **the decision is critical**: security, data loss, money or spend, anything irreversible or one-way, legal, or product scope and priority; or
- **you're blocked** by something only they can do (an approval, a credential, a purchase, starting a session you can't start).

An escalation brings: the question, the options, your recommendation, and what the consult agreed or disagreed on. One message, short.

## 4. Keep moving
- While an escalation waits, keep working on everything that doesn't depend on it.
- When the answer comes, record it with the decision.
