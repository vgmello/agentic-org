---
id: doc-1
title: Agentic Org — Vision
type: guide
created_date: '2026-11-20T23:35:00Z'
updated_date: '2026-11-21T00:04:00Z'
tags:
  - vision
  - north-star
  - ux
  - architecture
  - autonomy
  - processes
---
# Agentic Org — Vision

## What Agentic Org Is

Agentic Org is a self-hosted workspace for building and running organizations—from simple teams to complex multi-team companies—staffed by people and **autonomous AI agents**. You install it on infrastructure you control, shape its teams, projects, reporting lines, and budgets, and define your own **customizable processes** (including Desired State Specifications, or DSS) that govern how work gets done. Within those processes and guardrails, agents operate autonomously: managing projects, following reporting lines, and orchestrating tasks on their own within and across teams.

The product is designed first to feel effortless and approachable for **non-technical people**, while remaining deeply capable for **technical teams**. A founder, operator, or product lead should be able to structure an organization, customize how it works, hire autonomous agents, and steer outcomes without touching a terminal or understanding distributed systems. At the same time, engineers get Git-backed durability, composable process packages, clean architectural boundaries, and a fast agent command line (`agent-os`) underneath.

**Communication is the heart of the system.** People talk to agents—and **agents talk to each other**—through one unified, calm chat experience connected directly to tasks, projects, and living documents. Agents coordinate across teams, delegate along reporting lines, pause cleanly to ask humans when a decision is needed, and keep work transparent: shared decisions live in public comments, while an agent's private working notes let humans inspect its plan and reasoning without cluttering shared threads or polluting other agents' context.

Under the hood, Agentic Org is engineered to be fast, self-reliant, and trustworthy. Offices run close to the work, keep moving even when disconnected, treat durable files as the source of truth, and enforce clear authority, spend budgets, and isolated runtimes—so an organization can start small on one machine and scale gracefully into a complex, multi-office autonomous organization.

---

## Core Primitives & Non-Negotiable Aims

These are the enduring principles every product decision, UI flow, and architectural choice aims toward. The Architecture Non-Negotiables and Architecture Rules serve these higher aims.

1. **Remove Friction Relentlessly**
   Every unnecessary step, configuration hurdle, modal detour, or manual coordination tax is a defect. Sensible defaults work out of the box, common actions feel immediate, and complexity stays invisible until genuinely needed.

2. **Great User Experience — Far Beyond Visual Polish**
   Great UX means clear information architecture, clean visual hierarchy, predictable mental models, and effortless flow from one moment to the next. Users always know where they are, what matters now, and what happens next without thinking about how the software is wired.

3. **Simple for Non-Technical People, Powerful for Technical People**
   Lead with plain language, calm simplicity, and uncluttered screens. Never leak internal jargon, Git mechanics, or CLI plumbing into everyday UI—while preserving clean depth and precision for technical users and agents through progressive disclosure.

4. **Communication Is the Heart of the Organization**
   One unified chat connects everyone—people talking to agents, and agents talking to each other. Chat ties seamlessly into tasks, questions, approvals, and deliverables so delegating, collaborating, and coordinating across teams feels fluid and natural.

5. **Bounded Autonomy Inside User-Defined Processes**
   Every organization defines its own way of working through customizable processes and Desired State Specifications (DSS). Agents act autonomously inside those rules—owning projects, following reporting lines, and orchestrating tasks within and across teams—bounded by explicit authority, budgets, isolated execution, and transparent human oversight (including read-only human access to each agent's private task notes).

6. **Elegance Over Patchwork**
   Between two solutions that meet a requirement, choose the simpler, cleaner, more cohesive one. Solve root causes rather than stacking shims, flags, or special cases. Rebuilding an awkward seam cleanly beats carrying friction forward.

7. **9/10 Architecture — Vertical Slice, Modular, Flexible, Scalable**
   Organize end-to-end by cohesive **vertical slices** (UI module → BFF/API → office domain → storage contract) with thin binaries, one-way acyclic dependencies, replaceable adapters, files as durable source of truth, fail-closed security, and single-digit-millisecond local hot paths—so the system scales smoothly from one office to many without rewrites.

---

## Decision Tie-Breakers

When two good goals pull in opposite directions, resolve them in this order:

- **Simplicity beats exposed power:** Ship sensible zero-config defaults first; put advanced controls behind progressive disclosure.
- **Working vertical slices beat speculative architecture:** 9/10 architecture means clean, shippable end-to-end user value—never abstract horizontal layers built ahead of real use.
- **Ask in chat instead of guessing silently:** When an autonomous agent hits ambiguity, missing authority, or a high-stakes choice, it pauses cleanly and asks rather than improvising or blocking a turn.
- **Clean replacement beats legacy compatibility:** Until v1 ships, remove awkward seams and dead paths rather than layering compatibility shims on top of them.

---

## What We Are Not Building

- **Not a browser terminal or raw prompt console:** People collaborate with agents in chat and structured work items, never through PTY logs or brittle prompt plumbing.
- **Not a rigid, one-size-fits-all workflow tool:** Processes are customizable content, not hardcoded bureaucracy.
- **Not a cloud-locked black box:** Agentic Org is self-hosted, local-first, and backed by durable files you own.
- **Not an insider-only engineering cockpit:** Technical depth never comes at the expense of non-technical clarity.
- **Not speculative infrastructure without user value:** Every increment should move real people and agents closer to working inside the product.

---

## Vision Checklist (For Every Agent & Engineer)

Before finalizing a design, task plan, or pull request, check your work against every item:

- [ ] **Friction:** Does this remove user friction rather than adding steps, configuration, or cognitive load?
- [ ] **Non-technical UX & Information Architecture:** Is the flow plain-language, uncluttered, and fluent—not just styled well?
- [ ] **Communication & Chat:** Does it strengthen natural human-to-agent and agent-to-agent communication around work?
- [ ] **Bounded Autonomy & Processes:** Does it support autonomous agents working inside custom processes (including DSS), reporting lines, budgets, and transparent human oversight—from simple to complex orgs?
- [ ] **Elegance:** Is this the simplest, cleanest root-cause solution, with no unnecessary shims or accidental complexity?
- [ ] **Vertical Slice & 9/10 Architecture:** Does it respect vertical-slice boundaries, thin binaries, acyclic deps, replaceable adapters, fail-closed security, and files-as-source?
- [ ] **Flexible, Scalable & Real:** Does it deliver working end-to-end value without hurting hot-path speed or future scale?
