# Notes · v1 plan digest (2026-11-11, paused assessment)

Sources: ada's `knowledge/plans/v1-implementation-plan.md` (PLAN), `tracking/STATUS.md`, `designs/desktop-app-architecture.md` (ARCH). [F] fact, [I] inference.

## What v1.0 is
- [F] One human owner, who is also the instance admin. A second account can be added only through the CLI on HQ (O-1 default, PLAN:155).
- [F] Organization runs as offices: HQ holds the authoritative data, and satellites on other machines replicate with it. An instance can hold several offices. Employees are humans and agents, arranged into teams, reporting lines and an org chart. Projects and areas have no repo in v1.0. Work items are tasks, checklists, bugs and fix tasks. Chat is DMs and asks only. Claude Code is the only harness. Run modes are triggers, queues, limits, approvals and schedules. Hire is a proposal that someone approves. The copilot moves to Wave B.
- [F] **Drift from the early brief:** Tauri is deleted in M1. It's replaced by the `org-gateway` web UI, which `agentic-org setup` serves to a browser (PLAN:99-103, 212-215). Federation (M4) and Linux are in v1.0. The native window, tray and notifications move to v2. The in-app terminal is gone; taking over a running agent goes through the local CLI (B3-08). [I] The project brief and ARCH still describe a Tauri desktop app.
- [F] Count: STATUS gives 63 tasks / 194 pts (XL=6). PLAN gives 65 / 204 (XL=8, including the post-v1.0 tasks B3-13 and F2-12). Quote 194.

## Milestones
- M0 renames, contract, CI; no user change.
- M1 setup, sign-in, real org chart, create a team.
- M2 projects, tasks, bugs, agent profiles, approve an agent record, live updates, escrow.
- M3 message an agent, which wakes, does a no-repo task and replies; pause and resume; Home, Needs you and Activity.
- M4 satellite enrollment (M4a) and agents at the satellite (M4b).
- M5 signed release and the dogfood run on Mac and Linux.
- Wave A "make agents useful": git worktree per task, processes R1-3, delegates, specialists, memory checkpoints, channels.
- Wave B knowledge and reporting, costs, the real copilot, roles.
- Wave C Codex/Antigravity, GitHub, recovery after losing HQ.
- Out of v1.0: coding in a repository (PLAN:444), automatic promotion of a satellite, chat bridges, the browser terminal, importing old desktop data; person sign-in (A-7).

## Agents-as-colleagues coverage
- Identity: names, profiles and the home path are v1.0 (M2). **Avatars and personality are absent.** Roles are Wave B.
- Presence: Needs you and Working now are in M3. **The plan never says which states v1.0 shows** (the status model is only in ARCH:90-104).
- Memory and journal visibility: **absent**. Memory checkpoints are Wave A.
- Delegation: messages, asks and the goal with Edit goal are v1.0. **Assigning a task to an agent isn't stated explicitly.** Delegates are Wave A.
- Oversight: approvals in Needs you, the session status and log, durable Pause/Resume, the operation ledger and Activity are v1.0. Costs are Wave B. Employee supervision (EMP-4..14) is Wave A.
- Hire and offboarding: v1.0 (O-7 widened; DEAL-1..5).

## Owner decisions
O-1 and O-2 take the defaults (refined by A-7). O-3 (REC-1) is open and out of v1.0. O-4 was answered by a spike. O-5 is HTTPS, with loopback HTTP only when chosen. O-6 is macOS 14.4+ on Apple silicon and Linux x86-64 with systemd. O-7 is widened to Hire. Still open: Apple signing, and when messaging leaves the spool.

## Risks to product value [I]
- Infrastructure-heavy: the first time an agent does anything is M3, and the colleague experience lands mostly in Waves A and B.
- No git means v1.0 agents can only do no-repo tasks, which is weak for an MVP.
- Federation and Linux sit on the critical path before the core value is proven on one machine.
- The visible colleague layer is thin: no avatars, personality or memory/journal view; trust rests on Pause, approvals and Activity.
- No product success metrics; acceptance is SRV criteria and the dogfood run.
- The product's identity has drifted across the brief, ARCH and the plan.
