---
kind: spec
title: "REQ-06: conversational desired state specification and factory reconciliation"
status: owner-requirement
revision: 1
owner: lead
created: "2026-11-18"
updated: "2026-11-18"
requested_by: owner
---

> **Owner requirement.** The text from "REQ-06" down to the line that starts "How this fits" is the owner's document, kept as written. The last section is the lead's alignment with the selected architecture and lists what is settled and what needs the owner's answer.

# REQ-06: Conversational Desired State Specification (DSS) & Factory Reconciliation

## Overview

Enable users to author and evolve software through natural-language dialogue with an AI that produces a domain-anchored **Product Desired State Specification (Product DSS)** as the primary root artifact—triggering an automated downstream "factory" where **System Architect** and **Staff Engineer** agent personas derive **Technical DSS** changes on record (autonomously from existing standards or interactively with the user when standards are missing), optionally generate versioned **Architecture Standards** as a byproduct, break down implementation tasks across both artifacts, support execution agents with architectural Q&A, and reconcile application code to match the declared desired state.

## Core Mechanics

### 1. User-Facing DSS Layers & Product Domain Anchor (Root Artifact)

- **Dual User-Facing DSS Artifacts:** Both the **Product DSS** and the **Technical DSS** are the two specification layers that can involve direct user interaction.
- **Product Domain Layer as Root:** Everything in the project space is anchored to the **product domain layer**, organized by product domains, verticals, and categories/areas (e.g., *Identity & Access Management (IAM)* / *User Authentication* — login flows, password expiration rules) for straightforward discovery and retrieval.
- **Natural-Language Dialogue:** Users express ideas and changes in plain text; the AI collaborates back and forth in chat to clarify ambiguities and shape the specification.
- **Primary Entry Artifact (`Product DSS`):** Once sufficient context is gathered, the system derives and writes the **Product DSS** into the product domain hierarchy as the foundational first artifact.

### 2. Factory Trigger & Conditional Technical DSS Derivation (Architect & Staff Engineer Personas)

- **Where the Factory Starts:** The moment a **Product DSS** is ready, it initiates the automated downstream engineering pipeline ("the factory") to derive the **Technical DSS** as a downstream sub-artifact (sub-product) representing a **system-wide specification change on record** (covering architecture, protocols, database schemas/migrations, and external provider integrations).
- **Specialist Agent Ownership:** **System / Technical Architect** and **Staff Engineer** agent personas own the derivation and evolution of the **Technical DSS**, operating in one of two modes:
  - **Autonomous Mode (Standards or Existing Technical DSS Defined):** When architectural standards or existing Technical DSS documents already define how the system works and what technologies or patterns apply, no extra user input is required. The Technical Architect and Staff Engineer agents read those standards and existing Technical DSS documents and update the Technical DSS directly without interrupting the user.
  - **Interactive Mode (Missing Standards or Undefined Decisions):** When standards do not exist yet or architectural decisions remain open (e.g., how the system should work, which technologies to adopt), the System Architect and Staff Engineer personas interact directly with the user to clarify and decide those choices, feeding the agreed decisions into the Technical DSS.
- **Optional Architecture Standards Byproduct:** Deriving or updating a **Technical DSS** can optionally generate or update **Architecture Standards** as a byproduct artifact (not required on every Technical DSS, only when warranted). Once generated, Architecture Standards are treated and versioned like code as part of the project's processes and standards, and can be modified as part of any subsequent artifact evolution. *(Scope note: While standards can be project-specific or inherited from higher organization levels—including non-negotiable rules that cannot be violated—this specification covers only the Technical DSS's ability to generate versioned Architecture Standards as an artifact; full standards hierarchy, inheritance, and governance are defined in a separate Standards specification.)*

### 3. Implementation Planning, Task Breakdown & Agentic Execution

- **Dual-Artifact Task Decomposition:** Once both the **Product DSS** and the derived **Technical DSS** change on record are finalized, the implementation planning phase breaks down the feature and system-wide changes into concrete, deliverable tasks covering **both** artifacts.
- **Agentic Task Execution & Architectural Consultation:** Downstream execution agents implement the broken-down tasks. During execution, whenever an agent needs clarification on architecture, design intent, or system constraints, it queries the **System Architect** or **Staff Engineer** personas directly—leveraging the deep contextual knowledge those personas hold over the Technical DSS and standards documents they generated.

### 4. Desired State Reconciliation & Traceability Links

- **DSS as Authoritative Desired State:** Borrowing the desired-state model from IoT, each **Desired State Specification (DSS)** represents the authoritative desired state of the application rather than a one-off handoff ticket.
- **Versioned State Evolution:** To change behavior, the user updates the Product DSS to produce a new version, propagating any required updates into the derived Technical DSS.
- **Traceability Links & Code Convergence:** When a new DSS version is published, an execution agent inspects the specifications, identifies all linked code locations (`links`), and applies the required code changes to converge the application with the updated desired state.

## User Experience & Elegance

