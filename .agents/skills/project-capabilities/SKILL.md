---
name: project-capabilities
description: During adoption or material project change, assess required skills, prepare project-specific role briefs and validate scoped capability coverage for this project's actual stack.
---

The adopting agent performs this assessment in its existing session. Reuse generic
roles; create their project-specific operating guidance, not a permanent agent per
language. No framework, database or architecture is presumed from examples.
Read `.agents/agent-kit/reference/PROJECT_CAPABILITIES.md` for the catalog schema/commands.

1. Inspect actual manifests/versions, workspace members, existing local skills,
   project instructions, representative code, CI, ADRs, operations and maintenance
   responsibilities. Inventory scripts are hints, not a complete monorepo analysis.
   Identify ownership, interface boundaries and critical areas from evidence.
2. List the capabilities needed to implement and maintain this project. Reuse
   maintained project skills/conventions first. For missing procedures, write a
   concise project skill using PROJECT_SKILL.md, grounded in source and official
   version-relevant documentation. Cover implementation and review questions,
   established tools/checks, failure cases and escalation. Link longer references
   on demand. Do not fabricate expertise, install arbitrary third-party packs or
   copy all vendor manuals. Verify access to any essential external/native skill;
   unavailable essential guidance remains a blocker.
3. Create concise project briefs for staff-engineer, fullstack-engineer,
   code-reviewer and codebase-scout using PROJECT_ROLE.md. Architecture-reviewer
   uses the reviewer brief; test-runner uses the scout brief. The full-stack owner
   owns the complete feature, including all applicable layers and integration.
   Preserve native role names, model policy, permissions and verification hooks.
4. Fill `.agents/agent-kit/skill-catalog.json`: role brief paths, bounded skill descriptions,
   literal path prefixes, semantic risk tags, implementation/review references and
   critical flags. Include baseline project conventions, normally covering `.`.
   Add narrower rules for material stack/component/risk differences. Every critical
   rule needs review guidance. Do not load every skill body into the lead session.
5. Review the briefs against actual project facts. Capture stable evidence files
   with `kit_skills.py evidence --include-guidance --path ...`; record their hashes, the actual session
   and rationale, and set status ready only after the assessment. Use manifests,
   CI and architecture contracts as evidence rather than every volatile source file.
   --include-guidance also captures every referenced role/skill file and retains
   prior basis files. Review those bodies before recording hashes; changes require
   reassessment before subsequent assignments, not just during an existing run.
   If a project truly needs no extra framework-specific skill, document that decision
   and its evidence; baseline conventions and role guidance are still required.
6. Run validate, then resolve representative implementation/review assignments,
   including a critical area if present. Read the selected sources and exercise
   preflight. Verify missing/stale guidance is reported in a disposable fixture.
   Confirm the actual harness can load the chosen skills and use required tools;
   structural validation alone cannot establish understanding or live availability.
   Record coverage, limitations and evidence in ADOPTION.md. Configure/run real
   project checks through the normal adoption workflow.

Maintain this profile when dependencies/framework versions, architecture, ownership,
interfaces or critical procedures change, or repeated failures reveal missing
guidance. Changed evidence blocks validation until reassessed; do not merely refresh
hashes. Update only affected briefs/rules, retain valid rationale and rerun relevant
resolution/checks. Commit these project data files with the governed change. A new
clone/worktree reuses versioned guidance; generated task packets are checkout-bound
and must be resolved locally. No scheduled agents or automatic model calls are added.
