# Standalone v1 review decisions and verification map

Status: **documentation fixes and explicit release decisions**, 2026-11-10.
Responds to the owner's review of PR #5 at `c5e1a84`. This amendment specifies
required behavior; it does not implement or demonstrate the runtime/security
guarantees. Read [standalone-server.md](standalone-server.md),
[project-structure.md](project-structure.md), and
[server acceptance](../specs/server-installation-and-access-ux.md#6-acceptance-criteria).

## Numbered findings

| Finding | Decision / correction | Implementation evidence |
| --- | --- | --- |
| 1. CA escrow/recovery code | V1 organization creation provisions versioned encrypted CA escrow and protected owner-code delivery, with off-HQ retention acknowledgement. Encrypted escrow is copied; the code/usable CA key are not. REC-1 below explicitly gates the remaining manual recovery protocol. | SRV-25; dogfood 1.7; wrong-code, damaged-package and interrupted-delivery fixtures |
| 2. Cross-port cookies | Every protected read, mutation, stream, refresh and logout requires the instance/origin-bound cookie plus an independent proof held in origin-scoped browser storage. Cookie-only proof recovery is forbidden; SameSite is not port isolation. | SRV-23; dogfood 2.6; test a hostile same-host port and overwritten cookies |
| 3. HTTP process and create authority | A planned org-gateway child terminates one instance browser listener for multiple HQs. The host keeps keys/catalog/private launch; per-office org-server workers keep office behavior. Create organization requires instance-admin authority. | SRV-24; dogfood 1.6; gateway key-access and owner-only creation denial |
| 4. Offline Pause | HQ commits durable paused organization status even when the satellite is disconnected. Stop acknowledgement remains pending, and reconnect applies current versions before new work. HQ outage permits local lifecycle safety shutdown, without forging HQ status. | SRV-28; dogfood 4.8; later Resume supersedes stale stop work |
| 5. Managed homes | Retire home/clear_home mutation inputs under local API_VERSION 2; reject caller overrides. Launch from managed instance/office/member IDs and display the same resolved path read-only. | SRV-29, PROF-4; forged input/traversal/symlink fixtures |
| 6. Add agent record | Canonical UI label for the existing Add an existing agent HR proposal. Approval precedes member, home and messaging provisioning, including direct API calls. Initial built-in HR/owner bootstrap is the explicit exception. | SRV-30, STAFF-4, HRL-3; pre-approval direct-call rejection |
| 7. Protocol compatibility/history | Retained versions stay additive-only. First standalone local org-proto advances 1 to 2 with coordinated consumers; HTTP/launch/sync versions are separate. ADR-0001 retains the superseded decisions struck through. | SRV-32; negotiation and retired-caller fixtures |
| 8. Process crate ownership | Keep the existing pure org-process/org-docs plan. org-office::processes owns persistence/index/approval/I/O integration. Show org-git as planned v1 and org-github as deferred v2. | processes.md and project structure agree; source graph review |
| 9. Enrollment language/headless flow | Use On another machine/instance, Copy invitation and New invitation consistently. Verify codes on the destination terminal or through a trusted authenticated session to it. | OFC-3/4; headless acceptance and existing three-attempt cancellation |
| 10. Live spool/context | Preserve active Phase A asks, replies, deadlines and triage/read state before switching that team's transport. Fresh organizations skip import. Person-started CLI context is explicit; managed binding cannot fall back to spool. | SRV-31; interrupted/repeated cutover fixtures; AGENTS.md instructions |
| 11. Browser connection budget | One coordinated streaming fetch per browser origin across tabs, with auth headers; use bounded short polling when coordination is unavailable. Do not rely on HTTP/2 or per-tab EventSource. | SRV-27; dogfood 2.7 with eight tabs, HTTP/1.1, leader/session change |
| 12. Offline goals | Reject before admission when the owning office is known offline, without changing the goal or queuing it. A forwarded unknown outcome remains Pending confirmation using the same operation ID. | EMP-3, SRV-28; dogfood 4.9; retry/conflict fixtures |
| 13. Key-custody acceptance | Make fail-closed missing/wrong/locked keys and no plaintext dev fallback explicit acceptance. Restore the legitimate test credential without generating a replacement over existing data. | SRV-26; corrected dogfood 2.5/5.3 mappings and gateway/CLI key-denial tests |

## Minor findings

| Finding | Correction |
| --- | --- |
| Stale section names | Update references to The invitation and connection / Processes in one instance. |
| Setup/Tauri name collision | New setup Cargo package is org-setup, binary agentic-org. Remove the old Tauri workspace member before building/staging that same-named binary; verify artifact identity. |
| Governance wording | Restore Ce > 10 MUST trigger a design review. The existing repository review/merge governance remains in force. |
| Native desktop leftovers | Native tray/menu indicators are explicitly v2; current browser needs-you and request capture are in-page. Remove the native shortcut from the active capture mock-up. |
| Connection-loss copy | Follow LIVE-8 exactly: `Live updates paused. Showing what was loaded at <HH:MM>.`, or `Live updates paused.` if nothing loaded, with Retry / Retrying…. |
| Chat heading scope | Search/history text stays directly under §4, before §4.1 Finding the right teammate. |

## REC-1: open production-recovery release decision

CA escrow is mandatory v1 provisioning, not evidence of a completed HQ-loss
recovery implementation. Before claiming production recovery support, review and
exercise manual replacement/re-certification, highest-epoch handling and fencing
of a returning old HQ, recovery of required credentials, data-loss disclosure,
and reachable addresses. Device-bound office DB keys are separate from CA escrow.
Automatic promotion stays deferred; replication is not a point-in-time backup.

The reviewed procedure and disaster-recovery evidence remain open implementation
release work. This records the decision explicitly so no implementer treats remote
satellite enrollment alone as proof that HQ loss is recoverable.
