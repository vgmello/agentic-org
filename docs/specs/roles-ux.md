---
kind: spec
title: "Roles: what each agent is for, inherited and overridden per agent (UX)"
status: for-review
revision: 1
owner: ac-product-designer-eden
created: "2026-11-09"
updated: "2026-11-09"
requested_by: owner
reviewers: [ac-staff-engineer-ada, owner]
builds_on: [processes-ux, our-org-in-the-app, agent-run-modes-ux]
---

# Spec · Roles: what each agent is for, inherited and overridden per agent (UX)

Builds on `processes-ux.md` (levels, overrides, sealing, approvals, the "why" panel, autonomy §4a), `our-org-in-the-app.md` (the agent record §5, Hire §7, the HR lead §2a) and `agent-run-modes-ux.md` (start and wake).

## Changelog

**Revision 1 (2026-11-09):** first version, from the user's request. Designed by Eden with a peer consult (the decide-and-escalate procedure).

## The request (the user, 2026-11-09)

> "role the same as teams, should be able to be overwritten by agent e.g. I'm the role it was hired [as], + local overrides for their job, the procedures and standards override these btw (the procedures and standards hierarchy)."

## 0. Decisions

1. **A role is a doc, like a process.** Each role (software engineer, staff engineer, product designer…) is a Markdown file with front matter in the organization's handbook, `roles/<slug>.md`, with fixed sections: **Mission**, **Responsibilities**, **Working style**, **Boundaries**. Front matter: `id: doc-<ULID>`, `role: <slug>`, `title`, `department`, `version`, `owner`, `summary`, `aliases` (the words Hire's role search matches), `home_directories` (the folders made in a new agent's home, with what each is for), and suggestions only (`autonomy_default`, `runtime_default`, `model_default`), shown at hire, never applied by themselves. It goes through the merge flow, with versions, drift and the "why" panel, as processes do (`processes-ux.md` decisions 1, 8).
2. **Each agent can have a local override for its job,** `agents/<member id>/role.md` in the handbook, next to its private processes, naming what it overrides (`overrides: software-engineer@3`). What an override can change:
   - **Mission:** replaced.
   - **Responsibilities, Working style:** lines added; a line can be dropped only with a reason, shown on the line.
   - **Boundaries:** only added. The check is structural, since prose can't be judged by a machine: an override that drops or rewords an inherited Boundaries line is refused when it's written; a line it adds goes to the owner (decision 4).
   - **Not overridable at all:** autonomy, model, Can approve hires and Can approve processes, which are settings on the agent record (its Settings; autonomy and the approval flags only the owner sets, `processes-ux.md` §4a, `our-org-in-the-app.md` §5, §7.1), and the tools and permissions an agent has, which the office decides (`agent-sessions.md`): the role's default permission policy stays with the office, not in the role doc (where it's kept and versioned is A74). None comes from the role doc or an override.
3. **Precedence: procedures and standards win.** The effective role is the role, then the agent's override on top; the effective processes (`processes-ux.md` decisions 2–3: the lowest level wins, sealed ones can't be overridden) win over both. Machines can't reliably detect a conflict between prose lines, so the composed instructions say: "If your role and a process disagree, follow the process and flag it." The agent flags it (`org-agent role flag "<line>" --process <key>`). The line then shows "Flagged: may conflict with <process>" for everyone, and the flag goes to whoever owns the line: for a line from the role, the HR lead, who drafts a role change that marks it "Superseded by <process> (<level> v<n>)" (or rewords it), approved by the owner like any role change; for a line from the agent's override, the agent's manager, as an override change (decision 4). Until then the process still wins.
4. **Who approves:**
   - A role: proposed by the HR lead, approved by the owner. Anyone can ask the HR lead for a change or a new role (`@hr`, **New role…**, **Propose a change…**); only the HR lead opens the role's branch.
   - An agent's override: proposed by the agent, approved by its manager; by the owner when the agent reports to the owner, when the change touches Boundaries, or when the manager wrote it (a manager's own proposal for a report goes to the owner). A manager who is an agent gets the approval in its inbox (woken if on call) and answers `org-agent role approve <doc id>` or `role reject <doc id> --reason "…"`, as for processes (`processes-ux.md` decision 11).
   - A hire that needs a new role goes straight to the owner, skipping an agent with Can approve hires: one approval for the role and the hire together, shown as "Waiting for you: new role" (`our-org-in-the-app.md` §7.1, APPR-2).
   - No one approves their own change; the owner is the exception (as for processes and hires).
