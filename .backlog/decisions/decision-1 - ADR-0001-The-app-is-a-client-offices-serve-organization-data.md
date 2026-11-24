---
id: decision-1
title: 'ADR-0001: The app is a client; offices serve organization data'
date: '2026-11-21T00:43:00Z'
status: accepted
---

> **Review note (2026-11-21):** Active v1 architecture decision aligned with `doc-1` (Vision). Host, web, office, and CLI binary foundations exist (`crates/org-host`, `apps/agentic-org-web`, `apps/agentic-org-office`, `apps/agent-os`), while the browser-to-office bridge (`TASK-5`), `org-spool` removal (`TASK-15`), `org-store`-backed messaging (`TASK-45`), isolated private task notes (`TASK-50`), and multi-office satellite relay (`TASK-52`) remain open v1 work.

# ADR-0001 · The app is a client; offices serve organization data

Status: accepted. [ADR-0005](decision-5%20-%20ADR-0005-Standalone-server-HQ-web-application-and-named-instances.md) owns standalone
deployment. The [authority map](../docs/design/doc-62%20-%20Design-Documentation-Authority-Map.md) names the
owning document for each topic.

## The model

- **An organization** owns a recursive team forest, employees, roles, projects,
  offices and repository registrations. Each employee has exactly one home team.
  A team lead is a direct member who manages that team's subtree; direct reporting
  lines are a separate relationship. A security principal's owner authority is
  not derived from an employee record or its place in the chart.
- **A project** has one required accountable owner team and optional contributing
  teams. It may use several organization-registered repositories; each association
  selects its integration branch. A project is not a repository or an office.
- **A task** has a required accountable team and an optional project. A project
  task's accountable team is its project's owner. Current assigned team and
  individual assignee are independent nullable fields. A task has a type; every
  type uses the same `T-` reference. Assigning work changes neither its reporter
  nor its stable identity or reference.
- **An office** belongs to one organization. The organization has one HQ and may
  have satellites. Offices place agents and execute work; they do not own teams or
  projects. People have employee profiles but no execution placement. An agent's
  office placement is separate from its home team and protected by durable
  execution generations.
- **The application** is a browser client of HQ over authenticated HTTP. Opening
  it neither installs an office nor enrolls a satellite. Instance setup, service
  lifetime and browser sessions are separate.

The owning relationships and field contracts are in organization-data documents
[01](../docs/organization-data/doc-75%20-%20Organization-Data-01-—-Organization-Structure.md),
[02](../docs/organization-data/doc-76%20-%20Organization-Data-02-—-Project-Structure.md),
[03](../docs/organization-data/doc-77%20-%20Organization-Data-03-—-Features-and-Work.md),
[04](../docs/organization-data/doc-78%20-%20Organization-Data-04-—-Agent-Structure.md) and
[12](../docs/organization-data/doc-84%20-%20Organization-Data-12-—-Task-Assignment-and-History.md).

## Decisions

1. **One office is served by one worker process.** `agentic-org serve` composes
   the host library and supervises `agentic-org-office` workers through the
   private launch contract. The instance's `agentic-org-web` process terminates
   browser HTTP and holds no keys. [Executable migration](../docs/design/doc-63%20-%20Design-Executables-and-Entry-Points.md)
   records which entry points exist today. Satellites connect outward using their
   office identities; a certificate is not owner authority.
2. **Files are the source; the database is a projection.** Definitions, task
   files, feature definitions and published task history live in the remote
   organization Git repository, mirrored as durable local files. Code repositories
   may be remote or local-only. The office database indexes shared records and
   holds explicitly transient state; it is not the only durable source of tasks,
   messages, accounting or authority. Protected messaging sources, the usage
   archive, budget decisions, receipts and execution evidence survive cache
   deletion outside Git. Private task notes use a separate durable database
   and never enter shared task history. [ADR-0002](decision-2%20-%20ADR-0002-Storage-SQLite-with-one-batching-writer-per-database.md)
   owns the writer and flush guarantees.
3. **Clients use authorized capabilities, never SQL files.** The browser uses
   HQ's HTTP API and agents use their bound office through `agent-os`. Domain
   authorization, private-note ownership and per-message visibility apply to
   queries, search, feeds and exports as well as writes. An office replica or a
   shared schema is not a grant to expose every record to every agent.
4. **Creation and numbering authority are explicit.** HQ creates teams, projects,
   releases and milestones. Any office can durably save tasks and features while
   disconnected. A task gets its final reference from the creating office at once
   ([ADR-0019](decision-19%20-%20ADR-0019-Task-numbers-are-minted-by-the-creating-office.md)). A feature has a stable ID and
   a pending number until HQ allocates it and registers its slug. Acknowledged
   allocation evidence survives loss of a cache and does not rely on hourly Git
   publication. IDs are UUIDv7. Document
   [06](../docs/organization-data/doc-80%20-%20Organization-Data-06-—-Identity-and-Numbering.md) owns the scopes.
5. **Contracts change in place until v1 ships.** The local `org-proto` API, HTTP
   `/api/v1`, the sync contract and the launch contract are separate contracts.
   Each has one current form; no older version, adapter or mapping is kept
   ([pre-release rule](../docs/organization-data/doc-72%20-%20Organization-Data-—-README.md#no-compatibility-obligations-before-v1-ships)).
   Host, worker, client, agent helpers and fixtures change together, and an
   unsupported version is rejected before dispatch. The HTTP mutation codec is
   documented in [web-api-v1.md](../docs/design/doc-71%20-%20Design-Web-HTTP-API-v1.md).
6. **The agent CLI stays small and office-first.** `agent-os` communicates with
   its bound office; cross-office delivery is routed through HQ
   ([ADR-0003](decision-3%20-%20ADR-0003-Agent-messaging-is-office-first.md)).
7. **Repository layout:** `crates/` holds libraries and `apps/` holds runnable or
   shipped applications. Apps compose libraries; library code does not depend on
   applications. A table does not imply a crate, database or public API.

## Related decisions

- [Offices, HQ and satellites](../docs/design/doc-67%20-%20Design-Offices-HQ-and-Satellites.md): execution
  placement, synchronization, enrollment and recovery boundaries.
- [Teams and the org chart](../docs/design/doc-38%20-%20Design-Teams-People-and-Org-Chart.md): home membership,
  leadership, reporting and accountable project ownership.
- [Projects and Git](../docs/design/doc-35%20-%20Design-Projects-Git-and-GitHub.md): repository registrations,
  project associations and isolated task workspaces.
- [Recovery](../docs/organization-data/doc-91%20-%20Organization-Data-16-—-Recovery-and-Office-Continuity.md):
  protected sources and loss boundaries. Git alone does not restore an office.

## Not in v1

Automatic HQ promotion. Undo, advanced checklist execution, retrospectives,
automatic scheduling and memory, and process execution engines are not selected;
structural process and standard records do not select those engines.
