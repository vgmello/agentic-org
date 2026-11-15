# Notes · BUG-70 slices, Rowan's freezes and the current UI (2026-11-11, paused assessment)

R = Rowan's `knowledge/designs/`. UI = `apps/org-ui/src/` at **origin/v1 `2efce5d5`** (the local `v1` branch is behind). [F] fact, [I] inference.

## BUG-70
- [F] The v1 look has drifted from the Tauri prototype: no search or primary action in the top bar, no dashboard Home, chips, emoji and yellow callouts, 10 nav items instead of 6. S2, open. The owner OK'd reusing prototype code.
- [F] 33 slices, ≈5,210 lines (R/bug70-slice-order.md:11-43), plus S3b, J-BACK, A-CH3 and J-SIDEW. **A-AD1 (the agent profile page) was dropped** (Eden Q3: no profile pages in v1.0). Activity (AG-8, NB-3) and Processes are deferred.
- Groups:
  - Shell and nav (Jules)
  - Look primitives (Jules; J-AVATAR at order 17)
  - Agents home and Org chart as tabs (Avery; "Now" moves into the details sheet)
  - Work and Projects
  - Chat
  - Offices
  - Onboarding and dialogs
- [F] Done: J-BUG76, Offices pill, Agents roster, switch footer, BUG-2. A-QADD is at Tomas. S2 is on the branch `avery/agents-tabs`. `nav.ts` still has 10 items.

## Nav freeze (rev 4)
- 7 items: Agents, Chat, Projects, Work, Sessions, Activity, Settings.
- Org chart becomes a link next to Agents.
- Offices is reached through `/settings`.
- `/processes` stays, but is unavailable in production.
- The landing page is `/agents`, with no Activity column until a feed API exists.
- The sidebar footer reads "You · Owner", then the office line.
- Projects are listed in the sidebar (capped at 8).

## Freezes and what the owner sees
- [F] B1-11 "call lifecycle" is a DB transaction for one Work write, not an agent session.
- C1: 25 Work writes. Only members can change work, and agents act as themselves. Only the app deletes work. Retries are idempotent, and a stale edit gives a `conflict` that the UI already handles. **No live updates until C2.**
- Work ack (satellite to HQ) defines no owner-visible behaviour. [I] An item HQ rejects needs a surface the owner can see.
- X-02:
  - Password sign-in. **Every gateway restart or upgrade signs everyone out.**
  - Touch ID or the login password for sensitive operations.
  - The recovery code shows "pending" until acknowledged.
  - Open product questions: the Hire Review path preview (Q2); "(you)" shown for an admin who isn't the owner; whether satellite approvals reach the owner or expire as a deny.

## UI today (origin/v1)
- Routes:
  - `/welcome`, `/create`
  - `/agents`, `/org-chart`, `/chat`, `/projects[/id]`, `/work`, `/offices`, `/processes`
  - Placeholders: `/sessions`, `/activity`, `/settings`
- **Data:** production `orgApi` refuses every call. Work and Chat are always sample data.
- **Roster:**
  - Shows name, title, handle, runtime, how it runs, teams, office and manager.
  - **No avatar**, and the name links to the org chart.
  - **"Now" is always "—".**
  - Statuses: only Paused, Former, Started by a person, Asleep and Status unknown.
- **Org chart:** agents are square tiles with a robot icon, people are round. Actions: Edit teams, Pause/Resume, How it runs, Edit, Offboard.
- **Add an agent:** form fields only. No personality, avatar or approval step.
- **Chat (mock):** the richest colleague surface. Six presence states on avatars, a Needs you view, ask, approval and hand-off cards, typing indicators.
- **Work (mock):** an Assignee column and "N items need you". The task sheet requires a reason for blocked and for done anyway.
- **Missing everywhere:** memory, journal, personality, activity history, a session view, and any per-agent oversight or audit view.

## Tensions [I]
1. Polish before substance: BUG-70 is all look, while Now, Activity, live presence and the profile page are deferred or dropped.
2. Chat and Work are restyled while still on sample data, so they risk rework.
3. Two presence vocabularies: Chat's six states versus the roster's five words.
4. Identity is thin: initials only; J-AVATAR isn't adopted on the roster; nothing expresses personality or memory.
5. Oversight lives only in mock Chat, and the backend for approvals (B1-12, satellite relay) is open.
6. Offices is buried under Settings.
7. Global search finds only agents.
8. Sign-out on every restart is a UX cost.
