---
id: doc-17
title: Product UI Stream Handover (Track U)
type: guide
created_date: '2026-11-21T00:16:00Z'
updated_date: '2026-11-21T00:16:00Z'
tags:
  - workstreams
  - ui
  - handover
---
# Product UI stream (track U)

> [!NOTE] Review note (2026-11-21)
> Remaining Track U work (Step 4 Work tab & queued shared fixes, Steps 5–9, a2ui interaction review, and ADR-0031 private task notes UX) is now tracked in Backlog.md (`TASK-12`, `TASK-17`, `TASK-65`–`TASK-71`). Unmerged WIP branches noted for pickup: `v1-ui-work`, `v1-ui-chat-wip`, and `origin/app-frontend-dev/pointer-focus`.

Last updated: 2026-11-19 (ui-dev). The steps are those of the [UI plan](../plans/v1-ui-screens.md) .
Every screen runs against mock APIs (`bun run dev:mock`); production calls refuse until the backend
serves them. **The owner paused new work on 2026-11-19: finish, validate and merge what is built.**

## Status

| Step | Status | Evidence | Next / remaining |
| --- | --- | --- | --- |
| 0 · Foundation (shell, navigation, shared states, look tokens) | Done | `v1` (UI release merge) | Shared fixes below |
| 1 · Home, Inbox, Activity, Search, Preferences | Done | `v1` (UI release merge) | — |
| 2 · Teams and people (Members, team page, member profile, Org chart, Hire) | Done | `v1` (UI release merge); Opus reviews and fixes per feature | — |
| 3 · Organization settings (General, Roles, Agent setup, Members and access, Costs, Updates, Processes, Offices) | Done | `v1` (UI release merge); Opus reviews and fixes per feature | Two design decisions to apply (below); 9 Offices tests marked `fixme` for the settings layout at 960–1279 px with 200% text (SET-200) |
| 4 · Projects (list, frame, overview, team, activity, conversations, settings) | In progress | `v1` (UI release merge); reviewed and fixed | The Work tab shows v1's task board and bug list (Tasks, Bugs) until step 5 replaces them |
| 5 · Work items (task, bug, request pages, Work board/list, dialogs, budgets) | Not started | Branch `v1-ui-work` holds a stopped first start (committed as WIP, unreviewed) | Build from the plan's step 5; reuse or discard the WIP |
| 6 · Docs, features and releases | Not started | — | — |
| 7 · Costs and insights | Not started | — | — |
| 8 · Access and chat gaps | Not started | The release ships v1's Chat; an unfinished Chat rebuild is on branch `v1-ui-chat-wip` | Chat's missing features and module extraction |
| 9 · Closing (all browsers, screenshots, FEATURES pass, final design walkthrough) | Not started | — | Include an interaction review, with https://a2ui.org/ as a reference |
| Release of `v1-ui` into `v1` | Done | `v1` (UI release merge); full chromium suite 1,095 passed, 0 failed, 10 skipped (recorded `fixme`s) | — |

**Next up:** paused by the owner (2026-11-19). When it resumes: step 5 Work items (branch
`v1-ui-work` holds a stopped start), then the shared fixes, steps 6–8 and step 9. From now on UI
work branches from `origin/v1` and merges into `v1`.

## Branches and worktrees in flight

Worktrees live in `../agentic-org-ui-wt/` next to the main checkout. The working detail sits beside
them in `../agentic-org-ui-wt/checkpoints/`, outside the repository:

- `INDEX.md`: the working log;
- `followups.md`;
- one note per feature;
- `reviews/`: every review report;
- `prompts/`: the builder and reviewer prompts;
- the launch scripts, which pin the model.

A restarted ui-dev reads `INDEX.md` after this file. If that folder is gone, this file and the
[UI plan](../plans/v1-ui-screens.md) are enough to continue.

- `v1-ui` and `v1-ui-step2-all`: merged into `v1` by the UI release; no longer used (new work branches from `origin/v1`).
- `v1-ui-work` (worktree `work/`, pushed): the stopped step 5 start, committed as WIP.
- `v1-ui-chat-wip` (pushed): the unfinished Chat rebuild, kept for step 8.
- Feature branches `v1-ui-*` merged into `v1-ui-step2-all` can be deleted once `v1-ui` is in `v1`.

## Decisions made, still to apply

- Members and access shows usernames only to the owner, the administrator and the person themself.
- Updates puts what needs attention first: this instance, then offices (with any "Needs update"
  note at the top), then How to update.
- Project Activity lists newest first, like every other activity list.

## Shared fixes queued

- The project Work tab's Tasks/Bugs tabs need `aria-controls`, arrow-key navigation, and tests for the
  keyboard and for 320 px with 200% text (release review, P3).
- `apps/agentic-org-ui/e2e/README-gate.md` still mentions the retired non-chat-icons and
  project-profile specs.
- The settings layout and the shell at 960–1279 px with 200% text leave pages too little room.
- The shared ConfirmDialog doesn't close on browser Back; the shared Resource should show the office's
  `unsupported` answer as "not available in this version" with no Retry.
- Pointer focus return for menus and dialogs (app-shell-ux 305–307): an unreviewed start is on branch
  `origin/app-frontend-dev/pointer-focus`.

## Request from the backend stream: the extension UI slot

Owner-directed, 2026-11-20. The backend stream proposes how optional extensions (first Containers and Xcode, late v1, not
milestone one) appear in the web app: menu items, a settings section validated by the office,
their own panels, discovery of enabled extensions from the office through the web API, and UI code
that lives in `extensions/<name>/ui/` and is built into the web app (never Tailwind Plus outside
`apps/agentic-org-ui`). Proposal: [accounts, sandbox and extensions, section 8.4](../design/accounts-sandbox-and-extensions.md#84-the-ui-slot-proposal).
ui-dev is asked to review it, brainstorm the slot with backend-dev, and agree its shape before either
side builds it. Owner answer 2026-11-20: ui-dev leads the slot's design with the backend stream when L17
starts. One point to settle together: building extension UI into the app must not make the
app and the extension UI import each other (the note offers a shared UI-foundation package or
slot-provided primitives). Nothing under `apps/agentic-org-ui` was changed. Until then, milestone-one UI only keeps
the navigation and settings sections driven by data where that is cheap.

## Request from the backend stream: recovery is v2, and a trust download

Owner-directed, 2026-11-20. The recovery code, CA escrow and protected recovery delivery
are v2 ([note, section 6.7](../design/accounts-sandbox-and-extensions.md#67-recovery-is-v2-hq-loss-in-v1)).
The UI's recovery parts (the settings recovery row, the release command, the recovery
state and their end-to-end specs) stay as they are; nothing is removed without the
owner's OK. Planned, late v1: a "Trust this organization" download of the
organization's CA with its fingerprint shown at creation
([section 6.6](../design/accounts-sandbox-and-extensions.md#66-the-organizations-certificate-authority-and-trust));
its screen is to be agreed with ui-dev when it is built.

## For the other streams

- **Backend:** the UI expects the routes in `docs/design/web-api-v1.md` one by one; each screen swaps
  its mock for the real call when its route lands. B2.2's org wire change needs the UI mirrors updated
  in the same merge: message ui-dev first.
- **Linux:** nothing pending. The instance reader accepts `http_published` and `tls`; no screen shows
  TLS yet.
- **All:** UI build tooling, launchers and reviews run as headless Claude Opus sessions, each with its
  model pinned with `--model`. The owner's global default model is not used.
