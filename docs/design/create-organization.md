# Design: create an organization through standalone HQ

Status: **selected target flow; implementation pending**, revision 2, 2026-11-15.
Owner: lead. [Organization data](organization-data/01-organization-structure.md)
owns the records below; [ADR-0005](../adr/0005-standalone-server-v1.md) and
[installation UX](../specs/server-installation-and-access-ux.md) own installation.
The current `agentic-org setup` refuses before provisioning: protected bootstrap,
service registration and production browser handoff remain unavailable. The
published name-only create contract is existing evidence, not this complete target.

## Setup and ownership

Initial setup selects `default` or a named instance, establishes an authenticated
instance administrator, creates HQ only and registers/starts its service. Satellite
enrollment is a separate HQ-issued flow. Opening HQ in a browser does neither.
Organization ownership alone never authorizes creating an organization.

The owner account remains a host security principal. An optional person profile
representing that owner must have a valid home team like every other employee;
`is_owner` neither grants login nor creates a teamless profile. The built-in HR
bootstrap exception permits approved system provisioning, not invalid employees.
HR requires a selected valid home team before its profile is committed. No team
name, lead appointment or reporting line is inferred from the word HR.

## Create flow

1. An authenticated instance administrator chooses **Create organization**. Show
   the destination instance, organization name and explicit organization slug,
   remote organization repository registration and `state_branch`. Slugs are
   lowercase kebab-case, unique in the organization catalog, and are not silently
   recalculated after a rename. No browser storage-path picker appears. The
   organization repository must have `kind: remote` and a syntactically valid
   `remote_url`; local-only code repositories are allowed separately, never as
   organization Git. Offline setup validates the registration URL, state-branch
   syntax and local credential references/configuration only. It performs no
   fetch, remote authentication probe or other external request. Persist the remote
   registration and locally authored state with connectivity/publication readiness
   explicitly unverified or pending until online operation is authorized.
2. Collect the initial team choice needed by any bootstrap employees. Either
   select an existing valid team in a resumed plan or create a root team at HQ
   within that plan. Every employee has one required `home_team_id`, optional
   `role_id` and `reports_to_employee_id`, and an organization-unique handle.
   A lead must be a direct member and manages the subtree. A plan with a missing
   HR/owner-person home team cannot claim bootstrap complete. The exact initial
   team content remains a product choice; it is not selected by this document.
3. Submit one operation ID bound to actor, instance and the validated plan.
   `agentic-org-web` authenticates and authorizes before invoking a typed restricted
   host operation. The host verifies the web process capability and administrator
   grant. No browser HTTP parser or raw-key access lives in the host operation.
   Reserve the name, slug, IDs and plan under the catalog lock, then provision
   outside it. New entity IDs use UUIDv7; retained IDs and current ULID operation
   IDs keep their existing codecs. Numbered teams use durable HQ allocation.
4. Register the remote organization repository, persist `organization.yaml`
   (`id`, `slug`, `name`, `organization_repository_id`, `state_branch`, format and
   concurrency revisions and provenance), `repositories.yaml`, HQ's office record,
   initial team and any bootstrap profiles at their stable-ID paths. Employee
   identity stays in `profile.md`; runnable HR additionally requires a separate
   `execution.md`, selected model profile and fresh local credential/model evidence
   under [execution](organization-data/11-model-profiles-and-agent-execution.md).
   Creating its identity alone must show a specific not-ready state, not launch.
   Offline setup does not probe provider authentication/model entitlement; local
   configuration can be saved while execution readiness remains unverified.
5. The host creates and retains the office encryption key and starts
   `agentic-org-office` using its private launch contract. The worker owns its
   stored `office.json` marker. Retain adopt-versus-discard and quarantine rules:
   never discard a committed office after a lost response, nor invent replacement
   keys. The current launch-v2 first line cannot carry the full target plan; its
   versioned adaptation and the cross-store durable commit/recovery boundary must
   be implemented before this flow is enabled. Do not add guessed wire fields.
6. Provision versioned encrypted CA escrow and the same owner recovery code on
   retry. Protected delivery may remain Waiting or Shown, not confirmed while HQ
   becomes usable. Require off-HQ retention acknowledgement only before reporting
   recovery material collected; never send the code through the web process.
   [REC-1](standalone-server.md#ca-escrow-and-the-explicit-recovery-gate) still gates
   the supported manual HQ-loss recovery procedure.
7. Return the durable organization/office identity and separate server, runtime
   and recovery readiness, plus remote repository connectivity/publication readiness.
   Offline HQ service readiness must not imply that the configured remote was
   contacted or that any Git publication succeeded. A saved office may be
   Starting or unavailable.
   Persist accepted plan, required sources and result evidence before acknowledging;
   publish Git separately. Resolve a lost response using the same actor-bound
   operation and fingerprint, rather than creating another organization.

## Reads and failures

List only organizations the principal can access. The selected organization is a
UI preference, never a credential. Unauthorized and absent IDs reveal the same
metadata. Distinguish invalid/duplicate name or slug, missing remote repository,
invalid state branch or local credential configuration, invalid bootstrap team,
key-store failure, worker failure and temporary startup. When online operation is
authorized, report remote authentication/connectivity/publication failures in that
separate readiness state; do not infer them from offline validation or discard an
already-created HQ because a later publication fails. Preserve entered safe
fields and show the failing step; never log credentials or recovery material.

The currently published [HTTP v1 contract](web-api-v1.md) accepts `{name}` and
returns its existing view. It must stay exact until a reviewed versioned adapter
covers the additional target fields and validates all required references. Do not
silently insert them into revision 2 DTOs or advertise target readiness through
that old result. Local service repair must remain available when HTTP is down.

## Verification

Use disposable roots. Verify remote organization Git rejection of local-only
registration, explicit slug/branch round-trip, valid HR and optional owner-person
home teams, direct-member lead rules and missing execution configuration readiness.
Exercise lost/repeated create replies, fingerprint/actor conflicts, concurrent
name/slug reservations, interruption around each durable source and office marker,
key failures, online remote-auth failures, quarantine, owner-only create refusal,
two HQs on one listener, interrupted escrow delivery and stored-create/failed-worker
separation. Instrument offline setup to prove zero external requests, unchanged
remote registration and pending connectivity/publication status; later authorized
online verification must update that status without creating a second organization.
Prove acknowledged records and receipts survive projection loss; Git publication
alone cannot establish that boundary. These are required implementation checks,
not test results from this documentation revision.
