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
| 2026-11-17 | 2.10 | Second review round. Messaging: one writing office per file, with each employee's state in that employee's own files; a send relays after it answers and an ask always has a deadline. Process and standard resolution selected for v1. Numbering and permit rules stated without naming a database statement (ADR-0018). Office-minted task references accepted in part in ADR-0019 (letter code, tasks only); document 06 carries a note and is rewritten when the counting question is settled. Completion view ignores deleted manifest tasks and returns done counts. |
| 2026-11-17 | 2.9 | Owner architecture review. One storage tree (`durable/`, `runtime/`). HQ named as organization authority. Hash codec registry. Messaging runtime rules merged from ADR-0003. Budget hold checked at the turn permit. Agent re-placement separated from office relocation. IDs are ULIDs. No compatibility obligations before v1 ships. Task status is a closed list; milestone and release completion is derived. Employee state is one status field; the owner marker is removed. HQ-created records always carry a number. Host-owned records are listed. Document 14 no longer repeats SQL. Schema coverage gains implementation status. No application-level data encryption (ADR-0016); acknowledgement classes (ADR-0014). |
| 2026-11-15 | 2.8 | Reconciled set, before the review. |

The SQL is a structural draft for the selected scope. Document 11 owns model/execution behavior within this integrated document set; application migrations, task mutation/replication implementation, private task-note storage/access controls, memory consolidation, analytics retention and remaining v1 migration choices still require their own work. Existing data not mapped by this set must stay intact in import staging until a disposition is chosen. Run `python3 docs/design/organization-data/test_schema.py` for executable draft-schema checks.

Recovery sources are defined in [document 16](16-recovery-and-office-continuity.md); office caches are disposable only after durable sources and safe process reconciliation are accounted for.
