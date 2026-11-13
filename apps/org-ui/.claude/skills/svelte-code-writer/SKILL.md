---
name: svelte-code-writer
description: CLI tools for Svelte 5 documentation lookup and code analysis. MUST be used whenever creating, editing or analyzing any Svelte component (`.svelte`) or Svelte module (`.svelte.ts`/`.svelte.js`). If possible, this skill should be executed within the `svelte-file-editor` subagent for optimal results.
---

## CLI tools

You have access to `@sveltejs/mcp` CLI for Svelte-specific assistance. Run these commands from `apps/org-ui` with the pinned local binary `./node_modules/.bin/svelte-mcp` (after `bun install --frozen-lockfile`); never with `npx` or `bunx`, which would download a package at run time:

### List documentation sections

```bash
./node_modules/.bin/svelte-mcp list-sections
```

Lists all available Svelte 5 and SvelteKit documentation sections with titles and paths.

### Get documentation

```bash
./node_modules/.bin/svelte-mcp get-documentation "<section1>,<section2>,..."
```

Retrieves full documentation for specified sections. Use after `list-sections` to fetch relevant docs.

**Example:**

```bash
./node_modules/.bin/svelte-mcp get-documentation "$state,$derived,$effect"
```

### Svelte autofixer

```bash
./node_modules/.bin/svelte-mcp svelte-autofixer "<code_or_path>" [options]
```

Analyzes Svelte code and suggests fixes for common issues.

**Options:**

- `--async` - Enable async Svelte mode (default: false)
- `--svelte-version` - Target version, ideally the full version from `package.json` (e.g. `5.16.0`) so version-specific suggestions are enabled. The major must be 4 or 5 (default: 5)

**Examples:**

```bash
# Analyze inline code (escape $ as \$)
./node_modules/.bin/svelte-mcp svelte-autofixer '<script>let count = \$state(0);</script>'

# Analyze a file
./node_modules/.bin/svelte-mcp svelte-autofixer ./src/lib/Component.svelte

# Target Svelte 4
./node_modules/.bin/svelte-mcp svelte-autofixer ./Component.svelte --svelte-version 4
```

**Important:** When passing code with runes (`$state`, `$derived`, etc.) via the terminal, escape the `$` character as `\$` to prevent shell variable substitution.

## Workflow

1. **Uncertain about syntax?** Run `list-sections` then `get-documentation` for relevant topics
2. **Reviewing/debugging?** Run `svelte-autofixer` on the code to detect issues
3. **Always validate** - Run `svelte-autofixer` before finalizing any Svelte component
