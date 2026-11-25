---
name: skill-routing
description: Select project-specific skills and role guidance for a scoped assignment, check preflight, and specialize existing implementation/review agents without loading all project knowledge.
---

Use after task scoping/model routing and before direct implementation or delegation.
The shared workflow remains authoritative; this skill supplies project knowledge.
If `.agents/agent-kit/skill-catalog.json` is unconfigured/stale, use project-capabilities.
The catalog contains small descriptions and references; read bodies only as needed.

Create an ignored per-assignment folder under `.agents/agent-kit/logs/skills/TASK-WORKER/`.
Resolve the actual affected/planned paths and semantic risks identified from the
contract/source. Paths are literal repository prefixes, not language-specific rules:

```bash
python3 .agents/agent-kit/runtime/kit_skills.py resolve --path path/to/area \
  --role fullstack-engineer --phase implementation > .agents/agent-kit/logs/skills/TASK-WORKER/packet.json
```

Repeat --path for affected areas and --risk for applicable catalog risk tags.
The resolver adds matching baseline and narrower rules; broader scope selects more
guidance. Unknown risk tags, absent coverage/references or stale adoption evidence
block resolution. Do not narrow the scope or omit risks merely to select fewer skills.

Send each worker only the stable contract, required interfaces/ADRs, exact worktree
and base ref, computed model/effort, its skill packet, acceptance checks and budget.
Do not fork the full lead transcript or stuff every skill body into the prompt.
The worker reads packet sources, follows essential referenced skills/documentation,
and uses SKILL_PREFLIGHT.json to record its observed session, ordered read hashes and
applicable constraints/tools/checks. Missing capability returns a blocker. Then run:

```bash
python3 .agents/agent-kit/runtime/kit_skills.py preflight \
  --packet .agents/agent-kit/logs/skills/TASK-WORKER/packet.json \
  --ack .agents/agent-kit/logs/skills/TASK-WORKER/preflight.json
```

For a helper without shell/write tools, the lead resolves the packet, supplies
its references and records the helper's returned acknowledgment, then runs preflight.
Do not grant write/shell access just to save a declaration. A helper extracts facts
only; selected implementation guidance is context, not permission to implement.

DECLARED_READY validates references and the acknowledgment, not actual reading or
competence. The lead still checks the handoff, and tests/review validate behavior.
Source/catalog changes require fresh resolution and reading; a different worktree
needs its own packet. A critical rule also requires a conservative model/risk
assessment; it cannot be described as evidenced low-risk familiar work.

Prefer one full-stack owner. Delegate implementation only when substantial
independent work justifies the handoff and shared interfaces are already agreed.
The owner may propose/coordinate scoped fullstack-engineer delegates with different
skill packets, subject to lead authorization and a single task-wide budget. Use
separate worktrees, nonoverlapping file ownership, ready dependencies and declared
integration order. At most two concurrent writers across the whole task, including
the owner; the coordinating owner pauses its own writes while both delegates work.
Prefer direct fan-out from the main lead/feature owner. At most one nested worker
layer; flatten if the harness cannot support it within its existing thread limit.
Do not raise limits just to add managers. Children do not start another delegation
tree. The owner integrates, reconciles interfaces and checks the complete feature.

Resolve review packets with --role code-reviewer --phase review. Critical area
rules supply focused review procedures to the existing independent review passes;
do not add a review agent per framework. The Sol runner resolves/saves both review
and repair packets, injects their references and checks freshness. Read selected
guidance before analysis; missing knowledge/coverage means incomplete. A separate
specialist pass is justified only by material risk, a remaining knowledge gap or
project governance; record its scope/findings and resolve them through the existing
handoff/acceptance process. It is not an automatic extra tier or substitute for
fresh independent review, original intent validation or end-to-end checks.
