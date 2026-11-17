# Conflict resolution plan

Working file for resolving every contradiction found between the design documents, the organization data contract and the code on `v1-integration`. It records, per finding, both sides, the recommended resolution, what it gains, what it costs, the impact of applying it, the owner's note where one exists, and whether it has been applied.

The interactive register with per-entry status is the companion view. This file is the durable record in the repository; update it in the same change that applies a fix.

## Precedence rules used for every recommendation

1. **The organization data is newer than the ADRs and the older designs.** For data relationships, storage layout and durability it wins.
2. **An ADR decision that the organization data does not cover is not dropped.** It is merged into the organization data, and the ADR keeps its detail as evidence (A-03, A-06, E-01, E-02).
3. **An ADR decision the organization data does not contradict stands.** Process boundaries, the single writer and release verification remain as decided (ADR-0001, ADR-0002, ADR-0005, ADR-0008, ADR-0009), except where a later owner decision amends them (ADR-0016 removes database encryption).
4. **Code is evidence of current behaviour, never of the target.** Where code and the selected design differ, the design wins and the code changes, unless a recorded decision changes the design instead (B-04).
5. **Nothing is deployed, and the better architecture wins.** Owner, 2026-11-17: until v1 is declared shipped there are no versions to keep side by side, no migrations and no legacy shapes to preserve. Where a rebuild gives the cleaner result, rebuild (A-12, B-01, B-03).

## Status

52 findings, all reviewed by the owner on 2026-11-17. 34 are fixed in the documents. 18 are decided and recorded as requirements, with code still to change. A few questions remain for the owner; they are listed under "Questions still with the owner".

