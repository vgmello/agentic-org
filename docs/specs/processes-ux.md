---
kind: spec
title: "Processes: how agents work, written down and inherited (UX)"
status: current
owner: lead
created: "2026-11-08"
updated: "2026-11-18"
builds_on: [decisions-and-tech-debt-ux, work-items-ux, our-org-in-the-app, offices-and-teams-ux, agent-run-modes-ux, app-shell-ux]
---

# Spec · Processes: how agents work, written down and inherited (UX)

The data contract owns the records: [organization structure](../design/organization-data/01-organization-structure.md), [project structure](../design/organization-data/02-project-structure.md), [identity](../design/organization-data/06-identity-and-numbering.md) and [record contracts](../design/organization-data/08-record-contracts.md). The [processes design](../design/processes.md) owns resolution and delivery (decision 4). This spec owns what people see. A project's process package and its skills are the owner's [REQ-07](req-07-composable-process-packages-and-team-blueprints.md), designed in [process packages](../design/process-packages.md); this spec shows where its skills appear among a project's processes.

## The request (the user, 2026-11-08)

> A **process** is a natural-language text file added to the organization. Agents look up their processes when they start, to understand what they have to do. Processes exist at the **organization** level, can be **overridden** at the **team** level, and a team can have its own. Levels: organization → team → **project** (the lowest). A lower level overrides a particular process from the level above, or adds its own. An agent can also have its own **private** processes. Processes are committed to the repo. This matters so we understand how the agents work.

