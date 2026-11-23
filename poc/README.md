# Design Prototypes (`poc/`)

Standalone interactive static HTML prototypes used to explore and iterate on the Executive UI before porting patterns into the SvelteKit app.

## Contents

- `v6.html` — Current Executive UI prototype (served at `/` and `/v6`). Includes the 4 production themes (`Ocean`, `Classic`, `Report`, and `Report Night`) plus experimental prototype themes (`Graphite`, `Cozy Glass`).
- `v5.html` — Earlier Executive UI prototype iteration (served at `/v5`).
- `cozy.css`, `operations.css`, `fonts/` — Prototype-local stylesheets and font assets.
- `server.ts` — Local Bun static dev server with HMR.

## Running locally

```bash
bun poc/server.ts
```

Defaults to `http://localhost:3000` (override with `PORT=3001 bun poc/server.ts`).

## Production boundary

The production web UI lives in [`apps/agentic-org-ui/`](../apps/agentic-org-ui/). Nothing in `poc/` is imported into or bundled with production builds.
