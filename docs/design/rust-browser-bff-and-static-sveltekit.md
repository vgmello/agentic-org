# Rust browser BFF and static SvelteKit architecture

Accepted by Victor on 2026-11-14. Recorded by Ines for implementation and future API design.

The architecture uses a static SvelteKit browser application with a Rust browser API/BFF. Reusable Rust application and domain modules support the browser BFF and future integration API adapters. Browser APIs are the current delivery priority; integration APIs are a future extension.

## Backend boundaries

```text
Svelte browser UI -> Rust browser API/BFF -> Rust application/domain modules
Future integrations -> Integration API adapters -> the same modules
```

The browser BFF authenticates browser requests, maps authorized inputs into typed operations, and shapes results for screens. It can compose results and return summaries, pagination, or focused projections to avoid unnecessary browser requests and payloads.

Business rules, domain authorization invariants, reusable queries, state transitions, validation, and durable behavior belong to their owning application/domain modules. Those modules expose deliberate typed capabilities to callers and remain independent of browser cookies, routes, browser DTOs, and HTTP response objects. Existing gateway, host, and office privilege/process boundaries remain in force; the gateway does not acquire database/key custody to implement a projection.

Future integration adapters will map their own versioned contracts and principal/authentication models onto those capabilities. They reuse the same business rules rather than calling browser endpoints or duplicating policy. Separate adapters can coexist in the same Rust service where their deployment and privilege boundaries allow it; a BFF does not require a separate executable.

A module does not need an HTTP endpoint now to be reusable later. Export only the typed capabilities required by real callers. Do not publish speculative integration endpoints, add dependencies, or introduce generic framework layers merely to reserve future API support. Future integration work remains outside the immediate browser API scope.

## Frontend runtime and deployment

Use `@sveltejs/adapter-static` with a navigation fallback (`index.html`), `ssr = false`, and `prerender = false` for the authenticated browser SPA. The build produces HTML, JavaScript, CSS, and other assets; the Rust gateway serves those files and the browser APIs from the same origin. API, asset, and event failures must retain their own responses instead of receiving the SPA fallback.

The static build does not freeze application data. Browser JavaScript fetches current data from the Rust BFF and consumes the existing live-event transport. SvelteKit provides browser routing and route loading without a SvelteKit server process. Request-time server routes, server load functions, and SvelteKit server actions are not part of this deployment profile.

The topology works for both local and remote HQ hosting. A local installation serves the browser from the local gateway. A remote installation runs the gateway and backend on the remote host, with only the browser required on the user's computer. No SvelteKit runtime SSR requirement is introduced.

## Navigation and preloading

Static SPA deployment retains SvelteKit route-code and data preloading. For internal links, `data-sveltekit-preload-data="hover"` begins loading on hover or touch-start; `"tap"` begins on mouse-down or touch-start. `data-sveltekit-preload-code` supports code-only preloading, and `$app/navigation` provides programmatic preloading.

Automatic data preloading runs the destination route's required `load()` functions. In this SPA, universal `+page.ts` / `+layout.ts` load functions run in the browser and can fetch authorized read data from the Rust BFF. The prepared load result can be reused for the matching navigation. Fetches performed only after component mount are not automatically covered by route data preloading; integrate them with route loading or an explicit shared prefetch mechanism when that behavior is needed.

Keep preloaded operations safe to run speculatively: reads must not perform commands or durable state changes. Choose hover, tap, or code-only preloading according to request cost and data freshness. At the time of this decision, `apps/org-ui/src/app.html` already enables hover data preloading globally; that setting alone does not prove that each screen's current fetching participates in it.

## Existing design alignment and references

This decision preserves the existing domain modules containing vertical slices and Rust gateway/office boundaries. It clarifies that browser-oriented HTTP contracts are adapters over reusable capabilities, with future integration adapters planned as separate callers.

- [Product structure and module ownership](project-structure.md)
- [Standalone deployment and HTTP contract](standalone-server.md#5-http-and-browser-contract)
- [Existing browser gateway contract](gateway-api-v1.md)
- [SvelteKit single-page application deployment](https://svelte.dev/docs/kit/single-page-apps)
- [SvelteKit preloading](https://svelte.dev/docs/kit/link-options#data-sveltekit-preload-data)
- [SvelteKit browser and server load functions](https://svelte.dev/docs/kit/load#Universal-vs-server)
