---
kind: spec
title: "Decisions, evaluations, designs and tech debt (UX)"
status: current
owner: lead
created: "2026-11-08"
updated: "2026-11-17"
requested_by: owner
builds_on: [work-items-ux, our-org-in-the-app]
---

# Spec · Decisions, evaluations, designs and tech debt (UX)

The data contract owns the records: [project documents](../design/organization-data/02-project-structure.md), [tasks, features and shortcut provenance](../design/organization-data/03-features-and-work.md), [identity](../design/organization-data/06-identity-and-numbering.md), [record contracts](../design/organization-data/08-record-contracts.md) and [release content](../design/organization-data/09-release-content-and-flags.md). This spec owns what people see.

## The request (the user, 2026-11-08)

> Capture tech debt: every shortcut or quicker implementation an agent takes should be captured. Have a structure for ADRs and design documents in a project, so agents can use them. Help us find **why a decision was made** and **what technology we evaluated**. And bring this project's own designs into the tool that manages it.
>
> These are features of the product: agents don't write them in their own folders any more. They write them **through the tool**, which saves them in the project, in a proper structure, in its repository. *(the user, later)*

**What "great" looks like:** anyone, the owner or an agent, can open a piece of code, a task, a bug or a commit and answer in one or two clicks: *why is it like this? what else did we try? what did we knowingly leave unfinished, and what will it cost us?*

v1 delivers project knowledge documents, the Docs tab, the editor, Why links, `agent-os doc` commands and a view of each feature's known limitations. A debt register, exception records, agent guidance moments, decision approval and custom templates are [not in v1](#7-not-in-v1).

## 0. Decisions in this spec

0. **Written through the tool, kept in organization Git.** Decisions, designs, findings and the project brief are durable knowledge. A project may use several code repositories, but its documents do not follow any one code repository's `docs/` layout. Edits keep canonical fields, stable IDs and authenticated actor attribution.

### 0.1 The project structure (in organization Git, on its state branch)

```text
projects/<project-id>/
  project.yaml
  brief.md
  decisions/<document-id>.md
  designs/<document-id>.md
  reports/<document-id>.md
  features/<feature-id>/
    feature.md
    designs/<document-id>.md
    decisions/<document-id>.md
    reports/<document-id>.md
  releases/<release-id>/
    release.md
    changelog.md                 product changes
    documents/<document-id>.md
    milestones/<milestone-id>.md
    deployments/<deployment-id>/
      deployment.md
      changelog.md               technical attempt changes
      documents/<document-id>.md
  processes/<document-id>.md
  standards/<document-id>.md
  work/tasks/<task-id>.md
```

Supporting documents use `kind: brief | decision | design | report | changelog`. Every one requires a project; at most one feature, release or deployment ID is present and it belongs to the same project. `brief` is project-wide; feature support is design, decision or report; changelogs belong only to a release or deployment. `feature.md` projects into `features`, not into a duplicate document. Shared architecture stays at project scope and is linked from several features rather than copied. Processes and standards use their own contract at organization, recursive team or project scope (`processes-ux.md`).

A document has a stable `id`, an optional display `key`, schema and revision, title, `owner_employee_id`, lifecycle `draft | current | deprecated`, and the common creation and update attribution. Current requires an active accountable owner. IDs are UUIDv7. There is no document number counter: a label such as `ADR-0004` is a key, not a primary key. Renaming a title or key never renames the stable-ID file or breaks links.

Open tasks of every type are canonical organization-Git files; immutable task activity has its own durable JSON history. Closure keeps IDs, provenance and linked tombstones. Chat, private notes, usage, budget history, receipts and authority have protected durable sources outside shared Git.

### 0.2 How a document change lands

- **Publication:** a change is saved with the expected revision and actor attribution and published to the organization's state branch. Readers see an accepted but unpublished change honestly: "Waiting to publish · 1 change", with View change.
- **Code and knowledge are separate repositories.** A canonical organization document is never written on a code task branch.
- **Attribution:** creation and update kind and employee ID come from the admitted actor. Commit authorship and co-author trailers are display evidence and never authenticate an approval.
- **Source:** canonical content is read from the organization's configured `state_branch`. Code instructions follow each project/repository association's required `integration_branch`; there is no project-wide branch and no fallback.
- **Feature facts:** lifecycle, delivery and retirement data in the feature file is canonical; task membership and progress derive from task files. The office never overwrites a valid feature file from office state.

