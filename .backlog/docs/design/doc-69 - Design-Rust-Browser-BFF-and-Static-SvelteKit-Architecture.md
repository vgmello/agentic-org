---
id: doc-69
title: 'Design: Rust Browser BFF and Static SvelteKit Architecture'
type: guide
created_date: '2026-11-21T00:48:00Z'
updated_date: '2026-11-21T00:48:00Z'
tags:
  - design
  - architecture
  - web
  - bff
  - sveltekit
---
> **Review note (2026-11-21):** Core architecture is implemented in `apps/agentic-org-web` and `apps/agentic-org-ui` (`@sveltejs/adapter-static`). Open v1 work: building the browser-to-office bridge for the first unmocked vertical slice (`TASK-5`, B2 Q8), flipping `productionImportGuard` to `'error'` and removing static mock imports from production routes (`TASK-12`), adding dedicated async multiplexed office/SSE channels to `agentic-org-web` (`TASK-16`), and completing UI module migration (`TASK-17`).

# Rust browser BFF and static SvelteKit architecture

`agentic-org-web` is the browser BFF. `agentic-org-office` is the office worker. `agentic-org serve` runs the host, which starts both. The privilege boundaries between the three are those of the [host boundary design](office-host-boundary.md).

The architecture uses a static SvelteKit browser application with a Rust browser API/BFF. Reusable Rust application and domain modules support the browser BFF and future integration API adapters. Browser APIs come first; integration APIs are a later extension.

## Backend boundaries

```text
Svelte browser UI -> Rust browser API/BFF -> Rust application/domain modules
Future integrations -> Integration API adapters -> the same modules
```

The browser BFF authenticates browser requests, maps authorized inputs into typed operations, and shapes results for screens. It can compose results and return summaries, pagination, or focused projections to avoid unnecessary browser requests and payloads.

Business rules, domain authorization invariants, reusable queries, state transitions, validation, and durable behavior belong to their owning application/domain modules. Those modules expose deliberate typed capabilities to callers and remain independent of browser cookies, routes, browser DTOs, and HTTP response objects. The web, host and office privilege boundaries hold; the web process does not acquire database/key custody to implement a projection.

Future integration adapters will map their own versioned contracts and principal/authentication models onto those capabilities. They reuse the same business rules rather than calling browser endpoints or duplicating policy. Separate adapters can coexist in the same Rust service where their deployment and privilege boundaries allow it; a BFF does not require a separate executable.

A module does not need an HTTP endpoint now to be reusable later. Export only the typed capabilities required by real callers. Do not publish speculative integration endpoints, add dependencies, or introduce generic framework layers merely to reserve future API support. 

## Frontend runtime and deployment

Use `@sveltejs/adapter-static` with a navigation fallback (`index.html`), `ssr = false`, and `prerender = false` for the authenticated browser SPA. The build produces HTML, JavaScript, CSS, and other assets; agentic-org-web serves those files and the browser APIs from the same origin. API, asset, and event failures must retain their own responses instead of receiving the SPA fallback.

The static build does not freeze application data. Browser JavaScript fetches current data from the Rust BFF and consumes the live-event stream of the [web API](web-api-v1.md). SvelteKit provides browser routing and route loading without a SvelteKit server process. Request-time server routes, server load functions, and SvelteKit server actions are not part of this deployment profile.

The topology works for both local and remote HQ hosting. A local installation serves the browser from the local web. A remote installation runs the web process and backend on the remote host, with only the browser required on the user's computer. No SvelteKit runtime SSR requirement is introduced.

## Navigation and preloading

Static SPA deployment retains SvelteKit route-code and data preloading. For internal links, `data-sveltekit-preload-data="hover"` begins loading on hover or touch-start; `"tap"` begins on mouse-down or touch-start. `data-sveltekit-preload-code` supports code-only preloading, and `$app/navigation` provides programmatic preloading.

Automatic data preloading runs the destination route's required `load()` functions. In this SPA, universal `+page.ts` / `+layout.ts` load functions run in the browser and can fetch authorized read data from the Rust BFF. The prepared load result can be reused for the matching navigation. Fetches performed only after component mount are not automatically covered by route data preloading; integrate them with route loading or an explicit shared prefetch mechanism when that behavior is needed.

Keep preloaded operations safe to run speculatively: reads must not perform commands or durable state changes. Choose hover, tap, or code-only preloading according to request cost and data freshness. `apps/agentic-org-ui/src/app.html` enables hover data preloading globally; a screen benefits only if its fetching goes through route loading.

## References

Browser-oriented HTTP contracts are adapters over reusable capabilities of the domain modules; integration adapters are separate callers of the same capabilities.

- [Product structure and module ownership](project-structure.md)
- [Standalone deployment and HTTP contract](standalone-server.md#5-http-and-browser-contract)
- [Web API contract](web-api-v1.md)
- [SvelteKit single-page application deployment](https://svelte.dev/docs/kit/single-page-apps)
- [SvelteKit preloading](https://svelte.dev/docs/kit/link-options#data-sveltekit-preload-data)
- [SvelteKit browser and server load functions](https://svelte.dev/docs/kit/load#Universal-vs-server)
