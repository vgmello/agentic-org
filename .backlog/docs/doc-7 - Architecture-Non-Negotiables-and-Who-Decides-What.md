---
id: doc-7
title: Architecture Non-Negotiables and Who Decides What
type: guide
created_date: '2026-11-21T00:14:00Z'
updated_date: '2026-11-21T00:14:00Z'
tags:
  - architecture
  - governance
  - non-negotiables
---
# Architecture non-negotiables, and who decides what

> [!NOTE] Review note (2026-11-21)
> Reviewed against `v1` and Vision (`doc-1`). All 18 rules remain authoritative. Governance updates: (1) `TASK-2` resolved the tension between "Works alone" and Doc 11 HQ turn-permit admission by scoping HQ turn permits to shared multi-office pools (`hq_pool`) while unshared `office_local` bindings run locally offline; (2) Owner decision D28 (2026-11-20) recorded a non-negotiable 10 exception for Xcode-extension agents (L17 / `FC-194`).

Owner direction, 2026-11-17. Engineers and their agents make architectural decisions on
their own. This page says what does not vary while they do, and how a decision is made
and recorded. It is short on purpose: everything not listed here is yours to decide.

## What we are aiming for

- **Fast.** What an agent does all day (reading its inbox, sending a message, creating
  a task or commenting on one) answers in single-digit milliseconds, with a target median of
  1 ms and 95th percentile of 2 ms for an office-local call in the long-running mode.
- **Works alone.** An office keeps working when HQ or the network is unreachable.
  *Recorded exceptions (owner, 2026-11-19):* HQ turn-permit admission for **shared
  multi-office provider pools** (`hq_pool` in
  [document 11](organization-data/doc-88%20-%20Organization-Data-11-—-Model-Profiles-and-Agent-Execution.md)) and
  HQ-centralized refresh for single-use rotating provider OAuth tokens in multi-office
  Codex pools ([ADR-0032](../decisions/decision-32%20-%20ADR-0032-Browser-driven-CLI-sign-in-enrollment-credential-distribution-and-permission-presets.md)
  decision 6), where a satellite continues until its current short-lived access token
  expires. Office-local / unshared provider bindings (`office_local`, such as per-office
  Claude tokens or independent per-office Codex logins) admit and sustain turns locally
  without waiting for HQ.
- **Reliable and light.** Nothing acknowledged is lost. Memory and disk use stay small.
- **Simple.** Between two designs that meet the requirement, the simpler and more
  elegant one wins. Rebuilding a part is fine when the result is cleaner; nothing has
  shipped.
- **Usable early.** The team works inside the product as soon as it can carry them.

## The non-negotiables

These hold across every increment. Each names the document that owns its detail.

### Data

1. **Files are the source. Databases are projections.** The organization's records are
   files; a database is an index that can be deleted and rebuilt from them with no
   loss. ([Data contract](organization-data/doc-72%20-%20Organization-Data-—-README.md))
2. **One storage tree.** `durable/` holds sources and is what gets backed up.
   `runtime/<office-id>/` holds caches. No other root.
3. **One writer for each thing.** Every file and every stream of changes has exactly one
   office that writes it. HQ is the organization's authority: it numbers the records
   only HQ creates, registers slugs, and decides permits and budget holds. An office
   never writes another office's state.
4. **Identity never changes.** IDs are UUIDv7. A task reference is minted by the office
   that creates the task (`T-B37`) and is unique on its own. No record is renumbered and
   no number or ID is reused. ([Identity contract](organization-data/doc-80%20-%20Organization-Data-06-—-Identity-and-Numbering.md))
5. **Every change is recorded.** A change is idempotent under its operation ID, is
   attributed to an authenticated actor, and leaves a history entry. It is acknowledged
   only once it is committed, in the class its operation declares.
   ([ADR-0014](../decisions/decision-14%20-%20ADR-0014-Acknowledgement-classes-and-the-agent-os-latency-target.md))
6. **The database is replaceable.** Only a storage adapter knows which database is in
   use. Domain code states what it needs through contracts, and every storage contract
   has tests that do not depend on the engine. ([ADR-0018](../decisions/decision-18%20-%20ADR-0018-The-storage-engine-is-replaceable-PostgreSQL-must-be-possible.md))

### Processes and boundaries

7. **Four programs, four jobs.** `agentic-org` is the person's command line and, as
   `serve`, the host: it owns accounts, sessions and secrets and starts the offices.
   `agentic-org-office` holds every domain rule and is the only writer of organization
   data. `agentic-org-web` serves the browser and holds no keys and no database.
   `agent-os` is the only way an agent reaches the product.
8. **Office first.** An agent talks only to its own office. Nothing an agent does
   routinely waits for HQ, another office or the network.
   ([ADR-0003](../decisions/decision-3%20-%20ADR-0003-Agent-messaging-is-office-first.md))
