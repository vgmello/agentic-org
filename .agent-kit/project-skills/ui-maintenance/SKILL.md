---
name: ui-maintenance
description: Maintain or review the static Agentic Org SvelteKit UI with scoped conventions, Svelte MCP and browser checks.
---

Read apps/agentic-org-ui/AGENTS.md and components.json. package.json pins Svelte 5.57.3, SvelteKit 2.70.4, TypeScript 6.0.3, Tailwind 4.3.5, shadcn-svelte 1.8.0, Bun 1.4.4 and Node 26.13.0. Reuse maintained shadcn-svelte and playwright-cli skills for their respective tasks.

For .svelte and .svelte.ts/.svelte.js work use available svelte-code-writer/core-bestpractices skills and official Svelte MCP: list-sections, relevant get-documentation, and autofixer until clean. Report unavailable essential capabilities. Static indexes do not replace them.

Keep static adapter and authoritative Rust transport. No Node backend or production mock fallback. Preserve pending/unknown/unavailable outcomes, accessibility and Tailwind Plus's product-only boundary.

Within apps/agentic-org-ui use bun install --frozen-lockfile, bun run check, bun run test, bun run build, bun run lint and bun run format:check. format writes; format:check does not. Changed flows need relevant Playwright, CSP/gateway and WebKit journey commands from CI; full suites exceed the fast gate. Tests use temporary data; UI changes provide scoped screenshots/output evidence.

Consult official version-relevant docs through Svelte MCP or https://svelte.dev/docs and https://shadcn-svelte.com/docs as needed. Reassess framework, transport or ownership changes.
