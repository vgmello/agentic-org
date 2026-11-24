---
id: TASK-98.1
title: >-
  Attachments 1/6: Disk-only blob store, pure-Rust image sanitization,
  denied_office_globs, chunked org-proto RPCs & schema contract
status: To Do
assignee: []
created_date: '2026-11-24T01:57:00Z'
labels:
  - backend
  - rust
  - storage
  - attachments
  - security
  - data-contract
  - docs
milestone: m-0
dependencies:
  - TASK-97
documentation:
  - .backlog/docs/organization-data/doc-72 - Organization-Data-—-README.md
  - >-
    .backlog/docs/organization-data/doc-74 -
    Organization-Data-—-Schema-Coverage-and-Integrity-Review.md
  - >-
    .backlog/docs/organization-data/doc-79 -
    Organization-Data-05-—-Operational-Data-and-Private-Notes.md
  - >-
    .backlog/docs/organization-data/doc-82 -
    Organization-Data-08-—-Record-Contracts-and-Hash-Codec-Registry.md
  - >-
    .backlog/docs/organization-data/doc-90 -
    Organization-Data-15-—-Messaging-and-Chat.md
parent_task_id: TASK-98
priority: high
type: feature
ordinal: 107000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Builds the foundational disk-only, content-addressed raster image store (zero SQLite BLOBs, never in durable/state/ Org Git), pure-Rust sanitization and model-derivative pipeline, security glob hardening, chunked org-proto upload/download RPCs fitting inside MAX_FRAME_BYTES = 1 MiB, and dual-engine Memory/SQLite schema contracts.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Implements engine-independent BlobStore (FsBlobStore with private_fs full_sync atomic write-once + MemoryBlobStore) storing sanitized image bytes strictly on disk at <durable>/blobs/sha256/<aa>/<bb>/<sha256> (0700/0600, outside org-store Tree::ALL so Delete+Rebuild never scans blobs, zero SQLite BLOB columns) and unattached pre-send uploads at <durable>/local/offices/<office-id>/attachment-staging/ (excluded via TASK-97 companion_store_exclusions, 50 files / 200 MiB per-employee quota, 24h TTL GC).
- [ ] #2 Adds pure-Rust image crate (=0.25.x, default-features = false, png/jpeg/webp/gif, cargo deny check green) enforcing raster-only magic-byte + structural decode (image/png, image/jpeg, image/webp, image/gif; SVG explicitly banned), <= 10 MiB raw cap, <= 16,384 px / <= 64 MP decompression-bomb Limits / <= 200 frames, EXIF/GPS/IPTC/XMP metadata stripping with JPEG EXIF Orientation normalized before computing content_sha256, and deterministic cached model-delivery derivative generation (<= 1568px long edge, <= 1 MiB raw).
- [ ] #3 Hardens crates/org-office-sessions/src/manager.rs:1887-1913 denied_office_globs() to explicitly deny <durable>/blobs/** and <durable>/messaging/** so sandboxed agents can never read raw blobs or private messages from disk; enforces fail-closed ADR-0031/conversation/uploader authorization in AttachmentService::resolve_authorized_attachments and read_authorized_attachment_bytes before returning bytes or existence.
- [ ] #4 Adds 6 chunked org-proto RPCs (attachment.upload_begin, upload_chunk, upload_commit, upload_abort, inspect, read_chunk) transferring <= 512 KiB raw base64 chunks safely within MAX_FRAME_BYTES = 1 MiB, registered in operations.json and authority.rs.
- [ ] #5 Updates schema.sql (STRICT polymorphic attachments table for task, task_comment, review_finding, review_finding_reply + 3 indexes, zero new SQLite triggers), messaging-schema.sql (upgrade message_attachments with position/filename/width/height/alt_text/uploaded_by_employee_id/created_at/UNIQUE + add message_answer_attachments), doc-72, doc-74 (ENG-5), doc-79, doc-82, doc-90 (preserving exact 18 org-data docs), test_schema.py, dual-engine Memory/SQLite + Delete+Rebuild tests, engine_boundary.rs, and records ADR-0035.
<!-- AC:END -->