1. **One Docs tab.** It shows Brief, Decisions, Designs, Reports and scoped product and technical Changelogs. Features and processes have their own views. Evaluation content is a report, specs and plans are designs, and reviews are reports.
2. **Markdown with front matter, shaped by an editor.** Fields render in a metadata card, sections come from `##` headings, and relationships are stable-ID links. A form and a Source mode stay synchronized.
3. **Decision meaning lives in the body.** Whether a decision was accepted, superseded or rejected, and by whom, is written in its body; document status stays Draft, Current or Deprecated. Superseding links both records and keeps the old rationale.
4. **Specialized stages** (design review, evaluation verdicts) are body content, never a substitute for document lifecycle.
5. **Evaluations have a fixed useful shape:** question, options tried, criteria, evidence (numbers, links, commits), verdict and open questions, recorded as `kind: report` and linked from a decision's evidence.
6. **Why is a link on everything.** Tasks of every type, commits, files and documents can show the decisions, designs and reports behind them. Authored `relates_to` edges carry stable source and target IDs; task feature membership is not duplicated as an authored edge.
7. **Shortcuts are recorded on the task.** Task `details.shortcut` holds `{type: none}` or `{type: debt, ids: [...]}`, and a decision or report can explain a shortcut. There is no debt kind, table or counter in v1.
8. **Debt is never shaming.** A shortcut records a trade-off, who took it and why.
9. **This repository's own `docs/` is not imported.** New decisions are written in the product.

## 1. Words (glossary additions)

| Term | Means | Don't say |
| --- | --- | --- |
| Decision (ADR) | A recorded architecture decision: context, decision, consequences | Ruling, RFC |
| Design | How a capability will be implemented; links its product requirements | Implementation facts presented as requirements |
| Evaluation | A comparison or experiment that answers a question with evidence | Spike (the team's word: shown as "Evaluation (spike)"), POC |
| Capability requirements | Intended users, behavior, rules, limits and acceptance criteria in feature.md | Duplicate PRD record |
| Spec | UX flows, states and acceptance criteria | A separate document kind |
| Tech debt / Debt | A known shortcut whose proper fix is postponed | Hack, mess, TODO |
| Pay back | Doing the proper fix | Clean up, refactor (in labels) |
| Accepted debt | Debt we decided to live with, and why | Won't fix (that's bugs) |
| Known limitation | Something the product doesn't do or doesn't handle, on purpose or for now, with its workaround | Missing feature, gap, bug |
| Exception | A deliberate, approved departure from a rule, standard or decision, with its scope and when it's reviewed | Waiver, override, hack |
| Why | The decisions, designs and evaluations behind an item | Rationale, context (in labels) |

## 2. Flow 1 · Docs tab, by kind

```text
 Docs   [New ▾]  [Search docs]
 Brief (pinned)
 Decisions · 3
   ADR-0003 Messaging…             Draft
   ADR-0002 Storage…               Current
   ADR-0001 Application boundary…  Current
 Designs · 4
 Reports · 7
 Changelogs · 2
 Older · 5 (deprecated)

 Decision · ADR-0002 · Storage: SQLite, one writer
 Current · Owner sam · Updated 8 Nov · organization Git
 Context · Options considered · Decision · Consequences · Evidence
```

ADR labels here are optional display keys; the record and its filename use a separate stable ID.

- **Groups** by kind and scope, each with a count. Inside: optional display key, title and lifecycle badge (**Draft · Current · Deprecated**, icon plus word). Deprecated documents collapse under Older.
- **New ▾:** Brief (project scope) · **Decision** · **Design** · **Report**; Changelog is available only in release or deployment scope. An Evaluation entry uses the Report envelope with the evaluation body template. Each opens the metadata and body template (§3), allocates a stable UUIDv7 ID, shows the derived canonical path and offers an optional display key. The filename is not editable. Save is explicit.
- **Header** (reader): metadata from front matter (§3.0): kind, optional key, title, lifecycle, owner, creation and update attribution and time, and scope. Authored links show as links and the canonical path as read-only. A quoted "accepted by" statement in a decision's body is shown as body text, never as authenticated approval.
- **Footer:** **Led to** (items and documents linked from this one, or that link it as their Why) and **Mentioned in**.
- **Filters:** Status · Owner · search. Search matches permitted titles, stable IDs and display keys, and body text; feature definitions appear as feature results linked to their canonical record. Results are grouped by record type, include scope and status, and label body matches as excerpts. The view states its actual indexed scope and freshness, and any title-only, truncated-body or unavailable coverage; no matches never proves absence outside that coverage. Private notes, protected chat and inaccessible records never appear.
- **States:** a document whose front matter is missing or invalid shows "Details not recognized: <field> <problem>" with **Fix** (opens the Details panel), and the body still shows · a duplicate display key shows both with scope and **Resolve key…**; stable IDs never change · everything else as work-items Flow 8 (conflicts, history, Git errors).

