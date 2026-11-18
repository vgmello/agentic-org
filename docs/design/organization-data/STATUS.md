# Organization data: status, changelog and host-owned records

Companion to the [organization-data index](README.md), split out to keep the index short. It has the same authority as the index.

## Outside this set: what the host owns

This set describes an organization's data. Some records it refers to belong to the host and are defined in the [standalone design](../standalone-server.md) and the host's own database, not here:

| Host-owned record | What this set uses from it |
| --- | --- |
| Instance | Its immutable ID, as `instance_id` in assignment fingerprints. An instance can host several organizations. |
| Account | A signed-in person. Accounts are created only through one-time registration links. `agentic-org setup` prints the first one, and the account registered through it is the server's administrator: exactly one account holds that role at any time, and it can hand the role to another account in the browser. Further links are issued in the browser, from People, by the administrator or the organization's owner, each for an existing person record. A link works once, expires and is stored only as a digest; a forgotten password is reset through a link issued the same way (for the administrator, by `agentic-org admin reset` on the server machine), and command-line sign-in is approved in the browser through a one-time link. Only the administrator creates organizations. |
| Account to person link | In each organization an account can open, the account is tied to exactly one person record (an employee of kind `person`); never to an agent. The link is host data and grants no authority by itself ([document 04](04-agent-structure.md#accounts-people-and-the-organization-owner)). |
| Organization ownership | The account that created the organization owns it until that owner hands ownership to another person who has an account; an organization has exactly one owner. Ownership is a permission of the owning account, not an employee field. The owner acts in the organization as their own person record; `owner` in a record that addresses someone means that person at the time it is read. |
| Session | A signed-in browser or CLI session ([ADR-0013](../../adr/0013-stored-browser-sessions.md)). Never referenced by organization data. |
| Office secrets key and writer custody | Held by the host per office ([ADR-0016](../../adr/0016-no-application-level-data-encryption.md), [document 16](16-recovery-and-office-continuity.md)). Never stored under the organization data root. |

Nothing in an employee record grants sign-in, ownership or administration.

## Status and changelog

Every numbered document carries one status line. The vocabulary is: **draft** (still being shaped), **selected** (the contract to build to), **implemented** (the code matches). All numbered documents are selected. What the code actually holds is tracked per table in [schema coverage](SCHEMA-COVERAGE.md#implementation-status). A document's own revision number changes when its contract changes.

| Date | Set revision | Change |
| --- | --- | --- |
| 2026-11-18 | 2.20 | Owner: releases, milestones and features are primitives a development process chooses to use (a `releases` tab view; no rule that a change is in a release); `may_hire` applies to any lead, person or agent. |
| 2026-11-18 | 2.19 | Owner: the new primitive is a development process, separate from organization processes (tables and paths renamed from process package); it declares the project's tabs (`development_process_tabs` replaces `uses`); it can be imported as a file. 90 tables and 11 views. |
| 2026-11-18 | 2.18 | Owner: releases and milestones are first-class in every project whatever its development process, and releases are not optional; a development process's `uses` lists only `features`. A team is created before and without any project. |
| 2026-11-18 | 2.17 | Owner answers on REQ-07: a lead approves a hire and an agent lead needs `may_hire`, a new field of an agent's profile ([ADR-0022](../../adr/0022-a-lead-approves-a-hire.md)); a development process names the optional records it uses (`uses`); development processes do not move between organizations. |
| 2026-11-18 | 2.16 | Document 18 added for the owner's REQ-06 and REQ-07: development processes, a project's development process, the project documentation tree and its index, issued document versions, code links and the tasks that realize a version. `projects` gains `development_process_id`; `documents` gains `summary`, `path`, `version` and `issued_revision`, and an extensible `kind` for tree documents; project-wide decisions, designs and reports move into the tree. The shared draft has 89 tables and 11 views. |
| 2026-11-17 | 2.15 | Every actor is a person or an agent, never `owner`; accounts are tied to persons; creating an organization makes its top-level team, the creator's person record and the HR lead; a project creates its team; agents are onboarded only through HR; areas are removed; older material removed. |

The SQL is a structural draft. Implementing the schema, task mutation/replication, private task-note storage/access controls, memory consolidation and analytics retention is separate work. Run `python3 docs/design/organization-data/test_schema.py` for executable draft-schema checks.

Recovery sources are defined in [document 16](16-recovery-and-office-continuity.md); office caches are disposable only after durable sources and safe process reconciliation are accounted for.
