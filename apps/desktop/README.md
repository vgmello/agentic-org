# apps/desktop: Agentic Org

The desktop app: **Tauri 2 + SvelteKit 2 (static SPA) + Svelte 5 + Tailwind v4**, managed with **bun**. It is a *client*: it reaches an office server only through its Rust shell (`src-tauri`) and never reads an office's storage directly.

See [TECH-STACK.md](TECH-STACK.md) for the stack, how to run it and what is still undecided.

- `src/`: the UI. First screens: Welcome (Create / Open organization), Create organization, then the shell. The look follows the approved prototype (Tailwind Plus application UI, used here only, never published separately: see AGENTS.md).
- `src-tauri/`: the Tauri shell and a workspace member. `OfficeConnection` is its seam to an office (a mock today, `org-client` later).