5. **The role text never lives where the agent can edit it.** The office composes the role into the agent's instructions at each start from merged handbook content (with its processes, `agent-sessions.md` decision 8). The agent's home `CLAUDE.md` keeps its identity, memory and notes, not a copy of its role or boundaries (today HR copies the role into it at provisioning, which lets an agent edit its own boundaries and keeps role changes from reaching agents already hired). *(For ada, A74.)*
6. **Who can read a role and an override:** everyone in the organization. They describe what each agent's job is, which teammates need to know; only changing them is restricted (decision 4). This differs from private processes, which are the agent's own habits and visible only to the agent and the owner (`processes.md` A52, A69).
7. **Changing an agent's role** (**Change role…** in the agent's Settings, owner only, since the role field isn't otherwise editable) archives its old override (kept as history) and starts it with no override. The dialog says so: "ada becomes a Product Manager. ada's override for Staff Engineer is archived."

## 1. Flow 1 · Roles in the app

**Where:** **Agents › Roles** (a tab next to the roster), and a **Role** tab on each agent's profile.

```
 Agents › Roles                                                        [New role…]
 ─────────────────────────────────────────────────────────────────────────────────
 Software Engineer      v3   4 agents · 2 with overrides            ⚠ 1 drifted
 Staff Engineer         v1   1 agent
 Product Designer       v2   1 agent  · 1 with an override
 QA Engineer            v1   no agents yet
```

- **New role…** asks the HR lead to draft one, like `@hr`. Anyone can ask; the HR lead's proposal is approved by the owner (decision 4).
- **A role row** opens the role: its sections, its version history, the agents holding it (each with "Override" when it has one), and **Propose a change…**, which asks the HR lead to draft the change (like `@hr`; the owner approves it, decision 4).
- **Profile › Role** shows the **effective role**, every line badged with where it came from: **Role v3** · **Override** · **Flagged** · **Superseded by <process>**, words plus icons, never color alone. A dropped line shows struck through with its reason. **Why** opens the chain (role → override → processes), as the Processes "why" panel does. Actions: **Propose an override…** (the agent's manager or the owner; the agent itself through the CLI) and, with an override, **Edit override…**.
- **Drift:** when the role changes after an override was written, the row and the Role tab show ⚠ "Role changed: Software Engineer is now v3" with **Review**: a side-by-side diff, **Keep my override** · **Take the new version** · **Edit**, as for processes (`processes-ux.md` §2). In the app these are for the agent's manager and the owner; the agent does the same with `org-agent role propose-override` (`--keep` points its override at the new version, `--take-role` retires it). Each is an override change, approved under decision 4.
- **States:** loading (skeleton rows) · an agent with no role file yet (brought in before roles existed: "No role on record. HR's title: <title>." with **Propose a role…** to the HR lead) · **Propose an override…** disabled for anyone but the agent's manager and the owner, with "Only <manager> or the owner can propose <name>'s override." · no roles yet ("Roles say what each kind of agent is for. HR writes the first ones when you hire.") · the handbook unreachable ("Can't reach the handbook. Showing the last copy from <time>.") · a role file with invalid details ("<role>: details not recognized (<field>)." with **Fix**) · an override that drops or rewords an inherited boundary, or changes a setting ("An override can't remove or reword a boundary: <line>." / "Autonomy is set in the agent's Settings, not in a role.", refused when it's written) · a change waiting to merge ("Waiting to merge · v4 proposed by HR lead").
- **Accessibility:** badges are words plus icons; the diff has a "Changes only" list and keyboard navigation (as PROC-18).

## 2. Flow 2 · What the agent reads

