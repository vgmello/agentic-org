# UX review · Work (cross-project), Bugs and fix tasks

Reviewer: Eden · 2026-11-08 · Build: `app-dev/work-items` @ `e10b930` (work-bugs, report-bug, bug-page, task-page) · Against: `work-items-ux.md` (BUG, TRI, FIX, WORK), ada's W7.

## Outcome (same day)

Verified on v1 @ `9f5d41d` (fixes in `6fc7f74`), from my own e2e run (17 passed): Environment "This Mac (HQ) · Agentic Org 0.1.0" (never the user agent; branch and commit come with real offices), labelled metadata (Area · Reporter · Assignee), the verify question above the buttons ("Alex Kim fixed this. Is it fixed for you?"), the severity icon beside the word, "8 Nov" dates, Area with "Goes to: <triager>" in Report a bug. Still to do (app-frontend-dev, next work-screen branch): refs with the project key outside a project (`chk-B-1`). New lows: the time line shows bare "0" ("waiting on you 0 · blocked 0"), say "0 min"; **Create a fix task** still shows on a Fixed bug, hide it there or say "Reopen to create a fix task".

**Verified from the last review:** Definition of done, save on blur (no Save buttons), "Done · Completed", the toast "Task T-1 created.", and the ⋯ menu on cards.

**Good:** the Work tabs (Debt and Requests disabled), saved filters with counts and per-tab defaults, "1 item needs you", Report a bug (severity cards with their meanings, no default, inline errors in plain words), the bug's verify actions, and the time line.

| # | Severity | Finding | Screen | Fix |
| --- | --- | --- | --- | --- |
| 1 | Medium | **Environment is the raw browser user agent** ("Mozilla/5.0 … HeadlessChrome/158…"). Spec BUG-6: the office, the app version, and the project's branch and commit. | bug-page | Prefill "This Mac (HQ) · Agentic Org <version> · <branch> @ <short commit>" (or without the repo part). Never the user agent. |
| 2 | Medium | **Unlabelled metadata** "General · Alex Kim": it's unclear which is the area and which is the reporter or assignee. | bug-page | A small description list: Area General · Reporter Alex Kim · Assignee …. |
| 3 | Medium | **The verify step has no question.** The buttons are there, but not "Riley fixed this. Is it fixed for you?" (reporter) or "Sam Ortiz (reporter) is asked to check" (others), FIX-4. | bug-page | Add the banner line above the buttons. |
| 4 | Low | **Severity is a word without its icon** (§2.1: flame, octagon-alert, info, feather). It's not color-only, but scanning is slower. | work-bugs, bug-page | Add the icons. |
| 5 | Low | **Numbers outside the project** show "B-1" instead of "chk-B-1" (known, waits for project keys). | work-bugs | When keys land. |
| 6 | Low | **Dates in numbers** ("11/8/2026"). | bug-page, task-page | "8 Nov" (same as before). |
| 7 | Low | **Report a bug's Area** wasn't visible in the shot (below the fold?) and should start on General. | report-bug | Check it's there, and that "Goes to: <triager>" shows above Report. |