## 3. Templates: front matter and sections

Each kind has an editor form for its fields and body sections with hints. An evaluation uses the report envelope; spec and plan content uses design; review content uses report. New documents start from that form and body shape, and validation follows the contract. Templates never add fields or kinds to accepted records.

**Project brief**, at `projects/<project-id>/brief.md`: what it is, whom it serves, how it works now, current known version and availability with evidence, and links to capability definitions and release notes. It is the living overview, with one canonical file and no per-version copy. An unknown current version stays unknown; a planned release or a Live feature alone does not establish deployed availability.

**Capability requirements** stay in the feature definition's Purpose, How it works, Rules, Known limitations and Non-goals sections, including intended users and acceptance criteria. Supporting designs explain implementation. "Overview" and "PRD" are content labels, not document kinds.

**Decision**, at `projects/<project-id>/decisions/<document-id>.md`:
```markdown
---
schema_version: 1
id: existing-decision-id
revision: 1
key: ADR-0004
project_id: existing-project-id
feature_id: null
release_id: null
deployment_id: null
kind: decision
status: draft
owner_employee_id: existing-employee-id
title: Docs storage
created_at: "2026-11-15T10:00:00Z"
created_by_kind: employee
created_by_id: existing-employee-id
updated_at: "2026-11-15T10:00:00Z"
updated_by_kind: employee
updated_by_id: existing-employee-id
links: []
---
## Context
## Options considered
## Decision
## Consequences
## Evidence
## Tech debt accepted
## Not covered here
```

**Design, spec or plan content:** `kind: design`; sections Goal · Users and jobs · Flow · States · Risks · Acceptance criteria · Open questions as applicable. Feature content adds a same-project `feature_id` and uses the feature's `designs/`; reusable content stays project-wide.

**Evaluation:** `kind: report`; sections Question · Options · Criteria · Evidence (option × criterion) · Verdict · Left open. Measured evidence, commits and uncertainty are kept.

**Review:** `kind: report`; sections Subject and commit · Summary and verdict · Findings (severity, evidence, remedy) · Validation · Open decisions.

**Release and deployment knowledge:** a release `changelog.md` records product changes, the explicitly included feature and task IDs and availability or gating; a deployment `changelog.md` records the technical delta, commits and configuration, confirmed applied flags, validation and rollback. Each is a separate stable-ID `kind: changelog` document with exactly its own scope. Failed or rolled-back attempts keep their technical history; correcting release prose does not rewrite deployment snapshots.

### 3.0 How the UI presents a document

```text
 Decision · ADR-0004                                  Draft
 Docs storage
 Owner sam · Created 15 Nov · Updated 15 Nov
 Project Agentic Org · organization Git
 Related: T-A17 · Storage evaluation report
 ────────────────────────────────────────────────────
 Context | Options considered | Decision | Consequences | Evidence
```

- **Header card** comes from front matter: kind and optional key, title, lifecycle, owner as avatar plus name, scoped links and dates in words. The reader does not expose raw YAML. Related records show their lifecycle: "Storage evaluation · Current". A verdict such as "SQLite chosen" is body content and is labelled as an excerpt if displayed.
- **Section rail** follows `##` headings, with the expected sections marked. A missing or empty section shows a quiet hint ("No consequences written yet").
- **Lists** show optional key, title, lifecycle, owner, scope and dates. The Reports list can show a cited excerpt from an evaluation's Question and Verdict sections, labelled as an excerpt.
- **Editing:** Details shows the metadata as typed controls above the Markdown body. Scope pickers enforce a required project plus at most one same-project feature, release or deployment, and identity, which determines the path, is read-only. Source mode edits the same fields and stays synchronized.