- **At start:** the full effective role, in order: role, override, then "If your role and a process disagree, follow the process and flag it." and the processes (`processes-ux.md` §3).
- **At wake:** only what changed since it last read it (a new role version, a new override, a superseded line).
- **CLI:** `org-agent role show` (the effective role with its layers), `role why "<line>"`, `role propose-override` (opens a handbook branch for its manager), `role flag "<line>" --process <key>`, and for approvers `role approve <doc id>` / `role reject <doc id> --reason "…"`.
- **Hire:** the Hire review shows the role (title and version) and the role's suggestions, if any (its autonomy, runtime and model defaults), for the owner to confirm (`processes-ux.md` §4a); the new agent starts with no override.

## 3. UX acceptance criteria (ROLE)

- ROLE-1 · A role is a Markdown file with front matter in the handbook (`roles/<slug>.md`) with Mission, Responsibilities, Working style and Boundaries, changed through the merge flow and approved by the owner and proposed by the HR lead; anyone can ask the HR lead (`@hr`, New role…, Propose a change…).
- ROLE-2 · An agent can have one override (`agents/<member id>/role.md`) that replaces the mission, adds or drops (with a reason) responsibility and working-style lines, and only adds boundaries; an override that drops or rewords an inherited Boundaries line, or sets autonomy, model, tools or an approval flag, is refused when it's written, with the reason; added boundary lines go to the owner (ROLE-3).
- ROLE-3 · An override is approved by the agent's manager (in its inbox when the manager is an agent, with `role approve|reject <doc id>`), or by the owner when the agent reports to the owner, the change touches Boundaries or the manager wrote it; no one approves their own change except the owner. A hire that needs a new role goes straight to the owner (APPR-2).
- ROLE-4 · The effective role is role, then override; processes win over both. The composed instructions say so. A flagged line shows "Flagged: may conflict with <process>". A role line becomes "Superseded by <process> (<level> v<n>)" once the owner approves the HR lead's role change; an override line is changed through an override change (ROLE-3).
- ROLE-5 · Agents › Roles lists each role with its version, the agents holding it, how many have overrides, and drift; Profile › Role shows each line badged Role, Override, Flagged or Superseded (words plus icons) with a Why chain.
- ROLE-6 · When a role changes under an override, the override shows "Role changed" with a diff and Keep my override, Take the new version, Edit (the manager and owner in the app; the agent with `role propose-override --keep|--take-role`), each an override change approved under ROLE-3.
- ROLE-7 · At start the agent gets its full effective role; at wake, only what changed since it last read it. The role text isn't kept in the agent's own home.
- ROLE-8 · `org-agent role show|why|propose-override|flag|approve|reject` work with the same words as the app.
- ROLE-9 · Change role… (agent Settings, owner only, decision 7) archives the agent's old override, says so in the dialog, and starts it with none.
- ROLE-10 · The Roles views and Profile › Role cover loading, empty, the handbook unreachable, invalid details, refused overrides, a change waiting to merge, an agent with no role file yet (Propose a role…), and Propose an override… disabled with who can use it.

## 4. Questions

**For ada (architecture)**
- A74 · Roles move into the organization's handbook (`roles/<slug>.md`, Markdown with front matter, sections as above) instead of `hr/roles/*.toml`, so they share the processes machinery (merge flow, versions, drift, why); the HR lead drafts them on handbook branches, and `hr.py` reads them through the office (the handbook is office-only). Where HR's TOML fields go: `aliases` and `directories` move to the role's front matter (`aliases`, `home_directories`); `tools` stays with the office, as does the role's default permission policy (`agent-sessions.md` mentions it for one-offs; `permission_policy.set` is per member), not in the role doc. Where are role defaults kept and versioned? Agents' overrides go in `agents/<member id>/role.md`, readable by every member (unlike that folder's private processes: decision 6). The office composes the effective role into the instructions at start and wake (your `agent-sessions.md` decision 8), and HR stops copying the role into the agent's `CLAUDE.md`, so an agent can't edit its own boundaries. This overturns "hr/ is HR's ground" for roles only. Agree, or where should they live?
- A75 · A role's `autonomy_default`, `runtime_default` and `model_default` are suggestions shown at hire, never applied by themselves (autonomy is the owner's, §4a). `AgentProfile.role` is free text today ("software engineer") while role files use slugs (`software-engineer`): how does the record point at its role file (a slug field, or a mapping), and which version of it and of the override it last read?
