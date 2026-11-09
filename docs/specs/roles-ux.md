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

1. **A role is a doc, like a process.** Each role (software engineer, staff engineer, product designer…) is a Markdown file with front matter in the organization's handbook, `roles/<slug>.md`, with fixed sections: **Mission**, **Responsibilities**, **Working style**, **Boundaries**. Front matter: `id: doc-<ULID>`, `role: <slug>`, `title`, `department`, `version`, `owner`, `summary`, `aliases` (the words Hire's role search matches), `home_directories` (the folders made in a new agent's home, with what each is for), and suggestions only (`autonomy_default`, `model_default`). It goes through the merge flow, with versions, drift and the "why" panel, as processes do (`processes-ux.md` decisions 1, 8).
2. **Each agent can have a local override for its job,** `agents/<member id>/role.md` in the handbook, next to its private processes, naming what it overrides (`overrides: software-engineer@3`). What an override can change:
   - **Mission:** replaced.
   - **Responsibilities, Working style:** lines added; a line can be dropped only with a reason, shown on the line.
   - **Boundaries:** only added (tightened). Never removed or loosened.
   - **Not overridable at all:** autonomy, model, Can approve hires and Can approve processes, which are settings on the agent record set by the owner (`processes-ux.md` §4a, `our-org-in-the-app.md` §7.1), and the tools an agent may use, which the office decides (`agent-sessions.md`). None comes from a role or an override.
3. **Precedence: procedures and standards win.** The effective role is the role, then the agent's override on top; the processes (sealed first, then organization, the team chain, project, private) win over both. Machines can't reliably detect a conflict between prose lines, so the composed instructions say: "If your role and a process disagree, follow the process and flag it." The agent flags it (`org-agent role flag "<line>" --process <key>`). The line then shows "Flagged: may conflict with <process>" for everyone, and the flag goes to the HR lead, who drafts a role change that marks the line "Superseded by <process> (<level> v<n>)" (or rewords it); the owner approves it like any role change (decision 4). Until then the process still wins.
4. **Who approves:**
   - A role: proposed by the HR lead (or anyone through `@hr`), approved by the owner.
   - An agent's override: proposed by the agent, approved by its manager; by the owner when the agent reports to the owner, when the change touches Boundaries, or when the manager wrote it (a manager's own proposal for a report goes to the owner). A manager who is an agent gets the approval in its inbox (woken if on call) and answers `org-agent role approve <doc id>` or `role reject <doc id> --reason "…"`, as for processes (`processes-ux.md` decision 11).
   - A hire that needs a new role goes to the owner, even when an agent with Can approve hires approves the hire itself (`our-org-in-the-app.md` §7.1).
   - No one approves their own change; the owner is the exception (as for processes and hires).
5. **The role text never lives where the agent can edit it.** The office composes the role into the agent's instructions at each start from merged handbook content (with its processes, `agent-sessions.md` decision 8). The agent's home `CLAUDE.md` keeps its identity, memory and notes, not a copy of its role or boundaries (today HR copies the role into it at provisioning, which lets an agent edit its own boundaries and keeps role changes from reaching agents already hired). *(For ada, A74.)*
6. **Changing an agent's role** (**Change role…** in the agent's Settings, owner only, since the role field isn't otherwise editable) archives its old override (kept as history) and starts it with no override. The dialog says so: "ada becomes a Principal Engineer. ada's override for Staff Engineer is archived."

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

- **A role row** opens the role: its sections, its version history, the agents holding it (each with "Override" when it has one), and **Propose a change…** (a docs branch, approved by the owner).
- **Profile › Role** shows the **effective role**, every line badged with where it came from: **Role v3** · **Override** · **Superseded by <process>**, words plus icons, never color alone. A dropped line shows struck through with its reason. **Why** opens the chain (role → override → processes), as the Processes "why" panel does. Actions: **Propose an override…** (the agent's manager or the owner; the agent itself through the CLI) and, with an override, **Edit override…**.
- **Drift:** when the role changes after an override was written, the row and the Role tab show ⚠ "Role changed: Software Engineer is now v4" with **Review**: a side-by-side diff, **Keep my override** · **Take the new version** · **Edit**, as for processes (`processes-ux.md` §2).
- **States:** loading (skeleton rows) · an agent with no role file yet (brought in before roles existed: "No role on record. HR's title: <title>." with **Propose a role…** to the HR lead) · **Propose an override…** disabled for anyone but the agent's manager and the owner, with "Only <manager> or you can propose <name>'s override." · no roles yet ("Roles say what each kind of agent is for. HR writes the first ones when you hire.") · the handbook unreachable ("Can't reach the handbook. Showing the last copy from <time>.") · a role file with invalid details ("<role>: details not recognized (<field>)." with **Fix**) · an override that tries to loosen a boundary or change a setting ("This override can't loosen a boundary: <line>." / "Autonomy is set in the agent's Settings, not in a role.", refused when it's written) · a change waiting to merge ("Waiting to merge · v4 proposed by HR lead").
- **Accessibility:** badges are words plus icons; the diff has a "Changes only" list and keyboard navigation (as PROC-18).

