# Agentic Consulting desktop app (working name)

A Tauri 2 desktop app that manages the AI organization modeled in `../agents`. The architecture is in `../agents/projects/ac/agents/ac-staff-engineer-ada/knowledge/designs/desktop-app-architecture.md`, with ADRs in `../adrs/` next to it.

Stack: Tauri 2 · Svelte 5 + SvelteKit 2 (SPA, adapter-static) · Tailwind v4 · shadcn-svelte / Bits UI · Rust · SQLite (rusqlite).

## Layout and owners

| Path | What | Owner |
| --- | --- | --- |
| `app/` | SvelteKit UI and the Tauri shell (`app/src-tauri`) | Riley (`ac-software-engineer-riley`) |
| `crates/` | `org-proto` (API contract), `org-core`, `orgd`, `org-cli` | Remy (`ac-software-engineer-remy`) |
| `mods/presence/` | The Claude Code mod that forwards session events to `orgd` | Remy |
| `spikes/` | Throwaway spikes S1-S6, one folder each, with findings in a README | Remy (Riley for the S2 panel) |

- `crates/org-proto` is the contract between UI and backend. Changes to it need ada's (`ac-staff-engineer-ada`) review.
- Work on branches, and commit locally. Don't push anywhere: there's no remote yet.

## Expandable agent studio

The interactive 2D office prototype is in [`spikes/agent-studio`](spikes/agent-studio/README.md). It includes six animated agent roles, dynamic workspaces, an expanding room, and local layout persistence. See that directory for run and test commands.

