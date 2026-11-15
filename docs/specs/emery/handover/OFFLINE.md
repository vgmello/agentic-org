# Offline handover · Emery (Product) · 2026-11-13

I'm offline for about 3 days (owner order). Codex engineers keep going on `v1-offline`. This page gives the v1 scope, my default answers to product questions you're likely to hit, and where the acceptance criteria are. **If my PRDs and this page disagree, the PRD wins.** If a case isn't covered here, use the default rules at the end and write down what you decided (in your journal, or a note to ada) so I can confirm it when I'm back.

All paths below are under `projects/ac/agents/ac-product-manager-emery/knowledge/prds/` unless they say otherwise. UX specs are in the product repo at `origin/v1:docs/specs/`, and they're Eden's.

## 1. v1 scope on one page

**Platform (IA D45).** v1 runs only in the browser, against `org-gateway` on localhost. There's no desktop shell: Tauri and `apps/desktop` are parked. Supported browsers:
- Chrome or Chromium, last 2 stable majors;
- Firefox, stable plus the current ESR;
- Safari on macOS, last 2 majors.

**The shape (IA v2, `information-architecture-v2.md` D1-D45, stories in `ia-v2-stories.md` IA-01..IA-19).**
- Home and Inbox ("what needs me") come first.
- Then Projects and Work, Chat, Activity, and Teams (member profiles, the org chart).
- Settings holds Offices and Agent setup.
- No emoji anywhere. Icons are Heroicons outline 24. Amber means only "waiting on the owner".

**In v1.0:**
- **Tasks (`tasks-v1.md`):**
  - Created by me and the Created by filter;
  - "who asked", kept separate from who typed;
  - one level of subtasks;
  - server-captured timing;
  - Done requires an outcome;
  - reassignment is recorded, and reassigning to the owner makes a Hand-off;
  - the task, message and Inbox rules.
- **Projects planning (`projects-planning-v1.md`):**
  - backlog;
  - releases and milestones;
  - keys P-n, R-n, M-n, T-n;
  - release and milestone states.
- **Messaging asks (`messaging-asks-v1.md`):** AK1-AK10, the Rev 2 rulings R-a..R-e, and the CP chat performance criterion.
- **Routines (`ceremonies-and-retros-v1.md`, D1-D25):**
  - the retro is the first routine;
  - leads are agents only;
  - only the owner configures routines.
- **IA D44:** Routines tabs on team and project pages.
- **Costs (`costs-and-budgets-v1.md`):**
  - capture for Claude and Codex;
  - roll-ups;
  - budgets;
  - the release header's "Spent";
  - project cost by CLI (`cost show`, with the periods in §7);
  - Build work type only. Run/KTLO is v2, but every item and cost record is stamped with a work type.
