---
kind: spec
title: "Dogfood run: fresh HQ, satellite and named instances"
status: for-review
revision: 6
owner: ac-product-designer-eden
created: "2026-11-08"
updated: "2026-11-10"
requested_by: owner
reviewers: [ac-staff-engineer-ada, owner]
builds_on: [server-installation-and-access-ux, our-org-in-the-app, offices-and-teams-ux, work-items-ux, agent-run-modes-ux]
---

# Spec · Dogfood run: fresh HQ, satellite and named instances

**Revision 6 (2026-11-10):** rows 1.1, 1.2, 1.5, 1.6, 1.7 and 2.7 also cite the
screen criteria that now define them: SIGN and RCV in
[server-installation-and-access-ux.md §4a](server-installation-and-access-ux.md#4a-sign-in-first-access-and-recovery-code-rev-4)
and OFC-14 to OFC-25 in [offices-and-teams-ux.md §5a](offices-and-teams-ux.md#5a-create-organization-instance-admin-only-rev-14).
No action or pass condition changed.

**Revision 4 (2026-11-09):** replaces the old desktop/team-import walkthrough.
The product has never been deployed. Start from fresh disposable instances, with
no old HR import or desktop migration. Fresh instances skip live Phase A cutover;
its preservation contract is tested separately with disposable fixtures under §6. Run the real HTTP application and
server; explicitly injected fakes support lower-level tests but do not satisfy
this acceptance run. Commands in the linked design are proposed until implemented.

## 0. Preconditions and evidence

Use the same verified release on two supported machines, one macOS and one Linux.
Run once with macOS HQ/Linux satellite and once with Linux HQ/macOS satellite.
Use new roots and credentials; never tests' access to a user's actual office home.
Record versions, OS/architecture, instance IDs, service configuration and supported
runtime. Choose at least one implemented harness for a separately authorized real
agent smoke test. Keep provider credentials out of test output.

Before running, the release must implement browser authentication, HQ APIs, office
enrollment/sync, instance lifecycle and production key custody on both platforms.
The inspected branch does not yet meet these prerequisites. Missing capabilities
are **blocked**, not passed through mocks or marked ready because assets loaded.
See [SRV criteria](server-installation-and-access-ux.md#6-acceptance-criteria).

For every row record pass/fail/blocked, timestamp, exact release and evidence.
Retain failure diagnostics with credentials redacted. A docs review is not evidence
that any of these checks ran.

## 1. Install and access HQ

| Step | Action | Pass condition | Requirements |
| --- | --- | --- | --- |
| 1.1 | Install with no instance name, configure HQ and owner access | One `default` instance, platform-private data, valid release payload, ready service; no Tauri or frontend build tools | SRV-1, SRV-2, SRV-7, SIGN-21, SIGN-22, RCV-13 |
| 1.2 | Open its URL from a browser on the other machine | Authenticate and see the real organization without installing/enrolling the browser machine | SRV-8, SRV-18, SIGN-23 |
| 1.3 | Repeat install/setup without a name | Same instance/office identity, root, endpoint and release pin; no duplicate process, enrollment or configuration reset | SRV-2, SRV-6 |
| 1.4 | Stop the instance, repeat setup, then repeat while running | Start once when stopped; while running return status/URL without restarting it | SRV-6, SRV-15 |
| 1.5 | Attempt unauthenticated and cross-origin mutation/stream access | Refuse without disclosing protected data; browser auth does not grant a local agent owner rights | SRV-7, SIGN-1, SIGN-4 |
| 1.6 | Create a second HQ organization through the same listener as an instance admin; repeat as an organization owner without that right | One gateway listener routes both HQs; owner-only creation refused; host keys never reach gateway/CLI | SRV-24, OFC-14, OFC-19, OFC-22 |
| 1.7 | Provision recovery material, interrupt protected delivery before acknowledgement, resume, and retain the encrypted package/code off HQ | Same CA/code on pending retry; no secret in status/logs; acknowledged delivery is not later redisplayed. Wrong-code/damaged-escrow fixture fails closed and correct code recovers the same CA identity | SRV-25, RCV-1, RCV-6, RCV-8, RCV-10 |

## 2. Named instances and setup failures

| Step | Action | Pass condition | Requirements |
| --- | --- | --- | --- |
| 2.1 | Set up `lab` on the HQ machine, with an available endpoint | Different instance ID, root, credentials, sockets and service name; both instances run | SRV-3 |
| 2.2 | Repeat `lab` setup concurrently; try another name pointing to the same data root/alias | One persisted identity and service; duplicate storage writer refused before database access | SRV-4, SRV-6 |
| 2.3 | Configure an occupied port | Actionable conflict; no silent endpoint change or corruption of another instance | SRV-4 |
| 2.4 | Interrupt setup before/after office provisioning markers, then repeat | Resume recorded operation; no duplicate office, lost key or destructive discard of committed data | SRV-6 |
| 2.5 | Run noninteractive setup with complete inputs, then missing input and unavailable secret provider | Same result as wizard when valid; predictable failures without prompts, secrets or dev-key fallback | SRV-5, SRV-26 |
| 2.6 | Use default/lab and a hostile dev server on the same host; copy/overwrite cookies and try read, mutation, stream, refresh/proof recovery and logout | No cross-origin/instance authority without the matching proof; proof never enters URL/log/redirect; valid paired sessions still work | SRV-23 |
| 2.7 | Open eight HQ tabs under HTTP/1.1, issue commands, close the leader and change login session; repeat without worker/lock coordination | At most one shared stream per origin, command capacity retained, cursor-safe leadership change; fallback uses bounded short polling | SRV-27, SIGN-14, SIGN-20 |

## 3. Enroll and execute at a satellite

| Step | Action | Pass condition | Requirements |
| --- | --- | --- | --- |
| 3.1 | Create invitation at HQ; run satellite setup on the other OS using protected input | CLI identifies destination/execution consent and reports awaiting approval; no public satellite app port | SRV-5, SRV-9 |
| 3.2 | Restart pending setup; approve its code in HQ's browser | Same request/key and one office; verified certificate, completed snapshot and ready status | SRV-6, SRV-9 |
| 3.3 | Repeat satellite setup/start | Reuse enrolled identity and stream positions; no second invitation or office | SRV-6, SRV-10 |
| 3.4 | Provision an employee through the real domain/hiring flow and place it at that office | Stable member ID, server-managed home/configuration, scoped `org-agent`, honest runtime readiness | SRV-13, SRV-14 |
| 3.5 | Run a bounded non-repository task and send an agent message locally | Owning office durably commits work, supervisor accounts for it, HQ browser receives real results | SRV-10, SRV-21 |
| 3.6 | Inspect the session through HQ and pause/resume through supported authorized controls | Routed to the correct office/employee; no second main session or uncontrolled descendants | SRV-11, SRV-17 |

Use a non-repository task for 3.5 until a supported cross-office repository transport
exists. V1's current local-bare-Git design does not make an HQ directory reachable
from another host. This run cannot be cited as validation of cross-office coding;
add a real checkout/commit/transfer run once that separate capability is supported.

## 4. Disconnect, retries and event recovery

| Step | Action | Pass condition | Requirements |
| --- | --- | --- | --- |
| 4.1 | Drop the satellite connection while local work is running | Local permitted work continues; HQ shows stale/offline status with last seen | SRV-10, SRV-11 |
| 4.2 | Send a cross-office message and request live control while disconnected | Durable message queues according to its contract; control is unavailable or explicitly queued, never falsely completed | SRV-11 |
| 4.3 | Reconnect, including a repeated work frame/ack | Missing records synchronize exactly once logically; ownership/version checks hold | SRV-10 |
| 4.4 | Drop the browser event connection; make changes; reconnect and test an expired cursor | Recover all changes through a cursor or snapshot, with no snapshot/event gap; stale state was visibly marked | SRV-12 |
| 4.5 | Lose the HTTP response after a committed mutation; resend the same operation ID | Return the recorded result, one logical change; altered payload with same ID is refused | SRV-12 |
| 4.6 | Slow the browser/event consumer and exceed queue limits | Bounded resource use; explicit reconnect/resync behavior; agents and durable writes continue | SRV-12, SRV-17 |
| 4.7 | Reject/revoke an office identity; test an incompatible protocol range | No unauthorized reconnect or half-understood apply; actionable status and preserved local data | SRV-9, SRV-10 |
| 4.8 | Disconnect satellite, Pause at HQ, restart/reconnect it; repeat with a later Resume before reconnect | HQ records paused status with stop pending; current version reconciles before work, office acknowledgement resolves stop, stale pause never overrides later Resume | SRV-28 |
| 4.9 | Change a goal while the office is known offline; then lose a forwarded reply and retry/lookup with the same ID | First is unavailable with no saved/queued goal; uncertain forwarded outcome stays pending, resolves once, and preserves expected-version conflicts | EMP-3, SRV-28 |

## 5. Lifecycle and removal

| Step | Action | Pass condition | Requirements |
| --- | --- | --- | --- |
| 5.1 | Close all browser windows | Services and supervised agent work continue | SRV-8 |
| 5.2 | Stop one instance during work, then restart | No new admission; acknowledged writes durable; owned children stopped/reaped; peer instance unaffected | SRV-15, SRV-17 |
| 5.3 | Lose a required credential and try startup against existing data | Specific fail-closed result; no fresh/replacement key, DB/identity change or dev-store fallback; restore the legitimate test credential and recover | SRV-15, SRV-26 |
| 5.4 | Upgrade only `lab` to a matching test release and inject a failed readiness check | Default remains on its pin; complete helper/assets set; documented recoverable state, no unsafe binary-only schema rollback | SRV-16 |
| 5.5 | Uninstall `lab` | Only its service removed; data retained; default and referenced release payloads remain usable | SRV-16 |
| 5.6 | Explicitly delete the disposable `lab` data | Exact selected stopped instance removed; default data/credentials untouched | SRV-16 |

## 6. Source and feature acceptance

Verify SRV-29/30 with forged home/clear_home inputs and direct pre-approval agent
creation: no external path or unapproved member/home/binding is provisioned. Confirm
the profile displays the launcher's actual managed path. Verify SRV-32 with mixed
local protocol versions and setup build/staging: unsupported versions fail before
dispatch and the staged agentic-org is the org-setup CLI, never the retired shell.

Use disposable Phase A fixtures for SRV-31: unresolved asks/replies, expiry, read/
defer/done state, interrupted import and repeated import. Preserve IDs and state,
require the selected instance/office context, and prove managed agents reject
spool/context fallback. Do not import, mutate or delete the team's actual live
spool in this acceptance run. REC-1 remains separately gated recovery work; row
1.7 verifies escrow material, not completed promotion/fencing.

Verify SRV-22: retained browser code/package uses `apps/org-ui` / `org-ui`, and
server code/Cargo package/worker executable uses `apps/org-server` / `org-server`.
Check workspace, imports, test helpers, release/signing resolution and ownership
references against the [project structure](../design/project-structure.md). Verify
domain/feature moves preserve private SQL, transactions, public boundaries and
licensed UI containment before treating the reorganization as complete.

Verify no Tauri build/runtime dependency, native capture/menu, old-team import or
production mock fallback remains. Exercise the retained roster, team, hiring,
task/message and process features against their real advertised HTTP capabilities.
Report unsupported features as gaps; do not substitute test fixtures for product
behavior. Native/global capture and old import criteria are retired by the updated
specs. Run repository formatting, relevant Rust tests/Clippy and frontend checks,
tests and build on the implementation, with the deletion ledger reviewed.

The run passes only with required rows complete on both platform assignments,
real scoped runtime execution, and no unresolved data-loss/authorization/duplicate
execution findings. Backup recovery, cross-office repository work and optional
runtime coverage require their own evidence; do not infer them from this smoke run.
