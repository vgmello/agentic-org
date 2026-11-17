# Design: create an organization

[Organization structure](organization-data/01-organization-structure.md) owns the
records created here. The [standalone design](standalone-server.md) and the
[installation UX](../specs/server-installation-and-access-ux.md) own installation.
The request is the [create payload](web-api-v1.md#selected-create-organization-payload)
of the web API.

## Setup and ownership

Initial setup selects `default` or a named instance, establishes an authenticated
instance administrator, creates HQ only, and registers and starts its service.
Satellite enrollment is a separate flow started from HQ. Opening HQ in a browser
does neither. Owning an organization never authorizes creating another one; only an
instance administrator may create an organization.

The owner account is a host security principal. An optional person profile for the
owner has a valid home team like every other employee. No employee field grants
sign-in or ownership, and no employee is teamless. The built-in HR agent is an
employee: it needs a chosen, valid home team before its profile is committed. No
team name, lead appointment or reporting line is inferred from the word HR.

## Create flow

1. An authenticated instance administrator chooses **Create organization**. The
   form shows the destination instance and asks for the organization name, an
   explicit slug, the remote organization repository and its `state_branch`. Slugs
   are lowercase kebab-case, unique in the organization catalog, and never
   recalculated after a rename. There is no storage-path picker. The organization
   repository has `kind: remote` and a syntactically valid `remote_url`; local-only
   code repositories are registered separately and never hold the organization's
   files. Offline, validation checks the URL, branch syntax and local credential
   configuration only. It makes no fetch, authentication probe or other external
   request, and saves the registration with connectivity and publication marked
   unverified until online operation is authorized.
2. When bootstrap employees are planned, the form collects their team: an existing
   valid team in a resumed plan, or a root team created at HQ within the plan.
   Every employee has one required `home_team_id`, optional `role_id` and
   `reports_to_employee_id`, and an organization-unique handle. A lead is a direct
   member and manages the subtree. A plan whose HR or owner-person profile lacks a
   home team cannot complete. The initial team's content is a product choice that
   this design leaves open.
3. The request carries one operation ID bound to the actor, instance and validated
   plan. `agentic-org-web` authenticates and authorizes it, then calls a typed,
   restricted host operation. The host verifies the web capability and the
   administrator grant; it contains no HTTP parsing and never hands out a key. The
   host reserves the name, slug, IDs and plan under the catalog lock, then
   provisions outside it. Every new ID is a UUIDv7. HQ allocates team numbers.
4. The office registers the remote organization repository and writes
   `organization.yaml` (`id`, `slug`, `name`, `organization_repository_id`,
   `state_branch`, format and concurrency revisions, provenance),
   `repositories.yaml`, HQ's office record, the initial team and any bootstrap
   profiles at their stable-ID paths. Creating the organization is the first
   accepted transaction of its journal. Employee identity lives in `profile.md`. A
   runnable HR agent also needs `execution.md`, a selected model profile and local
   credential and model evidence under
   [execution](organization-data/11-model-profiles-and-agent-execution.md); without
   them it shows a specific not-ready state and is not launched. Offline setup does
   not probe provider authentication or model access.
5. The host creates and keeps the office secrets key
   ([ADR-0016](../adr/0016-no-application-level-data-encryption.md)) and starts
   `agentic-org-office` through the verified rendezvous of the
   [host boundary](office-host-boundary.md). The worker owns its `office.json`
   marker. A committed office is never discarded after a lost response, and no
   replacement key is invented.
6. The office provisions the encrypted CA escrow and one owner recovery code, the
   same code on every retry ([ADR-0007](../adr/0007-hq-creation-escrow.md)).
   Protected delivery may stay Waiting or Shown while HQ becomes usable. Off-HQ
   retention is acknowledged before recovery material is reported collected, and
   the code never passes through the web process
   ([ADR-0011](../adr/0011-protected-recovery-delivery.md)).
   [REC-1](standalone-server.md#ca-escrow-and-the-explicit-recovery-gate) gates the
   supported manual HQ-loss recovery procedure.
7. The result is the durable organization and office identity, with server,
   runtime and recovery readiness reported separately from remote repository
   connectivity and publication. Offline readiness never implies that the remote
   was contacted or that anything was published. A saved office may be starting or
   unavailable. The accepted plan, required sources and result are durable before
   the acknowledgement; Git publication follows separately. A lost response is
   resolved with the same actor-bound operation and fingerprint, never by creating
   another organization.

## Reads and failures

List only organizations the principal can access. The selected organization is a UI
preference, never a credential. Unauthorized and absent IDs reveal the same
metadata. Distinguish an invalid or duplicate name or slug, a missing remote
repository, an invalid state branch or local credential configuration, an invalid
bootstrap team, a key-store failure, a worker failure and temporary startup. Once
online operation is authorized, remote authentication, connectivity and publication
failures appear in their own readiness state; they never cause an already-created HQ
to be discarded. Keep the safe fields the person entered and show the failing step.
Never log credentials or recovery material. Local service repair works when HTTP is
down.

## Verification

Use temporary roots. Verify: a local-only registration is refused as organization
Git; slug and branch round-trip; HR and owner-person home teams are valid; lead rules
hold; a missing execution configuration shows not-ready. Exercise lost and repeated
create replies, fingerprint and actor conflicts, concurrent name and slug
reservations, interruption around each durable source and the office marker, key
failures, online remote authentication failures, owner-only create refusal, two HQs
on one listener, interrupted escrow delivery, and a stored create whose worker fails.
Instrument offline setup to prove zero external requests and pending connectivity
status, and prove that a later online check updates that status without creating a
second organization. Prove acknowledged records survive the loss of the projection.

## What the code does today

`agentic-org setup` refuses before provisioning. The host's `organization.create`
accepts a name only (FC-018). `crates/org-host/src/provisioning.rs` and the office's
`hq_creation` implement protected HQ creation with escrow and are covered by tests,
but no command reaches them. The UI's create flow runs against fakes
(`apps/agentic-org-ui/src/lib/create-flow.ts`).
