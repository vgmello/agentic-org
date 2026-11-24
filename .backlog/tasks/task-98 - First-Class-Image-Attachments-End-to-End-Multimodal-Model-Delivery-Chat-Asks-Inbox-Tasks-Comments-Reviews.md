---
id: TASK-98
title: >-
  First-Class Image Attachments & End-to-End Multimodal Model Delivery (Chat,
  Asks, Inbox, Tasks, Comments, Reviews)
status: To Do
assignee: []
created_date: '2026-11-24T01:57:00Z'
labels:
  - feature
  - multimodal
  - attachments
  - cli
  - ui
  - backend
  - rust
  - ax
  - security
  - docs
  - blog
milestone: m-0
dependencies:
  - TASK-97
documentation:
  - .backlog/docs/organization-data/doc-72 - Organization-Data-—-README.md
  - >-
    .backlog/docs/organization-data/doc-79 -
    Organization-Data-05-—-Operational-Data-and-Private-Notes.md
  - >-
    .backlog/docs/organization-data/doc-90 -
    Organization-Data-15-—-Messaging-and-Chat.md
  - >-
    .backlog/docs/organization-data/doc-88 -
    Organization-Data-11-—-Model-Profiles-and-Agent-Execution.md
  - .backlog/docs/design/doc-29 - Design-Agent-Sessions-and-Runtime-Isolation.md
  - .backlog/docs/design/doc-71 - Design-Web-HTTP-API-v1.md
  - .backlog/docs/specs/doc-40 - Spec-Chat-Channels-and-Structured-Asks-UX.md
  - .backlog/docs/specs/doc-49 - Spec-Work-Items-and-Tasks-UX.md
  - >-
    .backlog/docs/specs/doc-96 -
    Spec-Agent-Experience-AX-—-agent-os-Onboarding-and-Skills.md
priority: high
type: feature
ordinal: 106000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Delivers end-to-end v1 image attachments across the entire Agentic Org stack: people can paste screenshots from the clipboard, drag-and-drop, or pick images (PNG, JPEG, WebP, GIF <= 10 MiB; SVG banned; EXIF/GPS stripped) in Chat (DMs/channels/threads), Structured Asks & Ask Answers, Unified Inbox replies, Work Items (Tasks, Bugs, Feature Requests, Fix/Verify modals), Shared Task Comments, and Universal Reviews (TASK-92), with inline galleries and a fullscreen lightbox; images travel all the way into AI models (Claude base64 blocks, Codex localImage on turn/start + turn/steer, Agy safe staged read-only file delivery) with <= 1568px / <= 1 MiB model-delivery derivatives and untrusted-data fencing; agents can inspect and send images via agent-os CLI; raw image bytes live strictly on disk (zero SQLite BLOBs, never in Org Git) at HQ durable/blobs/sha256/ with HQ shipping and bounded LRU local image caches on remote satellite offices. Private task notes and Git-backed project docs media remain cleanly deferred to v2.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Disk-only content-addressed blob storage (zero SQLite BLOBs, never in Org Git, denied to agents via denied_office_globs), pure-Rust raster sanitization (PNG/JPEG/WebP/GIF <= 10 MiB, no SVG, EXIF/GPS stripped), chunked org-proto IPC (<= 512 KiB chunks under 1 MiB MAX_FRAME_BYTES), and dual-engine Memory/SQLite + Delete+Rebuild parity (TASK-98.1).
- [ ] #2 Chat messages, structured Asks, Ask Answers (both ParticipantAnswerFile and reply MessageRecord), Handoffs, and Unified Inbox items carry, display, and authorize image attachments (TASK-98.2).
- [ ] #3 Images travel end-to-end to AI models across claude, codex, and agy via Normalized Dual-Delivery (<= 1568px / <= 1 MiB derivatives, <= 4 inline/turn, ephemeral 0400 turn-scoped staging purged on turn end, zero base64 in durable wake/history JSON, untrusted-data + image prompt-injection framing, and failover/fail-soft handling) (TASK-98.3).
- [ ] #4 Tasks, bugs, feature requests, and shared comments persist/project image attachments and agents can send (--attach with two-layer exfiltration guard) and inspect/fetch authorized images via agent-os CLI within CATALOGUE_LIMIT = 4096 (TASK-98.4).
- [ ] #5 Web BFF streams <= 10 MiB raw raster uploads (keeping multipart/form-data & SVG blocked) and authenticated downloads with CSP img-src 'self' blob:, and the Svelte UI ships reusable clipboard-paste, drag-drop, file-picker, preview-strip, inline gallery, and fullscreen Lightbox across Chat, Asks/Handoffs, Inbox, Work Items, Comments, Activity, and ready for Reviews (TASK-98.5).
- [ ] #6 Multi-office federation ships local origin image blobs to HQ (authoritative durable/blobs/sha256/, never evicted until HQ BlobAck) and serves remote/replicated images on satellite offices via a bounded local LRU image cache with transparent on-demand pull from HQ (TASK-98.6).
<!-- AC:END -->