## 2. Flow 2 · What the agent reads

- **At start:** the full effective role, in order: role, override, then "Your processes win where they disagree with your role" and the processes (`processes-ux.md` §3).
- **At wake:** only what changed since it last read it (a new role version, a new override, a superseded line).
- **CLI:** `org-agent role show` (the effective role with its layers), `role why "<line>"`, `role propose-override` (opens a handbook branch for its manager), `role flag "<line>" --process <key>`, and for approvers `role approve <doc id>` / `role reject <doc id> --reason "…"`.
- **Hire:** the Hire review shows the role (title and version) and, if the role suggests one, its autonomy default for the owner to confirm (`processes-ux.md` §4a); the new agent starts with no override.

## 3. UX acceptance criteria (ROLE)

- ROLE-1 · A role is a Markdown file with front matter in the handbook (`roles/<slug>.md`) with Mission, Responsibilities, Working style and Boundaries, changed through the merge flow and approved by the owner (proposed by the HR lead or anyone through `@hr`).
- ROLE-2 · An agent can have one override (`agents/<member id>/role.md`) that replaces the mission, adds or drops (with a reason) responsibility and working-style lines, and only adds boundaries; an override that loosens a boundary or sets autonomy, model, tools or an approval flag is refused when it's written, with the reason.
- ROLE-3 · An override is approved by the agent's manager (in its inbox when the manager is an agent, with `role approve|reject <doc id>`), or by the owner when the agent reports to the owner, the change touches Boundaries or the manager wrote it; no one approves their own change except the owner. A hire that needs a new role goes to the owner.
- ROLE-4 · The effective role is role, then override; processes win over both. The composed instructions say so. A flagged line shows "Flagged: may conflict with <process>", and once the owner approves the HR lead's role change it shows "Superseded by <process> (<level> v<n>)".
- ROLE-5 · Agents › Roles lists each role with its version, the agents holding it, how many have overrides, and drift; Profile › Role shows each line badged Role, Override or Superseded (words plus icons) with a Why chain.
- ROLE-6 · When a role changes under an override, the override shows "Role changed" with a diff and Keep my override, Take the new version, Edit.
- ROLE-7 · At start the agent gets its full effective role; at wake, only what changed since it last read it. The role text isn't kept in the agent's own home.
- ROLE-8 · `org-agent role show|why|propose-override|flag|approve|reject` work with the same words as the app.
- ROLE-9 · Change role… (agent Settings, owner only) archives the agent's old override, says so in the dialog, and starts it with none.
- ROLE-10 · The Roles views and Profile › Role cover loading, empty, the handbook unreachable, invalid details, refused overrides, a change waiting to merge, an agent with no role file yet (Propose a role…), and Propose an override… disabled with who can use it.

## 4. Questions

**For ada (architecture)**
- A74 · Roles move into the organization's handbook (`roles/<slug>.md`, Markdown with front matter, sections as above) instead of `hr/roles/*.toml`, so they share the processes machinery (merge flow, versions, drift, why); the HR lead drafts them on handbook branches, and `hr.py` reads them through the office (the handbook is office-only). Where HR's TOML fields go: `aliases` and `directories` move to the role's front matter (`aliases`, `home_directories`); `tools` stays with the office (`agent-sessions.md`), not in a role. Agents' overrides go in `agents/<member id>/role.md`. The office composes the effective role into the instructions at start and wake (your `agent-sessions.md` decision 8), and HR stops copying the role into the agent's `CLAUDE.md`, so an agent can't edit its own boundaries. This overturns "hr/ is HR's ground" for roles only. Agree, or where should they live?
- A75 · A role's `autonomy_default` and `model_default` are suggestions shown at hire, never applied by themselves (autonomy is the owner's, §4a). Any contract field needed on `AgentProfile` beyond the role slug and the override's version?
