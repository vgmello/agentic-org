---
kind: spec
title: "REQ-08: the agent-os command line and core system primitives"
status: owner-requirement
revision: 1
owner: lead
created: "2026-11-18"
updated: "2026-11-18"
requested_by: owner
---

> **Owner requirement.** The text from "REQ-08" down to the line that starts "How this fits" is the owner's document, kept as written. The last section is the lead's review: what it matches, what was decided where it differs, and what needs the owner's answer. The design that merges it is the [agent command line design](../design/agent-os-command-line.md).

# REQ-08: Agent OS CLI (`agent-os`) & Core System Primitives

## Overview

Define **Agent OS (`agent-os`)** as the deterministic, **CLI-first** execution interface through which headless agents interact with the platform's domain-agnostic kernel primitives—keeping methodologies inside portable Process Packages (YAML + Skills), enforcing strict agent-first parameter and usability conventions, closing kernel runtime gaps, and reserving MCP selectively for external stateful connectors or shell-less runtime adapters.

## Core Mechanics

### 1. Architectural Boundary: CLI-First Kernel vs. Selective MCP

- **CLI-First Core Primitives (`agent-os`):** Expose all core platform primitives through the `agent-os` CLI so agents avoid upfront context-window token overhead (loading `--schema` on demand), compose and batch commands in a single shell turn, run identically across headless runners (`claude -p`, Codex), and produce plain-text command traces in Virtual Console logs.
- **Selective MCP Role:** Reserve Model Context Protocol (MCP) servers for external stateful integrations, browser/LSP intelligence, or third-party SaaS connectors granted to specific roles—plus an optional auto-generated thin MCP wrapper only when running agents in shell-less sandboxed environments.

### 2. Documented Primitives (Current Baseline — `REQ-01` to `REQ-07`)

- **Organization, Planning & Work Hierarchy (`org`, `project`, `milestone`, `release`, `task`):**
  - `org inspect`, `project create`, `project inspect` (binds active process and enforces project isolation).
  - `milestone create`, `milestone list`, `release create`.
  - `task create`, `task list`, `task transition` (drives status-based workflow handoffs).
- **Staffing, Roles & HR Provisioning (`agent`, `team`, `hr`):**
  - `team roster`, `agent inspect`.
  - `hr roles`, `hr inspect-role`, `hr hire` (first-class hiring primitive orchestrated by the active process to inspect role requirements, provision specialists, and onboard them onto the project team).
- **Process Runtime & Workflow Execution (`process`, `skill`, `block`):**
  - `process catalog`, `process inspect`, `process apply` (YAML manifest + role-bound skills).
  - `skill list`, `skill read`, `block run` (command- and status-triggered Markdown + JSON action blocks).
- **Storage, Information Architecture & Traceability (`ia`, `fs`, `link`):**
  - `ia read`, `ia register` (single-read live YAML Information Architecture index cataloging Markdown specs, diagrams, and images without `ls` loops).
  - `fs ls`, `fs read`, `fs write` (intra-project storage with eager folder materialization from Process YAML and strict cross-project isolation).
  - `link add`, `link list` (traceability links mapping DSS versions to target codebase locations for desired-state convergence).
