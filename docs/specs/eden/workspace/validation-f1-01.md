# Validation: F1-01 rename apps/desktop to apps/org-ui

**Verdict: No behaviour change found.**

- Change: merge `a0748a0` ("Merge jules/f1-01-org-ui into v1 (F1-01, Tomas READY 449f9ca)") on `v1`.
- Compared with its first parent `2174313` ("Merge eden/specs-19 into v1").
- Date: 2026-11-10. I tested the product as a reviewer only. I changed nothing in the product repo.

## Summary

- I drove 31 states (onboarding, every sidebar section, the agent profile, dialogs, tabs and the 404 page) in both builds.
- I ran each state at 2 sizes (1440×900, which is the Playwright default, and 960×640) and 2 OS colour schemes (dark and light), on WebKit. That is 124 screenshot pairs.
- **Every pair is pixel-identical**: 0 differing pixels in all 124, and the PNG files are byte-for-byte the same.
- In every state, the URL, page title, `<h1>`, the full visible text (`document.body.innerText`) and the count of visible controls match exactly.
- Every scripted interaction succeeded in both builds.
- The console output is the same in both builds. The only errors come from the deliberate 404 page.
- The after build's e2e suite passes: **`222 passed (3.8m)`**.

The app is dark only. `app.html` sets `class="dark"` and `<meta name="color-scheme" content="dark">`, and there is no theme toggle. So the "light" runs use WebKit's light OS preference and confirm the app ignores it. They render the same as the dark runs, in both builds.

Source check: apart from the rename, the only file under `src/`, `e2e/` or `static/` whose content changed is `src/lib/components/Sidebar.svelte`. One code comment changed there ("allowed inside apps/desktop only" became "allowed inside apps/org-ui only"). `package.json` and `bun.lock` changed only the package name (`agentic-org-desktop` became `org-ui`). Everything else in the merge is documentation, `Cargo.toml` paths and a doc comment in `org-proto`.

## Findings

None. F1-01 introduced no difference I could observe, and nothing stops the app from loading.

Two things look the same in both builds, so they are not F1-01 findings. I list them only for later design attention:

- At 960×640 the Org chart toolbar runs off the right edge: the last button, "Actual size", is cut off. See `960x640-dark/09-org-chart-teams.png` in both builds.
- After you open an agent from Agents (`/org-chart?member=kai`) and then click "Org chart" in the sidebar, the Kai details panel is still open at the top of the chart.

## Screens

Screenshots are in `validation-f1-01-shots/{before,after}/<run>/<name>.png`, where `<run>` is one of `default-dark`, `default-light`, `960x640-dark` or `960x640-light`. The table gives the default-dark paths. The same name exists in every run folder, and all four runs gave the same result.

| # | Screen and state | Interaction | Before | After | Result (all 4 runs) | Screenshot (default-dark) |
|---|---|---|---|---|---|---|
| 01 | Welcome (first run) | Visit `/`, which redirects to `/welcome` | ok | ok | Same, 0 px | `before/default-dark/01-welcome.png`, `after/default-dark/01-welcome.png` |
| 02 | Create organization form | Click "Create organization" | ok | ok | Same, 0 px | `…/02-create-form.png` |
| 03 | Create form, empty-name error | Submit with an empty name | ok | ok | Same, 0 px | `…/03-create-name-error.png` |
| 04 | Shell, Agents (after creating "Acme") | Fill in the name and create, which lands on `/agents` | ok | ok | Same, 0 px | `…/04-shell-agents.png` |
| 05 | Agents, "Add an agent" dialog | Click "Add agent" | ok | ok | Same, 0 px | `…/05-agents-add-dialog.png` |
| 06 | Agents, dialog cancelled | Press Escape | ok | ok | Same, 0 px | `…/06-agents-dialog-cancelled.png` |
| 07 | Agents, Active filter | Click the "Active" filter | ok | ok | Same, 0 px | `…/07-agents-filter-active.png` |
| 08 | Agent profile (Kai) | Click Kai's name, which opens `/org-chart?member=kai` | ok | ok | Same, 0 px | `…/08-agent-profile-kai.png` |
| 09 | Org chart, Teams | Sidebar, then Org chart | ok | ok | Same, 0 px | `…/09-org-chart-teams.png` |
| 10 | Org chart, List view | Click "List" | ok | ok | Same, 0 px | `…/10-org-chart-list.png` |
| 11 | Org chart, People tab | Switch tab | ok | ok | Same, 0 px | `…/11-org-chart-people-tab.png` |
| 12 | Org chart, Reporting lines tab | Switch tab | ok | ok | Same, 0 px | `…/12-org-chart-reporting-tab.png` |
| 13 | Projects list | Sidebar, then Projects | ok | ok | Same, 0 px | `…/13-projects.png` |
| 14 | Project page (Beacon), Overview | Click Beacon | ok | ok | Same, 0 px | `…/14-project-beacon.png` |
| 15 | Project page, Tasks tab | Switch tab | ok | ok | Same, 0 px | `…/15-project-tasks-tab.png` |
| 16 | Project, "New task" dialog | Click "New task" | ok | ok | Same, 0 px | `…/16-project-new-task-dialog.png` |
| 17 | Project, dialog cancelled | Press Escape | ok | ok | Same, 0 px | `…/17-project-dialog-cancelled.png` |
| 18 | Work (tasks) | Sidebar, then Work | ok | ok | Same, 0 px | `…/18-work.png` |
| 19 | Work, Bugs tab | Switch tab | ok | ok | Same, 0 px | `…/19-work-bugs-tab.png` |
| 20 | Chat | Sidebar, then Chat | ok | ok | Same, 0 px | `…/20-chat.png` |
| 21 | Chat, #release-v1 | Open the channel | ok | ok | Same, 0 px | `…/21-chat-channel.png` |
| 22 | Chat, thread panel | Click "3 replies" | ok | ok | Same, 0 px | `…/22-chat-thread.png` |
| 23 | Chat, thread closed | Click "Close thread" | ok | ok | Same, 0 px | `…/23-chat-thread-closed.png` |
| 24 | Chat, "Add a channel" dialog | Click "Add channel" | ok | ok | Same, 0 px | `…/24-chat-add-channel-dialog.png` |
| 25 | Chat, dialog cancelled | Press Escape | ok | ok | Same, 0 px | `…/25-chat-dialog-cancelled.png` |
| 26 | Sessions ("Coming soon" placeholder) | Sidebar | ok | ok | Same, 0 px | `…/26-sessions.png` |
| 27 | Activity ("Coming soon" placeholder) | Sidebar | ok | ok | Same, 0 px | `…/27-activity.png` |
| 28 | Processes | Sidebar | ok | ok | Same, 0 px | `…/28-processes.png` |
| 29 | Offices | Sidebar | ok | ok | Same, 0 px | `…/29-offices.png` |
| 30 | Settings ("Coming soon" placeholder) | Sidebar | ok | ok | Same, 0 px | `…/30-settings.png` |
| 31 | Unknown route (`/nope`) | Visit `/nope`, which shows "Something went wrong" | ok | ok | Same, 0 px | `…/31-not-found.png` |

