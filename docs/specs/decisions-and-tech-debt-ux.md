---
kind: spec
title: "Decisions, evaluations, designs and tech debt (UX)"
status: for-review
revision: 12
owner: lead
created: "2026-11-08"
updated: "2026-11-15"
requested_by: owner
reviewers: [ac-staff-engineer-ada, codex-dev, owner]
builds_on: [work-items-ux, our-org-in-the-app]
---

# Spec · Decisions, evaluations, designs and tech debt (UX)

Revision 12 reconciles authored knowledge with [project documents](../design/organization-data/02-project-structure.md), [tasks/features and retained shortcut provenance](../design/organization-data/03-features-and-work.md), [identity](../design/organization-data/06-identity-and-numbering.md), [record contracts](../design/organization-data/08-record-contracts.md) and [release content](../design/organization-data/09-release-content-and-flags.md). This is a UX proposal over selected target data, not implementation of commands or approval engines. The [authority map](../design/documentation-authority.md) distinguishes current contracts and deferred work.

## The request (the user, 2026-11-08)

> Capture tech debt: every shortcut or quicker implementation an agent takes should be captured. Have a structure for ADRs and design documents in a project, so agents can use them. Help us find **why a decision was made** and **what technology we evaluated**. And bring this project's own designs into the tool that manages it.
>
> These are features of the product: agents don't write them in their own folders any more. They write them **through the tool**, which saves them in the project, in a proper structure, in its repository. *(the user, rev 2)*

**Revision 12 (2026-11-15):** sharpens the single living brief/capability requirements model and title/ID/body search coverage; custom kinds, project lifecycle, doc comments and approval handlers remain deferred.

**Revision 11 (2026-11-15):** organization Git stores canonical project knowledge and typed tasks. Stable IDs differ from readable references; supporting documents have required project and at most one same-project feature/release/deployment scope. The body below replaces old code-repository storage and database-only work assumptions. Debt/limitation/exception workflows, custom kinds/templates, areas, Undo, guidance automation and process execution remain individually deferred proposals, while compatible content and accessibility requirements remain intact.

**What "great" looks like:** anyone, the owner or an agent, can open a piece of code, a task, a bug or a commit and answer in one or two clicks: *why is it like this? what else did we try? what did we knowingly leave unfinished, and what will it cost us?*

## How the team works today (the structure to keep)

ada already writes decisions and designs in a consistent way. The app adopts it rather than inventing a new one.

| Kind | Where today | Shape today |
| --- | --- | --- |
| ADR | product repo `docs/adr/0001-…md` (current), ada's `knowledge/adrs/0001–0009` (v0, pre-offices) | `# ADR-0002 · Title`, then a status line "Status: **accepted** 2026-11-08 (the user). Owner: lead. Refines ADR-0001 decision 2.", then Context · Decisions (numbered) · Consequences · Not covered here |
| Evaluation (spike) | inside ADRs ("Spike S1 confirmed it: p50 ~100 ms"; "we looked at Turso…"; "the user asked for Maestro to be evaluated") and spike READMEs in worktrees | free text with evidence and numbers |
| Design | product repo `docs/design/*.md`; drafts in ada's `knowledge/designs/drafts/` | "Status: **draft**. ada writes it with…", then Goal · The flow · … |
| UX spec | Eden's `knowledge/specs/*.md` | Owner/Status line, changelog, decisions, flows, criteria, questions |
| Review | ada's `knowledge/reviews/`, the reviewer's verdicts | summary with severity counts, findings |
| Plan | ada's `knowledge/plans/phase-1.md` | workstreams table with owners |
| Tech debt | nowhere: it's scattered in "Not covered here", review findings left unfixed, and 3 TODOs in the code | — |

## 0. Decisions in this spec

0. **Written through the tool, kept in organization Git.** Decisions, designs, findings and the project brief are durable knowledge. A project may use several code repositories, but its authored definitions do not inherit any one code repository's `docs/` layout. Tool-driven authoring is the UX proposal; edits must keep canonical fields, stable IDs and authenticated actor attribution.

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

Supporting `documents` use `kind: brief | decision | design | report | changelog`. Every one requires project; at most one feature/release/deployment ID is present and must belong to the same project. `brief` is project-wide; feature support is design/decision/report; changelogs belong only to a release or deployment. `feature.md` projects into `features`, not a duplicate document. Shared architecture stays at project scope and links from several features rather than becoming several canonical copies. Process/standard content uses the separate selected structural contract at organization, recursive team or project scope, not another general document kind.

A document has stable `id`, optional display `key`, schema/revision, title, `owner_employee_id`, lifecycle `draft | current | deprecated`, and common creation/update attribution. Current requires an active accountable owner. IDs are UUIDv7. Documents have no selected ADR/EVAL/DEBT numeric allocation engine. Existing labels can remain keys; they are not primary keys. Rename a title/key without renaming the stable-ID file or breaking ID links. No automatic renumbering of imported records.

Organization/team process and standard files share this organization repository (`processes/<id>.md`, `standards/<id>.md`, recursively nested `teams/<id>/.../processes/<id>.md`). Their overrides default allowed unless explicitly prohibited. Private process storage/access is an unresolved deferred proposal; shared Git cannot provide author-only visibility.

