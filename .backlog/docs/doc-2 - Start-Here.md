---
id: doc-2
title: Start Here
type: readme
created_date: '2026-11-21T00:14:00Z'
updated_date: '2026-11-23T03:54:00Z'
tags:
  - onboarding
  - guide
  - governance
---
# Start here

> [!NOTE] Review note (2026-11-21)
> Reviewed against current `v1` and Vision (`doc-1`). Two updates flagged for cutover (`TASK-32`): (1) The non-negotiables table row still says "seventeen fixed rules" whereas `architecture-non-negotiables` now defines **18** rules (rule 18 added 2026-11-18). (2) Relative links still point to legacy `docs/` paths pending `TASK-32` link cutover. See `doc-1` (Vision) for overarching product aims.

For an engineer, or an engineer's agent, who is new to this repository and has to build
part of Agentic Org. It says what the product is, where each kind of requirement is
written, and how to go from "what do I build?" to "it is done".

## The product in five lines

Agentic Org is a self-hosted server that runs an organization of people and AI agents:
teams, projects, tasks, chat and spend. An owner installs one server, creates an
organization and staffs it with employees, some of them agents. People use a browser.
Agents use a command-line tool, `agent-os`. The server keeps the organization's shared
records as files and publishes them to a Git repository; chat, usage and budgets stay
as protected files on the server. It starts and supervises each agent on machines the
owner controls.

Two pages explain it with diagrams. Read them once before anything else:

- [Architecture overview](../../docs/architecture/architecture-overview.html): the parts, the main
  rules and what runs today.
- [System architecture](../../docs/architecture/system-architecture.html): every process and
  connection, the data, the main flows.

They explain; they do not decide. The documents below decide.

## Where each kind of requirement is written

| You want to know | Read | What you find there |
| --- | --- | --- |
| What to build next, and in what order | [Implementation plan](doc-5%20-%20v1-Implementation-Plan.md) | Milestones and increments, each with its outcome, its requirements and its finish line |
| Every capability and how far it has got | [Feature checklist](doc-3%20-%20Feature-Checklist.md) | One row per capability: status, evidence, and a link to its requirement |
| What a person sees and does | UX specs in [`.backlog/docs/specs/`](specs) | Screens, flows and numbered acceptance criteria such as `TSK-4` or `CHAT-12` |
| What an agent sees and does (AX) | [Agent Experience (AX) Spec](specs/doc-96%20-%20Spec-Agent-Experience-AX-—-agent-os-Onboarding-and-Skills.md), [agent instructions](design/doc-27%20-%20Design-Agent-Instructions-Assembly.md) and [agent command line](design/doc-28%20-%20Design-agent-os-Command-Line.md) | Discovery tiers, token budgets, Day-1 onboarding, `agent-os` CLI ergonomics, phase-gated skills, conflict recovery and numbered acceptance criteria `AX-1`–`AX-24`, `CLI-1`–`CLI-15` and `INSTR-1`–`INSTR-23` |
| What is stored: records, fields, files, tables | Data contract in [`.backlog/docs/organization-data/`](organization-data/doc-72%20-%20Organization-Data-—-README.md) | Eighteen documents, one combined draft schema and a test suite that checks them against each other |
| How a part is built | Designs in [`.backlog/docs/design/`](design) | Processes, protocols, storage, sessions, Git, the web API |
| How the instance is to run under dedicated accounts, how agents are isolated and sandboxed, where secrets live, and how extensions (containers, Xcode) plug in | [Accounts, sandbox and extensions](design/doc-61%20-%20Design-Dedicated-Accounts-Agent-Sandbox-and-Extensions.md) (proposed, owner direction 2026-11-20) | The owner's decisions of 2026-11-20, requirements `ASX-1` to `ASX-20`, the spikes that come first, and where each item is tracked |
| What the owner asked for, in the owner's words | `req-` files in [`.backlog/docs/specs/`](specs) | The owner's requirement kept as written, then how it fits the selected architecture and what is still open |
| What must never vary, and what you may decide yourself | [Architecture non-negotiables](doc-7%20-%20Architecture-Non-Negotiables-and-Who-Decides-What.md) | The aims, seventeen fixed rules, and how a decision is made and recorded |
| Why a decision was made and how to test it | Decision records in [`.backlog/decisions/`](../decisions) | One decision each, with numbered acceptance criteria such as `ASK-3` or `ENC-2` |
| How code must be organized | [Architecture rules](doc-8%20-%20Architecture-Rules.md), [project structure](design/doc-68%20-%20Design-v1-Project-Structure.md), [Rust style guide](doc-9%20-%20Rust-Style-Guide.md) | Dependency direction, where logic lives, naming |
| Which document wins when two disagree | [Documentation authority map](design/doc-62%20-%20Design-Documentation-Authority-Map.md) | The owning document for each topic |
| How to work here | [`AGENTS.md`](../../AGENTS.md), [branch and merge](processes/doc-20%20-%20Process-Branch-Review-and-Integration.md) | Branches, checks, reviews |
| How to build and test it | [`DEVELOPMENT.md`](../../DEVELOPMENT.md), [local HQ runbook](runbooks/doc-26%20-%20Runbook-Development-Host-on-Local-Machine.md) | Build and test commands, and what can be run today: `agentic-org setup` on macOS (from source builds), or a disposable foreground host |

