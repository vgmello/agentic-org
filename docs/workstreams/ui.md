# Product UI stream (track U)

Last updated: 2026-11-19 (ui-dev). The steps are those of the [UI plan](../plans/v1-ui-screens.md) (on `v1-ui` until the release merge).
Every screen runs against mock APIs (`bun run dev:mock`); production calls refuse until the backend
serves them. **The owner paused new work on 2026-11-19: finish, validate and merge what is built.**

## Status

| Step | Status | Evidence | Next / remaining |
| --- | --- | --- | --- |
| 0 · Foundation (shell, navigation, shared states, look tokens) | In progress (built and reviewed; waiting for the release merge) | Branch `v1-ui` (not yet on `v1`) | Shared fixes below |
| 1 · Home, Inbox, Activity, Search, Preferences | In progress (built and reviewed; waiting for the release merge) | Branch `v1-ui` | — |
| 2 · Teams and people (Members, team page, member profile, Org chart, Hire) | In progress (built and reviewed; waiting for the release merge) | Branch `v1-ui`; Opus reviews and fixes per feature | — |
| 3 · Organization settings (General, Roles, Agent setup, Members and access, Costs, Updates, Processes, Offices) | In progress (built and reviewed; waiting for the release merge) | Branch `v1-ui`; Opus reviews and fixes per feature | Two design decisions to apply (below); 9 Offices tests marked `fixme` for the settings layout at 960–1279 px with 200% text (SET-200) |
| 4 · Projects (list, frame, overview, team, activity, conversations, settings) | In progress | Branch `v1-ui`; reviewed and fixed | The Work tab shows v1's task board and bug list until step 5 replaces them |
| 5 · Work items (task, bug, request pages, Work board/list, dialogs, budgets) | Not started | Branch `v1-ui-work` holds a stopped first start (committed as WIP, unreviewed) | Build from the plan's step 5; reuse or discard the WIP |
| 6 · Docs, features and releases | Not started | — | — |
| 7 · Costs and insights | Not started | — | — |
| 8 · Access and chat gaps | Not started | The release ships v1's Chat; an unfinished Chat rebuild is on branch `v1-ui-chat-wip` | Chat's missing features and module extraction |
| 9 · Closing (all browsers, screenshots, FEATURES pass, final design walkthrough) | Not started | — | Include an interaction review, with https://a2ui.org/ as a reference |
| Release of `v1-ui` into `v1` | In progress | `v1-ui` 08630081; final review in `../agentic-org-ui-wt/checkpoints/reviews/release2.md` (local only, outside the repository) | Fix the specs that still open the old project page, a fresh review of that fix, then merge into `v1` |

**Next up:** finish the release of `v1-ui` into `v1`: commit the project Work tab fix and the
remaining spec updates, run the browser suite, get a fresh review, message the other two leads,
merge.

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

- `v1-ui`: everything built so far; integration branch `v1-ui-step2-all` feeds it. Both are pushed.
- `v1-ui-work` (worktree `work/`, pushed): the stopped step 5 start, committed as WIP.
- `v1-ui-chat-wip` (pushed): the unfinished Chat rebuild, kept for step 8.
- Feature branches `v1-ui-*` merged into `v1-ui-step2-all` can be deleted once `v1-ui` is in `v1`.

## Decisions made, still to apply

- Members and access shows usernames only to the owner, the administrator and the person themself.
- Updates puts what needs attention first: this instance, then offices (with any "Needs update"
  note at the top), then How to update.
- Project Activity lists newest first, like every other activity list.

## Shared fixes queued

- The settings layout and the shell at 960–1279 px with 200% text leave pages too little room.
- The shared ConfirmDialog doesn't close on browser Back; the shared Resource should show the office's
  `unsupported` answer as "not available in this version" with no Retry.
- Pointer focus return for menus and dialogs (app-shell-ux 305–307): an unreviewed start is on branch
  `origin/app-frontend-dev/pointer-focus`.

## For the other streams

- **Backend:** the UI expects the routes in `docs/design/web-api-v1.md` one by one; each screen swaps
  its mock for the real call when its route lands. B2.2's org wire change needs the UI mirrors updated
  in the same merge: message ui-dev first.
- **Linux:** nothing pending. The instance reader accepts `http_published` and `tls`; no screen shows
  TLS yet.
- **All:** UI build tooling, launchers and reviews run as headless Claude Opus sessions, each with its
  model pinned with `--model`. The owner's global default model is not used.