"Tasks" in this build lives on a project's Tasks tab (rows 15 to 17) and in the cross-project Work screen (rows 18 and 19). There is no separate Tasks section. Settings, Sessions and Activity are placeholders in both builds.

## Console errors

These are identical in both builds and in all four runs. They all come from step 31, the deliberate unknown route:

| Build | Type | Message |
|---|---|---|
| before | error | Failed to load resource: the server responded with a status of 404 (Not Found) |
| before | warning | The next HMR update will cause the page to reload |
| before | error | Error: Not found: /nope |
| after | error | Failed to load resource: the server responded with a status of 404 (Not Found) |
| after | warning | The next HMR update will cause the page to reload |
| after | error | Error: Not found: /nope |

Neither build logged a console error or page error on any other screen.

## E2E (after build)

`cd apps/org-ui && PW_PORT=5190 bunx playwright test --reporter=line` gave:

```
  222 passed (3.8m)
```

This matches Jules's merge evidence (222/222). The suite's own config runs Chromium, not WebKit. The full log is `validation-f1-01-shots/e2e-after.log`.

## How each build was started

1. I made throwaway worktrees from `/Users/vgmello/projects/agentic-consulting/app`:
   - `git worktree add --detach <scratch>/after a0748a0`
   - `git worktree add --detach <scratch>/before 2174313`
2. I installed dependencies with bun 1.4.4: `bun install --frozen-lockfile` in `before/apps/desktop` and in `after/apps/org-ui`. Each installed 118 packages with no errors.
3. I started each dev server on the browser TypeScript mock office, the same mode the e2e suite uses (`bun run dev`, `src/lib/office/mock.ts`), with no Tauri and no real server:
   - before: `cd before/apps/desktop && bun run dev --port 5181 --strictPort`
   - after: `cd after/apps/org-ui && bun run dev --port 5182 --strictPort`
4. I confirmed each port served its own tree. The served `Sidebar.svelte` comment read "apps/desktop" on 5181 and "apps/org-ui" on 5182, and each process's working directory was its own worktree.
5. I drove both with `validation-f1-01-shots/validate.mjs`. It uses Playwright 1.65.0 from the after build's `node_modules` and WebKit revision 2388.
   - Each run used a fresh browser context with an empty `localStorage`.
   - The clock was fixed at 2026-11-10T12:00:00Z so that times match.
   - The script waited for `html[data-hydrated]` and for animations to finish before each screenshot.
   - Pixel diffs were done in the browser on canvas. Any channel difference greater than 0 counts as a different pixel.
6. I took the per-step comparison data (URL, h1, text, control count, pixel counts, console) from `validation-f1-01-shots/summary.json`.
7. I stopped both servers and removed both worktrees afterwards.
