# apps/desktop: agentic.org

The desktop app: **Tauri 2 + SvelteKit 2 (static SPA) + Svelte 5 + Tailwind v4.** It is a *client*: it connects to one or more office servers through `crates/org-client`. It never reads an office's storage directly.

- `src/`: the UI. The first screens are **Open organization** and **Create organization**. The look follows the approved prototype (Tailwind Plus application UI). Tailwind Plus code stays inside this app only. Never publish it as a separate kit or template (see the licence notes in AGENTS.md).
- `src-tauri/`: the Tauri shell. It's a menu-bar app that can hide from the Dock, holds the list of office connections, and on "Create organization" starts and registers the local office server.

The app agent creates this with the Tauri and SvelteKit tooling, then adds `apps/desktop/src-tauri` to the root `Cargo.toml` members.
