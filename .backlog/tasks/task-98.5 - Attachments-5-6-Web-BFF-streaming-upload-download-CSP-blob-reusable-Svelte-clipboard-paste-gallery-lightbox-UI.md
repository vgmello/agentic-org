---
id: TASK-98.5
title: >-
  Attachments 5/6: Web BFF streaming upload/download, CSP (blob:) & reusable
  Svelte clipboard-paste/gallery/lightbox UI
status: To Do
assignee: []
created_date: '2026-11-24T01:57:00Z'
labels:
  - frontend
  - ui
  - backend
  - rust
  - web
  - attachments
  - ux
  - security
  - docs
milestone: m-0
dependencies:
  - TASK-98.2
  - TASK-98.4
documentation:
  - .backlog/docs/design/doc-71 - Design-Web-HTTP-API-v1.md
  - .backlog/docs/specs/doc-39 - Spec-App-Shell-and-Navigation-UX.md
  - .backlog/docs/specs/doc-40 - Spec-Chat-Channels-and-Structured-Asks-UX.md
  - .backlog/docs/specs/doc-42 - Spec-Feature-Requests-and-Intake-UX.md
  - .backlog/docs/specs/doc-45 - Spec-Hiring-Profiles-and-Team-Moves-UX.md
  - .backlog/docs/specs/doc-49 - Spec-Work-Items-and-Tasks-UX.md
  - >-
    .backlog/docs/specs/doc-55 -
    REQ-04-Channel-Messaging-and-Question-Components.md
parent_task_id: TASK-98
priority: high
type: feature
ordinal: 111000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Delivers streaming raw-binary image upload/download in apps/agentic-org-web (without weakening CSRF or session-proof security) and reusable Svelte 5 clipboard-paste, drag-drop, file-picker, preview-strip, inline gallery, and fullscreen Lightbox primitives across every v1 UI surface.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Adds POST /api/v1/organizations/{org}/attachments (path-scoped in ingress.rs:42-62 for exact image/{png,jpeg,webp,gif}, keeping multipart/form-data & SVG 100% blocked; route-level streaming bypass of 1 MiB read_business_body in service.rs:380 under AsyncDispatch::Async { streaming: true } with 10 MiB / 30s cap and bounded concurrency streaming 512 KiB chunks to office IPC) and GET /api/v1/organizations/{org}/attachments/{id} (WebReply::Stream enforcing mandatory cookie + x-org-session-proof auth, nosniff, CSP: default-src 'none'; sandbox, CORP: same-origin, Cache-Control: private, no-cache, ETag/304).
- [ ] #2 Updates CSP img-src 'self' -> img-src 'self' blob: in repo-root config/csp/html.txt, apps/agentic-org-web/src/csp.rs:90, and apps/agentic-org-web/src/static_ui.rs:310 (keeping data: and remote https: blocked) and passes CSP/gateway tests.
- [ ] #3 Builds shared Svelte 5 primitives (apps/agentic-org-ui/src/lib/components/attachments/{AttachmentComposer,AttachmentGallery,ImageLightbox,model.ts} + authenticated fetch->blob: LRU <AttachmentImage> with revokeObjectURL lifecycle, Heroicons only): instant <100ms clipboard paste (⌘V/Ctrl+V) & drag-drop & file picker, pre-upload preview strip with 44px mobile tiles, optional Alt popover, image-only send, vision-capability honesty notice, aspect-ratio clamped single/2-4+ inline grids, and Bits UI Dialog Lightbox (Fit/100% zoom, Dark/Light canvas toggle for transparent PNGs, ←/→/Esc, SHELL-28 Back close, Download, metadata).
- [ ] #4 Wires primitives across all v1 surfaces: Chat/Threads/Pins/has:image (replacing Composer.svelte filename stub and retaining staged attachments in DraftStore), AskCard/QuestionForm/HandoffCard (inline display + paste/attach on Reply/Reject/free_text/Hand-back), InboxItemCard/InboxListRow, Work Items (CaptureDialog Task/Bug/Request, WorkDetailPage, BugSection Fix/Verify, RequestSection Ask-for-changes, DiscussionSection shared comments), ActivityRowItem/ActivityThreadPanel, and gentle redirect note on PrivateNotesPane paste.
- [ ] #5 Updates doc-39 (SHELL-28 Lightbox), doc-40, doc-42 (replace RQS-18 & §7 deferral), doc-45, doc-49 (replace §4 L151 & §16 L415 deferral), doc-55, doc-71, and doc-3 (FC-077); type check, unit tests, build, and Playwright browser checks (paste/upload/lightbox/mobile 44px/CSP) pass.
<!-- AC:END -->