## 4. Flow 2 · Decisions over time

- **Supersede:** on a Current decision, ⋯ › **Supersede with a new decision** opens a new decision prefilled with "Supersedes ADR-0002" and the context. The two records link both ways. Superseding changes no status: a person deprecates the old decision explicitly if that is wanted, and its rationale stays.
- **Editing a Current decision** warns once: "Decisions are a record. Supersede it if the decision changed, or edit only to fix a mistake." **Edit anyway** is allowed.

## 5. Flow 3 · "Why?" everywhere

- **Task and bug pages** get a **Why** block in the side column: the decisions, designs, specs and evaluations linked to the item or mentioning its reference, with status badges. **+ Link a decision** links one. Empty: "No decisions linked. **Link one** if this work follows a decision."
- **Commits** (on items and in Activity) show their Why when the message names a document key ("ADR-0002") or an item that has one.
- **Ask "why?"** (⌘K › "Why is…") searches decisions, evaluations and designs by text and shows matches with their status and date.
- **Evaluations answer "what did we try?":** evaluation reports keep Question and Verdict sections, and a list can show their cited excerpts together ("Turso vs SQLite → SQLite: Turso's concurrent writes are experimental").

## 6. UX acceptance criteria

Criterion IDs for debt, limitations and exceptions use **DBT-**, **LMT-** and **EXN-** so they never collide with record keys.

**Docs and decisions (DEC)**
- DEC-1 · The Docs tab groups kinds and scopes with counts and status, and one living project brief covers what, who, current behavior and version evidence. Capability requirements stay in feature.md; no duplicate Overview or PRD kind or per-release copy is created.
- DEC-2 · Each document shows its front matter as a header card: kind, optional key, title, Draft/Current/Deprecated lifecycle, owner, scope, attribution and dates, and authored links. Stable ID and path are read-only, there is no document number field, and the section rail follows body headings.
- DEC-3 · New Decision, Design or Evaluation opens the envelope and the right body template, derives the stable-ID file path and shows its scope; there is no numeric document counter and Save is explicit.
- DEC-4 · Not in v1: Accept, Reject and Request changes on a proposed decision.
- DEC-5 · Not in v1: proposed decisions in the owner's Needs you.
- DEC-6 · Supersede drafts a new decision linked to the old stable record, with links both ways, and changes neither record's status; the old rationale is kept.
- DEC-7 · Invalid metadata names the field and offers Fix while keeping a readable body. Display-key clashes show both identities and Resolve key; stable IDs never change.
- DEC-8 · Deprecated content collapses under Older.

**Why and evaluations (WHY)**
- WHY-1 · Task and bug pages show a Why block with the linked or mentioning decisions, designs, specs and evaluations, with status, and + Link a decision.
- WHY-2 · A document's reader lists Led to (items and documents that link it) and Mentioned in.
- WHY-3 · Evaluation content is a report. A list may show cited Question and Verdict excerpts in one line, labelled as excerpts.
- WHY-4 · Knowledge search finds permitted titles, stable IDs and keys, and body text, with scope, status and body excerpts; feature results link to their canonical definitions. The view states its actual scope, freshness and partial indexing, and excludes private notes, protected chat and inaccessible records. A title-only search says so.

**Tech debt (DBT)**
- DBT-1 · Not in v1: Record debt.
- DBT-2 · Not in v1: the shortcut question when a task is marked Done.
- DBT-3 · Not in v1: a debt document kind and `DEBT-` keys.
- DBT-4 · Not in v1: the Debt tab.
- DBT-5 · Not in v1: Planned and Paid debt states driven by payback tasks.
- DBT-6 · Not in v1: accepting debt with a reason.
- DBT-7 · Not in v1: offering "Tech debt accepted" lines as debt items.
- DBT-8 · Not in v1: recording a review finding as debt.
- DBT-9 · Not in v1: debt on the task page and a Tech debt tile.
- DBT-10 · Not in v1: a Debt pill in Work.
- DBT-11 · Not in v1: draft debt found by scanning TODOs or review findings.

**Our project (OURDOC)**
- OURDOC-1 · Withdrawn: this repository's documents are not imported.
- OURDOC-2 · Withdrawn: this repository's documents are not imported.

