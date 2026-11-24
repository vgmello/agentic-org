---
id: TASK-98.6
title: >-
  Attachments 6/6: Multi-office blob shipping to HQ & bounded satellite local
  image cache
status: To Do
assignee: []
created_date: '2026-11-24T01:57:00Z'
labels:
  - backend
  - rust
  - storage
  - federation
  - attachments
  - docs
milestone: m-0
dependencies:
  - TASK-98.1
  - TASK-52
documentation:
  - .backlog/docs/organization-data/doc-72 - Organization-Data-—-README.md
  - >-
    .backlog/docs/organization-data/doc-90 -
    Organization-Data-15-—-Messaging-and-Chat.md
parent_task_id: TASK-98
priority: high
type: feature
ordinal: 112000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Extends multi-office federation (TASK-52) so images created on any remote/satellite office work offline immediately, ship reliably to HQ's authoritative durable/blobs/sha256/ store on reconnect, and are served on remote offices through a bounded local LRU image cache with transparent on-demand pull from HQ — keeping binary blobs completely outside OrgSnapshotChunk's 64 MiB state snapshot cap.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Adds additive SyncFrame variants (BlobHasRequest/BlobHasResponse, BlobChunk <= 512 KiB within 1 MiB MAX_ENCODED_FRAME_BYTES, BlobAck, BlobFetchRequest) in crates/org-sync-proto/src/frame.rs with SHA-256 verification before atomic commit to disk.
- [ ] #2 Implements HQ authoritative custody (<durable>/blobs/sha256/<aa>/<bb>/<sha256>) and satellite durable outbox (<durable>/local/offices/<office-id>/blob-outbox/) so locally created image attachments work offline immediately and are NEVER evicted before HQ returns BlobAck.
- [ ] #3 Implements a bounded local LRU image cache on remote/satellite offices (<runtime>/<office-id>/blob-cache/sha256/<aa>/<bb>/<sha256>, configurable cap e.g. 1 GiB default): replicated/pulled blobs cache locally for instant browser loads and AI model staging, evicting oldest cached entries strictly after HQ custody is confirmed; cache misses transparently pull from HQ via BlobFetchRequest (with org/attachment authorization on HQ) or degrade cleanly (503 Retry-After / offline notice) when HQ is unreachable.
- [ ] #4 Verifies multi-office satellite->HQ upload, satellite-A->HQ->satellite-B on-demand cache pull, LRU eviction without losing un-acked local blobs, and OrgSnapshotChunk 64 MiB budget preservation in federation E2E tests; updates doc-72, doc-90, and ADR-0035.
<!-- AC:END -->
