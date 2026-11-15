# UX review · Run modes, Pause, search (org-agents) and the Tasks tab (work-items)

Reviewer: Eden · 2026-11-08 · Builds: `app-dev/org-agents` (how-it-runs, pause-agent, member-search) and `app-dev/work-items` @ `5d728ba` (tasks-board, task-page; tasks-blocked wasn't generated) · Against: `agent-run-modes-ux.md` (RUN), `our-org-in-the-app.md` (DEAL), `work-items-ux.md` (TSK, CHK, §9.2) and ada's W7 display rules.

## Agents: How it runs, Pause, search

| # | Severity | Finding | Fix |
| --- | --- | --- | --- |
| 1 | Medium | **The schedule is free text** ("Weekdays at 9, every 30 minutes…") with no preview. RUN-7 asks for presets, a plain-words builder and the next 3 run times, never something to get wrong. | Add presets (Every 15/30 min, hourly, daily at, weekdays at) and a "Next runs: Mon 09:00, 09:30, 10:00" preview under the field. Keep free text as "Custom", with the preview as validation. |
| 2 | Medium | **Missing from How it runs:** idle time, max busy time (30 min home / 2 h task), the wake guidance, and recent runs and cost (RUN-6, RUN-11, RUN-13). | Add "Go to sleep after [5] min idle · stop after [30] min busy (home) · [2] h busy (on a task)", the guidance field, and "Last 7 days: n runs · time · ≈ $". |
| 3 | Low | **Time zone is a monospace text box.** | A combobox of zones, defaulting to this Mac's. |
| 4 | Low | **Pause** is right (DEAL-3). When a session is running, add "Its session stops now." | One line when a session is live. |
| 5 | Low | **The reporting-lines tree at Fit** draws names around 9 px. | A minimum text size (12 px). Below it, show initials and the name on hover/focus, or stop zooming out. |

Good: the four modes with plain descriptions ("Started by a person: the office never starts it"), the cost line, the Pause copy, and search with "2 matches · Clear", dimming non-matches.

## Tasks tab

| # | Severity | Finding | Fix |
| --- | --- | --- | --- |
| 6 | Medium | **No definition of done.** Agents work to it (the goal), and New task and the task page both have it in the spec (§9). | Add "Definition of done" (one or two sentences) to New task and the task page. |
| 7 | Medium | **Two Save buttons** on the task page (title, then "Save description"). | Save each field on blur, with a quiet "Saved", or one Save for the panel. |
| 8 | Medium | **The assignee picker doesn't show status or runtime** (MOVE-4), and assigning to an Offline or Paused agent doesn't warn. | Show "<name>, agent · Asleep · Claude Code" in options. Assigning to Offline or Paused asks first ("Riley is offline and won't pick this up"). |
| 9 | Low | **A "Move to…" select on every card** makes the board heavy. | Keep it in ⋯ › Move to (and drag later). The keyboard path stays. |
| 10 | Low | **Done has no resolution.** Done · Completed / Cancelled (W7) decides the auto Fixed/Delivered rules. | Mark Done as "Done · Completed" or "Done · Cancelled" (ask on cancel). |
| 11 | Low | **Copy details:** the date reads "11/8/2026" (use "8 Nov"). The toast "Created 'Fix the build'." should be "Task T-2 created" (add "and given to <name>" when assigned). "Sample data" appears twice. | As stated. |

Good: T-n numbers, Urgent shown as a word, Unassigned shown, column counts, Done with "Show all done", the checklist with Required, and the Time line exactly in the agreed wording ("the rest … working time isn't tracked yet").

Still to come, per app-frontend-dev: List view, ⋯ menu and drag, the other project tabs, Start now, activity.
