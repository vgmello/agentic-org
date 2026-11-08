# apps/desktop: Agentic Org

The desktop app: **Tauri 2 + SvelteKit 2 (static SPA) + Svelte 5 + Tailwind v4**, managed with **bun**. It is a *client*: it reaches an office server only through its Rust shell (`src-tauri`) and never reads an office's storage directly.

See [TECH-STACK.md](TECH-STACK.md) for the stack, how to run it and what is still undecided.

- `src/`: the UI. First screens: Welcome (Create / Open organization), Create organization, then the shell. The look follows the approved prototype (Tailwind Plus application UI, used here only, never published separately: see AGENTS.md).
- `src-tauri/`: the Tauri shell and a workspace member. `OfficeConnection` is its seam to an office (a mock today, `org-client` later).
- `src/lib/org/`: the org chart's rules as pure functions (`model.ts`, `reporting.ts`, `team-edit.ts`, `people-edit.ts`, `agents.ts`), the commands the UI sends (`commands.ts`, `actions.ts`) and the browser mock HQ (`reducer.ts`, `api.ts`). `src-tauri/src/org_engine.rs` is the Rust mock HQ; both follow `crates/org-proto` and share `test-vectors/org-commands.json`.
- `INSTALLING-THE-OFFICE.md`: the office host and server inside the app bundle, the login service that starts the host, the dev install and the manual checklist.
- `e2e/`: Playwright specs, one per area, tests named after the UX criteria.
