# Start here

For an engineer, or an engineer's agent, who is new to this repository and has to build
part of Agentic Org. It says what the product is, where each kind of requirement is
written, and how to go from "what do I build?" to "it is done".

## The product in five lines

Agentic Org is a self-hosted server that runs an organization of people and AI agents:
teams, projects, tasks, chat and spend. An owner installs one server, creates an
organization and staffs it with employees, some of them agents. People use a browser.
Agents use a command-line tool, `agent-os`. The server keeps the organization's work as
files in a Git repository and starts and supervises each agent on machines the owner
controls.

Two pages explain it with diagrams. Read them once before anything else:

- [Architecture overview](architecture/architecture-overview.html): the parts, the main
  rules and what runs today.
- [System architecture](architecture/system-architecture.html): every process and
  connection, the data, the main flows.

They explain; they do not decide. The documents below decide.

## Where each kind of requirement is written

| You want to know | Read | What you find there |
| --- | --- | --- |
| What to build next, and in what order | [Implementation plan](implementation-plan.md) | Milestones and increments, each with its outcome, its requirements and its finish line |
| Every capability and how far it has got | [Feature checklist](FEATURES.md) | One row per capability: status, evidence, and a link to its requirement |
| What a person sees and does | UX specs in [`docs/specs/`](specs/) | Screens, flows and numbered acceptance criteria such as `TSK-4` or `CHAT-12` |
| What is stored: records, fields, files, tables | Data contract in [`docs/design/organization-data/`](design/organization-data/README.md) | Seventeen documents, one combined draft schema and a test suite that checks them against each other |
| How a part is built | Designs in [`docs/design/`](design/) | Processes, protocols, storage, sessions, Git, the web API |
| Why a decision was made and how to test it | Decision records in [`docs/adr/`](adr/) | One decision each, with numbered acceptance criteria such as `ASK-3` or `ENC-2` |
| How code must be organized | [Architecture rules](architecture-rules.md), [project structure](design/project-structure.md), [Rust style guide](rust-style-guide.md) | Dependency direction, where logic lives, naming |
| Which document wins when two disagree | [Documentation authority map](design/documentation-authority.md) | The owning document for each topic |
| How to work here | [`AGENTS.md`](../AGENTS.md), [branch and merge](processes/branch-and-merge.md) | Branches, checks, reviews |
| How to run it locally | [`DEVELOPMENT.md`](../DEVELOPMENT.md), [local HQ runbook](runbooks/local-hq.md) | Build, run and test commands |

Two folders are records, not requirements: [`docs/research/`](research/) holds what was
learned about vendor tools, and [`docs/verification/`](verification/) holds dated audits
of the code. The [conflict resolution plan](design/conflict-resolution-plan.md) is the
record of the 2026-11-17 architecture review; everything it decided is already written
into the documents above, so you do not need it to build.

## From a feature to working code

1. **Pick the increment** from the [implementation plan](implementation-plan.md). It names
   the checklist rows it covers.
2. **Open each checklist row** in [FEATURES.md](FEATURES.md) and follow its link. That is
   the requirement for behaviour.
3. **Read the owning data contract** for the records the feature touches. The data
   contract decides names, fields and relationships. If a UX spec or a design shows a
   different field, the data contract wins.
4. **Read the decision records** the increment lists. Their acceptance criteria are tests
   you are expected to make pass.
5. **Read what the code does today.** The increment says where to start. The checklist
   row's evidence and [executable migration](design/executable-migration.md) say what
   exists and what is reachable.
6. **Build the smallest path a person or agent can use,** not a layer.
   [Independent delivery](independent-delivery.md) explains what counts.
7. **You are done when** the increment's finish line holds, the acceptance criteria it
   names pass, the project checks are green, and the checklist row is updated with
   evidence.

## Rules that always apply

- **Nothing has shipped.** Write no migration, import, compatibility shim, version
  negotiation for old clients or legacy handling. Replace, do not adapt.
- **The documents are the requirement.** Where the code disagrees with them, the code
  changes. Where a requirement is missing, or two documents disagree, stop and ask the
  owner; do not invent behaviour.
- **Names.** The browser-facing process is "web". The agent command line is `agent-os`.
  The office worker is `agentic-org-office`.
- **Identity.** Every ID is a UUIDv7. A task reference is minted by the office that
  creates the task, for example `T-B37`, and is complete on its own.
- **Storage.** Files are the source and the database is a projection. Paths are
  `durable/` and `runtime/<office-id>/` only.
- **The goal.** The two engineers and their agents are to work inside this product as
  early as it can carry them. Creating an organization, signing in, people and teams,
  tasks and chat come first.

## When a requirement is unclear

Say exactly which sentence, in which document, and what the two readings are. Do not
pick one and build it. A short written question to the owner costs less than a feature
built on the wrong reading.
