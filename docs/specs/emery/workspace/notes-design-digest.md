# Notes · design and specs digest (2026-11-11, paused assessment)

E = Eden's `knowledge/`. R = `git -C /Users/vgmello/projects/agentic-consulting/app show origin/v1:docs/specs/<file>` (last change f09c13a9, 2026-11-10). **Cite R; Eden's copies have drifted** (our-org is rev 21 in R; employee-supervision and app-shell exist only in R). `desktop-app-v1.md` (Tauri, terminal panel) is the old base layer, not the current IA.

## Nav (owner, 2026-11-11; E/design-system.md:74-86)
- Agents (Home): the roster, with a details sheet. A standalone profile page is a later follow-up.
- Org chart: a tab of Agents (Teams, People, Reporting lines).
- Chat: channels, DMs, threads and a Needs you section.
- Projects: list plus a page (Overview, Tasks, Bugs).
- Work: Tasks, Bugs, Debt and Requests across projects.
- Sessions and Activity: placeholders.
- Settings, with Offices under it.
- Processes: hidden in v1.0.

## Terms and principles
- Glossary: "Agent: an AI employee with a home, memory and backlog"; Bot, assistant and worker are banned. employee-supervision says "employee", so the two are inconsistent.
- Truth over decoration: "Working" only shows with a live run. Otherwise: "Asleep · wakes when called", "Started by a person", "Status unknown".
- Statuses: Working (no v1.0 data), Needs you (future), Asleep, Paused, Former, Status unknown. Chat presence: Working, Online, Asleep, Offline. Amber means only "waiting on the owner".
- **Avatar conflict:** design-system §8 says round tinted initials for everyone with no robot glyph, but R offices-and-teams :973, TEAM-12 and our-org :198 say square robot tiles for agents.
- Subagents never appear in the UI.

## Coverage
- **Identity:** medium. Name, handle, title, focus, runtime, model, office, hired on, "requested by X · approved by Y". **Personality: none** (no personality, voice or tone field anywhere).
- **Presence:** strong on paper (run-mode detail, a health check that tells a known wait from an unresponsive agent, live updates in under 2 s). In v1.0 the data doesn't support Working or Needs you, and the roster's "Now" column is empty.
- **Memory and journal:** thin. The old Journal and Memory tabs were dropped; memory now means supervision checkpoints. Knowledge is "server-provided content".
- **Delegation:** strong. Assign task with a definition of done as the goal, goal versions with "Pending delivery", run modes (Always on, On call, Scheduled, One-off), and wake guidance. Agents delegate to a fresh copy of themselves, a private specialist or a general-purpose agent.
- **Oversight:** strong design, mostly after v1.0. Autonomy (Asks first or Full) with a list of critical decisions, escalations with a recommendation, a Decisions audit trail, Pause, Remove, Offboard and Unassign, Interrupt, a read-only session log (takeover only through the CLI), and costs labelled Reported or Estimated with budgets. **Gap: the per-tool permission prompt (old §2.4) has no counterpart.**
- **Hire:** strong (Describe → Suggestion → Review → Hiring → Done; HR lead built in; "Fill in the details yourself"; approval routing). **Onboarding after the hire is thin.**
- **Owner's inbox:** **fragmented.** Home specs Needs you, Working now and Latest, but in v1.0 Agents is Home with no Activity, and the only Needs you badge is on Chat. There's no single "what needs me" surface.
- **Chat:** the strongest area (conversations, not output; ask cards; messaging wakes the agent; owner read-all; mute and digest).
- **Copilot:** well-bounded, drafts only (F1: a drafted severity isn't marked as a suggestion).

## Eden's open items (ranked)
1. Plan placement: CHAT has no milestone exit (A-H1); Needs you and Home land in M3 but M2 and M4 need them (A-M3); autonomy and consults are in no wave (A-M4).
2. Status honesty: Working and Needs you have no v1.0 data.
3. No agent profile page in v1.0 (bug-70 AD-1, "product call Q3").
4. Round avatars versus square robot tiles.
5. Copilot drafted fields.
6. Spec questions: A45 (does a mention wake an agent?), A97/A98 (partial hires), A75 (linking the agent record to its role file), OQ-31 (show why a goal check failed).
7. Validation themes: widths, a11y, focus and status races. Nothing on whether agents feel like colleagues.

## R docs/specs files
agent-run-modes, app-shell, chat, costs-and-insights, decisions-and-tech-debt, dogfood-run, employee-supervision (owner-authored, for review), feature-requests, features, offices-and-teams, our-org-in-the-app, processes, project-status, retrospectives, roles, server-installation-and-access, work-items. (`desktop-app-v1` and `models-ux` are only in E.)