- **Communication, Consultation & Observability (`channel`, `consult`, `runner`, `logs`, `metrics`):**
  - `channel post`, `channel ask` (structured `ask_user` payloads for `choice`, `yes_no`, and `free_text` UI components).
  - `consult ask` (intra-project specialist Q&A and cross-project agent-to-agent requests via a target project's responsible agents).
  - `runner spawn`, `logs inspect` (Claude/Codex conversation parsing with tool-call and file-visualization toggles), `metrics query` (workflow version KPIs).

### 3. Missing Kernel Primitives to Add (Runtime Gaps)

- **Codebase & Artifact Lineage (`repo`, `worktree`, `artifact`):**
  - `repo attach`, `worktree create`, `worktree cleanup` (binds code repositories to projects and isolates parallel agent runs in dedicated Git worktrees).
  - `artifact publish`, `artifact drift` (tracks typed artifact versions, directed derivation edges such as `Product DSS -> Technical DSS -> Architecture Standard`, and upstream drift detection without hardcoding methodology names into the kernel).
- **Governance, Policies & Approval Gates (`policy`, `gate`):**
  - `policy list`, `policy check` (queries inherited organization standards and project-scoped policies, validating compliance against non-negotiable `mandatory` invariants).
  - `gate open`, `gate status` (manages formal approval checkpoints backed by `channel ask` for human sign-off or autonomous policy resolution).
- **Execution Lifecycle & Event Bus (`session`, `event`):**
  - `session suspend`, `session resume` (checkpoints detached agent execution state while awaiting async `ask_user`, `consult`, `hr hire`, or `gate` callbacks and resumes cleanly on completion).
  - `event emit`, `event subscribe` (publishes domain-agnostic lifecycle events and wires declarative Process YAML subscriptions).
- **Capabilities & Unified Threading (`grant`, `thread`):**
  - `grant list`, `grant check` (enforces role-scoped CLI primitives, MCP servers, LSPs, and filesystem boundaries at the CLI boundary).
  - `thread list`, `thread reply` (unifies user channels, intra-project consultations, and cross-project agent requests onto one auditable messaging model).

### 4. Agent-First CLI Parameter & Usability Standards

- **Strict `resource verb` Grammar & Named-Only Flags:** All commands follow `agent-os <resource> <verb> [--flag <value>]` with zero positional arguments to eliminate argument-order hallucinations.
- **Non-Interactive by Default:** Commands never block on TTY prompts; actions requiring async input or approval return structured `SUSPENDED` / `GATE_REQUIRED` responses.
- **Shell-Escaping Immunity:** Content-accepting commands support `--input-file <path>`, `--stdin`, and `--json '<payload>'` alongside flat scalar flags so multiline Markdown and YAML payloads never break shell quoting.
- **Deterministic Envelope & Token-Budget Controls:** Every command emits `{ "ok": true|false, "data": ..., "meta": ..., "error": ... }` and supports `--format json|yaml|compact`, `--fields <comma-list>`, `--limit <int>`, and `--cursor <token>`.
- **Runtime Introspection & Safe Validation:** `--schema` prints exact input/output JSON Schema on demand; `--dry-run` validates permissions, policy invariants, and revision locks without mutating state.
- **Self-Healing Errors, Context Injection & Optimistic Concurrency:**
  - Failures return structured `{ "code", "message", "hint", "retry_with" }` payloads (e.g., `CONFLICT_REV`, `SCOPE_FORBIDDEN`, `POLICY_VIOLATION`) for autonomous recovery.
  - Detached runners inject `AGENT_OS_ORG`, `AGENT_OS_PROJECT`, `AGENT_OS_AGENT_ID`, and `AGENT_OS_SESSION_ID` automatically.
  - Shared-file writes accept `--if-match <rev>` (protecting concurrent updates to the live YAML Information Architecture index), and mutating commands accept `--idempotency-key <key>` for safe retries.

## User Experience & Elegance

- **Zero-Bloat Context Efficiency (CLI-First):** Agents load only the skills required for their role and inspect `--schema` on demand—eliminating thousands of tokens of upfront tool-schema overhead on every model turn.
- **Predictable, First-Try Invocation:** Uniform `resource verb --flag` syntax, named-only parameters, and `--input-file` / `--stdin` streaming eliminate shell-escaping bugs and parameter guesswork.
- **Self-Healing Execution Loops:** Structured error hints (`CONFLICT_REV`, `SCOPE_FORBIDDEN`, `POLICY_VIOLATION`) and `--dry-run` pre-flight validation enable agents to self-correct autonomously without escalating transient CLI errors to the user.
- **Seamless Observability & Replayability:** Every primitive call appears as a clean, deterministic command string in Virtual Console logs—making agent execution transparent, filterable, and trivial to reproduce locally.

## How this fits the selected architecture

Written by the lead on 2026-11-18. It adds no requirement. The [agent command line design](../design/agent-os-command-line.md) holds the detail: the grammar, the envelope, every command with the contract behind it, and what milestone one and milestone two deliver.

**Verdict.** Accepted. Sections 1 and 4 are adopted whole and become the contract every command is written against; nothing has shipped, so today's `agent-os` parsing (positional arguments, text output) is replaced, not adapted. The `msg` commands keep their names. Sections 2 and 3 are a catalogue. About two thirds of it names things the data contract already defines, and those commands are delivered with the increment that builds each thing. The rest names things that are defined in requirements this repository does not hold.

**What it matches already.**

| The requirement | Where it already stands |
| --- | --- |
| `agent-os` is the interface for agents; it runs the same under every headless runner | [Non-negotiable 7](../architecture-non-negotiables.md) and [ADR-0004](../adr/0004-headless-agent-sessions.md) |
| Command strings in the console log | [REQ-05](req-05-virtual-console-conversation-log-viewer.md): the session view is built from the runner's conversation file, where each call is one shell command |
| Never blocks on a prompt; `SUSPENDED` | [ADR-0020](../adr/0020-required-asks-suspend-and-resume.md): an ask waits a bounded time and a required ask suspends the agent |
| `--idempotency-key`, `--if-match` | [Non-negotiable 5](../architecture-non-negotiables.md): every change carries an operation ID, and every record has a revision that a writer states |
| `--dry-run` | The [office store](../design/office-store.md) decides a change before it applies it, so the decision can run alone |
| `session suspend` and `resume` | ADR-0020, where the office does both |
| `thread`: one messaging model | [Messaging](../design/organization-data/15-messaging-and-chat.md): one conversation and message model for people and agents |
| `grant`: enforced at the boundary | [Non-negotiable 9](../architecture-non-negotiables.md): the office authorizes every call |
| `worktree`, `repo` | [Git design](../design/projects-git-github.md): one worktree per employee, task and repository, created and removed by the office; agents hold no Git credential |
| `hr hire` | [The hire flow](our-org-in-the-app.md): a hire is requested, then approved by a person |

**Decisions taken where the requirement and the contracts differ.** Each is recorded in the design.

1. **The office enforces; the command line only asks.** The requirement says grants are enforced "at the CLI boundary". An agent can run any program, so a check in `agent-os` protects nothing. Every check is made by the office; `agent-os` reports the result.
2. **The injected variables are context, not identity.** `AGENT_OS_ORG`, `AGENT_OS_PROJECT`, `AGENT_OS_AGENT_ID` and `AGENT_OS_SESSION_ID` are set at launch for the agent's scripts. The office takes identity from the binding of the run and ignores what a variable says. The launch variables all use the `AGENT_OS_` prefix.
3. **One catalogue for people and agents.** `agentic-org` offers a signed-in person the same commands from the same library ([implementation plan](../implementation-plan.md), A5). So `project create` or `repo attach` exist as commands, and the office refuses them to an agent, because changing structure is the owner's.
4. **`hr hire` asks; a person approves.** It returns `gate_required` with the request and, once it is known, who approves. No agent provisions or approves a hire.
5. **`gate` is not built in milestones one and two.** An approval here is a record with its own rules: who approves is derived from that person's position, never named by the requester, and a rejection needs a reason. The one approval those milestones have is the hire. For anything else an agent asks a required yes-or-no question, which is a question and not a recorded approval. `gate open` and `gate status` arrive with Process Packages, which are what declare gates. A gate resolved by a policy with no person is not in v1.
6. **`session suspend` and `resume` are outcomes, not calls.** The agent receives `suspended` from a required ask that got no answer in time and ends its turn; the office stops it and starts it again with the answer. A stopped process cannot call `resume`.
7. **Error codes keep one spelling.** The codes are the ones the office and the browser API already use, in lower case: the requirement's `CONFLICT_REV` is `conflict`, `SCOPE_FORBIDDEN` is `not_authorized`, `POLICY_VIOLATION` is `policy_violation`. `hint` and `retry_with` are added to every error.
8. **No product primitive is an MCP tool.** This closes the open point in the [processes design](../design/processes.md) and replaces the `ask_user` tool route in the alignment of [REQ-04](req-04-channel-messaging-and-question-components.md): the one route is `agent-os msg ask`. The thin MCP wrapper is not built, because every agent in v1 has a shell.
9. **Messages stay one family, `msg`.** Owner, 2026-11-18. `msg send`, `msg ask`, `msg reply` and `msg inbox` do what the requirement's `channel post`, `channel ask`, `consult ask`, `thread list` and `thread reply` name: there is one message record, and whom a message is addressed to decides where it goes. `channel` stays for listing, joining and leaving channels. Other names are changed so that every verb is a verb an agent can guess: `team roster` is `team inspect`, `hr roles` and `hr inspect-role` are `role list` and `role inspect`, `process catalog` is `process list`. `person inspect` is added beside `agent inspect`, as the same call under the name that fits a person.
10. **Standards are read through `process`.** Owner, 2026-11-18. A standard is a document of the same model as a process, inherited the same way, so `process list --kind standard` and `process inspect` do what `policy list` names, and there is no `policy` resource. `policy check` against written standards is not built: a text standard cannot be checked by a program. What code enforces is checked by `--dry-run`.
11. **`grant check` is `--dry-run`,** not a second command. `grant list` says what the office holds about the run's reach.
12. **The office creates and removes worktrees.** It creates them when the assignee starts a task, before the run that writes in them is launched, and removes them after the task closes. `worktree create` and `worktree cleanup` are therefore not commands; `worktree list` and `repo push` are.
13. **The long-running mode is a batch.** `agent-os session attach` reads one command a line and answers one envelope a line. [ADR-0014](../adr/0014-acknowledgement-classes-and-agent-os-latency.md) decision 5 now names this shape: the 1 ms target is met inside a batch, and a single command stays bounded by the start of its process.
14. **The common commands are fast; not every command has to be.** Owner, 2026-11-18. The design lists them (section 2.11) and holds them to a measured bound.

**What needs the owner's answer.**

1. **REQ-01, REQ-02, REQ-03, REQ-06 and REQ-07 are not in this repository.** Section 2 calls them the baseline. Only REQ-04 and REQ-05 were given. Without them these cannot be designed, only named: Process Packages (the YAML manifest, role-bound skills, `process apply`, `block run`), the Information Architecture index (`ia`), project storage (`fs`), traceability links to DSS versions (`link`), artifact lineage and drift (`artifact`), the event bus (`event`), `runner spawn` and `metrics`. None of them is needed for milestone one or two. Please send the five documents.
2. **Can an agent cause another agent to run? Settled.** Owner, 2026-11-18: yes. An ask or a hand-off from an agent starts one turn for an idle agent, under an hourly limit ([ADR-0023](../adr/0023-an-agents-ask-starts-a-turn.md)). `runner spawn` is not built: only the office starts a run.
3. **Process Packages and the processes this product already has.** Here a process is a written document that an agent receives and follows, inherited from the organization through the team to the project. The requirement's process is a package that also runs: it materializes folders, binds skills to roles, triggers blocks on a status and subscribes to events. Recommendation: keep the documents as the first layer, and design packages on top of them once the baseline requirements are in hand.

**What is delivered when.** Milestone one delivers the conventions of section 4 as a contract with its own tests, then the commands for the session, the directory, hiring, messages and asks, tasks, processes and standards, and repositories, each with the increment that builds the thing behind it. Milestone two delivers the same commands on Linux with no command added or changed, proved by the same tests. The design lists them one by one.
