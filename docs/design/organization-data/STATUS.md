# Organization data: status, changelog and host-owned records

Companion to the [organization-data index](README.md), split out to keep the index short. It has the same authority as the index.

## Outside this set: what the host owns

This set describes an organization's data. Some records it refers to belong to the host and are defined in the [standalone design](../standalone-server.md) and the host's own database, not here:

| Host-owned record | What this set uses from it |
| --- | --- |
| Instance | Its immutable ID, as `instance_id` in assignment fingerprints. An instance can host several organizations. |
| Account | The authenticated person. Instance-administrator authority, needed to create an organization, belongs to an account. |
| Organization owner | An account linked to the organization as its owner. It is a security principal, not an employee field. Shared records name it as actor kind `owner` with no employee ID; private notes key on the authenticated account. |
| Session | A signed-in browser or CLI session ([ADR-0013](../../adr/0013-stored-browser-sessions.md)). Never referenced by organization data. |
| Office secrets key and writer custody | Held by the host per office ([ADR-0016](../../adr/0016-no-application-level-data-encryption.md), [document 16](16-recovery-and-office-continuity.md)). Never stored under the organization data root. |

An owner or administrator who is also to appear in the organization gets an ordinary person record with a home team. Nothing in an employee record grants sign-in, ownership or administration.

## Status and changelog

Every numbered document carries one status line. The vocabulary is: **draft** (still being shaped), **selected** (the contract to build to), **implemented** (the code matches). All numbered documents are selected. What the code actually holds is tracked per table in [schema coverage](SCHEMA-COVERAGE.md#implementation-status). A document's own revision number changes when its contract changes.

| Date | Set revision | Change |
| --- | --- | --- |
| 2026-11-17 | 2.14 | Tasks: `priority` is the closed list urgent, high, normal, low; optional `effort`, a whole number of points. The two gaps left by removing the adoption interface have a proposed resolution in document 16. |
| 2026-11-17 | 2.13 | Legacy structures removed (owner: "all the legacy stuff we need to remove it"). Draft schema: no `directed_thread` kind, no source-hash columns on messages, one task-event version with required profile and bytes, no `legacy_definition_path`, no `legacy` attribution basis. Documents: the task adoption profile, the adoption and staging interface and the import rules are gone. |
| 2026-11-17 | 2.12 | Owner answers. `cancelled` is a task status and counts as done; an empty milestone is not done (confirmed). An ask carries `required` (ADR-0020). Creating a task uses the fast acknowledgement. |
| 2026-11-17 | 2.11 | Owner decisions. Stable IDs are UUIDv7. Task references are minted by the creating office, one sequence per office, with a letter code (`T-B37`); tasks are never unnumbered; two tables added (`office_codes`, `office_task_counters`). Messaging: card answers, close outcome and pins each have one writing office. Release progress counts follow milestone tasks. |
| 2026-11-17 | 2.10 | Second review round. Messaging: one writing office per file, with each employee's state in that employee's own files; a send relays after it answers and an ask always has a deadline. Process and standard resolution selected for v1. Numbering and permit rules stated without naming a database statement (ADR-0018). Office-minted task references accepted in part in ADR-0019 (letter code, tasks only); document 06 carried a note until revision 2.11 applied it. Completion view ignores deleted manifest tasks and returns done counts. |
| 2026-11-17 | 2.9 | Owner architecture review. One storage tree (`durable/`, `runtime/`). HQ named as organization authority. Hash codec registry. Messaging runtime rules merged from ADR-0003. Budget hold checked at the turn permit. Agent re-placement separated from office relocation. No compatibility obligations before v1 ships. Task status is a closed list; milestone and release completion is derived. Employee state is one status field; the owner marker is removed. HQ-created records always carry a number. Host-owned records are listed. Document 14 no longer repeats SQL. Schema coverage gains implementation status. No application-level data encryption (ADR-0016); acknowledgement classes (ADR-0014). |
| 2026-11-15 | 2.8 | Reconciled set, before the review. |

The SQL is a structural draft for the selected scope. Document 11 owns model/execution behavior within this integrated document set; implementing the schema, task mutation/replication, private task-note storage/access controls, memory consolidation and analytics retention still require their own work. Run `python3 docs/design/organization-data/test_schema.py` for executable draft-schema checks.

Recovery sources are defined in [document 16](16-recovery-and-office-continuity.md); office caches are disposable only after durable sources and safe process reconciliation are accounted for.
