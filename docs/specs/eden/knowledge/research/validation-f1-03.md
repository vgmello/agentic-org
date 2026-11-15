# Validation · F1-03 (Tauri removed) and BUG-19 (v1 07b9b9d vs 5068a6a)

**Verdict:** both do what they say, with no regression on the screens covered. One new S3 (N1) needs a product decision.
- **Pixel compare:** 398 of 428 pairs byte-identical; all Offices and Processes pairs identical. Of the 30 that differ, 26 are the "Bring in a team…" button removed from Agents and Org chart (expected, the import is retired); the other 4 are the known antialiasing and clock noise.
- **SRV-18:** every production area, in the static release, shows "<Area> is unavailable / This area is not available yet.", with one `main` and nav "Main", no sample data, no buttons and no API calls. Fakes can't be turned on. Unknown routes give the 404.
- **BUG-19 fixed:** exactly one `main` at all 428 steps and all 34 landmark steps (client-side navigation, Back, Forward, reload, dialogs, 404), never nested, named by the page's h1, and nav "Main" keeps its name. Before, only 16 of 428 steps had a `main`.
- **Local checks:** check `COMPLETED 1156 FILES 0 ERRORS 0 WARNINGS 0 FILES_WITH_PROBLEMS`, unit `Tests  1093 passed (1093)`, build `✔ done`, e2e `225 passed (6.4m)`.

**New findings, checked by me:**
- **N1 · S3, exposed by 07b9b9d.** In a browser window narrower than 960 px (600 to 800 px; 720 px is 200% zoom), the fixed 288 px sidebar never collapses. `(shell)/+layout.svelte:100-103` uses `w-72` and `pl-72` with no breakpoint. Chat squeezes to one letter per line, and Org chart scrolls sideways. Seen in `blitz-B/narrow-600x800-chat.png`. Tauri used to enforce the 960 px minimum. The design system's collapsed nav (64 px) is the fix. Product decision: mine. In a browser we can't stop people from narrowing the window or zooming, so the shell must work below 960 px (WCAG 1.4.10 reflow at 320 CSS px is the target).
- **N2 · S4.** On `/` the unavailable page says "Agentic Org is unavailable / This area is not available yet." There's no reason and no next step (design system, Errors). `platform/shell/Unavailable.svelte:8,25-26`.
- **N3 · S4.** The header's "Agentic Org" home link has no `focus-visible` style, so it falls back to the browser's ring rather than `--ring` (`Unavailable.svelte:14`).
- **S4, pre-existing:** the 404 page sets no `<title>` (`+error.svelte`).

Screenshots and scripts: `workspace/validation-f1-03-shots/` (not committed). The full working report follows.

---

# Validation: F1-03 (Tauri shell removed) and BUG-19 (main landmark), 07b9b9d compared with 5068a6a

**Verdict: F1-03 and BUG-19 do what they say, with no regression in any screen I covered.**

- **Pixels:** 398 of 428 pairs are byte-identical. The other 30 are the retired "Bring in a team…" button, which is expected, plus Chat timing and one known antialiasing flake.
- **SRV-18:** every production area shows an explicit unavailable state, in ordinary dev and in the static release. No sample data, no API calls, and no native controls.
- **BUG-19:** every screen has exactly one `main`, through client-side navigation, Back, Forward, reload and open dialogs.
- **New problems:** none blocks. Three are new or newly exposed:
  - N1 (S3): a browser window narrower than the old 960 px native minimum breaks the shell.
  - N2 (S4): the unavailable copy.
  - N3 (S4): one focus style.

- Change: merge `07b9b9d` ("Merge jules/f1-03-remove-tauri into v1") on `v1`, compared with `5068a6a`.
- Diff: 158 files, +769 −22,327.
  - `src-tauri/` is gone, along with the Tauri office, org and work adapters, existing-team import, `open-external`, the menu-bar `navigate` listener, login-item controls and the drag regions.
  - New: `platform/shell/Unavailable.svelte`, `capabilities.ts` and `unavailable-api.ts`.
  - Fakes are composed only by `vite dev --mode e2e` (`e2e/vite-composition.ts`).
  - The shell content wrapper is now `<main aria-labelledby="shell-page-title">` (`routes/(shell)/+layout.svelte:103`).
