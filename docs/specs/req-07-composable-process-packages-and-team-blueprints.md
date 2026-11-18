---
kind: spec
title: "REQ-07: composable process packages, agent roles and team blueprints"
status: owner-requirement
revision: 1
owner: lead
created: "2026-11-18"
updated: "2026-11-18"
requested_by: owner
---

> **Owner requirement.** The text from "REQ-07" down to the line that starts "How this fits" is the owner's document, kept as written. The last section is the lead's alignment with the selected architecture and lists what is settled and what needs the owner's answer.

# REQ-07: Composable Process Packages, Agent Roles & Team Blueprints

## Overview

Decouple the core platform's well-modeled execution primitives—including **projects**, **milestones**, **releases**, **tasks**, and **hiring**—from specific methodologies by modeling any workflow (such as the Desired State Specification (DSS) factory, Test-Driven Development (TDD), or general business operations) as a portable **Process Package** (YAML manifest + agent skills) that defines lean minimum seed agent archetypes, dictates when and how agents invoke the platform's **hiring primitive**, and maintains a **live YAML Information Architecture index** for single-read artifact lookup across Markdown, diagram, and image files.

## Core Mechanics

### 1. Generic System Primitives vs. Process-as-Skill Extension Point

- **Domain-Agnostic Core Primitives:** The underlying system provides clean, first-class structural and lifecycle primitives—**projects**, **milestones**, **releases**, **tasks**, and **hiring** (agent provisioning and onboarding via HR)—without hardcoding any specific methodology or trigger logic.
- **Process as Executable Skill:** A **process** is defined as a set of machine-readable skills that agents read and interpret to govern how they orchestrate platform primitives—including when and how to invoke the **hiring primitive**, use platform tools, apply standards, produce and consume artifacts, and store files.
- **Methodology-Agnostic Extensibility:** Because methodologies live in pluggable skills rather than hardcoded system logic, the DSS workflow (`REQ-06`) is simply one process implementation. Any process can be authored and attached to a project—such as **Test-Driven Development (TDD)**, conversational rapid-implementation workflows, or non-software workflows (e.g., managing a business or operating a lemonade stand).

### 2. Portable Process Package Format (YAML + Skills Bundle)

- **Self-Contained Process Bundle:** Each process is structured as a portable package (conceptually like a `.zip` archive) containing:
  - **Process & Agent Manifest (YAML):** Declares the process definition, workflow rules, the **minimum viable seed agent archetypes** required to initiate the process (including multi-hat role combinations), and the **dynamic folder structure / information architecture contract** governing where project documents and media are stored.
  - **Role-Bound Skills:** The specific skill definitions that each agent role uses to execute its work and generate expected deliverables.
- **Custom Process Authoring & Selection:** Users can choose from existing processes via a project process selector (drop-down) or create new custom process packages for any domain.

### 3. Project Process Assignment, Lean Seed Teams & Process-Driven Hiring

- **Project-Level Process Configuration:** When setting up a project within an organization, the user selects which process governs that project.
- **Minimum Viable Seed Employees:** Processes bring in as few initial agents ("employees") as possible at project start—only the lean multi-hat leadership archetypes required to drive discovery and specification. For example, the **DSS process** initializes with just **two seed employees**:
  - **Product Owner / Principal (Staff) Designer:** A hybrid archetype combining product ownership with deep UX and design capabilities to collaborate with the user on the **Product DSS**.
  - **System Architect / Senior Staff Engineer:** A hybrid technical leadership archetype to evaluate system architecture and drive the **Technical DSS**.
