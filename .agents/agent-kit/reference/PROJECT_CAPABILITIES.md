# Project-specific skills and full-stack delegation

Kit 0.9.0 adds an agent-led capability assessment to adoption, alongside tool/check
setup. The agent derives the necessary skills and role guidance from the actual
project. No particular UI framework, backend language or database is assumed.

The native roles remain stable. Their project-specific versions consist of the
generic role plus a concise local role brief and assignment-selected skills. This
keeps existing model/permission/verification gates intact. The adopting agent creates
these project data files, validates them and records remaining capability gaps.

## Catalog and project role briefs

Use the installed project-capabilities skill. Put new concise briefs under
`.agents/agent-kit/project-roles/` and `.agents/agent-kit/project-skills/`, or reference existing
maintained repository skill/convention files directly. Do not duplicate a useful
project guide just to satisfy a new directory layout. Official external references
belong inside the relevant brief and are consulted on demand.

`.agents/agent-kit/skill-catalog.json` has these fields:

| Field | Meaning |
| --- | --- |
| version | 1 |
| status | `needs_adaptation` until agent assessment/review; then `ready` |
| reviewed_by, rationale | Actual adopting/reviewing session and why this skill coverage fits the project |
| project_evidence | Manifest/CI/architecture evidence plus every referenced role/skill file, with reviewed SHA-256 hashes |
| roles | Paths to project briefs for `staff-engineer`, `fullstack-engineer`, `code-reviewer`, `codebase-scout` |
| skills | Bounded rules with the fields below |

Each skill rule has `id`, `description`, `paths`, `risks`, `implementation`,
`review` and `critical`. IDs/risk tags use lowercase letters, digits and hyphens.
Paths are literal repository-relative files/directory prefixes. `.` covers the
project. Each phase contains local instruction-file references; a phase can be
empty when the rule only applies to the other phase. Critical rules must supply
review guidance. A requested path matches overlapping prefixes in either direction;
scoping a whole directory selects guidance for its children too. Rules with risk
tags additionally require a matching declared semantic risk. Every requested risk
must have phase-appropriate coverage. Each requested path also needs phase guidance
at that path or an ancestor; a child-only rule cannot cover an entire parent scope.
Path rules never establish absence of semantic
risk; the lead still assesses the actual behavior and task contract.

Illustrative rule structure, to replace with evidence from the target project:

```json
{
  "id": "project-conventions",
  "description": "Shared project standards and integration checks",
  "paths": ["."],
  "risks": [],
  "implementation": ["docs/engineering.md"],
  "review": ["docs/engineering.md"],
  "critical": false
}
```

Add narrower rules for actual component procedures and critical areas. The default
catalog is intentionally empty/unconfigured; the installer cannot infer architectural
or domain knowledge. `scripts/adopt.py` inventories existing skills/guidance, while
the agent reads the relevant evidence and supplies the semantic assessment.

## Commands and checks

Capture the evidence for the assessment after inspecting it:

```bash
python3 .agents/agent-kit/runtime/kit_skills.py evidence --include-guidance --path path/to/manifest --path path/to/architecture.md
python3 .agents/agent-kit/runtime/kit_skills.py validate
python3 .agents/agent-kit/runtime/kit_skills.py resolve --path path/to/component --role fullstack-engineer --phase implementation
python3 .agents/agent-kit/runtime/kit_skills.py resolve --path path/to/component --role code-reviewer --phase review
```

Use the evidence output in the catalog. `--include-guidance` includes all referenced
role/skill files and retains previously recorded basis files. It computes hashes
without setting status or approving content; review the guidance before recording
the output. A modified role/skill file invalidates future readiness until reassessed.
Validation checks source freshness, required
roles, bounded local references and review coverage for critical rules. It makes no
model calls, runs no project commands and installs no external skills/plugins.
`kit_doctor.py` reports incomplete/stale capability setup as an error. Changed
evidence requires reassessment; updating a hash alone is not knowledge maintenance.

Resolution emits metadata and hashes, not skill bodies. The recipient reads the
selected files and essential references, records a preflight using SKILL_PREFLIGHT.json,
and runs `kit_skills.py preflight --packet PATH --ack PATH`. Missing, changed or
cross-checkout packets and incomplete acknowledgments fail. This is a local declaration
of reading/applicability, not a proof of understanding or model identity.
The lead records and validates a read-only helper's returned acknowledgment; the
helper does not gain shell/write tools for this purpose.

## Full-stack ownership and review

One owner remains responsible for the complete user outcome and integration.
Small/tightly coupled changes stay with that owner. Large independent pieces can
use multiple fullstack-engineer instances, each with its own skill packet, worktree,
file ownership and checks. Agree shared interfaces first. Count writers across the
entire task: default one, maximum two; a coordinating owner must not become a third.
Keep delegation shallow and within the client's thread limits. The main lead can
also be feature owner; no extra manager is required.

The owner integrates the pieces and verifies cross-layer behavior, errors, migrations
and output states wherever applicable. Each author retains its context for validation
of later repairs; the integration owner validates combined behavior and contracts.

The existing fresh Sol review/fix cycle automatically receives selected review and
implementation skill references. The final pass receives the same relevant review
guidance. Critical rule matches reject `low_risk=true` classifications. Missing/stale
capability setup blocks the runner before model use. Skill guidance changes during a
run invalidate its packet. Normal specialist focus adds no extra review session;
a separately justified expert pass must have a bounded purpose and feed its findings
back into existing acceptance. No autonomous specialist fleet is created.

## Upgrades and maintenance

The catalog and project briefs are versioned project data, preserved on kit upgrades.
Keep native adapter role files managed by the installer; put local specialization in
the referenced briefs. A 0.8 project needs this one-time capability assessment after
upgrading; it is not silently declared ready. New worktrees share committed catalog
and briefs, but resolve fresh packets for their own checkout. Material stack,
architecture, ownership or recurring quality changes trigger focused reassessment.

The mechanism is deterministic about selection and evidence freshness. Agents still
judge what skills are needed and whether they are correct. Real execution, output
checks and independent review remain essential. Actual time/token savings must be
measured through process-improvement; this feature does not claim them in advance.