The earlier “persistent in Git, transitional only in SQL” rule is obsolete. Open tasks of every type are canonical organization-Git files; immutable task activity has its own durable JSON history. Closure retains IDs, provenance, allocations and linked tombstones. Chat, private notes, usage, budget history, receipts and authority have protected durable sources outside shared Git. One consolidated SQL projection does not remove those privacy boundaries.

### 0.2 How a doc change lands

- **Proposed reviewed authoring:** tool changes may use organization-Git review branches with expected revision/blob, actor attribution and review before publication to the configured state branch. This requires a concrete implementation contract; the earlier `docs/<slug>-<short-id>` branch protocol is not a newly selected wire API.
- **Code and knowledge are separate repositories.** `doc new --here` and `debt add` cannot silently place the canonical organization document on a code task branch. A future convenience may stage a linked knowledge proposal and a code reference, but must expose each repository/branch and partial-failure state. No cross-repository atomic commit is assumed.
- **Readers see publication honestly:** “Waiting to merge · <branch> · 1 change”, with View change and merged/proposed toggle. Until publication, lists retain the accepted state with a pending-change marker. This is proposed review UI, not proof the merge service exists.
- **Attribution:** creation/update kind and employee ID follow admitted actor identity; commit authorship/co-author trailers remain useful display evidence but do not authenticate an approval. Quiet “from git” attribution must not grant authority from editable fields.
- **Source:** read canonical content from the organization's configured `state_branch`. Code instructions still follow each project/repository association's required `integration_branch`; there is no project-wide branch or default fallback for authored knowledge.
- **Feature facts:** file lifecycle/delivery/retirement data is canonical; task membership/progress derives from task files. The office must not overwrite a valid feature status from a database-only status/tasks array.

1. **One Docs tab, selected kinds with richer proposed views.** Show Brief, Decisions, Designs, Reports and scoped product/technical Changelogs. Features and process/standard content have their own record views. Evaluation content can be a report, specs/plans can be designs, and reviews can be reports. Richer category metadata and custom kind schemas remain deferred rather than adding unsupported SQL kinds.
2. **Markdown with front matter, shaped by an editor.** Render selected fields in a metadata card, sections from `##` headings and relationships as stable-ID links. A form and Source mode stay synchronized. Extension templates cannot replace required fields, lifecycle, scopes, identities or authorization.
3. **Proposed decision workflow:** Proposed/Accepted/Superseded/Rejected are decision semantics in the body pending an extension contract; selected document status remains Draft/Current/Deprecated. Superseding links both records and preserves the old rationale.
4. **Proposed specialized workflows:** design review/build stages and evaluation Planned/Running/Done verdicts are body content or deferred metadata, never substitutes for selected document lifecycle.
5. **Evaluations have a fixed useful shape:** question, options tried, criteria, evidence (numbers, links, commits), verdict and open questions. Record one as `kind: report`; link it from a decision's evidence.
6. **Why is a link on everything.** Tasks of every type, commits, files and documents can expose decisions/designs/reports behind them. Authored `relates_to` edges carry stable source/target IDs; derived task feature membership is not duplicated as authored `implements` edges.
7. **Deferred debt subsystem:** the earlier DEBT records, payment states, category views and automation below remain a proposal. Selected task details share task identity/revision and are saved atomically with its activity event. They preserve definition-of-done, branch, done-anyway evidence, reproduction/expected/steps/environment, closure/reopen/duplicate/merge context, separate same-project affects_feature_id, request content/routing/outcome, draft IDs and proposal estimates. Suggested triager/verifier fields are suggestions, not assignments/authority. Selected `details.shortcut` holds `{type: none}` or `{type: debt, ids: [...]}` without creating a debt table/kind/counter. A selected decision/report can already explain a shortcut; it must not silently acquire the proposed debt schema.
8. **Deferred capture workflow:** capture at the shortcut, a review finding, a TODO or a decision; one sentence can begin a draft. Prompts/scans create nothing without explicit acceptance.
9. **Debt is never shaming.** A shortcut records a trade-off, who took it and why. Area grouping and automated payback/Undo are individually deferred.
10. **No import of this repository's documents (owner, 2026-11-17).** This repository's software `docs/` stays where it is. Copying it into an organization is not a v1 feature; new decisions are written in the product.

## 1. Words (glossary additions)

