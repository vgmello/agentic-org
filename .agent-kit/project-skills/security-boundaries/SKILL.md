---
name: security-boundaries
description: Implement or review Agentic Org credential, protocol, persistence and coordination changes against existing authority and recovery contracts.
---

Follow docs/design/documentation-authority.md and the owning organization-data document. ADR-0002 governs one office writer; ADR-0005 retains host key custody and distinct human/service/agent identities. Inspect affected authorization/IPC/store tests. Gateway/agent processes never receive raw office keys.

Wire/runtime contract changes require inspecting affected consumers and updating trait implementations together under the solo lead's scope and local review. Preserve signing/key namespaces and versioned codecs. Test unauthorized principals, replay/deduplication, rollback/partial failure, reconnect and cleanup where affected. Executable renames do not authorize credential/data migration.

Read selected data design and current implementation separately. Draft SQL is not deployed. Preserve the contract's recoverable acknowledgement, receipts, projection, outbox and sequence boundary. Private notes/keys/logs do not belong in shared organization Git.

Preserve installed org-agent and pending Phase A asks/replies/triage under ADR-0003. Use disposable test roots. Run established CI plus focused negative/failure tests for changed behavior; no new blanket security gate is implied. Return unclear contract authority or conflicting security requirements to the local lead; offline developer agreement is not a prerequisite.

For protected recovery delivery, read active ADR-0011 REC-DEL-1 and retained ADR-0007 before implementation/review. The host macOS SDK bridge uses Objective-C ARC/blocks with a fixed bounded C ABI: fresh LAContext, no reuse, single terminal result, lifetime-safe late callbacks, cancellation/deadline and no global locks during OS UI. Verify actual kernel peer/audit-session/current-console/foreground-TTY context; environment strings or PTY allocation alone never prove presence. Test production and cfg(test) adapters separately. Generated debug/SDK-query fixtures never establish signed native physical authentication. Preserve same-connection live-authority ACK permission and durable acknowledgement/deletion reconciliation. The worker public-record transport cannot serialize local CA/private key state. Native bridge/build inputs stay exact, confined and source-pinned; no generated Rust source or arbitrary helper allowance.
