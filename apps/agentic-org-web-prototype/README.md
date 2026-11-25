# Design Prototypes (`apps/agentic-org-web-prototype/`)

Standalone interactive static HTML prototypes used to explore and iterate on the Executive UI before porting patterns into the SvelteKit app.

## Contents

- `v6.html` — Current Executive UI prototype (served at `/` and `/v6`). Includes the 4 production themes (`Ocean`, `Classic`, `Report`, and `Report Night`) plus experimental prototype themes (`Graphite`, `Cozy Glass`).
- `v5.html` — Earlier Executive UI prototype iteration (served at `/v5`).
- `cozy.css`, `operations.css`, `fonts/` — Prototype-local stylesheets and font assets.
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
