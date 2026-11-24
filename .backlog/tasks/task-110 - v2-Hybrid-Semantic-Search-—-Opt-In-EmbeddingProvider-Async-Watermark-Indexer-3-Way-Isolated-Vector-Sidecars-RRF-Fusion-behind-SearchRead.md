---
id: TASK-110
title: >-
  v2: Hybrid Semantic Search — Opt-In EmbeddingProvider, Async Watermark
  Indexer, 3-Way Isolated Vector Sidecars & RRF Fusion behind SearchRead
status: To Do
assignee: []
created_date: '2026-11-24T16:55:00Z'
labels:
  - v2
  - backend
  - rust
  - search
  - ai
  - ax
  - security
  - docs
milestone: m-1
dependencies:
  - TASK-91
  - TASK-52
documentation:
  - .backlog/docs/specs/doc-39 - Spec-App-Shell-and-Navigation-UX.md
  - .backlog/docs/specs/doc-52 - Spec-Knowledge-Decisions-and-Tech-Debt-UX.md
  - >-
    .backlog/docs/specs/doc-96 -
    Spec-Agent-Experience-AX-—-agent-os-Onboarding-and-Skills.md
  - >-
    .backlog/decisions/decision-18 -
    ADR-0018-The-storage-engine-is-replaceable-PostgreSQL-must-be-possible.md
  - >-
    .backlog/decisions/decision-31 -
    ADR-0031-Any-employee-changes-tasks-a-private-note-is-read-by-its-author-and-by-people.md
priority: medium
type: feature
ordinal: 125000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Extends TASK-91's SearchRead port with Tier B Hybrid Search (FTS5 BM25 + dense vector embeddings fused via Reciprocal Rank Fusion k=60) for high-recall single-shot human Cmd+K conceptual queries, large 5k+ org knowledge bases, and semantic duplicate/similar work detection (FC-125). Strictly honors doc-7 offline-first & zero-hidden-downloads rules (opt-in EmbeddingProvider adapters, zero synchronous write-path latency impact via async org-store consumer watermarks) and strict 3-way security partitioning.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Defines consumer-owned EmbeddingProvider port (doc-8) with DisabledEmbeddingProvider (default fallback -> pure Tier A FTS5), opt-in LocalEmbeddingProvider (checksum-verified local quantized model installed explicitly, never auto-downloaded), and opt-in RemoteEmbeddingProvider (reusing TASK-54 provider connections & SSRF validation, degrading cleanly to FTS5 when offline).
- [ ] #2 Implements async org-store watermark-driven EmbeddingIndexer (zero write-path latency regression; Delete+Rebuild brings FTS5 online in <2s while vectors backfill lazily by (model_id, chunk_sha256)) with domain-aware chunking: heading-aware Markdown chunks with breadcrumb prefixes for Docs/Processes/ADRs, title+body+checklist chunks for Tasks, and 3-5 message thread windows for Chat.
- [ ] #3 Stores disposable quantized INT8 embeddings (with pure-Rust SIMD scan or pgvector on Postgres per ADR-0018) strictly partitioned across 3 isolated runtime sidecars: runtime/<office-id>/search/shared-vec.db, protected/messaging-vec.db (0600), and HQ-only private/task-notes-vec.db (0600), enforcing identical pre-fusion authorization filters (ADR-0031, doc-86, doc-90).
- [ ] #4 Fuses lexical BM25 and vector top-K inside SearchRead using Reciprocal Rank Fusion (k=60), exposes index_mode ('hybrid' | 'lexical_fallback' | 'lexical') on search.query and /api/v1/organizations/{org}/search, powers semantic 'similar' work lookup (FC-125), and passes recall, privacy-isolation, offline-fallback, and rebuild tests.
<!-- AC:END -->