v1 delivers process and standard content, inheritance across organization, team and project, and delivery to the agent at launch. Private processes, approvals, enforcement and autonomy levels are [not in v1](#6-not-in-v1).

## 0. Decisions

1. **Content:** Markdown with a stable ID, an optional key, schema and revision, kind `process` or `standard`, title, lifecycle Draft, Current or Deprecated, an accountable owner, common attribution, an optional team or project scope, and `overridable` (default true). Current requires an active owner. An override is another record with its own ID and the same key. IDs are UUIDv7. A key or display reference never replaces identity.
2. **Scopes and resolution:** organization documents live at the organization root, team variants under the full recursive stable-ID team path, and project variants under `projects/<project-id>/processes/` or `standards/`, all in organization Git on its state branch. The set that applies to a piece of work is the organization's Current documents, then the accountable team's chain from the top team down, then the project's; at each step a lower level adds a document with a new key or replaces one with the same key ([processes design](../design/processes.md), decision 4). Work has one accountable team, so there is one chain; assignment, home team and contributing teams add no documents.
3. **A level can forbid replacement.** A document marked `overridable: false` cannot be replaced below it. A lower document that tries is refused when saved, and is ignored with a visible problem if it already exists. A document stops applying when its own level deprecates it; there is no "off" state.
4. **Delivery at launch.** The office gives the agent the effective set for its work. Every non-overridable document is included in full in a clearly marked must-follow section of the agent's system instructions. Overridable documents are guidance: summaries first, full text retrievable, within the size cap of the processes design. If the non-overridable set alone exceeds the instruction budget, the launch is refused with a problem naming the documents.
5. **Guidance on using the product's own commands** ships with the product as built-in skills, not as organization documents.
6. **The product tells the agent what binds it.** It does not mechanically block an action for breaking a process.
7. **A project's process package adds a layer.** The skills of the package a project follows apply after the team's documents and before the project's own; they are guidance, shown with a **Package** badge and the package's name. A project document with a skill's key replaces it for that project.
8. **Agents write for their own project.** An agent may create and change process and standard documents at the scope of a project it works in, never at team or organization scope, and never as not overridable.

## 1. The process file

```markdown
---
schema_version: 1
id: existing-process-id
key: review-and-merge
revision: 3
kind: process
title: Review and merge
status: current
owner_employee_id: existing-employee-id
team_id: null
project_id: null
overridable: true
created_at: "2026-11-15T10:00:00Z"
created_by_kind: employee
created_by_id: existing-employee-id
updated_at: "2026-11-15T10:00:00Z"
updated_by_kind: employee
updated_by_id: existing-employee-id
---
## Purpose
## Procedure
## Review and completion
```

Saved as `processes/<document-id>.md` at organization scope; nested team and project paths use the same stable-ID filename. Standards use `kind: standard` and describe constraints such as stack or security rules. Ownership is accountability, not an authorization grant. Scope fields and path must agree, and a document never names both a team and a project.

## 2. Flow 1 · Processes in the app

**Where:** a **Processes** item in the main navigation, separate from Knowledge; a **Processes** tab on each team and in each project's Docs; and a **Processes** tab on each agent's profile.

```
 Processes · checkout (project)                                         [New process]
 ───────────────────────────────────────────────────────────────────────────────────
 One open branch                 Organization rev 1  not overridable              applies
 Review and merge                Organization rev 3  overridable                  applies
 Code review                     Team Checkout rev 2 (replaces Organization)      applies
 Release checklist               Project rev 1 (added here)                       applies
```

- **The inheritance view:** each document, matched across levels by its key, with the level it comes from (Organization · Team · Package · Project badges), its revision, and whether it replaces one above, is added here, or is not overridable. For a project that follows a process package, the package's skills are listed with the roles each is for; a skill is opened read-only here and edited in the package.
- **Override for this team or project:** copies the inherited text as the starting point, with the same key and a new ID; the difference from the parent is one click away. Not available for a non-overridable document.
- **What this agent follows** (agent profile › Processes): the effective set for its current work, each with its source. With no current work: "No current work. Showing the organization's processes. Team and project processes apply when it starts a task."
- **New process:** opens a new document in organization Git with the scope preset.
- **States:** no processes yet ("Write down how your agents work. Start with how changes get reviewed and merged." with **Use the review-and-merge template**) · a replacement refused ("Platform's version of Security basics isn't applied: the organization marked it not overridable. You can add a process instead.") · loading (skeleton rows) · the organization repository unreachable ("Can't reach the organization repository. Showing the last copy from <time>.", with the rows from that copy) · a process file with invalid front matter ("Design handoff: details not recognized (overridable).", naming the field, with **Fix**, as DEC-7; the body still renders, and the document is left out of effective sets until it's fixed) · two files with the same key at one level and scope ("Two processes use the key `review-and-merge` in Checkout. Neither applies until one is renamed." with **Rename…**; both rows show it, and both are left out until one is renamed).
- **The Show select:** "Show" lists **Organization**, then each team, project and agent, with option labels like "Organization", "Web (team)", "Atlas (project)" and "Noor Haddad (agent)". The page heading is "Processes · <label>", and the title is "<label> · Processes · Agentic Org" (`app-shell-ux.md` §4).
- **Choosing a place and Back:** choosing a place with the Show select is a history entry, and the URL carries the place. Back and Forward move between the places viewed, and reload keeps the place.
- **Narrow widths** (`app-shell-ux.md` §2): below 640 px, the heading block stacks above the Show select, so no text column is under 200 px.
- **Accessibility:** level badges and kinds (replaces, added, not overridable) are a word plus an icon, never color alone.

## 3. Flow 2 · How an agent receives its processes

- **Reading them:** `agent-os process list`, `show` and `why` give the agent the effective set, a document's full text and the chain that produced it (PROC-8).
- **At launch:** the agent's system instructions carry a must-follow section with every applicable non-overridable document in full, and a guidance section: "Your processes for T-A17: Review and merge (organization rev 3), Code review (Checkout rev 2), Release checklist (checkout rev 1)", with summaries and how to retrieve the full text.
- **Fresh contexts:** a new session or an observed context reset receives the current effective set again; an earlier delivery never substitutes for delivery to a new context.
- **Can't compute the set:** if the effective set can't be computed at launch, the agent gets "Couldn't load your processes; using the last set from <time>" and works from that set, and the owner sees a warning in Needs you if it lasts over an hour. With no earlier set (the agent's first launch), the agent gets "Couldn't load your processes, and there's no earlier copy." and the owner is warned at once.

## 4. Code branches and cleanup

Each task targets zero or more repositories, and each project/repository association has its required integration branch; there is no single project branch and no fallback. Reassigning a task keeps its worktrees. Only after confirmed task closure may the office remove a task worktree and branch, after verifying every targeted repository's work is preserved: it deletes a remote task branch only when its head is an ancestor of the integration branch (checked on the mirror), after pushing a `merged/<branch>` tag at that head; a task closed without a merge gets an `archive/<branch>` tag first. The office deletes only `task/*` branches it created, never the default or integration branch, never with force, and never a tag ([Git design](../design/projects-git-github.md)).

## 5. UX acceptance criteria (PROC)

- PROC-1 · Process and standard content uses the envelope and path in §1, stable IDs, explicit scope, lifecycle and accountable owner.
- PROC-2 · Resolution is organization → accountable team ancestry → the project's process package, if any → optional project; assignment, home-team and contributor records add no documents.
- PROC-3 · Overrides are allowed unless a document above is `overridable: false`; the view names that source, and no lower level can defeat it.
- PROC-4 · Not in v1: turning a document off at a lower level.
- PROC-5 · Not in v1: private processes.
- PROC-6 · Withdrawn: contributing teams add no documents, so there is no cross-team conflict policy.
- PROC-7 · At launch the agent gets every applicable non-overridable document in full in a must-follow section, and overridable documents as summaries with retrievable full text within the size cap. A new session or a context reset gets them again.
- PROC-8 · `agent-os process list [--for <task>] [--kind <kind>]` (the effective set with sources), `process show <key>` (the effective text) and `process why <key>` (the chain organization → team → package → project, which level wins and why) use the same words as the app. `process new` and `process edit` write process and standard documents for the agent's own project (PROC-36). Commands that approve processes are not in v1.
- PROC-9 · Content views show source, scope, lifecycle, owner, key and revision, and the computed winner for each key.
- PROC-10 · An agent's profile shows the processes it follows for its current work, or, with no current work, "No current work" and the organization's processes.
- PROC-11 · Not in v1: approval of process changes.
- PROC-12 · Not in v1: processes that feed guidance moments and task checklists.
- PROC-13 · Not in v1: retrospective action items that change processes.
- PROC-14 · Not in v1: enforcing one open branch per agent.
- PROC-15 · Not in v1: an agent's open-branch stage on the quick view, roster and task page.
- PROC-16 · Views show loading skeletons, freshness of an unavailable source, and invalid metadata naming the field while keeping content readable; an invalid document or a key clash is left out of effective sets. Scope fields must match recursive paths.
- PROC-17 · If an agent's effective set can't be computed at launch, the agent gets "Couldn't load your processes; using the last set from <time>", and the owner sees a warning in Needs you if it lasts over an hour. With no earlier set (its first launch), the agent gets "Couldn't load your processes, and there's no earlier copy." and the owner is warned at once.
- PROC-18 · Scope and kind badges use words plus icons, never color alone.
- PROC-19 · Not in v1: docs branches listed in an agent's work state.
- PROC-20 · Not in v1: autonomy levels (Asks first, Full autonomy).
- PROC-21 · Not in v1: a Full autonomy badge.
- PROC-22 · Not in v1: built-in `asks-first` and `decide-and-escalate` processes.
- PROC-23 · Not in v1: consults as subagents.
- PROC-24 · Not in v1: a critical-decisions list.
- PROC-25 · Not in v1: escalations in Needs you.
- PROC-26 · Not in v1: a decision record on tasks.
- PROC-27 · Guidance on v1 capabilities (asking in a channel when the right person is unknown, marking an ask required, using `agent-os`) ships as overridable built-in guidance, and none is shipped for capabilities that are not in v1; limits and permissions are enforced in code and no process widens them.
- PROC-28 · Applicable processes and concise `agent-os` command help reach the agent as separate inputs. Standards are discoverable and retrieved when needed, with source and revision references; overridable standard bodies are not loaded into every prompt.
- PROC-29 · Fresh sessions and observed context resets receive the current effective set or explicit retrieval instructions; an earlier delivery never substitutes.
- PROC-30 · Overridable guidance stays within 24 KiB and the runtime's remaining instruction allowance; deferred bodies are named and retrievable. No rule is truncated mid-entry or replaced by a generated summary. Non-overridable documents are never shortened; if they alone do not fit, the launch is refused.
- PROC-31 · Choosing a place with the Show select is a history entry and the URL carries the place: Back and Forward move between the places viewed, and reload keeps the place.
- PROC-32 · The Processes page's heading is "Processes · <label>" and its title is "<label> · Processes · Agentic Org" (`app-shell-ux.md` §4), where <label> is the chosen Show option.
- PROC-33 · Below 640 px, the heading block stacks above the Show select, so no text column is under 200 px.
- PROC-34 · The Show select lists Organization, then each team, project and agent, with option labels like "Organization", "Web (team)", "Atlas (project)" and "Noor Haddad (agent)".
- PROC-35 · For a project that follows a process package, the project's view lists the package's skills as a Package layer with the package's name and revision and the roles each skill is for; an agent's profile shows only the skills for its role and the ones for everyone. A project document with a skill's key shows as replacing it.
- PROC-36 · An agent's write of a process or standard document is accepted only at the scope of one of its own projects and never with `overridable: false`; a refused write says which rule refused it. People write at every scope.

## 6. Not in v1

- **Private processes** for one agent; their storage and privacy are not designed. Shared Git cannot enforce restricted readership.
- **Approval of process changes** through a review branch and the level's approver, and the matching command-line actions.
- **Turning a document off** at a lower level, and drift review when a parent changes after an override.
- **Processes feeding guidance moments and checklists.**
- **Enforcing one open task branch per agent**, with one-time exceptions and a branch stage on the agent's work state.
- **Autonomy levels** (Asks first, Full autonomy), consults with a peer, a critical-decisions list, escalations in Needs you and decision records on tasks.
- **Projectless task bases**: how a projectless task chooses repository bases is open.
