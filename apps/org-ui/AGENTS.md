> Optional tooling: the Svelte MCP server (`.mcp.json`) and the `.claude/` skills and agent are a convenience, not a requirement. No gate, review or merge depends on them; work without them if they are not loaded. They run from the exact, lockfile-pinned dev dependency `@sveltejs/mcp` in `apps/org-ui/node_modules` (start Claude in `apps/org-ui` after `bun install --frozen-lockfile`); the package is never downloaded at run time, and without `node_modules` they fail instead. The documentation tools query svelte.dev; the autofixer runs locally.

You are able to use the Svelte MCP server, where you have access to comprehensive Svelte 5 and SvelteKit documentation. Here's how to use the available tools effectively:

## Available Svelte MCP Tools:

### 1. list-sections

Use this FIRST to discover all available documentation sections. Returns a structured list with titles, use_cases, and paths.
When asked about Svelte or SvelteKit topics, when the Svelte MCP is available, use this tool at the start of the chat to find relevant sections.

### 2. get-documentation

Retrieves full documentation content for specific sections. Accepts single or multiple sections.
After calling the list-sections tool, when the Svelte MCP is available, analyze the returned documentation sections (especially the use_cases field) and then use the get-documentation tool to fetch ALL documentation sections that are relevant for the user's task.

### 3. svelte-autofixer

Analyzes Svelte code and returns issues and suggestions.
When the Svelte MCP is available, use this tool whenever writing Svelte code before sending it to the user. Keep calling it until no issues or suggestions are returned.

### 4. playground-link

Do not use the `playground-link` tool in this repository at all: it posts code to an external site.
