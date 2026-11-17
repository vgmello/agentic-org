---
kind: spec
title: "Roles: what each agent is for, inherited and overridden per agent (UX)"
status: for-review
revision: 5
owner: lead
created: "2026-11-09"
updated: "2026-11-15"
requested_by: owner
reviewers: [ac-staff-engineer-ada, owner]
builds_on: [processes-ux, our-org-in-the-app, agent-run-modes-ux]
---

# Spec · Roles: what each agent is for, inherited and overridden per agent (UX)

Selected role data follows [organization structure](../design/organization-data/01-organization-structure.md) and [employee identity](../design/organization-data/04-agent-structure.md). [Execution](../design/organization-data/11-model-profiles-and-agent-execution.md) separately owns profiles, models, prompts and tools. These are target data contracts; role composition/approval commands, inherited process resolution and generated harnesses are not implemented by selecting these records.

## Changelog

**Revision 5 (2026-11-15):** links the retained deferred model-policy proposal. Revision 4 selected stable-ID role files and employee `role_id`, separating execution from identity. The detailed override/approval/composition UX is retained below as a deferred proposal with individually scoped criteria. It is not an alternative selected schema or an implemented CLI.

**Revisions 1–3 (2026-11-09–10), historical:** established the requested role/override experience, generated harness requirements and fallback when HR cannot run. Their slug filenames, extra front-matter fields and `agent-os role` command examples were proposals; selected canonical data is specified below. [Original revision 3](https://github.com/vgmello/agentic-org/blob/c6e520a36c4dc7c738c68915fac5e7d5d1cbe804/docs/specs/roles-ux.md) preserves that discussion.

## The request (the user, 2026-11-09)

> "role the same as teams, should be able to be overwritten by agent e.g. I'm the role it was hired [as], + local overrides for their job, the procedures and standards override these btw (the procedures and standards hierarchy)."

## 0. Decisions

1. **A role is a shared Markdown record in organization Git:** `roles/<role-id>.md`. Front matter carries `id`, optional organization-unique descriptive `key`, `schema_version: 1`, `revision`, `name` and common creator/update attribution. The body projects to `body_markdown`. New IDs are ULID; retained IDs are not regenerated. Mission, Responsibilities, Working style and Boundaries remain useful body headings, rather than a new schema of machine-enforced prose lines.
2. **An employee optionally references one role with `role_id`.** Agents and people can both reference roles. `role_text` retains an older free-text role without inventing a matching ID. Titles/focus are employee profile fields; a team appointment is `title_in_team` on the employee's one required home team. A role name, key or display title never substitutes for its stable foreign key.
3. **Execution and authority are separate.** Agent `execution.md` selects `model_profile_id`, target `model_id`, enabled state, tools and system prompt. An ordered model profile names provider connections; local credentials stay outside organization Git. Runtime driver comes from a connection. A role does not grant tools, change budgets, choose credentials, set approval flags or create management authority. Agent placement, process readiness and manual/security/budget holds remain independent facts. People have no execution config.
4. **No selected local-role override record exists yet.** The prior override, drift, approval and process-precedence experience below remains a detailed proposal. Do not create an authored `agents/<member-id>/role.md`, a new shared field or a new CLI command from this spec. A later design must choose canonical storage, references, approvals and structural validation before these features can be implemented.
5. **Processes and standards have selected structural data only.** Their records can be organization-, team- or project-scoped and are overridable by default unless prohibited (`overridable: false`). The hierarchy/resolution/composition engine remains unselected. Its proposed precedence must not be presented as an already enforced policy.
6. **Shared visibility is the UX intent.** Role content describes work for teammates; editing remains authorized and revision-checked. No prose and no employee field grants authenticated account rights. Private task notes are unrelated author-only durable records; the role proposal does not grant an owner exception to them.
7. **Changing a role** updates employee `role_id` with its expected revision and preserves history. A future selected override feature must explicitly archive the old override on a role change; today no override artifact is assumed to exist.

Example (common attribution omitted for readability; accepted writes supply the bound actor and timestamps):

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
Explain decisions and preserve useful evidence.
## Boundaries
Follow approved processes and enforced permission policy.
```

### Earlier override/composition proposal (deferred)

The user's request remains valuable product direction. Before selecting this feature, resolve these behaviors and their storage/authorization contracts:

- **Override semantics:** replace Mission; add Responsibilities/Working style lines, dropping a line only with a recorded reason; add Boundaries only. The proposed structural check rejects dropping or rewording an inherited boundary. Machines cannot establish arbitrary semantic equivalence of prose.
- **Settings remain separate:** no override changes autonomy, model, tools, Can approve hires/processes or permission policy. Defaults proposed in older role front matter (`aliases`, `home_directories`, `autonomy_default`, `runtime_default`, `model_default`, `department`, `summary`) are not selected role fields. Role search can use selected name/key/body; defaults need a later versioned configuration decision.
- **Deferred Models experience:** the [four-slot policy proposal](agent-run-modes-ux.md#deferred-model-policy-proposal) retains Organization → Role → Agent provenance, presets, Override/Reset/Why and a hire/profile summary. It requires its own selected storage/capability contract; no role field, inherited fallback, effort clamp or helper tool is added to current execution.
- **Proposed precedence:** base role, then approved local override, with applicable procedures/standards taking priority. Proposed composed instruction: “If your role and a process disagree, follow the process and flag it.” A flagged base line routes to HR for a role-change proposal; a flagged override line routes to its author. The owner approves the base-role correction; the manager approves an override correction under the rules below. A superseded line would name its process/version. No automatic semantic conflict detector is implied.
- **Proposed approvals:** HR drafts a role and the owner approves; anyone can ask HR. An agent proposes its own override and its explicit manager approves; route to the owner when no employee manager is recorded, the change touches Boundaries or the manager authored it. No self-approval except the owner. A hire needing a new role would combine role/hire approval with the owner. Team-lead subtree responsibility and explicit reporting must be reconciled in that future routing contract; do not infer approval rights from a title.
- **Proposed composition:** office injects merged approved role/process material on start, and changed material on wake. Editable home memory/notes cannot become an authority source or widen permission boundaries. The engine must distinguish portable employee identity and execution prompt from generated output.
- **Proposed visibility/history:** role and override are readable by organization participants; line provenance, revisions, dropped-line reasons and archived overrides survive changes. Private-process visibility is an unresolved separate proposal, never a private-task-note rule.
- **Proposed generated harnesses:** replicas reflect approved persona/role/processes without copying editable notes into boundaries. Private specialists remain scoped tools rather than new employees or authority overrides; fresh-self delegates do not inherit conversation by default. This requires a separate selected execution capability.

## 1. Flow 1 · Roles in the app

The base-role list/detail and optional employee association use selected data. Override, approval, drift and Why actions described here are deferred interaction requirements; show them only after their contracts are selected and implemented.

**Where:** **Agents › Roles** (a tab next to the roster), and a **Role** tab on each agent's profile.

```
 Agents › Roles                                                        [New role…]
 ─────────────────────────────────────────────────────────────────────────────────
 Software Engineer      rev 3   4 employees
 Staff Engineer         rev 1   1 employee
 Product Designer       rev 2   1 employee
 QA Engineer            rev 1   no employees yet
```

- **New role…** asks the HR lead to draft one, like `@hr`. Anyone can ask; the HR lead's proposal is approved by the owner (the deferred approval proposal in §0).
- **A role row** opens the role: its sections, its revision history, the employees referencing it (each with "Override" when it has one), and **Propose a change…**, which asks the HR lead to draft the change (like `@hr`; the owner approves it, the deferred approval proposal in §0).
- **Profile › Role** shows the **effective role**, every line badged with where it came from: **Role v3** · **Override** · **Flagged** · **Superseded by <process>**, words plus icons, never color alone. A dropped line shows struck through with its reason. **Why** opens the chain (role → override → processes), as the Processes "why" panel does. Actions: **Propose an override…** (the agent's manager or the owner; the agent itself through the CLI) and, with an override, **Edit override…**.
- **Drift:** when the role changes after an override was written, the row and the Role tab show ⚠ "Role changed: Software Engineer is now v3" with **Review**: a side-by-side diff, **Keep my override** · **Take the new version** · **Edit**, as for processes (`processes-ux.md` §2). In the app these are for the agent's manager and the owner; the agent does the same with `agent-os role propose-override` (`--keep` points its override at the new version, `--take-role` retires it). Each is an override change, approved under the deferred approval proposal in §0.
- **States:** loading (skeleton rows) · an agent with no role file yet (brought in before roles existed: "No role on record. HR's title: <title>." with **Propose a role…** to the HR lead) · **Propose an override…** disabled for anyone but the agent's manager and the owner, with "Only <manager> or the owner can propose <name>'s override." · no roles yet ("Roles describe employees' work. Choose an existing role or request a new one when role drafting is available.") · the HR lead can't run *(rev 3)*: **New role…**, **Propose a change…** and **Propose a role…** are disabled with "The HR lead can't run right now, so it can't draft roles." (`our-org-in-the-app.md` §2a) · the organization repository unreachable ("Can't reach organization data. Showing the last copy from <time>.") · a role file with invalid details ("<role>: details not recognized (<field>)." with **Fix**) · an override that drops or rewords an inherited boundary, or changes a setting ("An override can't remove or reword a boundary: <line>." / "Autonomy is set in the agent's Settings, not in a role.", refused when it's written) · a change waiting to merge ("Waiting to merge · v4 proposed by HR lead").
- **Accessibility:** badges are words plus icons; the diff has a "Changes only" list and keyboard navigation (as PROC-18).

## 2. Flow 2 · What the agent reads

The following start/wake composition, override and approval flows are deferred requirements, not current supported entry points. Current execution reads the separate execution configuration under document 11; a referenced role alone does not authorize or launch a process.

- **At start:** the full effective role, in order: role, override, then "If your role and a process disagree, follow the process and flag it." and the processes (`processes-ux.md` §3).
- **At wake:** only what changed since it last read it (a new role version, a new override, a superseded line).
- **Earlier CLI proposal (unimplemented):** `agent-os role show` (the effective role with its layers), `role why "<line>"`, `role propose-override` (the agent's own: opens a handbook branch for its manager), `role propose-override --for <member>` (a manager's, for a report: approved by the owner, the deferred approval proposal in §0), `role flag "<line>" --process <key>`, and for approvers `role approve <doc id>` / `role reject <doc id> --reason "…"`.
- **Hire:** *(rev 3)* when the HR lead can't run, the requester can leave role_id unset or select an existing role; a new-role drafting workflow requires the HR lead (`our-org-in-the-app.md` §7). The Hire review shows the role (name and revision) and any separately selected execution suggestions for the owner to confirm; suggestions are not role front-matter fields (`processes-ux.md` §4a); the new agent starts with no override.

## 3. UX acceptance criteria (ROLE)

- ROLE-1 · Selected target: shared `roles/<role-id>.md` uses stable ID, optional key, schema version, revision, name, common attribution and Markdown body. Mission, Responsibilities, Working style and Boundaries are body sections. Role drafting/approval remains deferred.
- ROLE-2 · Deferred: one local override with the proposed Mission/add/drop/Boundaries rules. No selected canonical override path or write service exists. Refusal messages must explain removed/reworded boundaries and settings placed in the wrong configuration.
- ROLE-3 · Deferred: approved overrides follow the manager/owner routing above, no self-approval except owner, and new-role hires route to the owner. A future authorization contract must reconcile explicit reporting with subtree lead responsibility.
- ROLE-4 · Deferred: the proposed composition puts processes above role/override and records flagged/superseded line provenance. Structural process records alone do not implement this precedence engine.
- ROLE-5 · Selected base view lists role name, revision and employee count and shows body/provenance/history. Deferred override counts, drift and line-level Why appear only with a selected override engine. Badges use words plus icons.
- ROLE-6 · Deferred: revision drift offers an accessible side-by-side diff plus Changes only list, Keep my override, Take the new version and Edit, with the same approval rules. Keyboard navigation preserves focus.
- ROLE-7 · Deferred: approved composition at start and deltas at wake must not copy editable home notes into authority-bearing instructions. Selected execution configuration remains distinct from identity and role records.
- ROLE-8 · Deferred: no `role show|why|propose-override|flag|approve|reject` CLI is claimed available. A later versioned contract must specify caller authorization, revisions, retries and parity with the app before these earlier command sketches are supported.
- ROLE-9 · Selected target: optional employee `role_id` updates use the expected employee revision; unknown free-text `role_text` is preserved without inventing a role. Deferred override archives must be resolved when that feature is selected.
- ROLE-10 · Selected base views cover loading, no roles, no role association, stale/unreachable organization data, invalid role detail and revision conflicts without losing input. Deferred drafting/override actions additionally explain unavailable HR, pending approvals/merge, invalid overrides and authorized actors. Empty copy never promises HR can run.
- ROLE-11 · Deferred: generated employee harnesses and private specialists must preserve approved authority, persona and process boundaries; no generated definition creates an employee, grants permissions or replaces role approvals.

## 4. Questions

- A74 · Selected storage is resolved: `roles/<role-id>.md` in organization Git; fields/body follow document 01. Still pending: local override storage, line identities, role defaults, permission-policy versioning and role/process composition/approval services. Do not port the earlier TOML or slug-path proposal as a parallel canonical format.
- A75 · Selected association is resolved: optional stable `employees.role_id` alongside preserved `role_text`. Last-read role/override revisions, drift and start/wake delivery need the deferred engine's own contract. Execution profile/model settings belong to `execution.md`, never to the role or identity record.