**Authoring through the tool (AUTH)**
- AUTH-1 · Tool authoring writes documents at canonical organization-Git scopes with stable IDs, required metadata and admitted attribution, and never on a code task branch.
- AUTH-2 · A status edit uses only Draft, Current and Deprecated with revision and attribution. Decision acceptance or rejection is written in the body.
- AUTH-3 · Canonical files changed directly in organization Git are validated and indexed from its state branch without losing content, stable identity or provenance. Files in a code repository are not canonical organization records.
- AUTH-4 · Each accepted document revision records the admitted actor and time (the document's attribution fields and its Git history), and Activity shows it by stable document ID with the optional key and title.
- AUTH-5 · `agent-os doc list`, `show`, `new`, `edit` and `link` use the same kinds, optional keys, stable identities, lifecycle and scopes as the app; `new` and `edit` write the canonical organization-Git record with the agent's admitted attribution and report the publication result, and never write into a code worktree.
- AUTH-6 · A task's `details.shortcut` keeps its tagged IDs; canonical knowledge is never placed on a code task branch.
- AUTH-7 · Commit co-authors and Git-derived fields are displayed as evidence without granting authority. Feature lifecycle facts stay in feature.md; progress derives from task membership and valid file edits are never overwritten from office state.

**Known limitations (LMT)**
- LMT-1 · Known limitations live in each feature's `feature.md` (Known limitations section). The Docs tab lists them per feature, linked to the feature definition; there is no separate limitation record or counter.
- LMT-2 · When Report bug has a feature (chosen or affected), it shows that feature's known limitations with **Read it** and **Report anyway**; it never blocks the report.
- LMT-3 · Not in v1: Record as known limitation from a Won't fix bug.

**Exceptions (EXN)**
- EXN-1 · Not in v1: exception documents as their own kind.
- EXN-2 · Not in v1: approving exceptions in Needs you.
- EXN-3 · Not in v1: exception review dates.
- EXN-4 · Not in v1: listing a rule's approved exceptions.

**Agent guidance (GUIDE)**
- GUIDE-1 · Not in v1: Settings › Agent guidance moments.
- GUIDE-2 · Not in v1: project overrides of guidance.
- GUIDE-3 · Not in v1: the Taking a shortcut prompt and its recorded answer.
- GUIDE-4 · Not in v1: owner-added guidance moments.
- GUIDE-5 · Not in v1: guidance changes in Activity.

**Templates (TPL)**
- TPL-1 · Every kind has a built-in template (fields with type, required flag, allowed values and hint, plus sections with hints), and New fills a document from it.
- TPL-2 · The editor's Details panel is generated from the template's fields, and the Source toggle edits the YAML directly, with both kept in sync.
- TPL-3 · `agent-os doc schema <kind>` prints a kind's fields and sections, and `doc validate` reports each front-matter problem by field, with the same rules as the app.
- TPL-4 · Not in v1: editing templates in Settings.
- TPL-5 · Not in v1: adding document kinds.
- TPL-6 · Not in v1: template changes and their effect on existing documents.
- TPL-7 · Withdrawn: documents are not imported, so there is no front-matter conversion.
- TPL-8 · Every document has an immutable stable ID separate from its optional display key. IDs are UUIDv7. Title and key changes keep stable-ID filenames and relationships.

## 7. Not in v1

- **A tech-debt register:** a debt kind with cost and effort, capture at the shortcut, at task Done, from review findings, from "Tech debt accepted" and from `TODO(debt)` scans, a Debt tab grouped and sorted by cost, and Planned and Paid states driven by payback tasks. Until then, `details.shortcut` on the task and a decision or report record shortcuts.
- **Exception records** with approval and review dates, and recording a known limitation from a Won't fix bug.
- **Agent guidance moments:** configurable prompts at set moments (starting a task, taking a shortcut, finishing, asking the owner, posting to chat), with organization defaults and project overrides.
- **Decision approval:** Accept, Reject and Request changes with authenticated actors, and proposed decisions in Needs you.
- **Custom templates and kinds.**
- **Document comments** and frozen release-note workflows.
- Project navigation follows the [integrated IA](work-items-ux.md#3-flow-1--project-page); a Docs tab is not required before this experience is served.