| Term | Means | Don't say |
| --- | --- | --- |
| Decision (ADR) | A recorded architecture decision: context, decision, consequences | Ruling, RFC |
| Design | How a capability will be implemented; links its product requirements | Implementation facts presented as requirements |
| Evaluation | A comparison or experiment that answers a question with evidence | Spike (the team's word: shown as "Evaluation (spike)"), POC |
| Capability requirements | Intended users, behavior, rules, limits and acceptance criteria in feature.md | Duplicate PRD record |
| Spec | UX flows, states and acceptance criteria | A separate selected document kind |
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

 Decision · ADR-0002 · Storage: SQLite with SQLCipher
 Current · Owner ada · Updated 8 Nov · organization Git
 Context · Options considered · Decision · Consequences · Evidence
```

ADR labels in this example are retained optional display keys; the canonical record and filename use a separate stable ID. The Reports group includes evaluation/review content as reports. Specialized Evaluation/Spec/Review categories require a separately selected classification contract; they are not extra authored `kind` values.

- **Groups** by selected kind and explicit scope, each with a count. Inside: optional display key, title and selected lifecycle badge (**Draft · Current · Deprecated**, icon plus word). Deprecated documents collapse under Older. Proposed/Accepted/Superseded/Rejected decision meanings and evaluation verdicts remain body content pending specialized metadata; they do not replace document lifecycle or create new kinds.
- **New ▾:** Brief (project scope) · **Decision** · **Design** · **Report**; Changelog is available only in release/deployment scope. An Evaluation entry may choose the Report envelope and evaluation body template. Each opens the selected metadata and body template (§3), allocates a stable UUIDv7 ID for a new record, shows the canonical derived path and offers an optional display key. The filename follows immutable identity and is not editable. There is no next ADR/EVAL document-number counter. Save is explicit and import preserves existing IDs/keys.
- **Header** (reader): selected metadata from front matter (§3.0): kind, optional key, title, lifecycle, owner, creation/update attribution/time and explicit scope. Show authored links as links and the canonical path as read-only. A quoted “accepted by” or “superseded by” statement may appear in the Decision body; it is not inferred as authenticated approval or read from unsupported front-matter fields.
- **Footer:** **Led to** (items and docs linked from this one, or that link it as their Why) and **Mentioned in**.
- **Filters:** Status · Owner · search. Search matches permitted canonical title, stable ID/display key and body text; feature definitions appear as feature results linked to their single canonical record. Group by record type, include scope/status and label body-match excerpts as excerpts. Declare actual indexed scope/freshness and title-only, truncated-body or unavailable coverage; no matches never proves absent content outside that coverage. Author-private notes, protected chat and inaccessible records never enter these shared results. **Earlier decisions (v0)** is a read-only group with a note: "From ada's notes before offices. Some are superseded by current decisions." (A18).
- **States:** a doc whose front matter is missing or invalid shows "Details not recognized: <field> <problem>" with **Fix** (opens the Details panel), and the body still shows, and the doc still shows · a duplicate display key shows both with scope and **Resolve key…**; stable IDs are never changed · everything else as work-items Flow 8 (conflicts, history, git errors).

## 3. Templates: front matter and sections

Each selected kind has an editor form for its contract fields and useful body sections with hints. An evaluation uses the report envelope; spec/plan content uses design; review content uses report. New documents start from that form/body shape and validation follows the selected contract. Custom schemas and CLI validation commands remain the deferred extensions in §3.1–3.2; they cannot add unsupported fields or kinds to accepted records.

**Project brief**, at `projects/<project-id>/brief.md`: what it is, whom it serves, how it works now, current known version/availability with evidence, and links to capability definitions/release notes. This is the living overview, with one canonical file and no per-version copy. Unknown current version stays unknown; a planned release or Live feature alone does not establish deployed availability.

**Capability requirements** stay in the feature definition's Purpose/How it works/Rules/Known limitations/Non-goals sections, including intended users and acceptance criteria. Supporting designs explain implementation. Shared architecture stays project-wide and links to several capabilities rather than becoming duplicated PRDs. “Overview”/“PRD” are content labels, not new selected document kinds.

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

**Design/spec/plan content:** selected `kind: design` and lifecycle; sections Goal · Users/jobs · Flow · States · Risks · Acceptance criteria · Open questions as applicable. Detailed feature content adds same-project `feature_id` and uses feature `designs/`; reusable content stays project-wide.

**Evaluation:** selected `kind: report`; sections Question · Options · Criteria · Evidence (option × criterion) · Verdict · Left open. Preserve measured evidence, commits and uncertainty.

**Review:** selected `kind: report`; sections Subject/commit · Summary/verdict · Findings (severity, evidence, remedy) · Validation · Open decisions. Reviewer identity and workflow authority require authenticated receipts if a later approval engine uses them.

**Release/deployment knowledge:** release `changelog.md` records product changes, explicit included feature/task IDs and availability/gating; deployment `changelog.md` records the technical delta, commits/configuration, migrations, confirmed applied flags, validation and rollback. Each is a separate stable-ID `kind: changelog` document with exactly its own scope. Failed/rolled-back attempts retain technical history; correcting release prose does not rewrite deployment snapshots.

### 3.0 How the UI presents a doc

```text
 Decision · ADR-0004                                  Draft
 Docs storage
 Owner ada · Created 15 Nov · Updated 15 Nov
 Project Agentic Org · organization Git
 Related: T-A17 · Storage evaluation report
 ────────────────────────────────────────────────────
 Context | Options considered | Decision | Consequences | Evidence
```

- **Header card** comes from selected front matter: kind and optional key (never an invented number field), title, lifecycle, owner as avatar plus name, scoped links and dates in words. The reader does not expose raw YAML. Related records show their selected lifecycle: “Storage evaluation · Current”. A verdict such as “SQLCipher chosen” is body content and must be identified as a body excerpt if displayed.
- **Section rail** follows `##` headings, with expected body sections marked. A missing/empty section shows a quiet hint (“No consequences written yet”), not an import error. Record-as-debt chips and Accept/Reject controls belong to separately deferred workflows (§4/§6).
- **Lists** use selected metadata: optional key, title, lifecycle, owner, scope and creation/update dates. The Reports list can expose a cited excerpt from an evaluation's Question/Verdict body sections. It cannot read unsupported `verdict` front matter or present an inferred excerpt as authoritative metadata. Decisions show document lifecycle and update date; `decided` dates and acceptance states require explicit body evidence or a separately selected extension. Missing body information remains absent rather than fabricated.
- **Editing:** Details exposes selected metadata as typed controls above the Markdown body. Scope pickers enforce required project plus at most one same-project feature/release/deployment scope, and read-only identity determines the path. Source mode edits the same contract and stays synchronized. Body edits preserve evidence and specialized semantics without introducing unsupported lifecycle/field values.

### 3.2 Changing templates (C17)

**Deferred custom template/kind extension.** The `.templates` paths below are the earlier code-repository proposal, not selected canonical storage. A future schema must preserve selected document scopes, lifecycle and immutable stable IDs; adding a UI category cannot bypass import validation by inventing a SQL kind.

- Templates are files in the repository: `docs/.templates/<kind>.md`, with the fields described in the front matter as a schema (type, required, allowed values, hint) and the sections as headings with hints. The organization has built-in defaults, and a project's files override them.
- **Settings › Docs › Templates** (project): one row per kind with **Edit**. The editor shows fields (add, rename, reorder, required on/off, allowed values) and sections (add, rename, reorder, hint), plus a preview of a new doc. **Save** commits `docs/.templates/<kind>.md`. **Reset to default** is also available.
- **New kinds:** **Add a doc kind…** with a name, a folder and fields, which becomes a new group in the Docs tab (for example "Runbooks"). Built-in kinds can't be deleted.
- **Agents** can read and change templates the same way (`agent-os doc template show|edit <kind>`). A changed template shows in Activity ("ada added the field `risk` to Decision").
- **Existing docs aren't rewritten** when a template changes. New required fields show "Missing: risk" on older docs with **Fill in**, and removed fields are kept and shown under "Other fields".

### 3.1 The agent's side: `agent-os doc` (C16)

**Deferred earlier CLI proposal.** These are earlier command sketches, not available `agent-os` commands or a new wire contract. Their path/branch output requires adaptation to organization Git and selected scope; `--here` cannot put canonical metadata in an arbitrary code worktree.

The retained command words below describe the earlier specialized CLI, including its unsupported `adr` kind/accepted status and same-task-branch convenience; they are historical syntax examples only. Target adaptation must use the selected kind/lifecycle envelope, stable ID/derived organization-Git path, optional display key, admitted attribution and explicit publication result. No next-number allocation or editable stable-ID filename carries forward. `--here`/debt convenience needs an explicit cross-repository staging contract, not a canonical file in a code worktree:

```
agent-os doc new adr "Docs storage" --project ac          → ADR-0004 · docs/adr/0004-docs-storage.md (proposed)
agent-os doc new design|evaluation|spec|plan|review "<title>"
agent-os doc new design "<title>" --here                   writes it into your task worktree, for you to commit
agent-os doc edit ADR-0004                                 opens $EDITOR on the file, then commits on a docs branch
agent-os doc status ADR-0004 accepted                      sets status and decided {on, by} in the front matter, commits
agent-os doc schema adr                                    prints the fields (allowed values, required) and sections
agent-os doc validate [path]                               checks front matter against the template
agent-os doc supersede ADR-0002 --by ADR-0004
agent-os doc link T-A17 ADR-0004                            the "Why" link
agent-os doc list --kind adr --status proposed
agent-os doc show ADR-0004
agent-os debt add "<what I did instead>" [--task T-A17] [--why …] [--fix …] [--cost low|medium|high] [--docs-branch]
```

The historical command vocabulary informed the app proposal. Target CLI/app parity and activity publication still require implementation; display a document's optional key/title while links and operation identity use its stable ID.

## 4. Flow 2 · Decision lifecycle

This specialized approval workflow is deferred; the selected document lifecycle remains Draft/Current/Deprecated and conveys no permission grant.

- **Proposed → Accepted / Rejected:** the reader of a Proposed ADR shows a banner "Proposed by ada · 2 days ago" with **Accept** · **Reject…** (reason required) · **Request changes…** (a message to the owner). A future service must authorize the admitted actor; editable owner/decided fields or a browser connection do not grant acceptance powers. The app suggests the authorized decision owner and records who ("Accepted 2026-11-08 by the user") by recording the decision statement, actor/time evidence and reason in the body, or through a separately selected authenticated approval extension. Selected `status` remains draft/current/deprecated; it must not become accepted/rejected, and no `decided` front-matter field is selected. The proposed change is committed through reviewed organization-Git publication. It shows "Accepted · waiting to merge" until the change lands (§0.2).
- **Proposed decisions reach Needs you** for the owner, one row each: "ada proposes ADR-0004 · Docs storage". Agents ask through messages as today. The row is the same thread.
- **Supersede:** on an Accepted ADR, ⋯ › **Supersede with a new decision** opens a new ADR prefilled with "Supersedes ADR-0002" and the context. Accepting the new one marks the old one Superseded, with links both ways.
- **Editing an Accepted ADR** warns once: "Accepted decisions are a record. Supersede it if the decision changed, or edit only to fix a mistake." **Edit anyway** is allowed.

## 5. Flow 3 · "Why?" everywhere

- **Task, bug and debt pages** get a **Why** block in the side column: the ADRs, designs, specs and evaluations linked to the item or mentioning its number, with status badges. **+ Link a decision** links one. Empty: "No decisions linked. **Link one** if this work follows a decision."
- **Commits** (on items and in Activity) show their Why when the message names a doc ("ADR-0002", "EVAL-0001") or an item that has one.
- **Ask "why?"** (⌘K › "Why is…") searches decisions, evaluations and designs by text and shows the matches with their status and date. The copilot version (smart features #13) answers in a sentence with links.
- **Evaluations answer "what did we try?":** evaluation reports retain explicit Question and Verdict body sections. A proposed specialized view may show their cited excerpts together (“Turso vs SQLCipher → SQLCipher: Turso's encryption is experimental”), while the selected list still groups them as Reports and shows their actual lifecycle. No inferred excerpt becomes an authored kind, verdict field or approval.

## 6. Flow 4 · Tech debt (a doc in the repository)

**Deferred debt schema and workflow proposal.** The file and fields below are the retained extension design, not a selected canonical location or supported import contract. An extension must first decide its mapping to selected decision/report documents or explicitly expand the model. DEBT labels are retained reference examples, not newly authorized counters. Area grouping, automatic payback and Undo are independently deferred.

**The file** (`docs/debt/0012-hard-coded-retry.md`):
```markdown
---
kind: debt
id: DEBT-0012
title: Retry count hard-coded in the office client
status: open                 # open | planned | paid | accepted; planned and paid are written by the tool
cost: medium                 # cost of leaving it: low | medium | high
effort: s                    # to pay it back: s | m | l
area: Office server
taken: { by: ac-software-engineer-river, on: 2026-11-08, task: T-A17, commit: 9c1e0b2, branch: task/T-A17-office-client }
because: [ADR-0004]          # the decision that accepted it, if any
payback: []                  # tasks that pay it back, e.g. [T-A40]
accepted: null               # { on, by, reason } when we decide to live with it
---

## What we did
## Why
## The proper fix
```

**Capture (one sentence is enough):**

| From | How | What's prefilled |
| --- | --- | --- |
| An agent, while working | `agent-os debt add "<what I did instead>"` would create an organization-Git knowledge proposal linked to the code branch/commit. The earlier same-code-branch write assumption is superseded; cross-repository staging/recovery remains undecided | the agent's current task, branch and commit, and the area from the task's team |
| Finishing a task | Marking a task Done asks "Did you take a shortcut?" **No** · **Yes, record it**. Agents get the same question from the CLI (`task done --debt "…"`, or `--no-debt`) | the task, branch, commits |
| A review | A finding the author leaves unfixed → **Record as debt** on the finding (the reviewer's verdict can list "Deferred" findings) | the review, the finding's text, the commit |
| An ADR | each line under "Tech debt accepted" becomes a debt doc whose `because` is the ADR | the ADR |
| Code | `TODO(debt): <text>` or `TODO(DEBT-0012)` in a commit's diff is offered as a draft ("2 possible debt items found") | file and line |
| Anyone | the project's Debt tab › **Record debt** | — |

**Record debt** (slide-over, generated from the debt template's fields, §3.0): **What we did** * (the title, 1–200 characters) · **Why** · **The proper fix** · **Cost of leaving it** Low · Medium · High ("Low: harmless for now" · "Medium: slows us down or risks bugs" · "High: will hurt users, security or data, or blocks a phase") · **Effort to pay back** S · M · L · Area · Links. **Record debt** commits the file (from the app: to the docs branch). Only "What we did" is required. A doc with empty sections shows "Needs details".

**Debt tab** (project, drawn from the files' front matter): grouped by area, sorted by cost, then effort (High cost, S effort first: "quick wins" at the top), with filters Status · Cost · Effort · Taken by · Area. A summary bar: "14 open · 3 High · 5 quick wins · 2 paid this week". Debt still on an unmerged branch shows with a "On task/ac-T-17-office-client, not merged" badge.

**Statuses:** **Open · Planned · Paid · Accepted.** Creating a payback task (a durable typed task) adds it to `payback` and sets Planned. When all payback tasks are Done and at least one is **Done · Completed**, the tool sets Paid in a commit on a docs branch, which lands through the merge flow (activity entry, with Undo as another commit). If they were all cancelled, the debt stays Planned. *(rev 8)* The tool writes Planned and Paid only once the debt doc is merged into the integration branch (it never writes on an agent's task branch). Until then the debt shows "**Planned once merged**" (or "Paid once merged"), and the status is written when the doc lands. **Accept…** needs a reason, written to `accepted`, and can name the ADR in `because`.

**Debt page:** the header card (§3.0) · What we did · Why · The proper fix · **Taken by** ("Taken by River in T-A17 · 9c1e0b2 · from git") · **Why** block (§5) · **Create payback task** · history (the file's commits).

**Where debt shows up besides its tab:** the task it was taken in ("Debt taken: DEBT-0012"), the commit, the area's header in the Bugs tab ("Payments · 2 open debt"), the project Overview tile ("Tech debt 14 · 3 High"), and Work › **Debt** (the third pill: Tasks | Bugs | Debt | Requests).

**For agents:** the tool asks at the right moment through agent guidance (§6.2), and every agent's manual adds one rule: "When you take a shortcut (skip a test, hard-code, defer a refactor, work around a bug, leave a TODO), record it with `agent-os debt add` in the same session." The task-Done question backs this up.

### 6.1 Known limitations and exceptions (docs in the repository)

**Deferred specialized kinds/lifecycle.** The earlier `docs/limitations/` and `docs/exceptions/` examples below describe earlier extension templates only. Selected feature limitations belong in `feature.md`; project-wide findings may use a report and rule rationale a decision. No limitation/exception counters, area model, approval or expiration engine are selected.

**Known limitation** (`docs/limitations/0003-<slug>.md`, `LIM-0003`): what the product doesn't do or doesn't handle, so nobody reports it as a bug or rediscovers it.
- Front matter: `kind: limitation` · `id` · `title` ("Offices can't sync while HQ is unreachable for more than 30 days") · `status: current | lifted` · `scope` (area or feature) · `since` (version or date) · `because: [ADR-…]` · `lifted_by` (task or version, when lifted) · `debt: [DEBT-…]` (when it's also debt).
- Sections: What doesn't work · Why · Workaround · When it might change.
- **Where it shows:** the project's Docs tab (**Limitations** group), and **in Report bug**: when the title matches a current limitation, the form says "This may be a known limitation: LIM-0003 · Offices can't sync after 30 days offline. **Read it** · Report anyway". Bugs closed as Won't fix can offer **Record as known limitation**.

**Exception** (`docs/exceptions/0002-<slug>.md`, `EXC-0002`): a deliberate, approved departure from a rule, a standard or a decision (for example from `architecture-rules.md` or an ADR).
- Front matter: `kind: exception` · `id` · `title` · `status: proposed | approved | expired | revoked` · `rule` (what it departs from: a doc and section, e.g. `architecture-rules.md#efferent-coupling`, or `ADR-0002`) · `scope` (module, area or files) · `approved: {on, by}` · `review_by` (a date) · `debt: [DEBT-…]`.
- Sections: The rule · What we do instead · Why · Risk · How we'll get back in line.
- **Lifecycle:** Proposed → **Approve** (Needs you for the owner, like proposed ADRs) → Approved. On `review_by` it shows in Needs you: "EXC-0002 is due for review: keep, renew or revoke". Past that date, it's shown as **Expired** in amber until decided.
- **Where it shows:** the Docs tab (**Exceptions** group), the rule's doc ("2 approved exceptions to this rule"), and the area.

### 6.2 Agent guidance: the prompts that steer agents (rev 5, the user)

**Deferred configurable guidance engine.** Moment storage, scheduling/wakes, answers and process/checklist composition below remain a proposal. The earlier `docs/.guidance/` is not a selected organization-data path; durable configuration schema and privacy must be decided before implementation.

The tool helps agents work the way the organization wants by **prompting them at set moments**, like the shortcut question. These prompts are a product feature, and the owner shapes them to their own way of working.

| Moment | Default guidance (editable) | Where the agent sees it |
| --- | --- | --- |
| Starting a task | "Before you start {task}, read its Why: {decisions}. Known limitations in {area}: {limitations}." | `agent-os task start`, the inbox adapter's first message |
| Making a design or architecture choice | "If this choice will matter later, record it: `agent-os doc new adr \"…\"`." | when an agent creates a branch or a design doc |
| Taking a shortcut | "Did you take a shortcut (skipped a test, hard-coded, deferred a refactor, worked around a bug)? Record it: `agent-os debt add \"…\"`." | `agent-os task done`, the app's Done |
| Departing from a rule | "This goes against {rule}. Propose an exception: `agent-os doc new exception`." | when a review or check names a rule |
| Finishing a task | the project's checklist (work-items §9.1) and "Is anything here a known limitation? `agent-os doc new limitation`." | `agent-os task done` |
| Asking the owner | "Say what you need, your recommendation and what happens if there's no answer." | `agent-os msg ask --to owner` |
| Waking up (on-call and scheduled agents) | "Read your inbox and backlog, do what's due, record what you did, then stop." (`agent-run-modes-ux.md` decision 6) | at every wake |
| Procedure changed | "New from <retro>: <the change>." (`retrospectives-ux.md` decision 7) | at the agent's next start or wake after the change merges |
| Posting to chat | "Post only what someone must answer, act on or know. Put details in the task or doc and link it." (`chat-ux.md` §5) | `agent-os msg` |

- **Settings › Agent guidance** (organization) and project Settings › Agent guidance (overrides): one row per moment with **On/Off**, the text (with placeholders like `{task}`, `{area}`, `{decisions}` and a picker to insert them), and a **Preview as** an agent and task ("What River will see on T-A17").
- **Add guidance…** adds a moment from a list (task started, task done, bug fixed, branch created, review requested, message to the owner) with its own text, for the owner's own practices ("Always add a screenshot for UI work").
- **Processes add to it** *(rev 9)*: a process can add its own prompt to a moment (`processes-ux.md` decision 9). It's shown after this text, and only while the moment is On.
- **Where it's kept:** a project's guidance is persistent, so it's a file in the repository (`docs/.guidance/<moment>.md`, Markdown with front matter: `moment`, `enabled`). The organization's defaults are settings in the office. A change shows in Activity ("You changed the guidance for Taking a shortcut").
- **Agents' answers are recorded:** "No shortcut" or a debt id shows on the task, so the owner can see the guidance working (and turn it down if it's noise).

## 7. Bringing our own project in (dogfood)

Withdrawn (owner, 2026-11-17). An earlier revision described an owner-run import of this repository's documents into an organization, with a per-file preview. It is not built in v1: the repository's documents stay in the repository, and the team writes new decisions and designs in the product. It may be proposed again later as an ordinary feature.

- Debt scanning from TODOs, review findings and “Not covered here” remains a deferred capture proposal. Draft candidates require Record/Skip, never silent record creation.

## 8. UX acceptance criteria

Criterion ids for debt, limitations and exceptions use **DBT-**, **LMT-** and **EXN-** so they never collide with the records' ids (`DEBT-0012`, `LIM-0003`, `EXC-0002`) *(renamed in rev 7, before any test used them)*.

**Docs and decisions (DEC)**
- DEC-1 · The Docs tab groups selected kinds/scopes with counts/status and one living project brief covering what/who/current behavior/version evidence. Capability requirements stay in feature.md; no duplicate Overview/PRD kind or per-release copy is created.
- DEC-2 · Each document shows selected front matter as a header card: kind, optional key, title, Draft/Current/Deprecated lifecycle, owner, scope, attribution/dates and authored links. Stable ID/path is read-only, no document number field is assumed, and the section rail follows body headings.
- DEC-3 · New Decision/Design/Evaluation opens the selected envelope and appropriate body template, derives the stable-ID file path and shows its scope; no numeric document counter is assumed and Save is explicit.
- DEC-4 · **Deferred specialized approval workflow:** Accept/Reject/Request changes may propose decision-body changes with actor/time evidence and a reason, or use a separately selected authenticated approval extension. They cannot put accepted/rejected in selected document status or invent decided front matter. Proposed publication remains visible until merged.
- DEC-5 · **Deferred specialized editor/approval workflow:** Proposed ADRs appear in the owner's Needs you, one row each.
- DEC-6 · **Deferred specialized editor/approval workflow:** Supersede drafts a new decision body linked to the old stable record and proposes reciprocal supersession evidence. Preserve selected Draft/Current/Deprecated lifecycle; a specialized Superseded state needs a separately selected extension.
- DEC-7 · Invalid metadata names the field and offers Fix while retaining a readable body. Display-key clashes show both identities/provenance and Resolve key; stable IDs never change.
- DEC-8 · Deprecated content collapses under Older; richer superseded/rejected/obsolete classifications remain proposed decision/body metadata.

**Why and evaluations (WHY)**
- WHY-1 · Task, bug and debt pages show a Why block with the linked or mentioning decisions, designs, specs and evaluations, with status, and + Link a decision.
- WHY-2 · A doc's reader lists Led to (items and docs that link it) and Mentioned in.
- WHY-3 · Evaluation content is a report. A proposed evaluation view may show cited Question/Verdict body excerpts in one line, distinguishing excerpts from selected metadata; no evaluation kind or verdict front-matter field is assumed.
- WHY-4 · Knowledge search finds permitted titles, stable IDs/keys and body text with scope/status and body excerpts; feature results link to their canonical definitions. Declare actual scope/freshness/partial indexing and exclude private notes, protected chat and inaccessible records. A title-only adapter is visibly limited.

**Tech debt (DBT)**
- DBT-1 · **Deferred debt workflow:** Record debt needs only "What we did". Missing fields show "Needs details" on the item.
- DBT-2 · **Deferred debt workflow:** Marking a task Done asks "Did you take a shortcut?", and Yes opens Record debt prefilled with the task, branch and commits.
- DBT-3 · **Deferred debt schema:** the earlier docs/debt template and DEBT reference proposal is not selected. Area membership requires a separate decision.
- DBT-4 · **Deferred debt workflow:** The Debt tab groups by area, sorts by cost then effort, and shows the summary bar with open, High, quick wins and paid this week.
- DBT-5 · **Deferred debt workflow:** *(rev 8)* A debt doc becomes Planned when a payback task is linked and Paid when all of them are Done and at least one is Done · Completed, each written to the front matter in a commit on a docs branch (never the default branch), with an activity entry and Undo. While the debt doc isn't merged into the integration branch yet, it shows "Planned once merged", and the status is written after it lands.
- DBT-6 · **Deferred debt workflow:** Accept needs a reason and can link the ADR that accepts it.
- DBT-7 · **Deferred debt workflow:** Each line under "Tech debt accepted" in an Accepted ADR is offered as a debt item linked to that ADR.
- DBT-8 · **Deferred debt workflow:** A review finding can be recorded as debt with the finding and commit linked.
- DBT-9 · **Deferred debt workflow:** The task page shows the debt taken in it, and the project Overview shows a Tech debt tile.
- DBT-10 · **Deferred debt workflow:** *(rev 4)* Work has a third pill, Debt, across projects. Debt on an unmerged branch is shown with its branch.
- DBT-11 · **Deferred debt workflow:** Draft debt found by a scan (TODOs, review findings, "Not covered here") is never created without Record.

**Our project (OURDOC)**
- OURDOC-1 · Withdrawn 2026-11-17: no import of repository documents in v1.
- OURDOC-2 · Withdrawn 2026-11-17: no import of repository documents in v1.

**Authoring through the tool (AUTH)**
- AUTH-1 · Tool authoring writes selected documents at canonical organization-Git scopes with stable IDs, required metadata and admitted attribution; reviewed branch publication remains a proposal. Code-task --here convenience requires explicit cross-repository adaptation.
- AUTH-2 · **Deferred specialized editor/approval workflow:** a future status edit uses only Draft/Current/Deprecated and selected revision/attribution. Decision acceptance/rejection semantics stay in explicit body evidence or a separately selected extension; the earlier accepted/decided front-matter writes are superseded.
- AUTH-3 · Canonical files changed directly in organization Git are validated/indexed from its state branch without losing content, stable identity or provenance. Code-repository files are import sources, not automatically canonical.
- AUTH-4 · Proposed activity records link creation/content/link changes to the admitted actor and stable document ID, displaying its optional key/title; no document number counter or editable identity is assumed. Specialized approval/supersede activity remains deferred.
- AUTH-5 · **Deferred CLI adaptation:** list/show use the same selected kinds, optional keys, stable identities, lifecycle and scope as the app; the earlier numbered ADR/EVAL kinds/statuses are not target fields.
- AUTH-6 · **Deferred debt capture:** a one-sentence shortcut can prefill task/branch/commit evidence; preserve task details.shortcut tagged IDs. Canonical knowledge cannot silently be placed on a code task branch.
- AUTH-7 · Commit co-authors and from-git fields are displayed as evidence without granting authority. Feature lifecycle facts remain in feature.md; progress derives from task membership and valid file edits are not overwritten from office-only state.

**Known limitations (LMT)**
- LMT-1 · **Deferred limitation workflow:** Limitations are docs in `docs/limitations/` with front matter, listed in the Docs tab's Limitations group with status Current or Lifted.
- LMT-2 · **Deferred limitation workflow:** Report bug shows a matching current limitation with Read it and Report anyway.
- LMT-3 · **Deferred limitation workflow:** A bug closed as Won't fix offers Record as known limitation, prefilled from the bug.

**Exceptions (EXN)**
- EXN-1 · **Deferred exception workflow:** Exceptions are docs in `docs/exceptions/` with front matter naming the rule, scope, approver and review date.
- EXN-2 · **Deferred exception workflow:** A proposed exception appears in the owner's Needs you with Approve and Reject.
- EXN-3 · **Deferred exception workflow:** On its review date, an exception appears in Needs you with Keep, Renew and Revoke, and past the date it shows as Expired.
- EXN-4 · **Deferred exception workflow:** The rule's doc lists its approved exceptions.

**Agent guidance (GUIDE)** *(rev 5)*
- GUIDE-1 · **Deferred guidance automation:** Settings › Agent guidance lists each moment with On/Off, editable text with placeholders, and Preview as an agent and task.
- GUIDE-2 · **Deferred guidance automation:** A project's guidance overrides the organization's and is stored in `docs/.guidance/<moment>.md`.
- GUIDE-3 · **Deferred guidance automation:** `agent-os task done` shows the Taking a shortcut guidance, and the answer (no shortcut, or a debt id) is recorded on the task.
- GUIDE-4 · **Deferred guidance automation:** Add guidance adds a moment from the list with the owner's own text.
- GUIDE-5 · **Deferred guidance automation:** Changing guidance appears in Activity, and turning a moment off stops its prompt everywhere (app and CLI).

**Templates (TPL)**
- TPL-1 · Every kind has a template (fields with type, required, allowed values and hint, plus sections with hints), and New fills a doc from it.
- TPL-2 · The editor's Details panel is generated from the template's fields, and the Source toggle edits the YAML directly, with both kept in sync.
- TPL-3 · `agent-os doc schema <kind>` prints the fields and sections, and `doc validate` reports each front-matter problem by field.
- TPL-4 · **Deferred specialized editor/approval workflow:** Settings › Docs › Templates edits a kind's fields and sections with a preview, saves by committing `docs/.templates/<kind>.md`, and Reset to default restores it.
- TPL-5 · **Deferred specialized editor/approval workflow:** Add a doc kind creates a new kind with its folder and fields, which shows as a group in the Docs tab. Built-in kinds can't be deleted.
- TPL-6 · **Deferred specialized editor/approval workflow:** Changing a template never rewrites existing docs. New required fields show "Missing: <field>" with Fill in, and removed fields show under "Other fields".
- TPL-7 · Moving or indexing a doc with a bold status line offers Add front matter with a per-file diff, Apply or Skip.
- TPL-8 · Every selected document has an immutable stable ID separate from its optional display key. IDs use UUIDv7. Title/key changes preserve stable-ID filenames and relationships.

## 9. Open questions

- C11 / TPL-7 · Front-matter import remains useful; map to selected fields and show a per-file diff, never fabricate provenance.
- C17 / TPL-4–6 · Deferred custom schema/template storage and extensibility; earlier docs/.templates locations are not selected organization-data paths.
- C12 / C18 / AUTH-6 · Deferred debt subsystem and cross-repository capture/recovery. Preserve tagged shortcut IDs without inventing debt tables.
- C16 · The earlier doc/debt CLI command proposal needs a supported agent-os contract and selected organization-Git scope mapping before availability is claimed.
- C13 / A36 · Organization knowledge uses state_branch; required integration_branch belongs to each code-repository association. Reviewed metadata publication still needs implementation design.
- A18 / A19 / A23 / A24 · Historical software docs remain import sources. Selected organization files use stable-ID paths, while protected operational records remain outside shared Git.
- A20 / U14 · Record useful rationale and shortcuts; automatic guidance/capture is independently deferred.
- U15 · Deferred decision approval/delegation policy must authenticate actors; document owner/status fields grant no security authority.

- Project navigation follows the [integrated IA](work-items-ux.md#3-flow-1--project-page); it does not require a Features/Docs tab before those experiences are served. Project Building/Live transitions, doc comments, approval/Inbox adapters and frozen release-note workflows remain proposals. Selected project/feature/document lifecycles and explicit revision/correction history take precedence; supporting content does not select those engines.