[`.backlog/docs/research/`](research) holds what was learned about vendor tools. It informs a
design; it is not a requirement.

## From a feature to working code

1. **Pick the increment** from the [implementation plan](doc-5%20-%20v1-Implementation-Plan.md). It names
   the checklist rows it covers.
2. **Open each checklist row** in [FEATURES.md](doc-3%20-%20Feature-Checklist.md) and follow its link. That is
   the requirement for behaviour.
3. **Read the owning data contract** for the records the feature touches. The data
   contract decides names, fields and relationships. If a UX spec or a design shows a
   different field, the data contract wins.
4. **Read the decision records** the increment lists. Their acceptance criteria are tests
   you are expected to make pass.
5. **Read what the code does today.** The increment says where to start. The checklist
   row's evidence and [executable migration](design/doc-63%20-%20Design-Executables-and-Entry-Points.md) say what
   exists and what is reachable.
6. **Build the smallest path a person or agent can use,** not a layer.
   [Independent delivery](doc-6%20-%20Independent-Delivery-and-Dependency-Boundaries.md) explains what counts.
7. **You are done when** the increment's finish line holds, the acceptance criteria it
   names pass, the project checks are green, and the checklist row is updated with
   evidence.

## Rules that always apply

- **Nothing has shipped.** Write no migration, import, compatibility shim, version
  negotiation for old clients or legacy handling. Replace, do not adapt.
- **The documents are the requirement.** Where the code disagrees with them, the code
  changes.
- **You decide.** Where a requirement is missing, or two documents disagree, make the
  decision yourself inside the [architecture non-negotiables](doc-7%20-%20Architecture-Non-Negotiables-and-Who-Decides-What.md),
  and write it into the document that owns the topic.
- **Names.** The browser-facing process is "web". The agent command line is `agent-os`.
  The office worker is `agentic-org-office`.
- **Identity.** Every ID is a UUIDv7. A task reference is minted by the office that
  creates the task, for example `T-B37`, and is complete on its own.
- **Storage.** Files are the source and the database is a projection. Paths are
  `durable/` and `runtime/<office-id>/` only.
- **The goal.** The two engineers and their agents are to work inside this product as
  early as it can carry them. Milestone one is one office on a Mac: getting in, the
  organization, agents hired through HR, messages, tasks. Linux, including a
  container, is built at the same time as Track X; a second office follows.

## When a requirement is unclear or missing

Decide. Follow the [authority map](design/doc-62%20-%20Design-Documentation-Authority-Map.md) when two
documents disagree; if it does not settle the matter, take the reading that fits the
[non-negotiables](doc-7%20-%20Architecture-Non-Negotiables-and-Who-Decides-What.md) and their aims best. Then change the
owning document so that it says what you decided, and add a short decision record for
anything architectural. Do not leave the old text beside the new.

Bring to the owner only what changes the product for the people using it, with a
recommendation, and keep working on what does not depend on the answer.