- **Time zones (`away-digest-and-timezones-v1.md`):**
  - TZ-1 (a member's zone);
  - TZ-2;
  - Z2, the display rules, including the schedule-time rule: full date, city and offset, never "today" or "tomorrow".
- **Shared definitions stay normative in v1.0** even though Insights moved out (see the "v1.0 scope" note in `project-insights-v1.md`):
  - completion %;
  - durations and the duration formatter;
  - day and week boundaries;
  - bug Done and the bug status mapping;
  - release-page milestone order (AC-IN1-3);
  - drill-down (AC-IN7-3);
  - freeze and unfreeze (AC-IN4-5);
  - Periods (req 25, AC-IN5-1, AC-IN5-2, AC-IN7-1).

**Moved to v1.1:**
- Project Insights: the tab, CU7 Insights Cost, burn charts, throughput and forecast.
- CU6, the org Cost page UI.
- The release cap and estimates.
- Copilot capture.
- TK-5 Review: the In review state and reviewers. Its CLI verbs are AC-TK10-1c.
- The "While you were away" digest and the daily summary (away-digest §7).
- TZ-3 and TZ-4.
- SET-3 (restart to update) is out of v1.0.

**Next (after v1.1):**
- pace and the hourly estimate, which need IN-12;
- the ceremonies pace routine is on hold until then.

## 2. Product questions you may hit, with my default answer

| # | Question | Default answer |
| --- | --- | --- |
| Q1 | A v1.0 screen cites something from a v1.1 PRD (Insights, TK-5, the digest). | Build only what the "v1.0 scope" note in that PRD calls normative. Leave the rest out, and don't build a stub UI for it. |
| Q2 | Should a v1.0 feature show anything for TK-5 "In review" or reviewers? | No. In v1.0 a task goes from In progress to Done. Leave out the reviewer parts of costs §5.4a. |
| Q3 | Who sees cost? (costs req 26, Rev 15) | The owner sees all cost. A PM sees only the cost of their own projects; on an agent's profile that's the records attributed to their projects, labelled "in your projects", with no budget bars. An agent's daily and monthly budget bars are owner only. Other members see no amounts. |
| Q4 | Can a report, retro file or agent evidence contain cost? | Not amounts or budget figures (costs 28a). Tokens and "Budget reached" are fine, with a pointer to the app. |
| Q5 | A run has no price (an unknown model). | Record tokens with no amount, and show the "no price" state as the costs PRD specifies. Never guess a price. |
| Q6 | How should a time or date be shown? | Use the viewer's zone. Relative times are fine for recent activity. Schedule times always show the full date, city and offset (Z2). If you're unsure, show an absolute date with a tooltip. |
| Q7 | Can the owner send an "ask"? | No. Only agents send asks (AK1). The owner's attempt is refused with "Only agents can send asks. Send a message, or create a task." (R-c). |
| Q8 | Does a notification interrupt an agent that's busy? | Never (AK9). A busy agent gets it at its next check (AK10). |
| Q9 | What does the CLI do on a bad request? | A refusal exits 1 with the PRD's text. A usage error exits 2. Messages are plain sentences, with no stack traces. |
| Q10 | The chat history is long. | Load the latest 200 messages, then offer "Load older" (CP). The memory budgets are in messaging-asks §3. |
| Q11 | Is a desktop or Tauri behaviour required? | No (D45). If a spec still says Tauri, read it as the browser against localhost. |
| Q12 | The spec and the code disagree on look and feel. | The design system v2 (Eden) wins. For UX questions, check Eden's handover first. |
| Q13 | Should an empty state or a zero be shown? | Show an empty state with one plain sentence and the next action. Never show fake sample data outside e2e fixtures. |
| Q14 | The gateway has no `/api` route for a screen. | Don't invent product behaviour to fill the gap. Build against the contract Rowan or ada defined, and flag the gap to ada. |
| Q15 | Should I rename an AC ID or a story ID? | Never. The IDs are stable. Add new IDs instead. |

**Default rules when nothing above fits:**
1. Pick the simplest behaviour that keeps the human in control and the agent presented as a colleague: named, accountable, visible in Activity.
2. Prefer showing state over hiding it.
3. Don't add scope.
4. Record what you decided.

## 3. Where the acceptance criteria are

- **M1 and near-term slices, one consolidated list:** `acceptance-tests-v1-prd-set.md`.
  - Given/When/Then checks per story, each tagged unit, integration, UI e2e or manual.
  - It covers ceremonies, D44, Tasks, Projects and messaging asks.
  - Its rulings section covers CLI exit codes and the browser set.
- **IA v2 screens (Home, Inbox, Projects, Work, Chat, Teams, Settings):** `ia-v2-stories.md`, with the AC-IA01-* .. AC-IA19-* criteria.
- **Tasks:** `tasks-v1.md`, AC-TK*. TK-5 is v1.1, apart from the AC-TK10-1c note.
- **Planning:** `projects-planning-v1.md`, AC-PL*.
- **Costs:** `costs-and-budgets-v1.md`, AC-CB*. AC-CB12-1 is scoped to levels that have a v1.0 UI.
- **Time zones:** `away-digest-and-timezones-v1.md`, AC-TZ1-*, AC-TZ2-* and Z2.
- **Messaging asks and chat performance:** `messaging-asks-v1.md`, AK1-AK10 and CP.
- **Routines:** `ceremonies-and-retros-v1.md`.
- **Shared definitions:** the "v1.0 scope" note in `project-insights-v1.md`.
- **Which feature sets are merged on v1, with their story and AC IDs:** `../workspace/merged-feature-sets.md` (a working note, not committed) and my journal for 2026-11-13.
- **Backlog triage of older UI items against IA v2:** `ia-v2-backlog-triage.md`.

## 4. Open on my side (for when I'm back)

- #22: product review batches 1-4. They start once the Org chart is the first real screen.
- #23: the real-data batch, which waits on the gateway `/api` path (P0).
- LOW follow-ups for backlog #19-#21. None of them blocks a build.
