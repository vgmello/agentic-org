# ADR-0001 · The app is a client; offices serve organization data

Status: accepted 2026-11-08; reconciled with the selected organization data model
2026-11-15. Owner: lead. [ADR-0005](0005-standalone-server-v1.md) owns standalone
deployment. The [authority map](../design/documentation-authority.md) separates
selected target contracts from implemented protocols and deferred engines.

## The model

- **An organization** owns a recursive team forest, employees, roles, projects,
  offices and repository registrations. Each employee has exactly one home team.
  A team lead is a direct member who manages that team's subtree; direct reporting
  lines remain a separate relationship. A security principal's owner authority is
  not derived from an employee record or its place in the chart.
- **A project** has one required accountable owner team and optional contributing
  teams. It may use several organization-registered repositories; each association
  selects its integration branch. A project is not a repository or an office.
- **A task** has a required accountable team and an optional project. A project
  task's accountable team is its project's owner. Current assigned team and
  individual assignee are independent nullable fields. Task, bug and request are
  types of the same task entity and use the same scoped T- sequence. Assigning
  work changes neither its reporter nor its stable identity or number.
- **An office** belongs to one organization. The organization has one HQ and may
  have satellites. Offices place agents and execute work; they do not own teams,
  projects or independent copies of numbering authority. People have employee
  profiles but no execution placement. An agent's office placement is separate
  from its home team and protected by durable execution generations.
- **The application** is a browser client of HQ over authenticated HTTP. Opening
  it neither installs an office nor enrolls a satellite. Instance setup, service
  lifetime and browser sessions remain separate.

The owning relationships and field contracts are in organization-data documents
[01](../design/organization-data/01-organization-structure.md),
[02](../design/organization-data/02-project-structure.md),
[03](../design/organization-data/03-features-and-work.md),
[04](../design/organization-data/04-agent-structure.md) and
[12](../design/organization-data/12-task-assignment-and-history.md).

## Decisions

1. **One office is served by one worker process.** `agentic-org serve` composes
   the host library and supervises `agentic-org-office` workers through the
   private launch contract. The instance's `agentic-org-web` process terminates
   browser HTTP separately from key custody. Setup/service registration and
   production web handoff remain pending as recorded in
   [executable migration](../design/executable-migration.md). Satellites connect
   outward using their office identities; a certificate is not owner authority.
2. **Canonical sources and indexed projections have different lifetimes.**
   Selected definitions, task files, feature definitions and published task history
   live in the remote organization Git repository, mirrored as durable local files.
   Code repositories may be remote or local-only. Office SQLite projects shared
   records and contains explicitly transient state; it is not the only durable
   source of tasks, messages, accounting or authority. Protected messaging sources,
   the usage archive, budget decisions, receipts and execution evidence survive
   cache deletion outside Git. Author-private task notes use a separate durable
   database and never enter shared task history. See
   [ADR-0002](0002-storage-sqlite-sqlcipher-one-writer.md) for encryption, writer and
   flush guarantees; its SQL boundary does not make ordinary files encrypted.
3. **Clients use authorized capabilities, never SQL files.** The browser uses
   HQ's HTTP API and agents use their bound office through `agent-os`. Domain
   authorization, private-note ownership and per-message visibility apply to
   queries, search, feeds and exports as well as writes. An office replica or a
   shared schema is not a grant to expose every record to every agent.
4. **Creation and numbering authority are explicit.** HQ creates teams, projects,
   releases and milestones. Any office can durably save tasks/features while
   disconnected, with a stable ID and pending final number. The shared authority
   allocates numbers and registers feature slugs; replicas never increment
   counters independently. Acknowledged allocation evidence must survive loss of
   a cache and cannot rely on hourly Git publication. New IDs use ULID; old
   stable IDs and explicit legacy reference mappings are preserved. Document
   [06](../design/organization-data/06-identity-and-numbering.md) owns the scopes.
5. **Contracts remain intentionally versioned.** Existing local `org-proto`,
   HTTP `/api/v1`, sync and private launch versions are distinct. The standalone
   plan calls for local API_VERSION 2 when breaking app-only calls/home mutation
   fields and changing authority; this documentation does not claim that transition
   is deployed. Retained versions are additive-only. Update host, worker, client,
   agent helpers and fixtures together and reject unsupported versions before
   dispatch. The current HTTP mutation codec is documented in
   [web-api-v1.md](../design/web-api-v1.md); target relationship changes
   require explicit mappings, not reinterpretation of existing fields.
6. **The agent CLI stays small and office-first.** `agent-os` communicates with
   its bound office; cross-office delivery is routed through HQ. Preserve live
   Phase A spool asks, replies, deadlines and triage until the separately verified
   [messaging cutover](0003-agent-messaging-office-first.md#live-phase-a-coordination-cutover-and-person-started-context).
   An installed legacy CLI is not replaced merely because executable names changed.
7. **Repository layout:** `crates/` holds libraries and `apps/` holds runnable or
   shipped applications. Apps compose libraries; library code does not depend on
   applications. A table does not imply a crate, database or public API.

## Related decisions

- [Offices, HQ and satellites](../design/offices-hq-and-satellites.md): execution
  placement, synchronization, enrollment and recovery boundaries.
- [Teams and the org chart](../design/teams-and-org-chart.md): home membership,
  leadership, reporting and accountable project ownership.
- [Projects and Git](../design/projects-git-github.md): repository registrations,
  project associations and isolated task workspaces.
- [Recovery](../design/organization-data/16-recovery-and-office-continuity.md):
  protected sources and loss boundaries. Git alone does not restore an office.

## Earlier requirements and implementation evidence

The [pre-reconciliation ADR](https://github.com/vgmello/agentic-org/blob/c6e520a36c4dc7c738c68915fac5e7d5d1cbe804/docs/adr/0001-app-is-a-client-office-is-a-server.md)
records the desktop/SMAppService design and earlier `org.db`/`office.db` split.
Those names describe inspected storage and are not a migration accomplished by
this decision. Existing protocol, key-custody and live coordination obligations
remain until their replacements are implemented and verified. The absence of a
deployed desktop population does not authorize deleting live coordination data.

## Out of scope for now

The selected data draft does not implement a migration, task mutation service,
private-note access control, backup/restore engine or distributed budget control.
Automatic HQ promotion and the optional Tauri shell remain later work. Areas,
Undo, advanced checklist execution, retrospectives, automatic scheduling/memory
and process resolution/execution remain unselected; structural process/standard
records do not select those engines.
