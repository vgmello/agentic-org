# Design: create an organization

[Organization structure](organization-data/01-organization-structure.md) owns the
records created here. The [standalone design](standalone-server.md) and the
[installation UX](../specs/server-installation-and-access-ux.md) own installation.
The request is the [create payload](web-api-v1.md#selected-create-organization-payload)
of the web API. The screens are
[offices §5a](../specs/offices-and-teams-ux.md#5a-create-organization-administrator-only).

## Setup and ownership

Setup selects `default` or a named instance, registers and starts its service and
prints a one-time registration link for the instance's administrator. It creates no
account, no organization and no office. The person who ran setup opens the link,
chooses a username and a password, and is signed in as the administrator: the one
account on the instance with the administrator flag. With no organization yet, the
application offers **Create organization**. Satellite enrollment is a separate flow
started from HQ.

Only the administrator creates an organization, and only in the browser. Whoever
creates an organization is its owner. Every account is tied to a person in each
organization it can open: creating the organization makes the creator a person in
it, and from then on what the account does there carries that person's name. There
is no nameless owner actor. Owning an organization never authorizes creating
another one.

Every employee has exactly one home team, so creation makes one top-level team,
named after the organization, without asking. The creator's person is its lead. The
built-in HR lead is created as an agent employee in the same team; it is part of the
product, is not kept running, and runs only when called. It is the only agent that
is not hired: every other agent comes from a hire by someone who may hire
([our organization in the app](../specs/our-org-in-the-app.md) §7). No team name,
lead appointment or reporting line is inferred from the word HR, and further teams
come later, a project's by default with the project.

## Create flow

1. The signed-in administrator chooses **Create organization**. The form shows the
   destination instance and asks for the organization name, an explicit slug, the
   remote organization repository and its `state_branch`, and nothing else. Slugs
   are lowercase kebab-case, unique in the organization catalog, and never
   recalculated after a rename. There is no storage-path picker. The organization
   repository has `kind: remote` and a syntactically valid `remote_url`; local-only
   code repositories are registered separately and never hold the organization's
   files. Offline, validation checks the URL, branch syntax and local credential
   configuration only. It makes no fetch, authentication probe or other external
   request, and saves the registration with connectivity and publication marked
   unverified until online operation is authorized.
2. The request carries one operation ID bound to the actor, instance and validated
   plan. `agentic-org-web` authenticates and authorizes it, then calls a typed,
   restricted host operation. The host verifies the web capability and the
   administrator flag; it contains no HTTP parsing and never hands out a key. The
   host reserves the name, slug, IDs and plan under the catalog lock, then
   provisions outside it. Every new ID is a UUIDv7. HQ allocates team numbers.
3. The office registers the remote organization repository and writes
   `organization.yaml` (`id`, `slug`, `name`, `organization_repository_id`,
   `state_branch`, format and concurrency revisions, provenance),
   `repositories.yaml`, HQ's office record, the top-level team, the creator's person
   (named after the account's username until edited) and the HR lead's profile at
   their stable-ID paths. Creating the organization is the first accepted
   transaction of its journal. Employee identity lives in `profile.md`. A runnable
   HR agent also needs `execution.md`, a selected model profile and local credential
   and model evidence under
   [execution](organization-data/11-model-profiles-and-agent-execution.md); without
   them it shows a specific not-ready state and is not launched. Offline creation
   does not probe provider authentication or model access.
4. The host ties the creator's account to the creator's person and records it as the
   organization's owner.
5. The host creates and keeps the office secrets key
   ([ADR-0016](../adr/0016-no-application-level-data-encryption.md)) and starts
   `agentic-org-office` through the verified rendezvous of the
   [host boundary](office-host-boundary.md). The worker owns its `office.json`
   marker. A committed office is never discarded after a lost response, and no
   replacement key is invented.
6. The office provisions the encrypted CA escrow and one owner recovery code, the
   same code on every retry ([ADR-0007](../adr/0007-hq-creation-escrow.md)). The
   code then waits to be collected on the HQ machine with
   `agentic-org recovery release --org '<organization>'`; the browser shows only its
   state. Off-HQ retention is acknowledged before recovery material is reported
   collected, and the code never passes through the web process
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
repository, an invalid state branch or local credential configuration, a key-store
failure, a worker failure and temporary startup. Once online operation is
authorized, remote authentication, connectivity and publication failures appear in
their own readiness state; they never cause an already-created HQ to be discarded.
Keep the safe fields the person entered and show the failing step. Never log
credentials or recovery material. Local service repair works when HTTP is down.

## Verification

Use temporary roots. Verify: a local-only registration is refused as organization
Git; slug and branch round-trip; the top-level team exists with the creator's person
as its lead and the HR lead as a member; the creator's account is tied to that person
and owns the organization; lead rules hold; a missing execution configuration shows
not-ready. Exercise lost and repeated create replies, fingerprint and actor
conflicts, concurrent name and slug reservations, interruption around each durable
source and the office marker, key failures, online remote authentication failures,
refusal for an account that isn't the administrator, two HQs on one listener,
interrupted escrow delivery, and a stored create whose worker fails. Instrument
offline creation to prove zero external requests and pending connectivity status,
and prove that a later online check updates that status without creating a second
organization. Prove acknowledged records survive the loss of the projection.

## What the code does today

`agentic-org setup` creates the instance and no organization. One command creates an
organization: the host's `organization.create`, served only on the admitted web
connection and reached from `POST /api/v1/organizations`
([web API](web-api-v1.md#selected-create-organization-payload)). The name-only
command and `organization.provision.v1` are deleted.

- **Creation is asynchronous at the host.** Admission checks the administrator,
  reserves the name, the slug, the UUIDv7 IDs and the plan, and answers `accepted`.
  A host thread then drives the create to its end, with no client attached. At
  startup the host resumes every reserved create, with up to three tries (0.5 s and
  1 s apart) for a failure that may pass (`unavailable`, `storage_error`, a
  timeout); any other refusal is not retried, and a lookup of the operation tries
  once more. Stopping the host halts the creation threads: no attempt starts, no
  retry continues and no link is recorded after the stop began
  (`crates/org-host/src/provisioning.rs`, `crates/org-host/src/service/organizations.rs`).
- **Completion** means the office worker reported `created` and the host adopted
  the office. Only then is the operation `linked`, the organization listed, and
  `account.changed` pushed to the web process.
- **The creator is a person.** The office writes a person record whose UUIDv7 ID is
  the owner member the account is linked to, named after the account's login, in the
  top-level team named after the organization, and makes that person its lead. The
  host records the account-to-person link and the owner.
- **The HR lead** is written as an agent employee profile in the same team. The
  profile carries `status` only (no `agent_status`), and nothing can run it: it has
  no execution configuration.
- **The recovery code** is generated at admission into the host's credential
  custody and never reaches the browser or the web process. It is collected once on
  the HQ machine with `agentic-org recovery release --org '<organization>'`
  (`crates/org-host/src/human/recovery.rs`), which today signs the person in with a
  password typed at the terminal; that sign-in is interim
  ([FC-016](../FEATURES.md)).
- **Files.** Creation is the first entry of the HQ's acceptance journal on the office
  store (B1b; `CreateOrganizationOp` in `crates/org-office/src/store/open/operations.rs`).
  The host gives the office its organization's data roots,
  `<instance-root>/<org-id>/durable` and `<instance-root>/<org-id>/runtime`
  (`crates/org-host/src/writer.rs` `data_roots`), and the entry writes the
  organization's files under `<org-id>/durable/state/`, with the public escrow package
  at `state/security/ca-escrow.v1.bin` and the CA key record, wrapped by the office
  secrets key, at `<org-id>/durable/local/offices/<office-id>/security/ca-key.v1.json`.
  The office answers `created` only after those files are written
  (`org_office::Office::provision_hq`). Its projection,
  `<org-id>/runtime/<office-id>/office.db`, is rebuilt from them when deleted.
- **Not built.** No call cancels a create, so a reserved name and slug stay reserved
  until the create completes (FC-188). An organization created before B1b, on the old
  `org.db` and `office.db`, is refused at its office's start and not migrated; it is
  created again.

Real-process evidence: `crates/org-host/src/web_process_tests/organizations.rs`.