| ID | Finding | Priority | Size | State | Commit |
| --- | --- | --- | --- | --- | --- |
| [A-01](#a-01) | Three vocabularies for where data lives on disk | Decide first | S | Fixed in documents | `cd36075` |
| [A-02](#a-02) | Budget holds are required before a turn, but the dispatch steps have no budget check | Before building the area | S | Fixed in documents | `2e44b70` |
| [A-03](#a-03) | Who hosts the numbering authority is never stated in the data contract | Before building the area | S | Fixed in documents | `5f3e225` |
| [A-04](#a-04) | Moving an agent between machines: supported in one contract, unsupported in another | Before building the area | S | Fixed in documents | `4b7e893` |
| [A-05](#a-05) | Three hash canonicalizations with no registry | Before building the area | S | Fixed in documents | `c71e33b` |
| [A-06](#a-06) | Messaging has a data model and no named runtime owner | Before building the area | S | Fixed in documents | `7a552bf` |
| [A-07](#a-07) | Human takeover: browser attaches to the agent terminal, or local CLI only | Before building the area | S | Fixed in documents | `e2ad9ec` |
| [A-08](#a-08) | Agent credentials: the user's own vendor sign-in, or office-local bindings | Before building the area | S | Decided; code pending | `cb49e54` |
| [A-09](#a-09) | Create organization: the published API takes a name, provisioning needs four more fields | Decide first | M | Decided; code pending | `cb49e54` |
| [A-10](#a-10) | Is GitHub part of v1 | Before building the area | S | Fixed in documents | `817c4c2` |
| [A-11](#a-11) | Git design algorithms still use the old two-clone, one-repository-per-project shape | Before building the area | M | Fixed in documents | `817c4c2` |
| [A-12](#a-12) | Which local API version the first standalone release speaks | Cleanup | S | Fixed in documents | `17c3964` |
| [A-13](#a-13) | A message file has one owning writer, but each recipient's state belongs to that recipient's office | Before building the area | M | Fixed in documents | `cb49e54` |
| [B-01](#b-01) | Storage: files are canonical in the design, databases are authoritative in the code | Decide first | L | Decided; code pending | `11464ce` |
| [B-02](#b-02) | Single home team is selected; the code is many-to-many | Decide first | M | Decided; code pending | `47e1596` |
| [B-03](#b-03) | One typed task is selected; the code has per-kind work items | Decide first | L | Decided; code pending | `47e1596` |
| [B-04](#b-04) | New record IDs: UUIDv7 in the contract, ULID in the code | Before building the area | S | Decided; code pending | `3bbc57f` |
| [B-05](#b-05) | Areas are deferred in the design and live in the code | Before building the area | S | Decided; code pending | `47e1596` |
| [B-06](#b-06) | Launch protocol: the host writes v2, the worker implements v3, three ADRs need v3 | Decide first | M | Decided; code pending | `cb49e54` |
| [B-07](#b-07) | Human authorization: grants are designed, the app token is still what is checked | Before building the area | M | Decided; code pending | `11464ce` |
| [B-08](#b-08) | Gateway channel limit: host allows 63, worker rejects the eighth | Cleanup | S | Fixed in documents | `cb49e54` |
| [B-09](#b-09) | v1 targets macOS and Linux; workers only start on macOS | Before building the area | S to decide, L to build | Decided; code pending | `11464ce` |
| [B-10](#b-10) | Calls defined in the contract and not served by the office | Before building the area | M | Decided; code pending | `11464ce` |
| [B-11](#b-11) | Libraries with no consumer | Cleanup | S | Fixed in documents | `11464ce` |
| [B-12](#b-12) | Setup is described in the present tense and refuses in the code | Cleanup | S | Fixed in documents | `11464ce` |
| [B-13](#b-13) | The storage engine should stay replaceable; three files and some contract wording tie it to SQLite | Before building the area | M | Decided; code pending | `cb49e54` |
| [C-01](#c-01) | Draft SQL allows an empty number on records only HQ creates | Before building the area | S | Fixed in documents | `17c3964` |
| [C-02](#c-02) | Task, release and milestone status have no published vocabulary | Before building the area | S | Fixed in documents | `5b92fb4` |
| [C-03](#c-03) | Employee state is spread over three fields and two documents | Before building the area | S | Fixed in documents | `17c3964` |
| [C-04](#c-04) | The owner is represented three different ways | Before building the area | S | Fixed in documents | `17c3964` |
| [C-05](#c-05) | The set depends on an Instance and never defines one | Before building the area | S | Fixed in documents | `17c3964` |
| [C-06](#c-06) | The README storage tree is incomplete | Cleanup | S | Fixed in documents | `cd36075` |
| [C-07](#c-07) | Diagrams that disagree with their own DDL | Cleanup | S | Fixed in documents | `17c3964` |
| [C-08](#c-08) | Documents 11 and 14 define the same records | Cleanup | S | Fixed in documents | `17c3964` |
| [C-09](#c-09) | Status labels and revisions differ across the set | Cleanup | S | Fixed in documents | `17c3964` |
| [C-10](#c-10) | The usage ledger has no named writer | Before building the area | S | Fixed in documents | `17c3964` |
| [C-11](#c-11) | No traceability from the draft schema to the code | Before building the area | S | Fixed in documents | `17c3964` |
| [C-12](#c-12) | Offline offices use a pending number; the rule for agents is not written down | Before building the area | S | Decided; code pending | `5b92fb4` |
| [C-13](#c-13) | Task history is published to Git as one file per change | Before building the area | S to keep, L to change | Fixed in documents | `17c3964` |
| [C-14](#c-14) | Process and standard inheritance is stored but not applied in v1 | Before building the area | M | Fixed in documents | `11464ce` |
| [D-01](#d-01) | Old executable names in live design text | Cleanup | S | Fixed in documents | `e2ad9ec` |
| [D-02](#d-02) | Named owners and approval gates that no longer exist | Cleanup | S | Fixed in documents | `e840ea3` |
| [D-03](#d-03) | Crates described as absent that exist | Cleanup | S | Fixed in documents | `11464ce` |
| [D-04](#d-04) | ADR-0004 is referenced and does not exist | Cleanup | S | Fixed in documents | `11464ce` |
| [D-05](#d-05) | The implemented-entry-points record lags the branch | Cleanup | S | Fixed in documents | `11464ce` |
| [D-06](#d-06) | Code comments that still describe the desktop app | Cleanup | S | Fixed in documents | `47e1596` |
| [D-07](#d-07) | The web process is still called the gateway in designs and code | Cleanup | S for prose, M for code | Decided; code pending | `e840ea3` |
| [E-01](#e-01) | Encryption at rest: decided for databases, unstated for the files that replace them, and questioned by the owner | Decide first | S to decide, M to apply | Decided; code pending | `b59b4e3` |
| [E-02](#e-02) | Messaging latency targets assume a batched database commit; the data contract stores one file per message | Before building the area | M | Fixed in documents | `cb49e54` |
| [F-01](#f-01) | Browser sessions live in memory, so every restart signs everyone out | Before building the area | M | Decided; code pending | `b59b4e3` |
| [F-02](#f-02) | A 15 ms send target for agent-os, against an owner target of 1 to 2 ms | Before building the area | M | Decided; code pending | `b59b4e3` |
| [F-03](#f-03) | Agents run as the same OS user as the data; isolation is not a replaceable part of the launch contract | Decide first | S for the setting, M to L for the separate user | Decided; code pending | `b59b4e3` |

Size: S is one documentation change, M is several documents or a bounded code change, L is a code migration.

## Handoff for implementing agents

Implementation was paused on 2026-11-17 while the architecture was reviewed. Before resuming, an implementing agent reads this section.

**Rules that apply to everything.**

- Nothing has shipped. Do not write migrations, compatibility shims, version negotiation for old clients or legacy import code. Replace, do not adapt ([pre-release rule](organization-data/README.md#no-compatibility-obligations-before-v1-ships)).
- Names: the browser-facing process is "web", the agent command line is `agent-os`, the office worker is `agentic-org-office`. Code identifiers that still say `gateway` are to be renamed (D-07).
- Paths: `durable/` and `runtime/<office-id>/` only.
- Nothing from before is carried over, including the former development team's earlier command-line tool and its message queue.

**Decisions in force.** Each is a requirement, with acceptance criteria where an ADR is named.

| Decision | Requirement | Checklist row | What changes for work in progress |
| --- | --- | --- | --- |
| Stored browser sessions | [ADR-0013](../adr/0013-stored-browser-sessions.md) | FC-159 | Sessions outlive the control connection. |
| Acknowledgement classes, agent-os target, send and ask | [ADR-0014](../adr/0014-acknowledgement-classes-and-agent-os-latency.md) | FC-160 | Committed class for messaging and non-critical task activity, declared in the domain crate. A send never waits for relay; an ask always has a deadline. |
| Agent isolation, private agent home, pool sign-in | [ADR-0015](../adr/0015-agent-isolation-modes.md) | FC-161 | The launch contract gains an isolation mode and a private home per agent. The user's own vendor folders are not used. |
| No application-level data encryption | [ADR-0016](../adr/0016-no-application-level-data-encryption.md) | FC-162 | SQLCipher goes; the office secrets key wraps private keys. |
| Headless sessions, chat, no browser terminal | [ADR-0004](../adr/0004-headless-agent-sessions.md) | FC-151 | No terminal frames, no `session.input`, `session.resize` or `session.output`. |
| macOS and Linux, including containers | [ADR-0017](../adr/0017-linux-and-container-hosting.md) | FC-164 | Five platform contracts; nothing else branches on the operating system. |
| Replaceable storage engine | [ADR-0018](../adr/0018-storage-engine-independence.md) | FC-165 | SQL only inside store adapters; engine-independent suites. |
| One launch contract, grants, seven web channels, host work reads | [Host boundary](office-host-boundary.md#owner-decisions-2026-11-17-that-this-document-must-be-read-with) | FC-013, FC-017, FC-162 | The rendezvous contract is the only one; `app_token` and `db_key` are removed. |
| Organization data revision 2.10 | [Index](organization-data/README.md#status-and-changelog) | several | Task status list, derived milestone and release completion, one employee status, ULIDs, one writer per messaging file. |
| Process and standard resolution | [Processes design](processes.md), decision 4 | FC-110, FC-112 | One chain: organization, team, project. Non-overridable documents go into the agent's system instructions. |
| Create-organization payload | [Web API](web-api-v1.md#selected-create-organization-payload) | FC-018 | Slug, organization repository, state branch, bootstrap team. |
| Local Git only in v1 | [Git design](projects-git-github.md) | FC-140, FC-163 | Plain remotes with office credentials; provider integrations are v2. |
| Single home team, typed tasks, no areas | Findings B-02, B-03, B-05 | FC-028, FC-040, FC-056 | Remove the old shapes; do not migrate them. |

**How the decisions reach an agent.** `AGENTS.md` sends every agent to the [documentation authority map](documentation-authority.md), which lists the ADRs and this plan. Each affected design, ADR and spec carries a dated note at the place an agent would read the old rule. [FEATURES.md](../FEATURES.md) has a row per decision.

**Task numbers.** [ADR-0019](../adr/0019-office-coded-task-numbers.md), accepted and in the data contract: a task reference is minted by the creating office, one sequence per office, with a letter code (`T-B37`), for tasks only. Build that; do not build HQ allocation or a pending-number state for tasks.

**IDs.** Every ID is a UUIDv7 ([identity contract](organization-data/06-identity-and-numbering.md)).

**Asking and waiting.** [ADR-0020](../adr/0020-required-asks-suspend-and-resume.md): an ask waits a bounded time and may be marked required; a required ask left unanswered suspends the agent, and the answer resumes it in the same session (FC-166).

**Owner requirements received in the review.** [REQ-04](../specs/req-04-channel-messaging-and-question-components.md), questions from an agent to a person, and [REQ-05](../specs/req-05-virtual-console-conversation-log-viewer.md), the session view. There are no external channels; questions are rendered in the product's own chat with shadcn-svelte and Bits UI components. REQ-05 can start with its proof of concept.

## Questions still with the owner

None. The owner answered every question raised in the review on 2026-11-17. Open design points that need a spike or a short design, not an owner decision, are listed in each ADR and under the order of work.

## Order of work

**The aim behind the order.** The owner's goal is to get the two engineers and their agents working inside the product as early as it can carry them. Creating an organization, signing in, people and teams, tasks and chat are what onboarding needs, so within each step below they come first.

1. **Before code.** Short designs: Linux admission, credential provider and confinement (B-09); the vendor sign-in spike for the private agent home (A-08); the open questions in the Git design (A-11); the envelope flush discipline (ADR-0014).
2. **Storage rebuild.** B-01 with B-13: rebuild the office store against the selected files and tables, behind engine-independent contracts. This is the largest piece and needs its own plan. B-02, B-03 and B-05 land with it, since they remove shapes the rebuild would otherwise carry.
3. **Host increment.** B-06 (one launch contract), then B-07 (grants), B-08 (channel limit), B-10 (work reads) and F-01 (stored sessions). This opens the web process.
4. **Setup.** A-09 (create payload), E-01 (no database encryption, wrapped keys), then `agentic-org setup` on macOS and Linux (B-09).
5. **Running agents.** F-03 and A-08 (isolation and private home), C-14 (process resolution at launch), then the first vendor adapter.
6. **Speed.** F-02 and E-02: committed class, session mode, measurement on a Mac.
7. **Mechanical cleanup, any time.** D-07: rename `gateway` identifiers in the code.

## A. Contract against contract

Two design documents or data contracts say different things.

### A-01

**Three vocabularies for where data lives on disk**

Priority: Decide first · Size: S · State: Fixed in documents

- **One document:** Usage, execution and the README use `<data-root>/<organization-id>/durable/…` and `runtime/<office-id>/office.db`. *(organization-data/README · shared storage contract; 10 line 51; 11 line 98)*
- **Another document:** Messaging, recovery and budgets use `<organization-operational-root>/…` and `<office-state>/cache/`. The standalone design uses `office-state/offices/<office-id>/`. *(15 lines 18, 23; 16 line 22; 17 lines 46, 53; standalone-server.md line 261)*
- **Why it matters:** Nobody can implement the durable layout, a backup, or a cache rebuild without knowing whether these are the same directory. This is the first decision the storage migration needs.
- **Which source wins:** Organization data (newer than the ADRs and designs)
- **Recommendation at the time of the finding:** Define each root once in the organization-data README, with one complete tree, and make documents 10, 11, 14, 15, 16 and 17 reference it. State plainly whether the operational root is `durable/` or a sibling of it.
- **What we gain:** One rule everyone can hold in their head: back up `durable/`, delete `runtime/`. Backup, restore and cache-rebuild can each be specified against one tree. C-06 closes at the same time.
- **What we lose:** The separately named operational root implied chat, receipts and budgets could sit on a different volume from the rest. That is still possible by mounting `durable/` elsewhere, but only as a whole, not per kind of data.
- **Impact of applying it:** Documentation only. No code moves: the code still uses `offices/<office-id>/` and migrates under B-01. The acceptance journal's relative path (`offices/<id>/journals/work-acceptance/…`) already matches `durable/recovery/`.
- **Owner note:** "OK the document on the organization data root I thinnk makes more sense right ? can we leverage that for evertyhing else can that be applied ?"
- **Applied in `cd36075`:** Yes, it applies to everything. The organization-data README now holds the one complete tree. Messaging, recovery and budgets moved under `durable/`; the private-notes database is shown; the three older root names are mapped once. Documents 15, 16, 17, the standalone and offices designs, the chat and cost specs and ADR-0003 use the same paths. ADR-0002 records that the data contract owns layout while its engine, single-writer and key-custody decisions stand.

### A-02

**Budget holds are required before a turn, but the dispatch steps have no budget check**

Priority: Before building the area · Size: S · State: Fixed in documents

- **One document:** "All offices consult the same authorized budget-control state before admitting new affected turns." *(17-budgets-and-controls.md line 41)*
- **Another document:** Delivery scope excludes "budget-triggered pausing". The dispatch cycle goes validate, route, permit, spawn with no hold check. A footer acknowledges document 17 without changing the steps. *(11-model-profiles-and-agent-execution.md lines 37, 314, 946)*
- **Why it matters:** Whoever builds the turn dispatcher from document 11 will ship execution that ignores budget holds, and document 17 will read as satisfied by data alone.
- **Which source wins:** Organization data document 17, with the check placed in document 11
- **Recommendation at the time of the finding:** Add one admission step to document 11's dispatch: "reject when a task or agent hold is active". Keep mid-turn pausing excluded. Document 17 owns the rule; document 11 owns where it runs.
- **What we gain:** Budget holds are enforced by construction. HQ already issues every turn permit, so checking the hold there costs no extra round trip and no office decides from a stale copy. It also satisfies "an unreachable coordinator cannot clear a hold" for free.
- **What we lose:** The permit coordinator now depends on budget-control state, so those two HQ responsibilities can no longer be built or deployed apart. A satellite cut off from HQ cannot start a held or an unheld turn, but that was already true for permits. Budgets still never stop a running turn, so some overshoot remains.
- **Impact of applying it:** Documentation now; one extra read in the permit transaction when execution is built (FC-098 to FC-100). New outcome code `BUDGET_HELD` and acceptance criterion AE-19.
- **Owner note:** "What is your recomendations wnat what does it gives us or make us lose ?"
- **Applied in `2e44b70`:** Applied on my recommendation; you asked for the trade-off, so say if you want it reverted. Document 11 section 6 step 3: HQ grants no permit while an unreleased hold applies to the task or agent, and the turn blocks with `BUDGET_HELD`. A running turn is never refused: neither permit renewal nor a failover switch permit is checked, so the current turn finishes as document 17 requires. Resuming a blocked turn under a hold is refused. Added the outcome row, AE-19, and the pointer from document 17.

### A-03

**Who hosts the numbering authority is never stated in the data contract**

Priority: Before building the area · Size: S · State: Fixed in documents

- **One document:** "The shared logical organization authority registers final numbers" and serializes allocations. The host of that authority is not named. *(06-identity-and-numbering.md line 36)*
- **Another document:** HQ is the organization authority: it creates teams and projects and allocates numbers. *(ADR-0001 decision 4; offices-hq-and-satellites.md)*
- **Why it matters:** "Logical authority" leaves room for a separate allocation service. Numbering durability and recovery depend on knowing which process owns the ledger.
- **Which source wins:** ADR-0001 and the offices design, merged into organization data
- **Recommendation at the time of the finding:** State in document 06: in v1 the HQ office worker is the organization authority and owns `durable/authority/`. Promotion to another office stays deferred.
- **What we gain:** One named owner for numbering, slug and key registration, permits, placement generations, budget state and corrections. Nobody builds a separate allocation service by reading "logical authority" literally.
- **What we lose:** Nothing in v1. It does commit to HQ being a single point for those functions until promotion is designed.
- **Impact of applying it:** Documentation only.
- **Owner note:** "lets follow your recommendations hq is the organization authority that is easy to understand"
- **Applied in `5f3e225`:** The README has a new "Organization authority" section: in v1 it is a role of the HQ office worker, not a separate service. Document 06 points to it, and the decisions table has a row for it. Promotion stays deferred.

### A-04

**Moving an agent between machines: supported in one contract, unsupported in another**

Priority: Before building the area · Size: S · State: Fixed in documents

- **One document:** Moving an agent needs the previous supervisor's verified cleanup acknowledgement, or a machine fence, and a new placement generation. *(11-model-profiles-and-agent-execution.md line 132)*
- **Another document:** "Restoring host custody itself, machine migration and handover remain explicit unsupported recovery cases." *(16-recovery-and-office-continuity.md lines 152 to 153)*
- **Why it matters:** These are probably two different things (re-placing one agent at another office, versus moving a whole office to new hardware), but the words overlap and the text does not separate them.
- **Which source wins:** Both, once the two operations are named separately
- **Recommendation at the time of the finding:** Name them separately in both documents: *agent re-placement* (supported, document 11) and *office relocation* (unsupported, document 16), with a cross-reference.
- **What we gain:** No one reads "machine migration unsupported" as forbidding agent moves, or reads the agent-move rule as permission to copy an office to new hardware. The v2 path is protected: the records that relocation needs are named, so a later change cannot quietly remove them.
- **What we lose:** Replacing the HQ machine is not possible in v1. If that machine dies, recovery is restore-from-backup on the same custody, not a move. That is the accepted v1 limit.
- **Impact of applying it:** Documentation only. Adds one guard for future work: binding a durable record to a machine identity without an incarnation, epoch or generation now needs a recorded decision.
- **Owner note:** "well fix it, lets we should be able to do that but that can be a v2 feature, the architecture should be extensible enough to support this."
- **Applied in `4b7e893`:** Document 16 has a new section, "Moving agents and moving offices". Agent re-placement stays selected (document 11 owns the fencing rule). Office relocation, including replacing the HQ machine, is marked v2, with the list of existing records that keep it possible: incarnation, epoch, placement generation, writer custody and hash-chained journals. Document 11 cross-references it. I read your note as putting office relocation in v2; tell me if you also meant agent re-placement.

### A-05

**Three hash canonicalizations with no registry**

Priority: Before building the area · Size: S · State: Fixed in documents

- **One document:** Usage and execution payloads: SHA-256 of UTF-8 JSON with lexically sorted keys and compact separators. *(10-usage-and-cost-accounting.md line 61)*
- **Another document:** Messaging envelopes use RFC 8785 canonical JSON, and keep a legacy `v1_lenprefixed_sha256` codec. Task events use "codec 2". *(15-messaging-and-chat.md lines 26, 134; 05-operational-data.md task-event codec 2)*
- **Why it matters:** Each is defensible alone. Without one list, an implementer will reuse the wrong canonicalizer and produce hashes that verify in tests and fail on recovery.
- **Which source wins:** New decision, recorded in organization data document 08
- **Recommendation at the time of the finding:** Add a small codec registry to document 08: name, canonicalization, where it is used, and a statement that the differences are intentional. The architecture rules already say hashing semantics stay with their owner.
- **What we gain:** Scales by rule instead of by memory: a stored hash always names its codec through schema version or an explicit column, new codecs are additive, old bytes are never recoded, and shared test vectors keep Rust and TypeScript in agreement. New record families get a default instead of inventing a fourth form.
- **What we lose:** A little freedom: adding a canonical form now needs a registry row first. Three forms stay in use for existing records; unifying them would mean rehashing written data, which the rules forbid.
- **Impact of applying it:** Documentation now. When each family is implemented it needs its test vectors. Document 11 said "canonical" without a definition; it is now assigned `canonical-json/2`, the form the acceptance journal already implements.
- **Owner note:** "We recommend a suggestion to make sure the system is scalable and flexible."
- **Applied in `c71e33b`:** Document 08 has a hash codec registry with six entries (`canonical-json/2`, `sorted-compact-json/1`, `jcs-rfc8785/1`, `v1_lenprefixed_sha256`, legacy `task-event/1`, `raw-bytes/1`) and five rules. Default for new JSON records is `canonical-json/2` because it keeps integer amounts and ordinals as exact tokens. Documents 05, 10, 11 and 15 point at it.

### A-06

**Messaging has a data model and no named runtime owner**

Priority: Before building the area · Size: S · State: Fixed in documents

- **One document:** Document 15 owns conversations, messages and recipient state, and excludes "transport/wake engines". *(organization-data/README boundaries table; 15-messaging-and-chat.md)*
- **Another document:** ADR-0003 assigns storage and routing to the office (office first, then through HQ). The ADR's status is still Proposed, with sections kept as compatibility evidence. *(adr/0003-agent-messaging-office-first.md)*
- **Why it matters:** The component that stores and routes chat is only decided in a proposed ADR. The live Phase A spool cutover depends on it.
- **Which source wins:** ADR-0003 for runtime rules, merged into organization data; organization data for storage
- **Recommendation at the time of the finding:** Accept ADR-0003's routing decision (or replace it), then add one line to document 15 naming the office worker as the single owning writer for its conversations.
- **What we gain:** The messaging contract is complete in one place: what the records are and who stores and routes them. The ADR keeps the detailed v1 algorithms as evidence, so nothing proven is thrown away.
- **What we lose:** The ADR's SQL storage design stops being the source and becomes a projection. Its latency targets (same-office send p50 under 15 ms) assumed one batched database commit; one protected file per message needs its own flush discipline, so those targets must be re-measured or restated. There are now two documents to keep consistent, with a stated tie-break.
- **Impact of applying it:** Documentation now. Opens one design question for the messaging adapter (envelope batching, tracked as E-02). The live Phase A spool is untouched.
- **Owner note:** "I think the ADR3 is right, but it should also be merged with the the organization data. How do they play together? I think we have to merge them. And what are the trade-offs?"
- **Applied in `7a552bf`:** How they play together: document 15 owns the data and where it is stored; ADR-0003 owns how messages move. Document 15 now has a "Runtime ownership and routing" section with eleven rules taken from the ADR (office-first commit, routing through HQ, the placing office as single writer of recipient state, hand-off by organization position, single-owner failure notices, deadlines, idempotent re-ask). ADR-0003 is marked accepted for exactly those rules (its section 2.2 and sections 3.1 to 3.4). The rest of the ADR, including the app token, the old CLI surface, urgent interruption and the spool, stays compatibility evidence. Tie-break: document 15 wins wherever the two differ. The merge exposed one new open question, recorded as A-13.

### A-07

**Human takeover: browser attaches to the agent terminal, or local CLI only**

Priority: Before building the area · Size: S · State: Fixed in documents

- **One document:** "The app attaches to the office's PTY" using pty-host frames of at most 64 KiB. *(agent-sessions.md line 414)*
- **Another document:** Takeover is a "local CLI operation until a separate browser-terminal contract is approved". *(agent-sessions.md line 24; standalone-server.md section 5)*
- **Why it matters:** A browser terminal is a large security surface. The body of the design still describes it as the mechanism.
- **Which source wins:** The newer header (local CLI only)
- **Recommendation at the time of the finding:** Mark the PTY-attach section as post-v1 and move it under a clearly labelled future heading.
- **What we gain:** v1 ships without a browser terminal, which is a large attack surface and a separate contract.
- **What we lose:** No in-browser takeover in v1; a person takes over an agent from a local terminal.
- **Impact of applying it:** Move the PTY-attach section of the session design under a post-v1 heading. No code exists for it.
- **Owner note:** "there is not browser terminal that is just read only, but I will send you two documents on how you that i visualized, the agent interaction is via chat"
- **Applied in `e2ad9ec`:** Recorded as ADR-0004: the office drives every agent headless, people interact through chat, the browser shows a session read-only, and there is no terminal in the browser. Your two documents are in the repository as specs. REQ-05 is the session view: a console decoupled from the agent's process, built by parsing the agent tool's conversation files (Claude and Codex), with toggles for tool calls and file-based visualizations, starting with a proof of concept. REQ-04 is questions from an agent to a person: one `ask_user` schema rendered as choice, yes/no or free-text components, built with shadcn-svelte and Bits UI (both already in the UI) in the product's own chat. No external channels and no React libraries, as you said. A default value is preselected; it is handed to the agent with the timeout of a non-required ask and never answers a required one. How long an agent waits is its own record, ADR-0020: an ask waits 10 minutes by default; a required ask left unanswered stops the agent after telling it, and the answer starts it again in the same session.

### A-08

**Agent credentials: the user's own vendor sign-in, or office-local bindings**

Priority: Before building the area · Size: S · State: Decided; code pending

- **One document:** Claude runs with "the user's real `~/.claude`", isolated by three switches. *(agent-sessions.md line 145)*
- **Another document:** Each office holds a private credential binding per provider connection; credentials never leave the office and are referenced, not shared. *(11-model-profiles-and-agent-execution.md sections 3, 4; 14)*
- **Why it matters:** The first makes the agent share a person's sign-in and quota. The second is the selected model and is what pools, permits and cost attribution assume.
- **Which source wins:** Organization data documents 11 and 14
- **Recommendation at the time of the finding:** Document 11 wins. Rewrite the session design's decision to: an office binding may explicitly point at a sign-in directory, and that is one binding type among several.
- **What we gain:** Quota, permits and cost attribution work, because they all assume one connection per subscription with an office-local binding.
- **What we lose:** The zero-setup path where an agent just uses whoever is signed in. It becomes one explicit binding type.
- **Impact of applying it:** Rewrite decision 6 of the session design. The three isolation switches it verified stay valid as the adapter's launch rules.
- **Owner note:** "ok yes we will use that existing setup on who is signed in but we have have the whole agent subscriptipon pool that you need to consider, so we need to be able to replace the agent ~ folder almost chrooting the agent into its own directory actually that could be a good isolation approach for non vms then we would chroot that and have a " virtual claude folder". when we would ocntrol that would go into it, which would only belong to that agent, that would also be a cheap " sandbox" lets evalute that"
- **Applied in `cb49e54`:** ADR-0015 gains three decisions. (10) Every agent has a private home that the office controls, in both isolation modes, at `durable/local/offices/<office-id>/agents/<employee-id>/home/`; the vendor folders are redirected into it and contain only what the office put there. The user's own `~/.claude`, Codex home and `~/.gemini` are never used by an office-run agent. (11) The sign-in comes from the subscription pool: the office keeps each connection's sign-in in its own binding and places a copy in the agent's home for the run. An agent can read the sign-in it runs with, which cannot be avoided, and no other. (12) Changing the filesystem root was evaluated: `chroot` needs privilege on both systems and is not workable on macOS, where tools and Xcode need the system volume; the same effect is a mount namespace on Linux and a Seatbelt profile on macOS. That is specified as the reserved `os-sandbox` mode, not v1. The private home gives the "virtual folder" now without privilege.
- **Still to do:** A spike per vendor runtime: how each is pointed at a relocated sign-in, and on macOS where the Keychain entry is looked up. Then the launch code (FC-161).

### A-09

**Create organization: the published API takes a name, provisioning needs four more fields**

Priority: Decide first · Size: M · State: Decided; code pending

- **One document:** `POST /api/v1/organizations` takes a command envelope with `{name}`. *(gateway-api-v1.md line 54)*
- **Another document:** Create needs an explicit slug, remote organization repository registration, state branch, and valid team references for bootstrap employees. *(gateway-api-v1.md line 174; create-organization.md line 97)*
- **Why it matters:** Organization creation is on the critical path (HQ bootstrap). The UI and the host operation cannot be wired to a contract that both sides know is incomplete.
- **Which source wins:** Organization data and the create-organization design
- **Recommendation at the time of the finding:** Publish the provisioning contract as the next API revision and bump what `/version` reports. Retire the name-only form before any client ships.
- **What we gain:** Unblocks the critical path: setup, host operation and the Create screen can be wired to one complete contract.
- **What we lose:** The name-only form. Nothing depends on it in production.
- **Impact of applying it:** Publish the provisioning payload as the next HTTP contract revision; update the DTOs in `org-proto` and the UI create form together. Depends on A-12 for the version number.
- **Owner note:** "whe never shopped anything os everything is in v1, if we need to fix it lets fix it, what is needed for the organization the api should support it,"
- **Applied in `cb49e54`:** The browser API design has a "Selected create-organization payload" and the create-organization design now agrees with it: name, slug, the organization repository registration (a remote URL and a reference to an office-held credential), the state branch, and the initial team when bootstrap employees are planned. The fields are the ones the create flow already required; none was invented. The name-only request is what the code accepts today, not a second supported shape.
- **Still to do:** Code: the host command and the web route accept the selected payload (FC-018).

### A-10

**Is GitHub part of v1**

Priority: Before building the area · Size: S · State: Fixed in documents

- **One document:** FC-140 "Git worktrees and GitHub integration" is one Planned v1 row. *(FEATURES.md line 296)*
- **Another document:** `org-github` is "Deferred v2 integration, not a standalone-v1 dependency". The Git design builds against a fake GitHub. *(project-structure.md line 137; projects-git-github.md)*
- **Why it matters:** Local Git and worktrees are needed for any agent to do code work. GitHub token minting and pull requests are a much larger piece.
- **Which source wins:** The project structure (GitHub is v2)
- **Recommendation at the time of the finding:** Split FC-140 into two rows: local repositories, caches and worktrees (v1), and GitHub App tokens and pull requests (v2 or explicitly gated).
- **What we gain:** v1 scope gets a reachable target: local repositories, caches and worktrees are enough for agents to do code work.
- **What we lose:** No pull requests or token minting from the product in v1; a person pushes or opens the pull request.
- **Impact of applying it:** Split FC-140 into two checklist rows and update the area summary counts.
- **Owner note:** "github and other remote git integrations is v2"
- **Applied in `817c4c2`:** Hosting-provider integrations (the provider app, short-lived tokens, creating repositories through a provider, pull requests) are v2. Checklist row FC-140 is now local Git only and the new row FC-163 holds the deferred integrations. One reading to confirm: plain Git over a configured remote, with the office's own credential, stays in v1, because the organization's files must be pushed somewhere to be backed up and shared with satellites.

### A-11

**Git design algorithms still use the old two-clone, one-repository-per-project shape**

Priority: Before building the area · Size: M · State: Fixed in documents

- **One document:** Hardening algorithms keep `mirror.git` and `work.git` under a project folder as "legacy adapter roles". *(projects-git-github.md lines 19 to 21, 97)*
- **Another document:** One bare cache per office per repository; many repositories per project; worktrees under the employee path. *(organization-data/README runtime tree; 04-agent-structure.md runtime location)*
- **Why it matters:** The design flags this itself, but the algorithms an implementer would follow are the old ones. Deny rules derived from the old paths do not transfer.
- **Which source wins:** Organization data (one cache per office per repository)
- **Recommendation at the time of the finding:** Rewrite the algorithm sections against the selected paths before `org-git` is started. Re-derive the credential and other-task denies for the new layout.
- **What we gain:** The Git adapter is designed once against the real paths, with deny rules that match them.
- **What we lose:** The hardening analysis done for the two-clone shape has to be re-derived, not copied.
- **Impact of applying it:** Rewrite the algorithm sections of the Git design before `org-git` starts. No code exists yet, so no rework.
- **Owner note:** "doe it"
- **Applied in `817c4c2`:** The Git design was rewritten against the selected layout: one bare cache per repository per office, many repositories per project and per task, one worktree per employee, task and repository. The old two-clone shape is gone and the guarantees that depended on it are listed under "Known limits". The rewrite found eleven gaps in the contracts (for example no record for office Git credentials, and no durable home for a branch's base commit); they are listed as open questions in the design.
- **Still to do:** The eleven open questions at the end of the Git design need answers before FC-140 is built. Two are already settled (ULIDs; unnumbered tasks go away if ADR-0019 is accepted).

### A-12

**Which local API version the first standalone release speaks**

Priority: Cleanup · Size: S · State: Fixed in documents

- **One document:** "`API_VERSION` stays 1: everything is additive." *(tasks-projects-docs.md line 273)*
- **Another document:** "The first standalone release uses local API_VERSION 2." Code declares version 1 and accepts 1 and 2. *(processes.md line 233; standalone-server-removal-inventory.md line 56; org-proto/src/lib.rs)*
- **Why it matters:** Low risk today because nothing is deployed, but removing `member.update_agent.home` is tied to the version number.
- **Which source wins:** The standalone decision (version 2)
- **Recommendation at the time of the finding:** Record one decision: version 2 for the first standalone release, with the list of calls removed at that boundary.
- **What we gain:** One boundary at which removed calls (managed-home overrides, `app_token`, per-kind work commands, area commands) all go.
- **What we lose:** Compatibility with version 1 clients, of which none are deployed.
- **Impact of applying it:** Record the decision and the list of calls removed at that boundary. The code already accepts 1 and 2.
- **Owner note:** "v1 there is not v1 clients, this was never deployed so we can break anything that is needed under v1, I will let you know once v1 is shipped until then there are no brekaing changes"
- **Applied in `17c3964`:** Recorded as a rule in the organization-data index, "No compatibility obligations before v1 ships", and in the documentation authority map: until you declare v1 shipped there are no versions to keep side by side, no migrations and no legacy shapes to preserve; the local API version is 1. ADR-0010 (legacy task adoption) is withdrawn.

### A-13

**A message file has one owning writer, but each recipient's state belongs to that recipient's office**

Priority: Before building the area · Size: M · State: Fixed in documents

- **One document:** "Save a whole envelope atomically under a single owning writer before acknowledging it." The message envelope embeds `recipients[]`. *(15-messaging-and-chat.md storage and durability)*
- **Another document:** The origin office writes message content. The office where each recipient is placed is the single writer of that recipient's state. *(15-messaging-and-chat.md runtime ownership and routing; adr/0003 sections 3.1 to 3.2)*
- **Why it matters:** A message to recipients in two offices has three writers for one file. Envelope revisions cannot order their changes.
- **Which source wins:** Undecided; needs a layout choice in organization data
- **Recommendation at the time of the finding:** Store recipient state per recipient (one small file per message and recipient, owned by the recipient's office), keeping the message envelope immutable after send. Until decided, cross-office recipient state follows the ADR's ordering rule, which document 15 now states.
- **What we gain:** Each office can save its own recipient state durably without rewriting a file another office owns, which is what office-first routing needs.
- **What we lose:** The simplicity of one self-contained file per message. Recipient state becomes separate files, or the envelope needs a defined merge rule.
- **Impact of applying it:** A source-layout decision in document 15 before the messaging adapter is built. Until then the ADR's ordering rule applies to cross-office recipient state. Found by the independent review of the A-06 merge.
- **Owner note:** "sure as long its still fast"
- **Applied in `cb49e54`:** Every messaging file now has exactly one writing office. A message file is written once by the sender's office. Everything that belongs to one employee (the state of each received message, answers, read markers, preferences, reactions) is in that employee's own `participants/<employee-id>/` folder, written only by the office that owns that employee's state. A read, done or answer writes one small local file, so it stays in the fast class and nobody waits for another office. A DM is created by the sender's office with its first message, so starting a conversation never waits for HQ. A pin belongs to the conversation file. One new open point is recorded: two disconnected offices could each create a DM between the same two people.

## B. Design against code

The selected design and the source on v1-integration disagree.

### B-01

**Storage: files are canonical in the design, databases are authoritative in the code**

Priority: Decide first · Size: L · State: Decided; code pending

- **Design says:** "Shared SQLite is the indexed projection." Domain records are YAML and Markdown in organization Git; the office database can be deleted and rebuilt. *(organization-data/README line 80; 16-recovery-and-office-continuity.md loss boundaries)*
- **Code does:** Each office has two authoritative SQLCipher databases, `org.db` (schema 3) and `office.db` (schema 8). Only 3 of the 75 draft table names exist in code (organizations, projects, teams). There is no Git code and no reader for the canonical domain files. *(crates/org-office/src/office.rs line 76; crates/org-office/src/store.rs; docs/design/organization-data/schema.sql)*
- **Why it matters:** This is the largest gap because it changes the base under working code instead of adding to it. Every existing command's persistence, the ID format, numbering, membership and task shapes all move. Recovery, federation, usage and budgets are all specified against the target layout, so none can be built correctly on the current one.
- **Which source wins:** Organization data, by an approved migration
- **Recommendation at the time of the finding:** Decide the approach before more domain work lands: (1) settle the disk layout (A-01), (2) pick one record family to move end to end as the pattern (the acceptance journal for task assignment already does this), (3) add a projection-rebuild test so "the cache is disposable" is proven, (4) authorize the migration, which the data contract currently says it does not do.
- **What we gain:** The properties the whole design is specified on: caches are disposable, organization data is reviewable in Git, recovery and federation have defined sources.
- **What we lose:** Speed while it happens. It touches 49k lines of working office code. One file per record costs more per write than a batched database commit. Until it is finished there are two shapes to keep straight.
- **Impact of applying it:** Largest item on the list. Do it per record family, not all at once: layout (done in A-01), then tasks through the existing acceptance journal, then organization records, then messaging. Add a rebuild-the-projection test per family. Needs your explicit go-ahead because the data contract says it authorizes no migration.
- **Owner note:** "if we need to rebuild we should do it, but the experience needs to be fast and memory efficient, and reliable, if we need to resign a few things we should do it now, whatever the better architecture is. The better and more elegant architecture always wins so keep that in mind,"
- **Applied in `11464ce`:** Direction recorded: rebuild the store against the selected contract instead of migrating the legacy tables. With nothing deployed there is no data to carry over, so the staged migration I first proposed is unnecessary. ADR-0018 says to do it together with the engine-independence work, since both touch the same code once.
- **Still to do:** Code: this is the largest piece of v1 work. It needs its own plan (which records first, in what order) before the agents start.

### B-02

**Single home team is selected; the code is many-to-many**

Priority: Decide first · Size: M · State: Decided; code pending

- **Design says:** Each employee has exactly one `home_team_id`. A project has one owner team and optional contributors. *(01-organization-structure.md; 04-agent-structure.md)*
- **Code does:** `memberships` and `project_teams` tables, with `team.add_member` and `project.add_team` on the wire. The checklist records the conflict. *(crates/org-proto/src/tests.rs lines 730, 743; FEATURES.md FC-028)*
- **Why it matters:** Lead authority, accountable ownership and the runtime folder path (`teams/…/agents/<employee-id>`) all assume one home team. Screens built on the current wire will need rework.
- **Which source wins:** Organization data
- **Recommendation at the time of the finding:** Change the model and wire contract together under the new API version. Nothing is deployed, so no data migration is needed.
- **What we gain:** Lead authority, accountable ownership and the runtime folder path all become well-defined.
- **What we lose:** An agent can no longer sit in two teams. Cross-team work is expressed through project contributors and assignment.
- **Impact of applying it:** Change the organization model, the wire contract and the UI together at the API version boundary. No data migration, since nothing is deployed.
- **Owner note:** "do it"
- **Applied in `47e1596`:** Checklist row FC-028 now states the requirement: single home team is the rule and the many-to-many membership code is removed, not migrated.
- **Still to do:** Code: remove multi-team membership from the office.

### B-03

**One typed task is selected; the code has per-kind work items**

Priority: Decide first · Size: L · State: Decided; code pending

- **Design says:** One task entity with `type: task | bug | request`, `T-` numbers scoped by project or team, an independent assignment pair, immutable reporter. *(03-features-and-work.md; 12-task-assignment-and-history.md)*
- **Code does:** `work_items` with `task.*` and `bug.*` command families, ULID identifiers, and per-kind number scopes such as `requests` and `project:<id>:task`. *(crates/org-work; crates/org-office/src/office/tests/instance.rs line 663; tasks-projects-docs.md section 3.2 mapping table)*
- **Why it matters:** Work is the product's core object. Assignment, history, numbering and every work screen sit on this shape.
- **Which source wins:** Organization data
- **Recommendation at the time of the finding:** Finish the selected-task path that the acceptance journal started, then retire the per-kind commands at the API version boundary.
- **What we gain:** One work object for the UI, the CLI and agents; one numbering scheme; history that survives type changes.
- **What we lose:** Separate bug and request command families and their per-kind numbers. Old `B-` and `REQ-` references need an explicit mapping.
- **Impact of applying it:** Finish the selected-task path on the acceptance journal, then retire `bug.*` at the API version boundary. Part of B-01's first family.
- **Owner note:** "again as mentioned there is not old thing, everything can be deleted this is not live yet, apply this for anything else like this"
- **Applied in `47e1596`:** Checklist row FC-040 now says the separate bug and request entities are replaced by typed tasks outright, with no import mapping. The same rule was applied wherever it came up: ADR-0010 is withdrawn and the legacy import row FC-143 is deferred.
- **Still to do:** Code: replace the per-kind work items. A follow-up removes the legacy-import tables from the draft schema.

### B-04

**New record IDs: UUIDv7 in the contract, ULID in the code**

Priority: Before building the area · Size: S · State: Decided; code pending

- **Design says:** "New records use UUIDv7; preserve existing v1 IDs exactly." *(organization-data/README shared identity contract)*
- **Code does:** The code mints ULIDs and uses the ULID timestamp for receipt retention. *(crates/org-proto/src/lib.rs line 108; crates/org-work/src/lib.rs line 104; 16 line 217)*
- **Why it matters:** Either works, but file names, sort order and the 30-day receipt rule depend on which one is used. Nothing is deployed, so there are no existing IDs to preserve yet.
- **Which source wins:** Your choice; I recommend keeping ULIDs and changing the contract
- **Recommendation at the time of the finding:** Choose once. If UUIDv7, switch minting before the first install and restate the receipt-retention rule without "operation-ULID time". If ULID, change the contract.
- **What we gain:** Keeping ULIDs costs nothing: the code, tests and the 30-day receipt rule already use them, and they sort by time just as UUIDv7 does.
- **What we lose:** The standard UUID format, which some external tools expect. Switching later would mean two ID formats forever, since IDs are never rewritten.
- **Impact of applying it:** Either a one-line contract change (keep ULID) or a minting change plus a restated receipt rule (switch). Decide before the first real install.
- **Owner note:** "sure lets do it"
- **Applied in `3bbc57f`:** Your decision: use whatever is the industry standard. That is UUIDv7 (RFC 9562), so every ID the product mints is a UUIDv7, written as lowercase hyphenated text. This reverses the ULID choice I had first recommended on a premise that was only partly true. The identity contract states the rule and the reason; example IDs in the documents are UUID-shaped; SQLite stores them as text and a database with a native UUID type uses that.
- **Still to do:** Code: most of it mints ULIDs today and changes with the store rebuild; the newer HQ-creation path already mints UUIDv7 (FC-142).

### B-05

**Areas are deferred in the design and live in the code**

Priority: Before building the area · Size: S · State: Decided; code pending

- **Design says:** FC-056 "Areas and advanced checklist execution": Deferred, not selected. *(FEATURES.md line 137)*
- **Code does:** `area.create` and `area.rename` are served, an `areas` table exists, and the bug-drafting feature attaches `area` context. *(crates/org-proto/src/tests.rs lines 2038 to 2039; smart-features.md line 102)*
- **Why it matters:** Deferred-but-present features get built on by accident.
- **Which source wins:** Organization data (areas are deferred)
- **Recommendation at the time of the finding:** Decide: remove the area commands at the API version boundary, or mark them as retained legacy with no new callers. Update the bug-drafting schema to use accountable team.
- **What we gain:** Nothing new gets built on a deferred concept.
- **What we lose:** The area field on bugs, which the bug-drafting feature currently uses for context.
- **Impact of applying it:** Remove `area.*` at the API version boundary and switch the drafting schema to accountable team.
- **Owner note:** "lets remove it"
- **Applied in `47e1596`:** Checklist row FC-056 and the smart-features design now say the `area.*` calls and tables are removed and a task's accountable team supplies that context.
- **Still to do:** Code: delete the area calls, tables and UI.

### B-06

**Launch protocol: the host writes v2, the worker implements v3, three ADRs need v3**

Priority: Decide first · Size: M · State: Decided; code pending

- **Design says:** "Launch v3 and satellite create remain separate pending contracts." *(office-host-boundary.md line 166)*
- **Code does:** `crates/org-office-launch/src/v3.rs` exists and the worker handles it. Nothing in `org-host` uses it. ADR-0007, 0011 and 0012 depend on v3. *(crates/org-office-launch/src/v3.rs; apps/agentic-org-office/src/launch.rs line 80)*
- **Why it matters:** The browser gateway handoff, grants, HQ creation with escrow and channel transfer all ride on v3. Until the host drives it, none is reachable.
- **Which source wins:** The ADRs (launch v3 is the contract)
- **Recommendation at the time of the finding:** Make host-side v3 the next host increment and update the boundary design to say v3 is the contract.
- **What we gain:** Unlocks the browser gateway, per-person grants, HQ creation with escrow and channel transfer in one host increment.
- **What we lose:** Launch v2 simplicity. Both must be handled until v2 is removed.
- **Impact of applying it:** Host-side code change in `org-host`; update the host boundary design to say v3 is current. Prerequisite for B-07 and B-08.
- **Owner note:** "why v3 we didn't ship v1,  just fix it"
- **Applied in `cb49e54`:** The host boundary design opens with the requirement: there is one launch contract, the verified-rendezvous design, and its version restarts at 1. A correction: the host already implements the rendezvous form and uses it for a registered, protected instance; it falls back to the older first-line form otherwise.
- **Still to do:** Code: remove the first-line form and the switch between the two, in host and worker.

### B-07

**Human authorization: grants are designed, the app token is still what is checked**

Priority: Before building the area · Size: M · State: Decided; code pending

- **Design says:** Every gateway request carries a human principal and a host-issued grant. *(ADR-0006 revision 5 amendment; ADR-0012)*
- **Code does:** The launch line still carries `app_token`. The worker receives the grant key and stores it unused: `LaunchAuth::Grants { _key }`. *(crates/org-office-launch/src/lib.rs line 541; apps/agentic-org-office/src/launch.rs line 94)*
- **Why it matters:** A browser request cannot be authorized per person until grants are verified by the worker.
- **Which source wins:** The ADRs (principal plus grant)
- **Recommendation at the time of the finding:** Implement grant verification with the v3 host work (B-06) and remove `app_token` at the same boundary.
- **What we gain:** Browser requests are authorized per person, which sign-in needs.
- **What we lose:** The single shared app token, which was simpler and only suited one desktop app.
- **Impact of applying it:** Worker verifies grants; remove `app_token`. Rides on B-06.
- **Owner note:** "do it"
- **Applied in `11464ce`:** Same table in the host boundary design: every request for a person carries that person's principal and a host-issued grant that the worker verifies; `app_token` and the `office.app_token` call are removed.
- **Still to do:** Code: worker verifies grants; remove the app token. Rides on B-06.

### B-08

**Gateway channel limit: host allows 63, worker rejects the eighth**

Priority: Cleanup · Size: S · State: Fixed in documents

- **Design says:** Eight channels per office, seven for the gateway. *(adr/0012-office-channel-lifecycle.md)*
- **Code does:** "Actual host Endpoint::open allows 63 gateway, worker rejects eighth." Recorded as a defect in the ADR. *(adr/0012-office-channel-lifecycle.md line 18)*
- **Why it matters:** Harmless until the gateway runs, then it becomes confusing refusals under load.
- **Which source wins:** ADR-0012
- **Recommendation at the time of the finding:** Set the host limit to seven when the host-side v3 work lands.
- **What we gain:** Host and worker agree, so capacity refusals are predictable.
- **What we lose:** Nothing.
- **Impact of applying it:** One constant in the host, changed with B-06.
- **Owner note:** "sure lets fix it"
- **Applied in `cb49e54`:** Nothing to change. The host already refuses the eighth web channel: its channel registry enforces seven and a test asserts it. The figure of 63 was an observation at an older commit, quoted in ADR-0012, which now carries a note. My finding was out of date.

### B-09

**v1 targets macOS and Linux; workers only start on macOS**

Priority: Before building the area · Size: S to decide, L to build · State: Decided; code pending

- **Design says:** FC-011 "macOS/Linux installation and named instances". *(FEATURES.md line 71)*
- **Code does:** Non-macOS spawn returns "office servers run only on macOS". The Linux key provider is undecided and the Landlock and seccomp adapter is absent, so the gateway fails closed. *(crates/org-host/src/spawn.rs line 252; ADR-0002 decision 2; ADR-0008)*
- **Why it matters:** Linux is a different amount of work from a port: key custody, process admission and confinement all need a design.
- **Which source wins:** Your choice; I recommend macOS first and Linux as a named follow-up
- **Recommendation at the time of the finding:** Decide whether Linux ships in the first release or the one after, and record it on FC-011.
- **What we gain:** A first release that is actually verified on one platform, with Linux's key custody and confinement designed properly instead of rushed.
- **What we lose:** Linux servers in the first release, which matters if you intend to host on a Linux machine.
- **Impact of applying it:** Update FC-011 to say which. Linux needs a credential provider decision, process admission and a Landlock and seccomp adapter.
- **Owner note:** "we need both, I need to be able to host the office inside a docker container we need to abstract what we need property"
- **Applied in `11464ce`:** Recorded as ADR-0017 and checklist row FC-164. macOS and Linux are both v1, and a Linux instance may run as the main process of a container. The differences sit behind five small contracts, each with a macOS and a Linux implementation: process admission, credential provider, confinement of the web process, service lifecycle, and agent isolation. No platform fails open: a missing adapter refuses to start and says why.
- **Still to do:** Three short designs before code: Linux process admission, the Linux credential provider and the Linux confinement adapter. The ADR says plainly that Linux admission is weaker than macOS code signing against a local attacker.

### B-10

**Calls defined in the contract and not served by the office**

Priority: Before building the area · Size: M · State: Decided; code pending

- **Design says:** `org-proto` defines `work.get`, `work.list`, `work.snapshot`, `work.activity`, and `msg.ask`, `msg.reply`, `msg.defer`, `msg.done`, `msg.threads`. *(crates/org-proto/src/work.rs; crates/org-proto/src/msg.rs)*
- **Code does:** The office dispatch serves work writes and `msg.send | inbox | read | wait` only. *(apps/agentic-org-office/src/serve.rs)*
- **Why it matters:** The UI needs reads before any work screen can be real. A contract that is wider than the server hides this.
- **Which source wins:** The contract; serve the calls
- **Recommendation at the time of the finding:** List unserved calls in the crate README, and serve the work reads as part of the first real work screen.
- **What we gain:** Work screens can show real data. The contract stops overstating the server.
- **What we lose:** Nothing.
- **Impact of applying it:** Serve the work reads with the first real work screen; list unserved calls in the crate README meanwhile.
- **Owner note:** "sure"
- **Applied in `11464ce`:** Same table in the host boundary design: the host serves the work reads the web needs through the office channel.
- **Still to do:** Code: the host socket answers only `hello` and `instance.status` today.

### B-11

**Libraries with no consumer**

Priority: Cleanup · Size: S · State: Fixed in documents

- **Design says:** Internal foundations are labelled and are not counted as features; each increment proves its smallest production path early. *(docs/independent-delivery.md)*
- **Code does:** `org-process`, `org-copilot` and `org-release` have no dependents. The host database module is marked dead code outside tests. *(Cargo.toml files; crates/org-host/src/hostdb)*
- **Why it matters:** About 12k lines that compile and test green without being reachable. They age against contracts that are still moving.
- **Which source wins:** The independent-delivery rule
- **Recommendation at the time of the finding:** For each: name the increment that will consume it, or park it explicitly. `org-release` is consumed by setup; `org-process` is tied to a deferred engine.
- **What we gain:** Each unused library has a named consumer or an explicit parked status, so it stops drifting unnoticed.
- **What we lose:** Nothing; no code is deleted.
- **Impact of applying it:** Three short notes. `org-release` is consumed by setup; `org-copilot` needs a runtime adapter; `org-process` is tied to a deferred engine.
- **Owner note:** "evalute their porpuse and see what needs implementnation and what can be paked and the things that shoudl be marked should be highligated on what their propurpose is and when should the work there continued"
- **Applied in `11464ce`:** The project structure design has a new table, "Libraries that are built but not yet reached", with purpose, whether it is reached today, the decision and when work continues. Continue: `org-process` (now needed for process resolution), `org-release` (setup and update), `org-pki` (HQ creation), `org-runtime` and `org-supervise` (running agents), the host database (stored sessions). Parked: `org-copilot` (until a vendor adapter exists) and `org-sync` (until the store reads the selected files). Review at cutover: `org-spool`.

### B-12

**Setup is described in the present tense and refuses in the code**

Priority: Cleanup · Size: S · State: Fixed in documents

- **Design says:** Sections of the standalone design describe setup journals, release pins and launchd registration as how setup works. *(standalone-server.md sections 2 and 4)*
- **Code does:** `setup` returns "HQ setup is unavailable … no setup state was changed". *(apps/agentic-org/src/lib.rs line 37)*
- **Why it matters:** A reader cannot tell target from built without cross-checking `executable-migration.md`.
- **Which source wins:** The code (it refuses), reflected in the design
- **Recommendation at the time of the finding:** Add a one-line status marker at the top of each affected section, in the same vocabulary the feature checklist uses.
- **What we gain:** Readers can tell target from built without cross-checking a second file.
- **What we lose:** Nothing.
- **Impact of applying it:** One status line per affected section of the standalone design.
- **Owner note:** "sure"
- **Applied in `11464ce`:** The standalone design's release and setup sections now open with an implementation status line saying what exists and what refuses.

### B-13

**The storage engine should stay replaceable; three files and some contract wording tie it to SQLite**

Priority: Before building the area · Size: M · State: Decided; code pending

- **Design says:** Storage adapters implement ports owned by the domain crates; the office store stays private. Volatile infrastructure sits behind an abstraction. *(project-structure.md section 3; architecture-rules.md dependency inversion)*
- **Code does:** SQL is confined to `org-office` and `org-host`, and 23 of 26 SQL-using files in `org-office` are in `store/`. Three are outside it: `messaging.rs`, `organization/bootstrap.rs`, `organization/admission.rs`. Contracts name `BEGIN IMMEDIATE`, and encryption is tied to SQLCipher. *(crates/org-office/src; 06-identity-and-numbering.md; 11 section 6; ADR-0002)*
- **Why it matters:** The owner wants to be able to move to PostgreSQL later without a re-architecture, and without over-building for it now. The code is nearly there; these are the places it leaks.
- **Which source wins:** The architecture rules (infrastructure behind ports)
- **Recommendation at the time of the finding:** Three small steps and no PostgreSQL work. Move the SQL in the three files behind the store module. Word the invariant in the contracts as "one accepting writer per stream; sequence allocation, deduplication and outbox commit atomically", keeping SQLite mechanics as implementation notes. Keep encryption a property of the adapter, not of the domain. A fourth step is already a requirement: under ADR-0014 decision 8 each operation declares its acknowledgement class in the domain crate and the store adapter alone honours it.
- **What we gain:** A later move to another database is a second implementation of the store, which is already split by area behind small interfaces (about 14,700 lines in all), with domain rules, wire contract and UI untouched. The invariant is stated in terms that hold for any engine.
- **What we lose:** A small refactor now for a move that may never happen. Nothing is built for PostgreSQL.
- **Impact of applying it:** Three files in `org-office`, and wording in documents 06 and 11. Best done alongside B-01, which touches the same code. One step is already a requirement: ADR-0014 decision 8 makes the acknowledgement class part of the storage contract. Raised by the owner's review of the overview.
- **Owner note:** "we will need to support. postgresl we need to be able to abstract what we need there. so your reocmmendation + extran thigns might be needed"
- **Applied in `cb49e54`:** Recorded as ADR-0018 and checklist row FC-165. Only store adapters know an engine. Invariants are stated without one (one accepting writer per stream, not one database connection). The store is chosen at one seam at start-up. Every storage contract gets an engine-independent test suite; replication and messaging already have one, and a new engine is accepted only when it passes them all. The extra things beyond my first recommendation: portable value types, constraints the SQLite schema enforces with triggers also listed in words, and backup and file layout treated as adapter properties. Documents 06 and 11 no longer name a database statement.
- **Still to do:** Code: move two files behind the store, add the seam, write three suites (work, organization, host). No PostgreSQL adapter is built now.

## C. Organization data improvements

Gaps and inconsistencies inside docs/design/organization-data itself.

### C-01

**Draft SQL allows an empty number on records only HQ creates**

Priority: Before building the area · Size: S · State: Fixed in documents

- **Contract says:** Only locally created tasks and features, or unresolved imports, may have `number: null`. *(06-identity-and-numbering.md; organization-data/README)*
- **But:** `teams`, `projects`, `releases` and `milestones` all declare `number INTEGER CHECK (number > 0)` with no `NOT NULL`. *(schema.sql table definitions)*
- **Why it matters:** The schema is meant to enforce the contract. As written, a team without a number is valid.
- **Which source wins:** The contract text; fix the SQL
- **Recommendation at the time of the finding:** Add `NOT NULL` on those four, or add an explicit import-staging state column and constrain null to it. Add a case to `test_schema.py`.
- **What we gain:** The schema enforces the rule it documents.
- **What we lose:** The ability to stage an HQ-created record without a number. If import staging needs that, it gets an explicit state.
- **Impact of applying it:** Four `NOT NULL` constraints in `schema.sql` and one test case.
- **Owner note:** "do it"
- **Applied in `17c3964`:** Teams, projects, releases and milestones now require a positive number in the draft schema, with a test.

### C-02

**Task, release and milestone status have no published vocabulary**

Priority: Before building the area · Size: S · State: Fixed in documents

- **Contract says:** "Status values represent data"; no transition engine is selected. Feature status is guarded: `in_progress` and `live` need an owner. *(03-features-and-work.md; 08-record-contracts.md)*
- **But:** `status TEXT NOT NULL` with no allowed set for tasks, releases and milestones. Deployments, teams and documents do have enumerated values. *(schema.sql)*
- **Why it matters:** Without a transition engine you still need one list of valid values, or the UI, the CLI and each agent will invent their own. Budget holds already refer to an "in-progress task" becoming blocked.
- **Which source wins:** New decision in organization data
- **Recommendation at the time of the finding:** Publish the value list per record type in document 03 (values only, no transitions) and add CHECK constraints or a lookup table.
- **What we gain:** The UI, the CLI and every agent use the same status words. Budget holds and reports can rely on them.
- **What we lose:** Free-form statuses. Adding a value becomes a contract change.
- **Impact of applying it:** A value table in document 03 and CHECK constraints. No engine is implied.
- **Owner note:** "we will need to mark stasks and release and milestones as done, the releation and milestones could be the agregation of their assigned tasks, it dont not need to have an ACTUAL status but if all the tasks under the milestone are done, then the milestone is done. And then if all milestones under the release is done, then the release is done. And then if there's no milestones, which is also possible, then all the tasks on the release is done, then the release is done. But we just yes, but we do need a first class citizen field to indicate when the task is done whatever that is. It could be the status or it could be another field. If the status then I think there would be need a num. So to keep things flexible, we can just have markers done and then honestly, let's just simplify this and create any numb for the test status on the things that we we we know what we need to do like normal inum for the statuses those are supposed to be well known at this point let's create the statuses the enum the last value in the enum is done if it's there that's it"
- **Applied in `5b92fb4`:** Task status is a closed list: `todo`, `in_progress`, `blocked`, `cancelled`, `done`. Milestones and releases have no status column. Two views compute completion as you described: a milestone is done when all its tasks are finished; a release is done when all its milestones are done, or, with no milestones, when all its tasks are finished. As you confirmed, an empty milestone is not done, and a cancelled task counts as done. The views also return task and finished counts for progress bars. Tests cover each case.

### C-03

**Employee state is spread over three fields and two documents**

Priority: Before building the area · Size: S · State: Fixed in documents

- **Contract says:** Lifecycle is `active | inactive`. Pause and hold are separate operational states owned by execution and budgets. *(08-record-contracts.md; 11; 17)*
- **But:** The employees table also keeps `agent_status IN ('active','paused','offboarded')` and `offboarded_at` as legacy fields. *(schema.sql lines 559 to 562; 04-agent-structure.md field reconciliation)*
- **Why it matters:** "Is this agent allowed to run" currently needs four inputs: employment status, legacy agent status, a pause command and budget holds.
- **Which source wins:** New decision in organization data
- **Recommendation at the time of the finding:** Add one short state table to document 04: employment (active, inactive) × runtime (running, paused) × holds, with the single rule for runnable. Drop `agent_status` from the target or define its one-way mapping.
- **What we gain:** One rule for "may this agent run", instead of four inputs read in different places.
- **What we lose:** The legacy `agent_status` field as a second source.
- **Impact of applying it:** A short state table in document 04; drop or map `agent_status` in the draft schema.
- **Owner note:** "it could also be a enum but yes"
- **Applied in `17c3964`:** Employee state is one field with a closed list: `active`, `paused`, `inactive`. Only agents can be paused, and an offboarded employee must be inactive.

### C-04

**The owner is represented three different ways**

Priority: Before building the area · Size: S · State: Fixed in documents

- **Contract says:** Shared metadata records the owner as `kind: owner` with no employee ID. The owner account is a host security principal. *(05-operational-data.md relationships; documentation-authority.md)*
- **But:** `employees.is_owner` remains as a marker that "grants nothing". Private notes key on the authenticated subject ID instead. *(schema.sql line 558; 04-agent-structure.md; 05 private notes)*
- **Why it matters:** A column named `is_owner` that grants nothing is an authorization bug waiting for someone to trust it.
- **Which source wins:** The security model (owner is a host principal)
- **Recommendation at the time of the finding:** Remove `is_owner` from the target schema or rename it `legacy_owner_marker`. Add one paragraph that says how an owner principal maps to an optional person record.
- **What we gain:** Removes a column that looks like an authorization grant and is not one.
- **What we lose:** Nothing functional.
- **Impact of applying it:** Remove or rename `is_owner` in the draft schema; one explanatory paragraph.
- **Owner note:** "do it"
- **Applied in `17c3964`:** The owner marker on the employee record is removed. Who the owner is comes from the organization's authority record only.

### C-05

**The set depends on an Instance and never defines one**

Priority: Before building the area · Size: S · State: Fixed in documents

- **Contract says:** Assignment fingerprints include `instance_id`, and creating an organization needs "instance-administrator authority". *(12-task-assignment-and-history.md; create-organization.md)*
- **But:** No document in the set defines instance, account or administrator records. They live in the host database. *(organization-data/README boundaries table; crates/org-host/src/hostdb/schema.rs)*
- **Why it matters:** The boundary between host-owned identity and organization-owned data is the main trust boundary, and it is only implied.
- **Which source wins:** New text in organization data
- **Recommendation at the time of the finding:** Add a short "outside this set" section to the README: instance, accounts, sessions and the owner principal belong to the host, with the one field by which organization data refers to each.
- **What we gain:** The host-owned and organization-owned boundary is written down, which is the main trust boundary.
- **What we lose:** Nothing.
- **Impact of applying it:** One "outside this set" section in the README.
- **Owner note:** "do it"
- **Applied in `17c3964`:** The index has a new section, "Outside this set: what the host owns", defining the instance, accounts, sessions and the office registry.

### C-06

**The README storage tree is incomplete**

Priority: Cleanup · Size: S · State: Fixed in documents

- **Contract says:** The shared storage tree lists `state`, `accounting`, `authority`, `journal`, `local`, `checkpoints`. *(organization-data/README lines 62 to 76)*
- **But:** The private-notes database lives at `durable/private/task-notes.db`. Messaging, recovery and budgets live under a root the tree does not show. *(05-operational-data.md line 551; 15; 16; 17)*
- **Why it matters:** The README is the entry point. A backup built from its tree would miss chat, budgets, receipts and private notes.
- **Which source wins:** Organization data
- **Recommendation at the time of the finding:** Resolve with A-01: one complete tree, each directory tagged source or cache, and tagged with what a backup must include.
- **What we gain:** A backup built from the README tree is complete.
- **What we lose:** Nothing.
- **Impact of applying it:** Closed by A-01.
- **Applied in `cd36075`:** Closed with A-01: the README tree now shows `messaging/`, `recovery/`, `budgets/` and `private/task-notes.db`, each marked as a source.

### C-07

**Diagrams that disagree with their own DDL**

Priority: Cleanup · Size: S · State: Fixed in documents

- **Contract says:** Task events: `occurred_at` "required". Deployments: `environment` and `target` marked as foreign keys. *(05-operational-data.md line 407; 07-release-deployments.md lines 21 to 22)*
- **But:** `occurred_at TEXT` is nullable (for adopted legacy events). `environment` and `target` are free text. *(05-operational-data.md line 428; schema.sql line 1064; 07 line 47)*
- **Why it matters:** Small, but the diagrams are what people read first.
- **Which source wins:** The DDL
- **Recommendation at the time of the finding:** Fix both labels. For deployments, decide whether environment names should be a per-project list; free text will drift (prod, production, Production).
- **What we gain:** Diagrams match the tables.
- **What we lose:** Nothing. Optionally a decision on whether environments become a per-project list.
- **Impact of applying it:** Two diagram label fixes.
- **Owner note:** "do it"
- **Applied in `17c3964`:** Diagrams corrected, and the test that compares every diagram with its table now covers them.

### C-08

**Documents 11 and 14 define the same records**

Priority: Cleanup · Size: S · State: Fixed in documents

- **Contract says:** Document 11 is the "data and execution specification" and says "Implement the decisions here". *(11-model-profiles-and-agent-execution.md line 3)*
- **But:** Document 14 is "the data-model view of the selected execution contract, using the same records and SQL". *(14-model-providers-and-connection-pools.md line 3)*
- **Why it matters:** Two places to edit one table. They agree today. Document 13 shows the fix already applied once: it is now a redirect.
- **Which source wins:** Document 11 keeps behaviour, document 14 keeps the data view
- **Recommendation at the time of the finding:** Keep the DDL in one of them and have the other link to it.
- **What we gain:** One place to edit each table.
- **What we lose:** Nothing.
- **Impact of applying it:** Move or link the duplicated DDL.
- **Owner note:** "do it"
- **Applied in `17c3964`:** Document 14 no longer repeats the SQL; it points to document 11, which owns those records.

### C-09

**Status labels and revisions differ across the set**

Priority: Cleanup · Size: S · State: Fixed in documents

- **Contract says:** The README says the set is at revision 2.8 and is a "structural draft" that authorizes no migration. *(organization-data/README line 3)*
- **But:** Documents sit at 2.6, 2.7 and 2.8; document 12 is revision 4, 11 is revision 6, 14 is revision 1, 17 is revision 2. Document 11 says implement; 17 calls itself a data and behavior specification. *(line 3 of each numbered document)*
- **Why it matters:** A reader cannot tell which documents are settled enough to build from.
- **Which source wins:** New convention
- **Recommendation at the time of the finding:** Use one status vocabulary on every document (draft, selected, implemented) and a set-level changelog in the README.
- **What we gain:** A reader can tell which documents are settled enough to build from.
- **What we lose:** Nothing.
- **Impact of applying it:** One status line convention on 17 documents and a changelog in the README.
- **Owner note:** "do it"
- **Applied in `17c3964`:** Every numbered document has the same status line, and the index has one status vocabulary and a changelog. The set is at revision 2.10.

### C-10

**The usage ledger has no named writer**

Priority: Before building the area · Size: S · State: Fixed in documents

- **Contract says:** Events append to per-producer JSONL with a checkpoint before acknowledgement. The collection protocol must satisfy the contract before it ships. *(10-usage-and-cost-accounting.md durable storage)*
- **But:** Only document 11's dispatch step places capture in the office supervisor path, and only in passing. *(11-model-profiles-and-agent-execution.md section 6)*
- **Why it matters:** The ledger gates every cost and budget feature (FC-084). It needs the same single-writer statement the other durable sources have.
- **Which source wins:** New text in organization data
- **Recommendation at the time of the finding:** State in document 10: the office worker that ran the turn is the producer and the only appender for its stream.
- **What we gain:** The ledger gets the same single-writer statement as every other durable source.
- **What we lose:** Nothing.
- **Impact of applying it:** One sentence in document 10.
- **Owner note:** "do it"
- **Applied in `17c3964`:** Document 10 names the writer of the usage ledger.

### C-11

**No traceability from the draft schema to the code**

Priority: Before building the area · Size: S · State: Fixed in documents

- **Contract says:** The draft has 75 tables and 8 views, with executable checks in `test_schema.py`. *(organization-data/SCHEMA-COVERAGE.md)*
- **But:** The code has its own in-code schemas keyed on `user_version`. Three table names overlap. *(crates/org-office/src/office.rs; crates/org-office/src/store.rs)*
- **Why it matters:** There is no artefact that says which draft tables are implemented, renamed, or still missing, so drift is invisible.
- **Which source wins:** New practice
- **Recommendation at the time of the finding:** Add a status column to SCHEMA-COVERAGE (absent, legacy shape, implemented) and update it in the same change as each storage increment.
- **What we gain:** Drift between the draft schema and the code becomes visible, and B-01 gets a progress measure.
- **What we lose:** A small upkeep cost on each storage increment.
- **Impact of applying it:** One status column in SCHEMA-COVERAGE.
- **Owner note:** "do it"
- **Applied in `17c3964`:** Schema coverage has an implementation status table: of 75 tables and 10 views, 16 exist in the code in an older shape and 69 are absent.

### C-12

**Offline offices use a pending number; the rule for agents is not written down**

Priority: Before building the area · Size: S · State: Decided; code pending

- **Contract says:** Any office creates tasks and features offline with `number: null`. "An office must not independently increment its replicated counter." *(06-identity-and-numbering.md line 36; organization-data/README)*
- **But:** The owner asked whether remote offices get number allocations to work offline, and how a model avoids losing a reference when a placeholder is replaced. The specs show "Pending number" and a copyable ID for people; nothing states what agent tools must accept and print. *(work-items-ux.md WORK-8; tasks-projects-docs.md line 86)*
- **Why it matters:** There is no placeholder to replace, which is the safe design, but an agent given only a readable number later, or only an ID earlier, must be able to resolve both.
- **Which source wins:** Organization data (pending numbers), plus one new rule
- **Recommendation at the time of the finding:** Keep pending numbers for v1 and do not hand out number blocks. Add one rule to document 06: agent tools accept the permanent ID at all times, show it until a number exists, and show both afterwards. Number blocks per office stay a recorded option for later.
- **What we gain:** Numbering stays in one place with nothing to reconcile, and an agent can never hold a reference that stops resolving.
- **What we lose:** Readable numbers while offline. People and agents use the long permanent ID until HQ is reachable.
- **Impact of applying it:** One rule in document 06 and matching behaviour in `agent-os` when typed tasks are built. Raised by the owner's review of the overview.
- **Owner note:** "the problem problem I have with this is if an agent is creating a new task in a remote office it needs to align with that hq first, which will be slow, we if we implement a task convention per office that would go away, the problem is agents will reference the task I key or project key whatever, right? Projects are fine to be centralized at HQ level. But tasks aren't right because we want them to be incredibly fast so the problem  it's how long that scene could take for creating or updating or managing tasks. I don't want to go to HQ, that's too long of a round trip. So we need to think it's smarter and be efficient. we might just add the office index in front of the task id, all tasks start with T-{OFFICEINDEX}{TASKID}, so the first task would immediately start with T-11, and on the second office it would be T-21, etc, this is incredibly problematic issue. I need you to once you get to this point to do some extra analysis on this and how to get this properly implemented. Use the sub-agent for this.   For Everything the agents need to do a lot on a day-to-day, they should be VERY fast, things that they don't happen very often, like creating milestones, creating releases, creating you know projects that can wait for HQ sync, but tasks and things that happen that can happen inline and offline, that has to happen incredibly fast."
- **Applied in `5b92fb4`:** Decided and written into the data contract. A task reference is `T-`, the creating office's letter and a number: HQ's first task is `T-A1`, the second office's thirty-seventh is `T-B37`. Each office counts all its tasks in one sequence, as you settled ("by office, keep it simple"), so a reference is unique in the organization on its own and never needs a project in front. The office numbers the task itself, so creating a task never waits for HQ and a task never shows "Pending number". Creating a task uses the fast acknowledgement, as you decided. Tasks only: features and everything else keep their HQ numbers. Commands that need a project take it from the agent's context and require it otherwise. The draft schema has the office codes and counters, with tests.
- **Still to do:** Code (FC-142): office codes and counters, the reference parser in `agent-os`, the default project.

### C-13

**Task history is published to Git as one file per change**

Priority: Before building the area · Size: S to keep, L to change · State: Fixed in documents

- **Contract says:** Each accepted task revision has an immutable event file at `work-history/tasks/<task-id>/<revision>.json`, published with the task. *(05-operational-data.md task activity history)*
- **But:** The owner asked whether history belongs in Git at all. Every comment, checklist tick and reassignment becomes a file and a Git object. The same events must exist locally anyway, in the journal. *(Owner review of the architecture overview, 2026-11-17)*
- **Why it matters:** History is what answers who did what and when after a task closes, and what lets an office database be rebuilt. The question is where it lives and in what shape, not whether it exists.
- **Which source wins:** Organization data as specified, with a revisit trigger
- **Recommendation at the time of the finding:** Keep it as specified for v1: the acceptance journal already writes this shape, and changing it costs code. Set a trigger to revisit (for example repository size or file count per task). The fallback is one append-only file per task, or keeping events only under `durable/` and publishing a summary on close.
- **What we gain:** Keeping it: no rework of the acceptance journal, and history survives loss of the machine up to the last publish.
- **What we lose:** Keeping it: many small files in the organization repository over time. Changing it: the per-revision identity that the journal, hashes and conflict detection are built on.
- **Impact of applying it:** No change now. Record the trigger in document 05. Raised by the owner's review of the overview.
- **Owner note:** "fine"
- **Applied in `17c3964`:** Kept as designed, with a written trigger to revisit: more than 100,000 history files, or more than 1,000 for one task.

### C-14

**Process and standard inheritance is stored but not applied in v1**

Priority: Before building the area · Size: M · State: Fixed in documents

- **Contract says:** Content, scope and the override lock are selected. "Deferred resolution: organization → accountable team's ancestor chain → optional project is a candidate resolution order." The process engine is out of scope. *(processes.md decisions 3 to 4; organization-data/README decisions table)*
- **But:** The owner describes it as working behaviour: each team has its own processes and standards that inherit from the organization unless the organization forbids overriding. `org-process` holds earlier resolve and render rules with no consumer. *(Owner review of the architecture overview, 2026-11-17; crates/org-process)*
- **Why it matters:** Without resolution, a team's standards are files nobody is shown. The owner treats this as a core primitive.
- **Which source wins:** Owner intent; needs a scope decision in organization data
- **Recommendation at the time of the finding:** Select a narrow resolution for v1 and nothing more: a read-only effective set per task (organization, then the accountable team's parent chain, then the project, honouring the lock), shown in the UI and readable through `agent-os`. Keep enforcement, approvals, automatic delivery into sessions and checklist composition deferred. Decide that contributing teams do not take part.
- **What we gain:** Team and project standards become visible and usable, which is the reason to store them per scope. The existing `org-process` rules get a consumer.
- **What we lose:** A little v1 scope. It also forces the open question of whether contributing teams take part in resolution.
- **Impact of applying it:** A selected read-only resolution rule in documents 01 and processes.md, one office query, one UI view and one `agent-os` read. Enforcement stays deferred. Raised by the owner's review of the overview.
- **Owner note:** "It's easy. We have organization and then we have teams and we have projects. That's pretty much what we need support. We have process standards at organization level and process standards at the team level and process standards at the project level. And then each one of the high the top hierarchy can indicate if the downstream process could be overwritten by something else. That's basically how it works. Then we should implement this. From an overriding perspective, that's something that it's a little bit for the agents to decide. We can say this is a non-overridable project process, you always have to comply towards it and then that's be pr everything that's not overrideable it has to be highlighted in the Asian system prompts or something like that. Of course we also have to decide on the system prompts as how to use the tools and so on. That has to be provided out of the box or side of skills or MCP. I don't know.. Maybe just a set of skills. But the ones that are not overridable has to be clearly educated so the agents would follow those processes. And same thing for standards, process standards they behave very similar from that perspective."
- **Applied in `11464ce`:** Resolution is selected for v1 in the processes design, decision 4. Three levels: organization, team, project. A lower level may add a document or replace one from above with the same key, unless the upper one is marked not overridable. Every non-overridable document is always given to the agent, in a marked section of its system instructions that says it must be followed. Overridable ones are guidance. Guidance for the product's own commands ships as built-in skills. Standards behave the same way. Because an employee has one home team and a task one accountable team there is one chain, so the earlier rules for conflicts between several team chains are not needed. Checklist row FC-112 moved from Deferred to Planned.
- **Still to do:** Not selected, and stated as such: mechanically blocking an action that breaks a process, and approval workflows. Code: simplify `org-process` to the one-chain rule and call it at agent launch.

## D. Stale documentation

Text that was true once and now points people at the wrong name, owner or status.

### D-01

**Old executable names in live design text**

Priority: Cleanup · Size: S · State: Fixed in documents

- **Text says:** Agents push with `org-agent push`. `org-server` is the composition root. *(projects-git-github.md lines 103, 150; adr/0003 throughout)*
- **Now:** The executables are `agent-os` and `agentic-org-office`. *(executable-migration.md; AGENTS.md)*
- **Why it matters:** The installed live tool is still called `org-agent`, so this is easy to get wrong in both directions.
- **Which source wins:** Current names
- **Recommendation at the time of the finding:** Sweep the designs. Keep `org-agent` only where the text means the installed Phase A tool, and say so.
- **What we gain:** Designs name the executables that exist.
- **What we lose:** Nothing.
- **Impact of applying it:** Text sweep. Keep `org-agent` only where it means the installed Phase A tool.
- **Owner note:** "There is no such thing called org agent, it's org or is agent OS for everything. Start doing the replacements on the on the names. Just get it replaced. There's no more let's remove the old names all like everywhere."
- **Applied in `e2ad9ec`:** Replaced across the design documents, ADRs and specs: `org-agent` is `agent-os` and `org-server` is `agentic-org-office`. As you decided, nothing from before is carried over: the procedure for carrying the former development team's message queue into the product is withdrawn, its checklist row is closed, and AGENTS.md and the agent guidance no longer protect the earlier tool or its queue. AGENTS.md now states the goal instead: onboard the two engineers and their agents onto the product as early as it can carry them. Research notes and dated verification records were left as written.

### D-02

**Named owners and approval gates that no longer exist**

Priority: Cleanup · Size: S · State: Fixed in documents

- **Text says:** "Owner: ada", steps that need a named person's go, and merges into `v1`. *(processes.md line 3; projects-git-github.md; standalone-server.md section 8; FEATURES.md review line)*
- **Now:** One lead owns everything; merge to `v1-integration` after local review; no external approval. *(AGENTS.md current working mode)*
- **Why it matters:** An agent that reads the design before AGENTS.md will wait for an approval that cannot come.
- **Which source wins:** AGENTS.md
- **Recommendation at the time of the finding:** Replace owner lines with "lead" and delete approval gates, in one documentation change.
- **What we gain:** No agent waits for an approval that cannot come.
- **What we lose:** Nothing.
- **Impact of applying it:** Text sweep of owner lines and approval gates.
- **Owner note:** "Do it."
- **Applied in `e840ea3`:** Named owners and approval steps that no longer exist were replaced by "lead" or removed.

### D-03

**Crates described as absent that exist**

Priority: Cleanup · Size: S · State: Fixed in documents

- **Text says:** `org-sync`, `org-sync-proto` and `org-pki` "are absent on the inspected branch". `org-pki` is listed under planned components. *(standalone-server-removal-inventory.md line 227; project-structure.md line 135; adr/0007 line 15)*
- **Now:** All three are workspace members with source and tests. *(Cargo.toml)*
- **Why it matters:** Someone will plan to write a crate that is already there.
- **Which source wins:** The code
- **Recommendation at the time of the finding:** Update the three lists. In ADR-0007, keep the observation as dated evidence and add the current state.
- **What we gain:** Nobody plans to write a crate that exists.
- **What we lose:** Nothing.
- **Impact of applying it:** Three list updates.
- **Owner note:** "Do it."
- **Applied in `11464ce`:** The three places that said `org-sync`, `org-sync-proto` or `org-pki` were absent now say they exist.

### D-04

**ADR-0004 is referenced and does not exist**

Priority: Cleanup · Size: S · State: Fixed in documents

- **Text says:** The session design replaces "the unmerged ADR-0004 draft" and plans to rewrite it. *(agent-sessions.md lines 56, 799)*
- **Now:** `docs/adr` goes from 0003 to 0005. *(docs/adr directory)*
- **Why it matters:** The headless-first execution decision has no ADR of its own.
- **Which source wins:** The session design's decision
- **Recommendation at the time of the finding:** Write ADR-0004 as the headless-first decision, or add a stub that points to the section of the session design that holds it.
- **What we gain:** The headless-first execution decision has an ADR number that resolves.
- **What we lose:** Nothing.
- **Impact of applying it:** One new short ADR or a stub.
- **Owner note:** "Do it."
- **Applied in `11464ce`:** ADR-0004 now exists and records the decision that replaced the unmerged draft: headless sessions, chat, no browser terminal.

### D-05

**The implemented-entry-points record lags the branch**

Priority: Cleanup · Size: S · State: Fixed in documents

- **Text says:** The F0 baseline table says the office has not imported the writer, bootstrap, adoption or escrow additions. *(executable-migration.md F0 integration baseline)*
- **Now:** The branch has since merged the durable journal, native replay, CA escrow, and HQ creation with admitted bootstrap. *(git log on v1-integration: eacb505, 0d4035d, 55c7d76)*
- **Why it matters:** This file is named by the authority map as the answer to "what is implemented".
- **Which source wins:** The branch
- **Recommendation at the time of the finding:** Update the table in the same change as each delivered unit, as the feature checklist already requires.
- **What we gain:** The file named as the answer to "what is implemented" is current.
- **What we lose:** Nothing.
- **Impact of applying it:** Update the table per delivered unit.
- **Owner note:** "Do it."
- **Applied in `11464ce`:** The executable migration record has a new section, "Where the entry points stand", listing what each merged unit added and that none opened an entry point: setup still refuses, the web process still refuses to listen, the host answers two calls.

### D-06

**Code comments that still describe the desktop app**

Priority: Cleanup · Size: S · State: Fixed in documents

- **Text says:** `org-client` is "used by the desktop app's Tauri shell and by agent-os". *(crates/org-client/src/lib.rs lines 4 to 5)*
- **Now:** The Tauri shell is removed. *(standalone-server-removal-inventory.md; project-structure.md)*
- **Why it matters:** Minor, and cheap to fix.
- **Which source wins:** The code
- **Recommendation at the time of the finding:** Update the crate documentation.
- **What we gain:** Accurate crate documentation.
- **What we lose:** Nothing.
- **Impact of applying it:** Two lines.
- **Owner note:** "replace them"
- **Applied in `47e1596`:** The stale comment in `org-client` is corrected. Other mentions of the desktop app in the code are tests and guards that check the old shell is not imported, which are correct as they are.

### D-07

**The web process is still called the gateway in designs and code**

Priority: Cleanup · Size: S for prose, M for code · State: Decided; code pending

- **Text says:** `gateway-api-v1.md`, "Gateway" throughout ADR-0008 and ADR-0012, `ChannelKind::Gateway`, the `org-proto` `gateway` module. The word appears in 27 documents and 53 source files. *(docs/; crates/org-proto/src/gateway)*
- **Now:** The executable is `agentic-org-web`. The owner asked to say "web" to avoid confusion with a model gateway, which does not exist. *(Owner review of the architecture overview, 2026-11-17)*
- **Why it matters:** A reader new to the project assumes a gateway sits in front of the models.
- **Which source wins:** Current executable name
- **Recommendation at the time of the finding:** Rename in prose across the designs now, and add one glossary line where the file name `gateway-api-v1.md` is kept. Rename code identifiers at the API version boundary, since some are on the wire.
- **What we gain:** Nobody assumes a gateway in front of the models.
- **What we lose:** Continuity with ADR text and file names that say gateway; mitigated by one glossary line.
- **Impact of applying it:** Prose sweep across 27 documents now; identifier rename in 53 source files at the API version boundary (A-12). The three architecture pages already say web. Raised by the owner's review of the overview.
- **Owner note:** "replace them too , no gateway anymore, everything that was gateway in that situation it becomes web. Not sure if there's gateway elsewhere, but whenever gateway was mentioned on that manner, it's now web."
- **Applied in `e840ea3`:** Replaced in prose across the documents; the browser API design is renamed `web-api-v1.md` and nine headings and their links were updated. Where a document quotes an identifier that exists in the code, it still shows the code's spelling.
- **Still to do:** Code: 83 source files still use `gateway` in module, type and function names (for example `org_proto::gateway`). Renaming them is a mechanical change for the agents; I left the code alone while they are paused.

## E. ADR decisions the data contract did not carry over

The organization data was written after the ADRs. These are ADR decisions it replaces without restating, found on a second pass.

### E-01

**Encryption at rest: decided for databases, unstated for the files that replace them, and questioned by the owner**

Priority: Decide first · Size: S to decide, M to apply · State: Decided; code pending

- **ADR says:** "Organizations will hold confidential information, so the server's database must be encrypted at rest." Every reader and writer keys SQLCipher with a host-held key. *(adr/0002-storage-sqlite-sqlcipher-one-writer.md context and decisions 1 to 2)*
- **Organization data says:** Sources move to files under `durable/`. Only the private-notes database is stated as SQLCipher. Document 15 asks for "encrypted storage … according to the existing storage boundary" without defining it; documents 10, 16 and 17 say "protected". *(05-operational-data.md lines 551, 563; 15-messaging-and-chat.md line 15; 10; 16; 17)*
- **Why it matters:** Today chat bodies and receipts sit in an encrypted database. Moved to files as specified, they would lose encryption at rest without anyone deciding that. The owner also asked whether encryption is needed at all, given the server is like any database host. It protects two things disk encryption does not: agents run as the same OS user as the data, and copies of the data directory leave the machine. It costs signed binaries, an undecided Linux key store, and data loss if the key is lost.
- **Which source wins:** Your decision. I lean to dropping application-level encryption when sources move to files
- **Recommendation at the time of the finding:** Decided: drop application-level encryption of data and rely on disk encryption, file permissions and the agent isolation mode. Private keys are the exception: they stay in the office's store, each encrypted with one host-held key per office.
- **What we gain:** If dropped: no key custody to lose, no signed-binary requirement for storage, Linux unblocked, files a person can inspect and repair. If extended instead: chat, receipts and budgets stay unreadable to a same-user process and to anyone holding a copy.
- **What we lose:** If dropped: a second line of defence against an agent tool that escapes its sandbox, and protection for copies and backups of the data directory. If extended: throughput, simplicity, and recoverability when the key is lost.
- **Impact of applying it:** Follows from F-03. If agents run as a separate user, drop application-level encryption and state that protection is disk encryption plus file permissions. If they stay same-user, decide between encrypting every protected source and accepting the risk. Either way the storage contract gets one explicit at-rest rule before B-01 moves any protected record family.
- **Owner note:** "Owner, 2026-11-17: skip application-level encryption and remove that complexity."
- **Applied in `b59b4e3`:** Decision recorded as ADR-0016 and checklist row FC-162; implementation pending. Organization data is not encrypted by the application; disk encryption, file permissions and the isolation mode protect it. Secrets stay in protected custody. An office holds private keys: the organization CA key today, and each office's TLS identity key and the invitation key once satellites are built. You chose where they go: each office's private keys stay in the office's own store, encrypted with one key per office that the host holds and hands over at launch. The host never receives the private keys, the reviewed HQ-creation flow is unchanged, and a missing key no longer stops an office opening its data (commit 4ca8890). The cost is that the per-office key and its handling stay, in a smaller role.
- **Still to do:** Code: remove SQLCipher and wrap the office's private keys (FC-162).

### E-02

**Messaging latency targets assume a batched database commit; the data contract stores one file per message**

Priority: Before building the area · Size: M · State: Fixed in documents

- **ADR says:** Same-office send p50 under 15 ms and p95 under 30 ms, cross-office ask and reply p50 under 60 ms, achieved by sharing one full-flush commit across concurrent senders. *(adr/0003-agent-messaging-office-first.md section 5, lines 852 to 865)*
- **Organization data says:** "Save a whole envelope atomically under a single owning writer before acknowledging it", one JSON file per message under `durable/messaging/`. *(15-messaging-and-chat.md storage and durability)*
- **Why it matters:** One durable flush per message file under a 16-agent burst will not meet targets that were designed around one shared flush.
- **Which source wins:** ADR-0003's targets, re-measured against the new storage
- **Recommendation at the time of the finding:** Design the envelope flush discipline before the messaging adapter is built: either batch several envelopes behind one directory flush, or journal first and materialize files after, as the acceptance journal does. Measure, then keep or restate the targets.
- **What we gain:** Messaging stays fast enough for many agents after the move to protected files.
- **What we lose:** Either some durability granularity (batching several envelopes behind one flush) or the current latency targets.
- **Impact of applying it:** Design the envelope flush discipline and measure it before the messaging adapter is built. Recorded as an open item in document 15.
- **Owner note:** "We need to be faster than this. I want a single digit millisecond for messages sending out. Unless if course is an ask, which can take a little bit longer, especially for talking in between offices and then that could take even a little bit longer so cross office is fine intra office should be incredibly fast single digit millesecond, we might want to relay the message first and start it later. Like so you know especially interoffice, we're talking about interoffice, which is pretty much where the team is going to be working on kind of interoffice should be single digit if it's fine forget it should be single digit as well so we we need to architecture this properly. If we if it's interoffice we could relay the message first or or or just cache it and then save it afterwards. You know? And then if if if if we if we we we we can put a retry there also so we we should optimize for 80% of the time and then if there's a problem we just retry it also the ask would also have to have a timeout no matter what saying oh the agent didn't reply in in a in in in the particular time but we should also allow the agents to specify depending of the messages how long he would expect a reply for this, depending on the question, it would take longer. But the ask is a holding operation, right? I'm gonna ask and then sometime I'm gonna come back with the information. The agent should be able to ask on a super thread and then you know and then the ask will be waiting for the reply for the other agent. But they just send a message which is asynchronous, like it's like, you just send it noncessary waiting for the part to continue work, that should be incredibly fast. Whenever doing the just the send, it should be single digit. Ask can be more. Just the send should save it locally and then get relayed elsewhere, however that happens."
- **Applied in `cb49e54`:** ADR-0014 gains decision 9 and two acceptance criteria, and document 15 a matching paragraph. A plain send answers in single-digit milliseconds in every topology, including to another office: the sender's office saves it locally and answers; relaying through HQ happens afterwards and is retried until it succeeds or the message expires. A failure becomes a delivery notice, never a slower send. An ask is the one holding operation: it always has a deadline, the sender may say how long a reply is expected to take (otherwise 120 seconds for an agent and 24 hours for a person), and it returns a timeout when the deadline passes.
- **Still to do:** Code and measurement on a Mac (FC-160).

## F. Design choices the owner challenged

Choices that are internally consistent but that the owner questioned in review. Each has an evaluation and a recommendation.

### F-01

**Browser sessions live in memory, so every restart signs everyone out**

Priority: Before building the area · Size: M · State: Decided; code pending

- **Design says:** The web process keeps only an in-memory link between cookie and session. "A gateway restart loses mappings; require reauthentication." The host's session list is also a map in memory. *(gateway-api-v1.md section 4; crates/org-host/src/service/authentication.rs line 39)*
- **Owner asks:** Why keep sessions in memory at all? Sign tokens with a key pair generated at provisioning and verify them, so no session state is needed. *(Owner review, second round, 2026-11-17)*
- **Why it matters:** Every update or crash signs every person out. The reasons for the current design (a web process with no secrets, instant sign-out) can be kept without that cost.
- **Which source wins:** Your direction (no sign-out on restart); the mechanism is your choice
- **Recommendation at the time of the finding:** Two ways work. Stored sessions: the host keeps each session as a database row and the web process re-checks a cookie once after a restart; sign-out stays instant. Signed tokens: the host signs a short-lived token with a private key it keeps, the web process verifies with the public key, and a per-account marker in the host database makes password change and sign-out-everywhere immediate. I recommend stored sessions for v1 and signed tokens if there is ever more than one web process. In both, the key or table belongs to the host, because accounts are per instance.
- **What we gain:** People stay signed in across updates, crashes and host restarts. The web process still holds no secret.
- **What we lose:** Stored sessions: session rows on the host's disk, which the current design avoids. Signed tokens: instant sign-out, unless tokens are short-lived and a marker is kept; plus key rotation and clock handling to get right.
- **Impact of applying it:** A change to section 4 of the browser API design and to the host's session code. Best decided before sign-in (FC-014) is finished. No office or domain change.
- **Owner note:** "Owner, 2026-11-17: start with the cookie, cached in memory and reloaded from the database on restart; sign-out deletes from both. Expand to signed tokens in v2 if needed."
- **Applied in `b59b4e3`:** Decision recorded as ADR-0013 and checklist row FC-159; implementation pending. Sessions are stored at the host as digests, cached in memory by the web process and re-checked once after a restart. Sign-out deletes from the store and the cache. Sessions of the local command-line tool stay tied to their connection. The cookie and proof contract is unchanged, so API tests sign in exactly as a browser does. Signed tokens are a v2 option. The key question is moot for v1 because nothing is signed; if tokens come later, the host signs, because the web process relays sign-in to the host and has nowhere durable to keep a key.
- **Still to do:** Code: stored sessions at the host (FC-159).

### F-02

**A 15 ms send target for agent-os, against an owner target of 1 to 2 ms**

Priority: Before building the area · Size: M · State: Decided; code pending

- **Design says:** Same-office send p50 under 15 ms and p95 under 30 ms: about 2 ms process start, 0.5 ms connect, 6 to 10 ms for one full flush to disk. Every write is acknowledged only after it would survive a power cut. *(adr/0003 section 5; adr/0002 decision 4)*
- **Owner asks:** 15 ms is too long. It should be 1 to 2 ms at most; it is only a save to the local office database. *(Owner review, second round, 2026-11-17)*
- **Why it matters:** Measured here, starting any process costs about 1.1 ms and `agent-os --version` about 1.5 ms, so the budget is spent before any work. The rest of the 15 ms is waiting for the disk by rule, not slow code.
- **Which source wins:** Your target, by changing the durability rule for chosen operations
- **Recommendation at the time of the finding:** Two changes. Let chosen operations be acknowledged after a commit that survives a process crash but not a power cut, with the full flush following in the next batch; keep the strict rule for task changes, assignments and budget actions. And add a long-running `agent-os` mode, one per agent session, so a call is one exchange on an open socket. Then measure on the target Mac and replace the estimates in ADR-0003 with numbers.
- **What we gain:** Sends in well under a millisecond once a process is kept open, and about 2 to 3 ms on a Mac with a process per call. Higher throughput with many agents.
- **What we lose:** For the fast class only: a message acknowledged in the last few milliseconds before a power cut or kernel crash can be lost while the sender believes it was sent. It conflicts with the rule "acknowledge after durable evidence", so that rule needs a stated exception.
- **Impact of applying it:** An amendment to ADR-0002 decision 4 and ADR-0003 section 5 naming which operations may use the fast acknowledgement; a long-running mode in `agent-os`; measurements on the target Mac. The binary built on Linux is 1.56 MB against a 1 MB target, which should be checked on a Mac too.
- **Owner note:** "Owner, 2026-11-17: 15 ms is too long; target 1 to 2 ms. If the local office database accepted the write, that is acceptable."
- **Applied in `b59b4e3`:** Direction recorded as ADR-0014 and checklist row FC-160; implementation pending. Named messaging operations (the committed class) are acknowledged once the office's durable local store commits, with the full flush following within a bounded interval; everything else keeps the strict rule. agent-os gains a long-running session mode. Target: median 1 ms, 95th percentile 2 ms. Measured here: a database commit takes 0.013 ms without a forced flush and 0.29 ms with one; starting a process takes 1.1 ms. You then chose the scope: messaging and non-critical task activity (comments, checklist ticks, watching; a comment that opens or answers an owner question stays strict). Creation, status, assignment and everything else still wait for the drive. Because you asked that this survive a change of database, the class is declared once per operation in the domain code and honoured by the storage adapter; domain code never names a database setting (ADR-0014 decision 8, commit 4ca8890).
- **Still to do:** Code and measurement: committed class and the agent-os session mode (FC-160).

### F-03

**Agents run as the same OS user as the data; isolation is not a replaceable part of the launch contract**

Priority: Decide first · Size: S for the setting, M to L for the separate user · State: Decided; code pending

- **Design says:** "OS-user sharing is not a security isolation boundary … stronger isolation needs separate users/containers." Agents rely on the vendor runtime's tool sandbox and the office's permission policy. `SpawnSpec` has program, arguments, working directory and environment only. *(standalone-server.md line 299; agent-sessions.md security; crates/org-supervise/src/spawn.rs line 12)*
- **Owner asks:** Run agents as a different user at least, if that needs no drastic re-architecture. Keep the runtime extensible: a separate user, a lightweight sandbox, or strong isolation. Not everything can be a container, because some agents build iOS features. *(Owner review, second round, 2026-11-17)*
- **Why it matters:** This is the root of the encryption question. Application-level encryption exists mainly because same-user agents are not stopped by file permissions. Separate the agent from the data and that reason goes away.
- **Which source wins:** Your direction, staged
- **Recommendation at the time of the finding:** Make isolation a named, replaceable setting in the launch contract, chosen per agent, with four levels: tool sandbox only, OS sandbox as the same user, a dedicated OS user, and a container or VM. Target the dedicated user for v1, after a short spike on a Mac confirming the vendor sign-in and Xcode work as a second account. It needs one administrator step at install and a small privileged launcher, because the office cannot start or signal another user's process. Keep tool-sandbox-only as a per-agent fallback for iOS work.
- **What we gain:** Agents cannot read the organization's data even if a tool escapes its own sandbox. Application-level encryption becomes unnecessary, which removes key custody and unblocks Linux (E-01, B-09). Isolation can differ per agent, so iOS work is not blocked.
- **What we lose:** Zero-setup installs: a separate user needs one administrator step and a privileged launcher to maintain. More to set up for agents that use Xcode or a vendor sign-in. The OS-sandbox level may not work with tools that start their own sandbox.
- **Impact of applying it:** Add an isolation field to the launch contract (`org-runtime`, `org-supervise`) and name the levels in document 11. Start, stop, cleanup and orphan recovery must work across users. Workspace ownership and the data-root permissions change. Needs a macOS spike before committing to the default.
- **Owner note:** "Owner, 2026-11-17: provision the separate user at setup, and keep supporting same-user as today. Keep the runtime's isolation replaceable."
- **Applied in `b59b4e3`:** Decision recorded as ADR-0015 and checklist row FC-161; implementation pending. Isolation is a mode in the launch contract with two v1 values: same-user (today) and dedicated-user, one account per instance, which setup provisions with one administrator step. The office's local socket must also admit that account, because today it admits only its own user. An agent is never silently downgraded. A macOS spike must pass before dedicated-user becomes the default.
- **Still to do:** A macOS spike, then the launch code (FC-161).

## Checks

Documentation changes in this plan are verified with `python3 docs/design/organization-data/test_schema.py` when the draft schema is touched and `python3 .agent-kit/runtime/kit_skills.py validate` when a hashed evidence file changes. Line numbers, names and quotations in the findings refer to `v1-integration` at `55c7d76`, the commit the analysis was made against; they show what was found, including names that have since been replaced. Where the owner's note differs from the recommendation at the time, the note and the "Applied" line are what holds.