- **Zero-Interference Autonomy When Standards Exist:** Users are never bogged down answering repetitive technical questions when standards or existing Technical DSS documents already define the answer—the Technical Architect and Staff Engineer agents apply them silently and accurately.
- **Focused, High-Value Architectural Dialogue:** User interaction on the Technical DSS occurs only when genuine architectural choices are undefined, guided naturally by System Architect and Staff Engineer personas.
- **Self-Service Agentic Collaboration:** Execution agents resolve architectural questions directly with the System Architect and Staff Engineer personas rather than escalating routine technical ambiguities back to the user.
- **Single-Entry Conversational Flow & Declarative Confidence:** Users work naturally at the product domain level while versioned DSS artifacts, explicit traceability links (`links`), and dual-artifact task breakdowns keep the codebase aligned.
- **Living Standards as a Natural Byproduct:** Architecture Standards emerge organically from Technical DSS decisions and evolve with code-like versioning—avoiding upfront bureaucratic policy authoring before building.

## How this fits the selected architecture

Written by the lead on 2026-11-18. It adds no requirement; it places the owner's requirement in the existing contracts. Under [REQ-07](req-07-composable-process-packages-and-team-blueprints.md) the DSS workflow is one process package, so nothing in the product is specific to DSS: the product gains general records, and the methodology is content. The package is described in the [desired state design](../design/desired-state-specifications.md); the general records are in [document 18](../design/organization-data/18-process-packages-and-project-documentation.md).

**What it matches already.** People talk to agents through the product's chat, and an agent's question is a structured card with choices, yes/no or free text ([REQ-04](req-04-channel-messaging-and-question-components.md)), which waits and can be marked required ([ADR-0020](../adr/0020-required-asks-suspend-and-resume.md)). Standards exist at organization, team and project level, are inherited, and can be marked not overridable ([process design](../design/processes.md)); the agent receives the ones that apply. Work is tasks, grouped by milestones and releases. An agent works in a worktree of a registered repository and its task records the commits that were integrated.

**Where each part of the requirement lands.**

| Requirement | In the product |
| --- | --- |
| Product DSS, the root artifact, by product domain, vertical and area | Documents of kind `product-dss` in the project documentation tree, under `product/`. Domains and verticals are nested sections, an area is a document, and all are listed in the project's index. |
| Dialogue with the user | Chat with the product lead agent, with asks for the points that need a decision. |
| Technical DSS, a system-wide change on record | Documents of kind `technical-dss` under `technical/`. Each issued version records the Product DSS versions it was derived from. |
| "Ready", "finalized", "a new version is published" | **Issuing a version**: the working text is fixed as version N, written once. The desired state is the latest issued version. |
| Autonomous mode | The technical lead's skill: read the standards that apply and the existing Technical DSS first, and ask only what neither answers. |
| Interactive mode | Asks to the person, marked required when the work cannot continue. The answer is written into the Technical DSS, and into a standard when it will be needed again. |
| Architecture Standards as a byproduct, versioned like code | Project standards (`projects/<project-id>/standards/`), the existing record. Agents of a project's team may now add process and standard documents at that project's scope. |
| Task breakdown across both artifacts | Tasks whose `realizes` names the document versions they bring the code in line with. |
| Execution agents consult the architect | A directed ask from the engineer to the technical lead, answered in the lead's own session, which holds its context. |
| Traceability links (`links`) | `code_links` on a document: repository, path and optional symbol. The field is not called `links` because that name is taken by links between records. The reverse lookup answers "which documents speak about this file?". |
| Code convergence | Derived, like a milestone's completion: a version is converged when every task that realizes it is finished. |

**Points settled by the lead.**

1. **"Published" is called "issued" here.** The product already uses "publish" for sending organization files to Git. Issuing is a different act: fixing the text that work is done against.
2. **The factory is agents following skills, not a pipeline in the product.** Issuing a version starts nothing by itself. The skill tells the lead what follows: derive, plan, hand over as tasks. This keeps methodology out of the product (REQ-07 section 1) and matches the rule that the product has no process engine.
3. **The System Architect and the Staff Engineer are one seed agent,** the technical lead, as REQ-07 section 3 describes. "Persona" is that agent's role.
4. **Whether to ask the person before issuing is the skill's instruction, not a gate.** The DSS package tells the lead to confirm a version with the person through a required yes/no ask before issuing it. The product records who issued and does not check for an approval.
5. **No line numbers in code links.** They drift with every edit. A link is a path and, where useful, a symbol.
6. **Convergence is derived from tasks, not asserted.** Nobody marks a version "implemented".
7. **Agents write standards for their own team's project only.** They add standards and change the project's own; they never replace one inherited from above and never mark one not overridable. Organization and team standards stay with people.

**Points that need the owner.**

1. **Agents starting other agents.** The "automated" factory needs the next agent to start when work reaches it. It is point 1 of [REQ-07's alignment](req-07-composable-process-packages-and-team-blueprints.md#how-this-fits-the-selected-architecture), with the recommendation there. Until it is answered a person presses Run on the technical lead when a Product DSS version is issued, and on each engineer when its task is assigned.
2. **The Standards specification.** The requirement refers to a separate Standards specification for hierarchy, inheritance and governance. The [process design](../design/processes.md) already defines three levels, inheritance by key and the non-overridable lock, for processes and standards alike. Say if a further requirement is coming, or if that design is the specification.

**Settled by the owner on 2026-11-18:** the DSS package does not use feature records, and software delivery by features is a separate package ([REQ-07's alignment](req-07-composable-process-packages-and-team-blueprints.md#how-this-fits-the-selected-architecture)); a lead approves a hire, and an agent lead needs the May hire permission ([ADR-0022](../adr/0022-a-lead-approves-a-hire.md)).
