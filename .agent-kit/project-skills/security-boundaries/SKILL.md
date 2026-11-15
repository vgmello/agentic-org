---
name: security-boundaries
description: Implement or review Agentic Org credential, protocol, persistence and coordination changes against existing authority and recovery contracts.
---

Follow docs/design/documentation-authority.md and the owning organization-data document. ADR-0002 governs one office writer; ADR-0005 retains host key custody and distinct human/service/agent identities. Inspect affected authorization/IPC/store tests. Gateway/agent processes never receive raw office keys.

Wire/runtime contracts require ada and affected trait-implementer agreement. Preserve signing/key namespaces and versioned codecs. Test unauthorized principals, replay/deduplication, rollback/partial failure, reconnect and cleanup where affected. Executable renames do not authorize credential/data migration.

Read selected data design and current implementation separately. Draft SQL is not deployed. Preserve the contract's recoverable acknowledgement, receipts, projection, outbox and sequence boundary. Private notes/keys/logs do not belong in shared organization Git.

Preserve installed org-agent and pending Phase A asks/replies/triage under ADR-0003. Use disposable test roots. Run established CI plus focused negative/failure tests for changed behavior; no new blanket security gate is implied. Escalate missing owner agreement or unclear authority to the lead.
