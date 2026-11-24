---
id: TASK-16
title: >-
  Add dedicated async multiplexed office/SSE channels to agentic-org-web and
  clean up minor BFF/UI polish
status: Done
assignee:
  - '@lead'
created_date: '2026-11-20T23:24:00Z'
updated_date: '2026-11-21T02:14:00Z'
labels:
  - bff
  - rust
  - ui
  - p2
milestone: m-4
dependencies:
  - TASK-5
references:
  - apps/agentic-org-web/src/service.rs
  - apps/agentic-org-web/src/host.rs
  - apps/agentic-org-web/src/api.rs
  - apps/agentic-org-ui/src/app.html
documentation:
  - docs/architecture/architecture-and-data-audit.html
  - docs/design/rust-browser-bff-and-static-sveltekit.md
  - docs/design/web-api-v1.md
priority: medium
type: enhancement
ordinal: 16000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Audit findings M-12 (BFF transport) and L-05: agentic-org-web currently serializes host calls over one blocking Unix stream with a 100 ms idle poll and a 2-worker BlockingPool, which will bottleneck once multiplexed office requests and SSE live events land. Separately, api.rs duplicates admin checks and performs a serde_json roundtrip for host.recovery, and app.html sets hover preload despite effect-based fetching.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 agentic-org-web serves office RPCs and SSE live-event streams over dedicated async multiplexed channels without blocking on the 2-worker host BlockingPool or 100 ms poll loop.
- [x] #2 agentic-org-web preserves its verified zero-DB, zero-office-key, and OS confinement boundaries (dependency_closure test passes).
- [x] #3 JSON roundtrip mapping in apps/agentic-org-web/src/api.rs is replaced with typed From conversions, duplicate BFF auth short-circuiting is cleaned up, and app.html preload attributes match SvelteKit client fetching.
<!-- AC:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
Completed UI app.html preload adjustment (data-sveltekit-preload-data='off' matching SvelteKit client-mount fetching). Verified UI format:check, lint, svelte-check, vite build, and check:build-assets PASS.
<!-- SECTION:NOTES:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Delivered async multiplexed office & SSE channels in agentic-org-web (event-driven host reader/writer threads replacing the 100ms poll loop, async MultiplexedChannel office bridge, EventHub + POST /api/v1/events/stream SSE endpoint bypassing the 2-worker BlockingPool), preserved zero-DB/zero-office-key/OS confinement boundaries, replaced host.recovery serde_json roundtrip with typed recovery_state mapping, removed duplicate BFF admin short-circuiting, and set app.html data-sveltekit-preload-data='off'.
<!-- SECTION:FINAL_SUMMARY:END -->
