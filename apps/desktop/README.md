# apps/desktop: Agentic Org

**V1 direction changed 2026-11-09:** the target is a standalone Rust server
on macOS/Linux serving the browser application from HQ. Tauri is deferred to v2.
See the [server design](../../docs/design/standalone-server.md) and [deletion inventory](../../docs/design/standalone-server-removal-inventory.md).
This documentation change specifies the pivot; the source described below is the
inspected pre-pivot implementation. Its Tauri/bundle commands are historical
development instructions, not installation instructions for the new v1. Do not
claim the new CLI, HTTP server or remote sync exists until implemented. No migration
of the undeployed desktop app or its data is required.

**Target naming:** retain the browser UI as `apps/org-ui` (package `org-ui`) and
rename the worker app/package/binary to `apps/org-server` / `org-server`. See the
[proposed project structure](../../docs/design/project-structure.md). This file's
current source paths and desktop commands are pre-pivot evidence; the docs PR
performs no directory moves. Licensed UI stays inside the renamed UI application.

The desktop app: **Tauri 2 + SvelteKit 2 (static SPA) + Svelte 5 + Tailwind v4**, managed with **bun**. It is a *client*: it reaches an office server only through its Rust shell (`src-tauri`) and never reads an office's storage directly.

See [TECH-STACK.md](TECH-STACK.md) for the stack, how to run it and what is still undecided.

- `src/`: the UI. First screens: Welcome (Create / Open organization), Create organization, then the shell. The look follows the approved prototype (Tailwind Plus application UI, used here only, never published separately: see AGENTS.md).
- `src-tauri/`: the Tauri shell and a workspace member. `OfficeConnection` is its seam to an office (`RealOffice` on `org-client` by default; `MockOffice` with `AGENTIC_ORG_OFFICE=mock`, whose `AGENTIC_ORG_MOCK*` settings only apply then).
- `src/lib/org/`: the org chart's rules as pure functions (`model.ts`, `reporting.ts`, `team-edit.ts`, `people-edit.ts`, `agents.ts`), the commands the UI sends (`commands.ts`, `actions.ts`) and the browser mock HQ (`reducer.ts`, `api.ts`). `src-tauri/src/org_engine.rs` is the Rust mock HQ; both follow `crates/org-proto` and share `test-vectors/org-commands.json`.
- `INSTALLING-THE-OFFICE.md`: the office host and server inside the app bundle, the login service that starts the host, the dev install and the manual checklist.
- `e2e/`: Playwright specs, one per area, tests named after the UX criteria.
