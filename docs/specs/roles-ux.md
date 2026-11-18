---
kind: spec
title: "Roles: what each agent is for (UX)"
status: current
owner: lead
created: "2026-11-09"
updated: "2026-11-17"
requested_by: owner
builds_on: [processes-ux, our-org-in-the-app, agent-run-modes-ux]
---

# Spec · Roles: what each agent is for (UX)

Role data follows [organization structure](../design/organization-data/01-organization-structure.md) and [employee identity](../design/organization-data/04-agent-structure.md). [Execution](../design/organization-data/11-model-profiles-and-agent-execution.md) owns profiles, models, prompts and tools.

## The request (the user, 2026-11-09)

> "role the same as teams, should be able to be overwritten by agent e.g. I'm the role it was hired [as], + local overrides for their job, the procedures and standards override these btw (the procedures and standards hierarchy)."

v1 delivers shared roles and the employee's reference to one role. Per-agent overrides are [not in v1](#4-not-in-v1).

## 0. Decisions

1. **A role is a shared Markdown record in organization Git:** `roles/<role-id>.md`. Front matter carries `id`, an optional organization-unique descriptive `key`, `schema_version: 1`, `revision`, `name` and the common creator and update attribution. The body projects to `body_markdown`. IDs are UUIDv7. Mission, Responsibilities, Working style and Boundaries are the usual body headings; they are prose, not machine-enforced fields.
2. **An employee optionally references one role with `role_id`.** Agents and people can both reference roles. `role_text` is a free descriptive role, independent of `role_id`; a role is never invented from it. Title and focus are employee profile fields; a team appointment is `title_in_team` on the employee's one home team. A role name, key or title never substitutes for the stable foreign key.
3. **Execution and authority are separate.** An agent's `execution.md` selects `model_profile_id`, target `model_id`, enabled state, tools and system prompt. A model profile names provider connections; credentials stay outside organization Git. A role does not grant tools, change budgets, choose credentials, set approval flags or create management authority. Agent placement, process readiness and manual, security or budget holds are independent facts. People have no execution configuration.
4. **Processes and standards are separate records** scoped to the organization, a team or a project, overridable unless marked `overridable: false`. The [processes design](../design/processes.md) owns how the set for a task is resolved.
5. **Role content is shared.** It describes work for teammates; editing is authorized and revision-checked. No prose and no employee field grants account rights.
6. **Changing an employee's role** updates `role_id` with the expected employee revision and keeps history.

Example (common attribution omitted; accepted writes supply the bound actor and timestamps):

```markdown
---
schema_version: 1
id: existing-role-id
revision: 3
key: software-engineer
name: Software Engineer
---
## Mission
Build reliable software within the assigned team's scope.
## Responsibilities
Implement and review assigned work.
## Working style
Explain decisions and keep useful evidence.
## Boundaries
Follow approved processes and enforced permission policy.
```

## 1. Flow 1 · Roles in the app

**Where:** **Agents › Roles** (a tab next to the roster), and a **Role** tab on each agent's profile.

```
 Agents › Roles                                                        [New role…]
 ─────────────────────────────────────────────────────────────────────────────────
 Software Engineer      rev 3   4 employees
 Staff Engineer         rev 1   1 employee
 Product Designer       rev 2   1 employee
 QA Engineer            rev 1   no employees yet
```

- **New role…** opens an editor for a new role record. Asking the HR lead to draft one, like `@hr`, is available when the HR lead can run.
- **A role row** opens the role: its sections, its revision history, the employees referencing it, and **Edit**.
- **Profile › Role** shows the referenced role, its revision and its body, or the employee's `role_text` when no role is referenced. A person with authority can change the referenced role.
- **States:** loading (skeleton rows) · an employee with no role ("No role on record. Title: <title>." with **Choose a role…**) · no roles yet ("Roles describe employees' work. Create one, or ask HR to draft one.") · the HR lead can't run: asking HR to draft is disabled with "The HR lead can't run right now, so it can't draft roles." (`our-org-in-the-app.md` §2a) · organization data unreachable ("Can't reach organization data. Showing the last copy from <time>.") · a role file with invalid details ("<role>: details not recognized (<field>)." with **Fix**) · a revision conflict keeps the person's input and shows the newer version.
- **Accessibility:** labels are words plus icons; keyboard navigation throughout.

## 2. Flow 2 · What the agent reads

The office composes an agent's instructions ([session design](../design/agent-sessions.md), decision 8) from its identity and role (`profile.md`, including the referenced role's body), its prompt and tools (`execution.md`), the turn's explicit objective, the effective processes and standards, built-in `agent-os` help, and the project's instruction files from the verified integration-branch cache. A referenced role grants no authority and starts nothing. Composing a per-agent role override is not in v1.

- **Hire:** when the HR lead can't run, the requester can leave `role_id` unset or select an existing role (`our-org-in-the-app.md` §7). The Hire review shows the role (name and revision) and any execution suggestions for the owner to confirm; suggestions are not role fields.

## 3. UX acceptance criteria (ROLE)

- ROLE-1 · Shared `roles/<role-id>.md` uses stable ID, optional key, schema version, revision, name, common attribution and a Markdown body. Mission, Responsibilities, Working style and Boundaries are body sections.
- ROLE-2 · Not in v1: a per-agent role override.
- ROLE-3 · Not in v1: approval routing for role overrides.
- ROLE-4 · Not in v1: composing role and override text with processes into an agent's instructions with per-line provenance.
- ROLE-5 · The roles view lists role name, revision and employee count and shows body, provenance and history. Labels use words plus icons.
- ROLE-6 · Not in v1: drift review between a role revision and an override.
- ROLE-7 · An agent's instructions are composed by the office from its identity and role, its execution configuration, the turn's objective, the applicable processes and standards, `agent-os` help and the project's instruction files (session design, decision 8); editable home notes never become authority-bearing instructions, and execution configuration stays distinct from identity and role records.
- ROLE-8 · Withdrawn: there is no `agent-os role` command family. An agent reads roles with `agent-os hr roles` and `hr inspect-role` ([agent command line](../design/agent-os-command-line.md), section 3.2).
- ROLE-9 · An employee's `role_id` is updated with the expected employee revision; free-text `role_text` is kept without inventing a role.
- ROLE-10 · Role views cover loading, no roles, no role association, stale or unreachable organization data, invalid role details and revision conflicts without losing input. Empty copy never promises that HR can run.
- ROLE-11 · Not in v1: generated employee harnesses and private specialists.

## 4. Not in v1

- **Per-agent role overrides** and everything built on them: line-level rules for what an override may replace, add or drop, approval routing (agent proposes, manager approves, owner for boundaries), drift review when the role changes, composing role, override and processes into an agent's instructions with "Why" provenance, and the matching command-line actions.
- **Role defaults** for model, runtime or autonomy. Model choice lives in execution configuration ([run modes](agent-run-modes-ux.md#5-not-in-v1)).
- **Generated harnesses** that replicate an employee's persona and processes.
