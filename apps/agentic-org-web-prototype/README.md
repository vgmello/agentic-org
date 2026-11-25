# Design Prototypes (`apps/agentic-org-web-prototype/`)

Standalone interactive static HTML prototypes used to explore and iterate on the Executive UI before porting patterns into the SvelteKit app.

## Contents

- `v6.html` — Current Executive UI prototype shell (served at `/` and `/v6`). Includes the 4 production themes (`Ocean`, `Classic`, `Report`, and `Report Night`) plus experimental prototype themes (`Graphite`, `Cozy Glass`).
- `v6/` — Modular stylesheets and plain ES modules imported by `v6.html` (bundled natively by Bun with zero build config):
  - `v6/styles/` — Base, Home, and per-theme stylesheets (`base.css`, `home.css`, `theme-ocean-classic.css`, `theme-report.css`, `theme-graphite.css`).
  - `v6/data.js`, `v6/state.js`, `v6/ui.js`, `v6/main.js` — Mock org fixtures, persisted prototype state, shared UI helpers, and boot entry.
  - `v6/pages/` — Page renderers (`home.js`, `inbox.js`, `work.js`, `projects.js`, `insights.js`, `chat.js`, `activity.js`, `teams.js`, `agent-profile.js`, `settings.js`).
  - `v6/shell/` — App shell (`sidebar.js`, `router.js`, `cmdk.js`, `keyboard.js`, `dialogs.js`, `actions.js`).
- `v5.html` — Earlier Executive UI prototype iteration (served at `/v5`).
- `cozy.css`, `operations.css`, `fonts/` — Shared prototype stylesheets and font assets.
- `server.ts` — Local Bun static dev server with HMR.

## Running locally

```bash
bun apps/agentic-org-web-prototype/server.ts
```

Defaults to `http://localhost:3000` (override with `PORT=3001 bun apps/agentic-org-web-prototype/server.ts`).

## Quick browser check

```bash
bun apps/agentic-org-web-prototype/smoke.ts work graphite
bun apps/agentic-org-web-prototype/smoke.ts activity ocean
```

Pass a v6 route and theme to check the affected page in Chromium. The command uses
the installed Playwright dependency and starts and stops its own preview. It saves
a screenshot under `.playwright/`. Work and Activity also check their right-side
panels and keyboard dismissal.

## Production boundary

The production web UI lives in [`apps/agentic-org-ui/`](../agentic-org-ui/). Nothing in `apps/agentic-org-web-prototype/` is imported into or bundled with production builds.
