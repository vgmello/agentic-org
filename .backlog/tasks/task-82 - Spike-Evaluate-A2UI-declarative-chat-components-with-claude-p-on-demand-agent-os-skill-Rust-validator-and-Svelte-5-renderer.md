---
id: TASK-82
title: >-
  Spike: Evaluate A2UI declarative chat components with claude -p, on-demand
  agent-os skill, Rust validator, and Svelte 5 renderer
status: To Do
assignee: []
created_date: '2026-11-22T04:09:00Z'
updated_date: '2026-11-23T04:07:00Z'
labels:
  - spike
  - backend
  - ui
  - messaging
  - agents
  - a2ui
  - blog
milestone: m-1
dependencies:
  - TASK-46
references:
  - https://a2ui.org/
documentation:
  - >-
    .backlog/docs/specs/doc-55 -
    REQ-04-Channel-Messaging-and-Question-Components.md
  - >-
    .backlog/docs/organization-data/doc-90 -
    Organization-Data-15-—-Messaging-and-Chat.md
  - .backlog/docs/design/doc-27 - Design-Agent-Instructions-Assembly.md
  - .backlog/docs/design/doc-36 - Design-Smart-Features-Not-in-v1.md
priority: medium
type: spike
ordinal: 86000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
REQ-04 (TASK-46) currently supports only a fixed 3-type AskQuestion card (choice, yes_no, free_text). To support richer declarative agent-driven UI components in chat (and future Tess surfaces in TASK-80) without allowing raw HTML/script generation or runtime-specific hacks, we want to evaluate adopting the A2UI declarative JSON protocol. Because managed agents run headless (claude -p with --strict-mcp-config and --disable-slash-commands, plus future Codex and agy adapters) and standing agent instructions are capped at 10 KiB (doc-27), agents must discover the A2UI component catalog on demand via an agent-os skill/schema and emit payloads via agent-os CLI rather than raw stdout or MCP. This spike evaluates real claude -p reliability, Rust server-side validation, and native Svelte 5 shadcn-svelte rendering before committing to full implementation.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Defines a candidate versioned A2UI component catalog (allowlisted primitives mapped 1:1 to local shadcn-svelte / Bits UI components) and a typed action-allowlist security contract fitting messages.metadata_json (<= 64 KiB MAX_META_BYTES, content_sha256 immutability per doc-90, zero raw HTML, strict CSP).
- [ ] #2 Evaluates on-demand skill/schema delivery (agent-os skill read / --schema staying within the 10 KiB BUILTIN_INSTRUCTIONS_MAX_BYTES cap) with real headless claude -p runs across representative chat/card prompts, documenting schema-validity rate, token/latency overhead, error/fallback handling, and portability to Codex and agy via agent-os CLI.
- [ ] #3 Assesses in-repo Rust schema validation plus a thin native Svelte 5 shadcn-svelte catalog renderer (versus external A2UI packages), verifying fail-closed unknown-component fallback, accessibility, offline pinned-dependency compliance, and zero {@html} usage.
- [ ] #4 Delivers a concise evaluation report and go/no-go ADR recommendation covering v1 vs v2 targeting, synergy with TASK-80 (Tess) and TASK-71, and proposed follow-up implementation tasks if approved.
<!-- AC:END -->