- How it starts now (B's README): `bun run dev` and the release show the areas as unavailable. The retained UI runs only on the test composition, so I compared the screens with B on `bun run dev --mode e2e`.
- Date: 2026-11-10. I tested as a reviewer only and changed nothing in the product repo. The worktrees and servers are gone.

## 1. No regressions: pixel comparison (A against B)

**107 states × 4 runs** (1440×900 and 960×640, OS dark and light, WebKit) gives **428 pairs. 398 are byte-identical.** The states are:

- 31 Offices states (every pushed status, restart answers, counts, keyboard);
- 45 Processes states (every place, sheet, scenario and the keyboard path);
- the 31-state F1-01 main set.

| States | Pairs | Why they differ | Check against the spec |
| --- | --- | --- | --- |
| `04`, `06`, `07` (Agents), `08`, `09`, `10` (Org chart), plus `05` at 960 | 26 | "Bring in a team…" is gone from the Agents header and the Org chart toolbar. Controls go down by 1 and the rest of the text is identical | Expected: the inventory retires existing-team import (`standalone-server-removal-inventory.md` acceptance: "Existing-team import … gone from v1 navigation"). Hire stays. At 960 the Org chart toolbar now overflows by 98 px, not 241 px. BUG-2 is still there, with less overflow |
| `16-project-new-task-dialog` | 3 | 12 px of antialiasing at the Title focus ring, caught mid-transition | Known flake (F2-01b, office-order). Text and focus are identical |
| `20-chat` (960 light) | 1 | "Waiting 8 min" against "Waiting 9 min" | Run timing |

- **Offices:** all 124 pairs are identical. **Processes:** all 180 pairs are identical.
- **Window chrome:** removing the drag regions changes no pixel. The sidebar keeps its 32 px top padding, which used to clear the traffic lights, so the shell looks exactly as before. The inventory (line 53) asks only for the attributes to go.
- **Behaviour per step** (URL, headings, alerts, focus, overflow, title, controls): the same in all 428 steps, apart from the Bring in button, the Chat timing and the expected changes below.
- **Expected change, the scroller:** the content scroller is now `main`, not a `div`. The scroll offsets are identical.
- **Expected change, the landmarks:** see §3.
- **Console:** the same in both builds. Only the deliberate 404 shows errors.

## 2. SRV-18: unserved production areas

**Spec:** SRV-18 (`server-installation-and-access-ux.md:1299` at 07b9b9d) says: "Production browser paths use the HTTP API, with an explicit unavailable state for missing features. Missing Tauri globals or API failure never select sample data." **It gives no copy.** The copy is therefore judged against the design system (finding N2).

I tested against B's ordinary `bun run dev` (5236) and the **static release** (`bun run build`, then `bun run preview`, 5237). I used WebKit at 1440 and 960, dark and light, plus Chromium on `/settings`. Both targets gave the same result:

| Area (path) | HTTP | State shown | Tab title |
| --- | --- | --- | --- |
| `/`, `/welcome`, `/create` | 200 | **"Agentic Org is unavailable"**, "This area is not available yet." (`role=status`) | "Agentic Org unavailable · Agentic Org" |
| `/chat` | 200 | "Chat is unavailable", same body | "Chat unavailable · Agentic Org" |
| `/agents`, and `/agents?mock=healthy&org=empty&copilot=on` | 200 | "Agents is unavailable" | "Agents unavailable · …" |
| `/org-chart`, `/org-chart?member=kai` | 200 | "Org chart is unavailable" | ✓ |
| `/projects`, `/projects/beacon` | 200 | "Projects is unavailable" | ✓ |
| `/work` | 200 | "Work is unavailable" | ✓ |
| `/sessions` | 200 | "Sessions is unavailable" | ✓ |
| `/activity` | 200 | "Activity is unavailable" | ✓ |
| `/processes`, `/processes?place=team:web` | 200 | "Processes is unavailable" | ✓ |
| `/offices`, `/offices?office=crashed` | 200 | "Offices is unavailable" | ✓ |
| `/settings` | 200 | "Settings is unavailable" | ✓ |
| `/nope`, `/projects/beacon/extra`, `/settings/anything` | 404 | The root error page: "404 · Something went wrong · Not Found · Back to the start" (not "unavailable", as intended) | **empty** (pre-existing, see the notes) |

Every unavailable page has these properties:

- **Layout:** a header with the logo and an "Agentic Org" home link, `nav "Main"` with all 10 areas, and the current one marked `aria-current=page`.
- **Landmarks:** one `main` labelled by the heading.
- **Nothing leaks:** no sample names (Acme, Beacon, Noor, kai) and no "Sample data". There are no buttons and no `data-tauri-drag-region`.
- **No API calls:** the release made none. Dev made only `GET /package.json?import`, Vite serving `pkg.version`, which is BUG-46's area.
- **No sideways scroll** at 960.

Forged state doesn't enable anything. `__TAURI_INTERNALS__`, `__mockOfficeStatus` and a remembered sample connection in `localStorage` don't enable fakes. Client-side navigation through all 10 areas, Back ×3 and the home link stay unavailable, with `aria-current` following.

**Screenshots:** `validation-f1-03-shots/srv18-dev/` and `srv18-release/`, at 1440 and 960, dark and light. Jules's `SRV-18-unavailable.png` matches mine.

**Not a finding, a note:** the release bundle still contains the sample and mock modules. The import guard reports 9 in report mode: copilot mock, messaging sample, processes sample and world, org sample, and package.json. They are never rendered, because the root layout replaces every route. The README says this stays until the module cutovers.

## 3. BUG-19: landmarks

`landmarks.mjs` (A and B) walks these steps:

- create organization;
- each of the 10 nav items, by client-side navigation;
- Projects › Beacon, then a chat channel and its thread;
- Back ×6 and Forward ×3;
- reload;
- the Add agent sheet and a Processes sheet, open;
- direct visits to `/org-chart?member=kai`, `/nope` (then Back), `/projects/nope` and `/welcome`.

The pixel run also recorded the landmarks at all 428 B steps.

- **A:** no shell screen has a `main`. Only onboarding and the 404 have one (16 of 428 steps). The landmarks are `navigation "Main"`, plus a stray `banner` on Projects › Beacon (its `<header>` sat outside any sectioning element).
- **B:** **exactly one `main` at all 428 steps and all 34 landmark-script steps, never nested.** It stays through client-side navigation, Back, Forward, reload, open dialogs and the 404. On shell routes the `main` is named by the page's h1, and `navigation "Main"` keeps its name (`Sidebar.svelte:27`). The stray `banner` is gone, now inside `main`.

Landmark tree per route (B, from Playwright's ARIA snapshot; `region "Notifications alt+T"` is the toaster on every shell route):

| Route | Landmarks |
| --- | --- |
| `/welcome` | main › region "Organizations on this Mac" |
| `/create` | main |
| `/agents` | navigation "Main" · main "Agents" |
| `/org-chart` | navigation "Main" · main "Org chart" › region "Owner" › region "Not in a team 2", region "Unassigned projects" *(pre-existing nesting and a long "Owner" name, the same in A)* |
| `/projects` | navigation "Main" · main "Projects" |
| `/projects/beacon` | navigation "Main" · main "Projects" › navigation "Breadcrumb" |
| `/work` | navigation "Main" · main "Work" › navigation "Saved filters" |
| `/chat` (thread open) | navigation "Main" · main "Chat" › navigation "Chat", region "Conversation", complementary "Thread" |
| `/sessions`, `/activity`, `/settings` | navigation "Main" · main "<name>" |
| `/processes` (sheet open) | navigation "Main" · main "Processes" · dialog "Branch, review and merge" |
| `/offices` | navigation "Main" · main "Offices" › region "Unplaced · 1 agent" |
| `/nope` (404) | main |
| production unavailable pages | navigation "Main" · main "<Area> is unavailable" (and the header) |

- Jules's `shell-main-offices.png` matches.
- There's no skip link. The first Tab goes to the Chat nav link, in both builds. The `main` landmark now gives assistive tech the bypass.

## 4. What else F1-03 removed or changed (former Tauri behaviour, now in a browser)

| Was | Now (B) | Sensible in a browser? |
| --- | --- | --- |
| Window controls and an overlay title bar (`tauri.conf.json` `titleBarStyle: Overlay`); drag strips in the sidebar, shell header and onboarding | No `data-tauri-drag-region` (0, was 4). Pixels unchanged, and the empty 32 px traffic-light padding stays | Yes. The padding is harmless |
| Native minimum window, 960×640 (`minWidth`/`minHeight`) | **No minimum.** A browser can be narrower | **No: finding N1** |
| Menu bar `navigate` events (`isAppPath`) | Gone. The URL and history do the job | Yes. Back, Forward and deep links to any route work (§3) |
| Chat links opened by the Rust shell (`open_external_url`) | Plain `<a target="_blank" rel="noopener noreferrer">`. A click opens a new tab and the chat stays put | Yes |
| Login Items: "Open Login Items" and "Turn on" on Welcome and Create failures, with `needs_approval`, `not_installed` and `turned_off` | The buttons and those failure codes are gone. Old `?mock=` values now act as healthy in the test fakes. Unreachable and storage errors keep "Try again" | Yes. Service setup moves to the CLI (SRV) |
| Existing-team import (file and folder picker, `tauri-plugin-dialog`) | Buttons, dialogs and modules removed. Hire stays (`retired-import.spec`) | Yes, as specified |
| Tab and window title | The window title was fixed. Each route already sets "<Area> · Agentic Org", and unavailable pages "<Area> unavailable · Agentic Org" | Yes, except the 404, which has an empty title (pre-existing) |
| Production data sources (`inTauri()`, then Tauri or mock) | Unavailable by default. `ensureRunning`, `connections` and the others reject with `unavailable` (503 for the connection load). No mock fallback | Yes (SRV-18) |

## New problems

### N1 · S3 · newly exposed by 07b9b9d · below 960 px the shell doesn't adapt

- **Repro:** open the app in a browser window 800, 720 or 600 px wide. 720 is a 1440 laptop at 200% zoom.
- **Expected:**
  - The design system: "a left nav at 240 px that collapses to 64 px icons … The window's minimum width is 960 px".
  - Tauri enforced that minimum (`tauri.conf.json` `minWidth: 960`, `minHeight: 640`, deleted in F1-03).
  - A browser can't enforce it, and SRV-8 and SIGN-23 (dogfood 1.2) open the app "from a browser on the other machine".
  - So below 960 the shell has to adapt (collapse the nav), or the spec has to say what happens. WCAG 1.4.10 Reflow also applies at 320 CSS px.
- **Actual:**
  - The sidebar stays fixed at 288 px over the content.
  - At 600 the content gets 312 px: Chat's conversation squeezes to one letter per line, and Org chart scrolls 458 px sideways.
  - At 720 Org chart scrolls 338 px sideways.
  - The same holds in A's plain browser mode, so the code didn't change. F1-03 only removed the guard.
- **Screenshots:** `validation-f1-03-shots/blitz-B/narrow-600x800-chat.png`, `narrow-600x800-org-chart.png` and `narrow-720x450-*.png` (`blitz-B.json`, `narrow-*`).
- **Code:** `routes/(shell)/+layout.svelte:100` `fixed inset-y-0 z-50 flex w-72` and `:103` `pl-72`.
- **For ada:** a product decision is needed: a responsive collapse, or a documented minimum browser width.

### N2 · S4 · new in 07b9b9d · the unavailable copy doesn't say why or what next, and blames the whole product on `/`

- **Actual:**
  - On `/`, `/welcome` and `/create` the heading is "**Agentic Org is unavailable**", which reads as if the product is down, and the tab says "Agentic Org unavailable · Agentic Org".
  - The areas read "Agents is unavailable" and "Sessions is unavailable".
  - The body is always "This area is not available yet."
- **Expected:**
  - SRV-18 gives no copy. The design system says errors give "what happened, why (if known) and what to do next".
  - Suggested copy: "Agents isn't available yet. This server doesn't serve it in this version." Use the area's name on `/`, for example "Agentic Org isn't set up in this browser yet".
- **Screenshots:** `srv18-release/1440-dark_.png` and `1440-dark_agents.png`.
- **Code:** `platform/shell/Unavailable.svelte:8` (`area` falls back to "Agentic Org"), `:25-26`; `platform/unavailable-api.ts:1`.

### N3 · S4 · new in 07b9b9d · the unavailable page's "Agentic Org" link has no focus style of its own

- **Actual:** it shows the browser's default ring (`auto 3px` at 50% alpha). The nav links on the same page use a 2 px outline.
- **Expected:** the design system's `--ring`, "2 px plus a 2 px offset".
- **Screenshot:** `srv18-release/keyboard-focus.png`.
- **Code:** `Unavailable.svelte:14` `<a href="/" class="text-lg font-semibold">`.

### Pre-existing, not catalogued (S4)

- **The 404 page has an empty tab title in A and B.** In a browser tab that leaves just the URL. `routes/+error.svelte` has no `<svelte:head><title>`.
- **Desktop copy is still in the retained screens:**
  - "Organizations on this Mac" (`welcome/+page.svelte:61`);
  - "Start the office on This Mac" (`create/+page.svelte:58`);
  - the sidebar's "This Mac";
  - the Send button's screen-reader hint "(⌘ Enter)" (`Composer.svelte:218`), where Ctrl+Enter also works.
- These show only with test fakes in B, and the inventory (line 208) already plans the rewrite. It needs no action now, but the cutover should pick it up.

Already catalogued and not re-reported: BUG-2 (Org chart toolbar at 960, better by 143 px now), BUG-18 and BUG-23..27 (Offices sheet), BUG-46 (package.json import) and O1.

## 5. Local checks in B (07b9b9d)

- `bun run check` (heavy): `COMPLETED 1156 FILES 0 ERRORS 0 WARNINGS 0 FILES_WITH_PROBLEMS`
- `bun run test` (heavy): `Test Files  49 passed (49)` / `Tests  1093 passed (1093)`
- `bun run build` (heavy): `Wrote site to "build"` / `✔ done`. The import guard is in report mode and lists 9 modules.
- `bun run test:e2e --workers=2` (heavy, `PW_PORT=4273`; builds, then the `chromium` and `production` projects): `225 passed (6.4m)` (exit 0)

## Method

- **Worktrees:** `git worktree add --detach` for A, B and C under the session scratchpad `f1-03/`. All are removed now, and every server is stopped.
- **Servers:**
  - A: `bun run dev` (5233; A has no modes).
  - B: `bun run dev --mode e2e` (5234) for the screen comparison, plain `bun run dev` (5236), and `bun run preview` of the build (5237) for SRV-18.
- **Scripts** (in `validation-f1-03-shots/`, copied and adapted; the originals are untouched):
  - `validate.mjs`: the office-order and F2-01b flows together, with builds and flows from the environment.
    - The overflow probe now uses the content scroller (`.pl-72`) in both builds, because the old one looked inside `main`.
    - Every step records its landmarks (DOM and ARIA snapshot).
  - `landmarks.mjs`: BUG-19.
  - `srv18.mjs`: SRV-18, run as dev and as release.
  - `blitz.mjs`: the former Tauri behaviours.
- **Clock and capture:** each flow ran in a fresh WebKit context with the clock at 2026-11-10T12:00Z. Each step also has a `.norm.png` with every scroller at the top.
- **Outputs:** `summary.json` and `results.json` hold the pixel and behaviour data. The diff images are in `diff/<run>/`.