- **Initial Seed Team Onboarding:** Upon selecting a process, the system presents this minimal seed roster to the user—allowing the user to instantiate ("hire") those seed agents or assign an existing team covering those archetypes.
- **Process-Orchestrated Invocation of the Hiring Primitive:** While **hiring** (backed by **HR**, the platform's agent-provisioning subsystem) is a generic platform primitive, the active **process** dictates *when* and *how* agents use it. In the **DSS process**, the seed leads first talk back and forth with the user to complete the **Product DSS** and **Technical DSS** definitions; **once that definition is complete**, they evaluate which specialist roles are needed to deliver the project (such as a **REST Backend Engineer** or a **Frontend Engineer**) and invoke the platform's **hiring primitive**:
  - **Role Inspection & Provisioning Request:** The lead agent queries **HR** via the hiring primitive, inspects the target role definition and the requirements HR needs to instantiate that role, and instructs HR to provision the specialist agent.
  - **Team Onboarding:** Once provisioned by HR, the new specialist agent is onboarded directly into the active project team under the leadership of the seed employees.

### 4. Process-Defined Information Architecture & Artifact Storage

- **Agent-Agnostic Storage Model:** From an agent's perspective, the core platform does not hardcode domain-specific file types such as "DSS files." Instead, artifacts are persisted as standard **Markdown files** alongside **diagrams** and **images**, and the active process defines the project's initial information architecture contract.
- **Eager Folder Materialization from Process YAML:** A project's folder structure is dynamic rather than bound to a single rigid platform-wide layout. As soon as a process is selected for a project, the initial folder hierarchy declared in its **Process YAML** is materialized **eagerly**—establishing the baseline layout from day one.
- **Keep Folder Rules Out of Reusable Agent Skills:** Agent skills are discouraged from encoding or memorizing project-specific folder rules, because a project's concrete structure is a living workspace that evolves as work progresses. While basic directory listing (`ls` equivalent) exists as a fallback capability, agents are discouraged from relying on repeated `ls` traversals to hunt for documents.
- **Live YAML Information Architecture Index (Single-Read Lookup):** Each project maintains a **live, machine-readable YAML index file** representing the project's **Information Architecture (IA) structure**—cataloging the logical hierarchy and locations of all files (Product DSS / PRD documents, Technical DSS specs, diagrams, and images). Rather than running multiple `ls` calls, agents simply read this single YAML index to immediately pinpoint where any artifact lives.
- **Agent-Maintained Living Index:** The YAML Information Architecture index is a living document—seeded when the project process initializes and **written/updated directly by agents** whenever new Product DSS / PRD documents, Technical DSS artifacts, diagrams, or images come alive or change.
- **Strict Project Isolation & Agent-to-Agent Cross-Project Collaboration:** Agents **do not** perform direct cross-project file discovery or browse another project's index or folder tree, because external projects may follow different processes and information architectures that only their own agents understand. When an agent needs context or deliverables from another project, it **communicates directly with the agents responsible for that target project** (agent-to-agent consultation), relying on those agents to consult their own project's YAML index and provide the requested information.
- **Example Hierarchy (DSS Process):** When a project adopts the **DSS process**, its Process YAML eagerly materializes a documentation hierarchy such as **Project Documentation** containing peer folders for **Product Documentation** (housing Product DSS / PRD Markdown files) and **Technical Documentation** (housing Technical DSS Markdown files, diagrams, and images), tracked continuously in the project's live Information Architecture YAML index.

## User Experience & Elegance

- **Lean Day-One Staffing with Zero Upfront Bloat:** Starting a project with only the bare minimum seed archetypes (such as two multi-hat leads for the DSS process) keeps teams lightweight and focused—avoiding premature hiring before product and technical scope are clear.
- **Process-Timed Hiring After Discovery:** Under the DSS process, seed leads focus purely on conversing with the user until the Product and Technical DSS are clear, and only then invoke the platform's hiring primitive to inspect role specs with HR, provision the exact specialists needed, and onboard them seamlessly.
- **Infinitely Adaptable Without Platform Bloat:** Keeping the core system anchored in clean, well-modeled primitives (projects, milestones, releases, tasks, and hiring) while encapsulating methodologies inside portable YAML + skills packages keeps the workspace calm and adaptable across any domain.
- **Transparent, Portable Workflows:** Bundling agent role definitions and their executable skills into a single self-contained package makes processes easy to inspect, reuse across projects, and customize.
- **Instant, Single-Read Artifact Pinpointing:** Maintaining a live YAML Information Architecture index eliminates slow multi-step directory traversals (`ls` loops) and keeps reusable agent skills clean—letting both users and agents see the entire logical structure of specs, diagrams, and images in one place as new Product DSS / PRD artifacts come alive.
- **Clean Cross-Project Boundaries:** Encapsulating each project's information architecture behind its own responsible agents keeps cross-project collaboration clean and reliable without forcing agents to reverse-engineer unfamiliar project structures.

## How this fits the selected architecture

Written by the lead on 2026-11-18 and updated the same day with the owner's answers. It adds no requirement; it places the owner's requirement in the existing contracts. The records are in [document 18](../design/organization-data/18-development-processes-and-project-documentation.md) and the behaviour is in the [development process design](../design/development-processes.md). The DSS process itself is [REQ-06](req-06-desired-state-spec-authoring-and-reconciliation.md).

**The name.** The requirement's "Process Package" is called a **development process** in the product, the owner's agentic software development process (ASDP). It is a different primitive from the **organization processes** the product already has, which say how the organization works whatever is being built (owner, 2026-11-18).

**What it matches already.** Projects, teams, releases, milestones and tasks are first-class records with no methodology in them ([documents 02](../design/organization-data/02-project-structure.md) and [03](../design/organization-data/03-features-and-work.md)). Hiring is a product capability behind the built-in HR lead, and the hire flow already lets an agent ask for a hire with `agent-os hire request` ([hire flow](our-org-in-the-app.md)); the command is specified and not built. A project always has a team; a team needs no project. The product has no process engine and blocks nothing for breaking a process: it tells the agent what applies and the agent acts. Documents are written through the product and kept in organization Git, never in a code repository's `docs/` ([knowledge UX](decisions-and-tech-debt-ux.md)).

**Where each part of the requirement lands.**

| Requirement | In the product |
| --- | --- |
| Process package: YAML manifest and skills | A development process: a folder in organization Git, `development-processes/<process-id>/`, with `process.yaml`, role definitions, skills and document templates. Some are built in; any person of the organization creates, copies, imports and edits its own. |
| Process as skills that agents interpret | A skill is guidance bound to roles. An agent receives its project's development process as a section of its instructions, beside and separate from organization processes. |
| Seed archetypes, multi-hat | Roles declared in the manifest, some marked `seed`; `hats` label what a combined role covers. |
| Process selector on a project | `development_process_id` on the project, chosen when it is created or later. |
| Seed roster: hire, or assign an existing team | Choosing a development process shows its seed roles. Each is filled by a hire proposal into the project's team, or by an employee who already has that role; naming an existing team as the project's owner covers the "existing team" case. |
| Process-timed hiring by the leads | A skill tells the lead when to hire. The lead reads the role with `agent-os role show` and asks with `agent-os hire request`. A lead with hiring power approves, and the request goes up the lead chain until it reaches one. |
| Onboarded into the project team under the seed leads | The new agent's home team is the project's team, whose lead the owner appoints: a person or one of the seed agents. The proposal suggests the requesting lead as the reporting line. |
| What a project shows | The development process declares the project's tabs from the product's generic views. |
| Dynamic folder structure, materialized eagerly | The project documentation tree `projects/<project-id>/docs/`. Choosing a development process adds the sections it declares, at once. |
| Markdown, diagrams and images, no hardcoded "DSS file" | Tree documents are ordinary supporting documents whose `kind` may be one the process declares; media files sit beside them. |
| Live YAML index, single read | `projects/<project-id>/docs/index.yaml`, read with `agent-os doc index`. |
| `ls` as a fallback | `agent-os doc ls`. |
| Project isolation, ask the other project's agents | An agent is refused another project's index, listing and files; the refusal names whom to ask, and an ask starts that agent. |

**Decided by the owner, 2026-11-18.**

1. **Two kinds of process.** The system has primitives; a development process defines how a project uses them; organization processes define how agents interact and work in general. They are separate primitives. A development process may adjust the organization's overridable guidance for its project, and a document marked not overridable is always followed.
2. **A lead with hiring power approves a hire.** Allow hiring is a permission of a lead, person or agent, given lead by lead; a request escalates up the lead chain until it reaches someone who has it ([ADR-0022](../adr/0022-a-lead-approves-a-hire.md)).
3. **Agents behave like people who are always online.** Anything addressed to an agent starts it: a message, a mention, an ask or a task assignment, from anyone. An idle agent shows as available; senders see presence and decide whether to wait ([ADR-0023](../adr/0023-an-ask-or-an-assignment-starts-an-agent.md)).
4. **Releases, milestones and features are primitives a process chooses to use.** They exist in the system for every project; nothing forces a change into a release. Software delivery by features is its own process, built in beside DSS, and both built-in processes use releases.
5. **A development process declares the project's tabs.** The tabs are generic; a process says which things it has and they become tabs ([design](../design/development-processes.md#4-the-projects-tabs)).
6. **Built-in processes, and processes anyone can create or edit.** A process can be imported into the system as a file. There is no link between organizations.
7. **Teams and projects live side by side.** A team is created before any project and never needs one; a project always has a team.

**Decided by the lead,** inside the [non-negotiables](../architecture-non-negotiables.md) and recorded in the design.

1. **"Written directly by agents" means written through `agent-os`.** An agent has no file access to organization data (non-negotiables 7 and 10). Agents keep the index current by what they do; the office writes `index.yaml` in the same accepted change, so the index and the tree cannot disagree.
2. **No trigger engine.** The manifest's workflow rules are stages that people and agents read. Nothing in the product acts on them.
3. **Skills are delivered by the office, not loaded by the vendor tool.** Agents run with the vendor tools' own skill loading switched off ([agent sessions](../design/agent-sessions.md)).
4. **A development process grants nothing.** It cannot name tools, models, credentials, budgets or approvers.
5. **Project-wide documents move into the tree.** The fixed `decisions/`, `designs/` and `reports/` folders at project level are replaced by sections of the tree. Documents of a feature, a release or a deployment stay beside their record, and so do the brief and the changelogs.
6. **A process's roles are matched to the organization's roles by key,** and missing ones are created when the process is chosen. An employee still has one role.
7. **Changing a project's development process removes nothing.**
8. **A team has one lead.** With two seed leads, the owner appoints one of them, or a person, as the project team's lead; the other directs by request.
9. **A project's tree has one writing office,** handed over like a task's writer.
10. **The views a tab can use are the product's:** documents of a section, tasks of given types as a list or a board, the feature map, and releases with their milestones. A new kind of view is a product change.
11. **How agents starting agents is bounded:** a start never interrupts a running turn, passes the permit and budget checks, and one agent may start another only so many times an hour.

**Point that needs the owner.**

1. **Import.** You said earlier that processes do not move between organizations, and later that a process can be imported. The design reads this as: a person can import a process file and download one, and nothing connects two organizations. Say if that is wrong.
