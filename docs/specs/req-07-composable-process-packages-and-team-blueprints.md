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

Written by the lead on 2026-11-18. It adds no requirement; it places the owner's requirement in the existing contracts. The records are in [document 18](../design/organization-data/18-process-packages-and-project-documentation.md) and the behaviour is in the [process package design](../design/process-packages.md). The DSS process itself is [REQ-06](req-06-desired-state-spec-authoring-and-reconciliation.md).

**What it matches already.** Projects, releases, milestones and tasks are first-class records with no methodology in them ([documents 02](../design/organization-data/02-project-structure.md) and [03](../design/organization-data/03-features-and-work.md)). Hiring is a product capability behind the built-in HR lead, and an agent can already ask for a hire with `agent-os hire request` ([hire flow](our-org-in-the-app.md)). Creating a project creates its team. The product has no process engine and blocks nothing for breaking a process ([process design](../design/processes.md), decision 7): it tells the agent what applies and the agent acts. Documents are written through the product and kept in organization Git, never in a code repository's `docs/` ([knowledge UX](decisions-and-tech-debt-ux.md)).

**Where each part of the requirement lands.**

| Requirement | In the product |
| --- | --- |
| Process package: YAML manifest and skills | A folder in organization Git, `process-packages/<package-id>/`: `package.yaml`, role definitions, skills and document templates. Packages that ship with the product are built in. |
| Process as skills that agents interpret | A skill is guidance bound to roles. It reaches an agent through the same resolution and delivery as process documents, as one more layer between the team's documents and the project's own. |
| Seed archetypes, multi-hat | Roles declared in the manifest, some marked `seed`; `hats` label what a combined role covers. |
| Process selector on a project | `process_package_id` on the project, chosen when it is created or later. |
| Seed roster: hire, or assign an existing team | Choosing a package shows its seed roles. Each is filled by a hire proposal into the project's team, or by an employee who already has that role; naming an existing team as the project's owner covers the "existing team" case. |
| Process-timed hiring by the leads | A skill tells the lead when to hire. The lead reads the role with `agent-os role show` and asks with `agent-os hire request`. The hire still goes through HR and approval. |
| Dynamic folder structure, materialized eagerly | The project documentation tree `projects/<project-id>/docs/`. Choosing a package adds the sections it declares, at once. |
| Markdown, diagrams and images, no hardcoded "DSS file" | Tree documents are ordinary supporting documents whose `kind` may be one the package declares; media files sit beside them. |
| Live YAML index, single read | `projects/<project-id>/docs/index.yaml`, read with `agent-os doc index`. |
| `ls` as a fallback | `agent-os doc ls`. |
| Project isolation, ask the other project's agents | An agent is refused another project's index, listing and files; the refusal names whom to ask. |

**Points settled by the lead.** Each is a decision inside the [non-negotiables](../architecture-non-negotiables.md), recorded in the design.

1. **"Written directly by agents" means written through `agent-os`.** An agent has no file access to organization data (non-negotiables 7 and 10). Agents keep the index current by what they do: adding a section, saving a document with its title and summary, storing a diagram. The office writes `index.yaml` in the same accepted change, so the index and the tree cannot disagree.
2. **No trigger engine.** The manifest's workflow rules are stages that people and agents read. Nothing in the product acts on them. This is the requirement's own rule ("without hardcoding any specific methodology or trigger logic") and the existing one.
3. **Skills are delivered by the office, not loaded by the vendor tool.** Agents run with the vendor tools' own skill loading switched off ([agent sessions](../design/agent-sessions.md)). A skill's title and when-to-use line are in the agent's instructions and its body is retrieved with `agent-os process show`.
4. **A package grants nothing.** It cannot name tools, models, credentials, budgets or approvers, and its skills never displace a document the organization marked not overridable.
5. **Built-in packages ship with the product; an organization customizes a copy.**
6. **Project-wide documents move into the tree.** The fixed `decisions/`, `designs/` and `reports/` folders at project level are replaced by sections of the tree, which a project without a package starts with. Documents of a feature, a release or a deployment stay beside their record, and so do the brief and the changelogs.
7. **A package's roles are matched to the organization's roles by key,** and missing ones are created when the package is chosen. An employee still has one role.
8. **Changing a project's process later removes nothing.**

**Points that need the owner.**

1. **Agents starting other agents.** Today a turn starts only when a person presses Run or answers a required ask; a message, an ask or an assignment does not start an agent ([run modes](agent-run-modes-ux.md), RUN-3; checklist row FC-104 is deferred). The flows in REQ-06 and REQ-07 have one agent hand work to another, ask another, or ask another project's agents, and expect it to act. Recommendation: select a narrow "on call" rule. An agent gets a turn when an ask is addressed to it, or when a task is assigned to it by its lead chain or by a lead of the task's project; the turn still needs a permit, respects budget holds and never interrupts a running turn. Until this is answered the design works with a person pressing Run on the next agent, and nothing else in it depends on the answer.
2. **Who approves a hire the lead asks for.** The requirement has the lead instruct HR to provision. The contract in force is that a person approves every hire ([document 04](../design/organization-data/04-agent-structure.md)). The design keeps it: the lead's request goes to the nearest person in the lead chain of the project's team, else to you. Say if approving a staffing plan once should cover the hires named in it.
3. **Features beside the Product DSS.** The requirement's list of primitives omits features, and a Product DSS document defines behaviour as a [feature definition](features-ux.md) does. Recommendation: keep features as a record for projects whose process uses them; the DSS package does not, and its tasks point at the document versions they realize. Say if the DSS documents are meant to replace feature definitions everywhere.
4. **The word "process".** The product already calls a single guidance document a process, with a Processes page. A project's process is now a package. The design says "process package" for the bundle, "skill" for its guidance and "process document" for the existing documents. Say if you want the existing documents renamed, for example to "procedures".
5. **Packages between organizations.** Moving a package to another organization or server as an archive is selected as the last increment. Say if it is needed sooner or not in v1.
