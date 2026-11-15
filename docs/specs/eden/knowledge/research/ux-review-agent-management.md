# UX review · Agent management (pre-merge)

Reviewer: Eden · 2026-11-08 · Build: branch `app-dev/org-agents` @ `0f50b76`, from e2e screenshots (agent-quick-view, edit-agent, offboard-agent, add-agent, move-member, agents-roster) · Against: `docs/specs/our-org-in-the-app.md` (PROF, MOVE, DEAL, STAFF-4), `agent-run-modes-ux.md` (RUN) and `offices-and-teams-ux.md` §7.

**Summary.** This is the heart of the dogfood goal, and most of it is right. The Offboard and Move dialogs are exemplary, the Agents roster reads well, and Edit says "Changes take effect at the agent's next session". The main gap: the app shows the admin flag **Active** where people need the **live status** (Working, Waiting, Asleep, Offline, Paused), so the owner can't yet see what's happening.

## Outcome (same day)

All 7 findings fixed on `app-dev/org-agents` @ `cd95d7f`, verified on the new screenshots: the live status words with icons (Asleep · wakes when called, Paused, Former, Started by a person, Status unknown, never Active), the handle-change warning, Model on Add, the Now column and line, Reports to defaults to the team lead, the Former tab, and the tile glyph and chip layout. Still to see: Pause/Resume, How it runs…, Show paused, search, arrow keys.

## Follow-up (same day): the screens still to see, on v1 @ `9f5d41d`

From my own e2e run (org-chart-agents and agents-roster, 20 passed): pause-agent, agent-paused, how-it-runs, show-paused-on, member-search, tile-keyboard-focus.

**Good:** the Pause dialog ("won't be started or woken, and new tasks and messages wait until you resume", and the teams it stays in), Resume in the strip, the toast "Paused Noor Haddad. Messages and tasks wait until you resume.", the paused glyph on tiles, Show paused, Search with "2 matches · Clear" and the matches ringed, the arrow-key focus ring, and How it runs with each mode explained plus a schedule and time zone.

| # | Severity | Finding | Fix |
| --- | --- | --- | --- |
| F1 | Medium | **How it runs has no idle or busy limits.** `agent-run-modes-ux.md` line 69 and RUN-6: "Go to sleep after [5] min idle · stop after [30] min busy (home) · [2] h busy (on a task)". The defaults may apply, but the owner can't see or change them. | Add the line under the mode for On call and Scheduled (and the busy limits for Always on). |
| F2 | Low | **Save is disabled without saying why** when Scheduled has no When. | Under When: "Add when it runs to save." (or keep Save enabled and show the inline error). |
| F3 | Low | **"Paused" appears twice in the strip**: a chip beside the action buttons (it reads like a button) and the status line. | Keep the status line with its icon; drop the chip. |
| F4 | Low | **"1 project has no team yet."** is yellow text with no icon or action. | A warning icon, and a link that opens that project's **Assign team…**. |

## Findings

| # | Severity | Finding | Screen | Fix |
| --- | --- | --- | --- | --- |
| 1 | High | **Status means the wrong thing.** The roster's Status column says "Active" for everyone, and the quick view shows no status. Spec: a status word from presence and run mode, such as Working, Waiting, Needs you, **Asleep · wakes when called** (on call, no live run), **Offline** (always on, no session), Paused, or "Started by a person" (PROF-1, PROF-3, RUN-2). "Active" is the admin flag and shouldn't appear as a status word. | agents-roster, agent-quick-view | Show the derived status (ada A27: asleep and offline are derived) with its icon and word. Until presence is wired, show "Asleep · wakes when called" for on-call agents with no live run and "Status unknown" otherwise, never "Active". Keep Paused from `AgentStatus`. |
| 2 | Medium | **Changing the messaging name isn't explained.** Edit lets you change `sam` to `sam-principal`, while Add says the name is "kept for good". Others address the agent by this handle, so a change breaks their messages and scripts. | edit-agent, add-agent | Allow it, with a warning under the field: "Others reach Sam as `sam` today. After saving they must use `sam-principal`, and `sam` stays reserved." In Add, change "kept for good" to "Unique, and stays reserved even after offboarding." |
| 3 | Medium | **Add agent has no Model** (Edit does). | add-agent | Add Model (optional) beside Runs on, as in Edit. |
| 4 | Medium | **No "now" in the roster or quick view.** "See everything that's happening" needs the agent's current task or last activity ("Reviewing eden/specs-2 · 4 min ago"). | agents-roster, agent-quick-view | Add a **Now** column and a line in the quick view once Activity (G5) lands, with an empty state until then: "—". |
| 5 | Low | **Reports to defaults to Sam Okafor** in Add agent with Web selected. Spec (hire): the default is the chosen team's lead (Noor Haddad), else the owner. | add-agent | Default from the first chosen team's lead, else the owner. |
| 6 | Low | **The roster tab says "Left".** The spec calls them **Former members**. | agents-roster | "Former". |
| 7 | Low | **Agent tiles are crowded.** The robot glyph sits above the initials and the "+1 team" chip overlaps the tile's top edge. | chart | Put the glyph inside the tile's corner and the chip beside the tile, not over it. |

## Not in the screenshots (please send)

Pause / Resume dialog (DEAL-3: Pause stops at once, and the card shows Paused), **How it runs…** (run modes, schedule with time zone, idle and max busy time), Show paused (DEAL-5), member search (TEAM-11) and arrow-key navigation (TEAM-10).

## Good, keep it

- Offboard lists only what applies: teams and lead, tasks back unassigned, reports to the agent's own manager, and the record and handle kept.
- Move is one step ("they leave one team and join the other"), says which projects change and that the team loses its lead, and disables "already in" teams with a reason.
- The roster shows runtime, how it runs, teams, office (Unplaced) and manager. The All / Active / Paused / Left counts.
- The quick view shows messaging name, runtime and mode in one line. Add agent explains it's for agents that already run.

**Verified (v1 @ ac08482):** F1 done (limits shown read-only; ask: label them "Defaults for all agents"). F2 still open. F3, F4 not checked yet.

**Live org (v1 @ 01f3913):** verified: changes made elsewhere show on the open chart without a reload (decision 8). Medium sent: a silent fallback when the live subscription fails; show "Live updates paused. Showing what was loaded at <time>." with Retry.
