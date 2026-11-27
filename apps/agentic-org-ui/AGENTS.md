Claude Code and Codex start at the repository root (see the root `AGENTS.md`, "Tools for agents"). Both get the Svelte MCP tools below from the exact, lockfile-pinned dev dependency `@sveltejs/mcp` in `apps/agentic-org-ui/node_modules`: Claude Code through the root `.mcp.json`, Codex through `.codex/config.toml`. Run `bun install --frozen-lockfile` in `apps/agentic-org-ui` first. This configuration never downloads the package at run time and fails without `node_modules`; the upstream Svelte plugin, whose server downloads it, is not used. If the tools are unavailable, report that limitation rather than claiming they were used. The documentation tools query svelte.dev; the local server's autofixer runs locally.

## UI Rules

- **Mandatory Svelte Architecture & Quality Rules:** Before creating, editing, or reviewing any file under `apps/agentic-org-ui/`, read and follow [`.agents/rules/svelte-architecture-and-quality.md`](../../.agents/rules/svelte-architecture-and-quality.md), load the repo's vendored `svelte-core-bestpractices` and `svelte-code-writer` skills (in `../../.agents/skills/` — do not edit vendored copies), run `svelte-autofixer` on new/modified `.svelte` files, and run `bun run gate` before every commit.
- **Periodic 6-Dimension Independent Audit Cadence:** `bun run gate` automatically checks `audit-checkpoint.json` (`bun run audit:cadence`) and flags `ACTION REQUIRED` every **10 UI commits**, **14 days with UI changes**, or **1,500 changed UI lines** (and before closing any UI milestone). When triggered, run a fresh internal `code-reviewer` pass followed by `bun run audit:ui` (Opus 5.5 `xhigh` 6-dimension audit, bar `>= 9.0/10` across all 6 dimensions), fix any High/Medium finding, and update `audit-checkpoint.json` (see `.agents/rules/svelte-architecture-and-quality.md` §9).

You are able to use the Svelte MCP server, where you have access to comprehensive Svelte 5 and SvelteKit documentation. Here's how to use the available tools effectively:

## Available Svelte MCP Tools:

### 1. list-sections

Use this FIRST to discover all available documentation sections. Returns a structured list with titles, use_cases, and paths.
When asked about Svelte or SvelteKit topics, ALWAYS use this tool at the start of the chat to find relevant sections.

### 2. get-documentation

Retrieves full documentation content for specific sections. Accepts single or multiple sections.
After calling the list-sections tool, you MUST analyze the returned documentation sections (especially the use_cases field) and then use the get-documentation tool to fetch ALL documentation sections that are relevant for the user's task.

### 3. svelte-autofixer

Analyzes Svelte code and returns issues and suggestions.
You MUST use this tool whenever writing Svelte code before sending it to the user. Keep calling it until no issues or suggestions are returned.

### 4. playground-link

Generates a Svelte Playground link with the provided code.
After completing the code, ask the user if they want a playground link. Only call this tool after user confirmation and NEVER if code was written to files in their project.