9. **The office authorizes every call.** A person's request carries their principal and
   a grant from the host. An agent's request carries the binding of its run. Being
   admitted to a socket is never authorization. Authority comes from the caller's
   place in the organization. Two records carry it, a team's lead appointment and a
   lead's permission to hire, and each is given only by someone already above
   the holder. No other field on a record grants authority, and nobody grants any to
   themselves. Only someone who may hire can hire; there is no approval step. Any
   active employee changes tasks. Work-instruction precedence (owner, then
   `reports_to_employee_id` chain, then ancestor team leads) is defined in
   [document 04](organization-data/doc-78%20-%20Organization-Data-04-—-Agent-Structure.md#who-an-agent-takes-instructions-from)
   and never grants extra office call authority.
   ([ADR-0029](../decisions/decision-29%20-%20ADR-0029-Only-someone-who-may-hire-can-hire-there-is-no-approval.md),
   [ADR-0031](../decisions/decision-31%20-%20ADR-0031-Any-employee-changes-tasks-a-private-note-is-read-by-its-author-and-by-people.md))
10. **Agents run headless, apart from the data.** The office starts each agent as a
    supervised child process with a private home. An agent never holds the
    organization's secrets, a credential that can push, or another agent's sign-in.
    ([ADR-0004](../decisions/decision-4%20-%20ADR-0004-Agent-sessions-run-headless-people-interact-through-chat.md), [ADR-0015](../decisions/decision-15%20-%20ADR-0015-Agent-isolation-is-a-selectable-mode-with-a-dedicated-OS-user-provisioned-at-setup.md))
11. **People talk to agents through chat.** There is one chat, the product's own. No
    terminal in the browser. A message never interrupts a busy turn. An agent that
    cannot continue without an answer is suspended and resumed, not left holding a turn.
    ([ADR-0020](../decisions/decision-20%20-%20ADR-0020-An-ask-waits-with-a-timeout-a-required-ask-suspends-the-agent-until-it-is-answered.md))

### Security and platforms

12. **Secrets stay out of data.** No secret is stored in Git, in an organization file or
    in the clear in a database. Private keys are wrapped by a key the host holds. Data
    itself is not encrypted by the application.
    ([ADR-0016](../decisions/decision-16%20-%20ADR-0016-No-application-level-encryption-of-organization-data.md))
13. **macOS and Linux, including a container.** Platform differences sit behind small
    contracts with an implementation for each. A missing piece refuses to start; it
    never runs unprotected. ([ADR-0017](../decisions/decision-17%20-%20ADR-0017-v1-runs-on-macOS-and-Linux-including-inside-a-container.md))

### Code

14. **Nothing legacy.** Until the owner declares v1 shipped: no migration, import,
    compatibility shim, retained protocol version or old name. Replace, do not adapt.
15. **Structure.** Logic lives in libraries and binaries stay thin. Dependencies point
    one way and there are no cycles. Domain crates own their contracts.
    ([Architecture rules](doc-8%20-%20Architecture-Rules.md), [project structure](design/doc-68%20-%20Design-v1-Project-Structure.md))
16. **The UI** is a static SvelteKit application built with shadcn-svelte and Bits UI.
    Licensed Tailwind Plus components never leave `apps/agentic-org-ui`.
17. **Tests never touch a real organization's data.**

### Speed

18. **The commands an agent uses all day are fast** (owner, 2026-11-18). Reading the
    inbox, sending a message, replying, and reading a task, creating one or commenting
    on one (the list is in section 2.11 of the command design) answer in single-digit
    milliseconds from one office-local store, with the targets
    stated above. Nothing may be added to the path of these commands that makes them
    wait: no second journal or commit point, no call to HQ, no model call, no wait
    for the drive beyond their declared class. A command that is slow by nature
    (a model call, the network, a full flush) is named as slow in the command
    design. No command holds its caller for longer than one short hold, 90 seconds
    unless changed: a longer wait returns and is continued by another call.
    ([ADR-0014](../decisions/decision-14%20-%20ADR-0014-Acknowledgement-classes-and-the-agent-os-latency-target.md),
    [agent command line](design/doc-28%20-%20Design-agent-os-Command-Line.md) section 2.11)

## Who decides what

**You decide, without asking,** anything the requirement documents do not settle or
settle in two ways: internal structure, algorithms, names, file formats inside the
contract's intent, the order of work, what to delete, how to test. When two documents
disagree, follow the [authority map](design/doc-62%20-%20Design-Documentation-Authority-Map.md); if that does
not settle it, take the reading that fits the non-negotiables and the aims above best.

**Record what you decided where the next person will look.** Change the document that
owns the topic so that it states the decision. For a decision about architecture, add a
short decision record in [`.backlog/decisions/`](../decisions): the decision, why, and what you rejected.
Update the [feature checklist](doc-3%20-%20Feature-Checklist.md) row. Do not leave the old text beside the
new: replace it.

**A non-negotiable does not vary.** If you find that one cannot be met, look again for
a design that meets it. If there truly is none, do not stop: choose the best option,
build it, and write a decision record that names the non-negotiable, says why it could
not hold, what you chose and what it costs. Tell the owner in your next report. An
exception is recorded, never silent.

**Bring to the owner** only what changes the product for the people using it: adding or
removing something a person can do, or a choice that cannot be undone later. Bring it
with a recommendation, and keep working on what does not depend on the answer.
