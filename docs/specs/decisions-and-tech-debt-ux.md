---
kind: spec
title: "Decisions, evaluations, designs and tech debt (UX)"
status: for-review
revision: 7
owner: ac-product-designer-eden
created: "2026-11-08"
updated: "2026-11-08"
requested_by: owner
reviewers: [ac-staff-engineer-ada, codex-dev, owner]
builds_on: [work-items-ux, our-org-in-the-app]
---

# Spec · Decisions, evaluations, designs and tech debt (UX)

Extends `work-items-ux.md` rev 5 (Docs, work items) and `our-org-in-the-app.md` (our own project in the app).

## The request (the user, 2026-11-08)

> Capture tech debt: every shortcut or quicker implementation an agent takes should be captured. Have a structure for ADRs and design documents in a project, so agents can use them. Help us find **why a decision was made** and **what technology we evaluated**. And bring this project's own designs into the tool that manages it.
>
> These are features of the product: agents don't write them in their own folders any more. They write them **through the tool**, which saves them in the project, in a proper structure, in its repository. *(the user, rev 2)*

**Revision 7 (2026-11-08): reviewer fixes.** §0.2 now says the docs flow **needs** an extension of the git design (A36), since that design lets the office push only an agent's own `task/…` branch and leaves merging for later. ada agreed office-created `docs/<slug>-<short-id>` branches (A36). "Integration branch" replaces "docs branch" where it meant the branch the Docs tab reads. Branch examples follow `task/<task-id>-<slug>`. The kind list matches §0.1. The Spec template includes `created` and `updated`, and member fields hold member ids.

**Revision 6 (2026-11-08): doc changes land through branches (review fix).** Following `docs/design/projects-git-github.md` (never push to the default branch; the owner is the commit author, the agent is co-author): every doc change is a commit on a task or docs branch that lands through the merge flow, and the app shows "Waiting to merge" (§0.2). DEC-4, DBT-5, AUTH-1 rewritten. C13 partly answered.

**Revision 5 (2026-11-08): agent guidance (the user).** The shortcut question, and every prompt the tool uses to steer agents, is **agent guidance**: short prompts at set moments that the owner can customize per organization and per project (§6.2). U14 resolved (yes, as guidance), U15 my recommendation. GUIDE-1 to GUIDE-5 added (50 criteria).

**Revision 4 (2026-11-08): what lives in the repository and what doesn't (the user).** **Persistent knowledge goes in the repository** as Markdown with front matter: decisions, designs, specs, evaluations, architecture and design reviews, plans, **tech debt**, **known limitations** and **exceptions**. **Transitional items stay in the office database:** tasks, bugs, internal queues, messages and other things that are acted on, fixed and can be deleted. Tech debt therefore becomes a doc kind (`docs/debt/`, `DEBT-0012`) instead of a work item. Two new kinds: **Known limitation** (`docs/limitations/`) and **Exception** (`docs/exceptions/`), §6.1. A23 resolved, C12 rewritten, new C18. DBT-3, DBT-5, DBT-10, AUTH-6 rewritten. LMT-1 to LMT-3 and EXN-1 to EXN-4 added (45 criteria).

**Revision 3 (2026-11-08): Markdown with front matter, and editable templates (the user).** Every doc is Markdown with **YAML front matter** that follows its kind's template (§3): the UI presents it as a header card and sections, and agents read the same structure (`org-agent doc schema`). Templates live in the repository and can be changed in the app or by agents (§3.2). The status line is gone as the source (decision 2 rewritten), and existing docs are converted when they move in (§7). C11 resolved. DEC-2, DEC-4, DEC-7, AUTH-1, AUTH-2 rewritten. TPL-1 to TPL-6 added (41 criteria).

**Revision 2 (2026-11-08):** decision 0 (authored through the tool, stored in the repo) and the folder structure (§0.1), the CLI surface for agents (§3.1), our existing docs **move** into the repo instead of being linked (§7), A19 resolved, new criteria AUTH-1 to AUTH-6 (35 in all).

**What "great" looks like:** anyone, the owner or an agent, can open a piece of code, a task, a bug or a commit and answer in one or two clicks: *why is it like this? what else did we try? what did we knowingly leave unfinished, and what will it cost us?*

## How the team works today (the structure to keep)

ada already writes decisions and designs in a consistent way. The app adopts it rather than inventing a new one.

| Kind | Where today | Shape today |
| --- | --- | --- |
| ADR | product repo `docs/adr/0001-…md` (current), ada's `knowledge/adrs/0001–0009` (v0, pre-offices) | `# ADR-0002 · Title`, then a status line "Status: **accepted** 2026-11-08 (the user). Owner: ada. Refines ADR-0001 decision 2.", then Context · Decisions (numbered) · Consequences · Not covered here |
| Evaluation (spike) | inside ADRs ("Spike S1 confirmed it: p50 ~100 ms"; "we looked at Turso…"; "the user asked for Maestro to be evaluated") and spike READMEs in worktrees | free text with evidence and numbers |
| Design | product repo `docs/design/*.md`; drafts in ada's `knowledge/designs/drafts/` | "Status: **draft**. ada writes it with…", then Goal · The flow · … |
| UX spec | Eden's `knowledge/specs/*.md` | Owner/Status line, changelog, decisions, flows, criteria, questions |
| Review | ada's `knowledge/reviews/`, the reviewer's verdicts | summary with severity counts, findings |
| Plan | ada's `knowledge/plans/phase-1.md` | workstreams table with owners |
| Tech debt | nowhere: it's scattered in "Not covered here", review findings left unfixed, and 3 TODOs in the code | — |

## 0. Decisions in this spec

0. **Written through the tool, kept in the project's repository.** Decisions, designs, evaluations, specs, plans, reviews and the brief are product features. Agents create and change them with `org-agent` (§3.1) and people with the app. The tool writes the file into the project's repository in the structure below and commits it on a branch that lands through the merge flow (§0.2). Agents' home folders keep only their own memory and journal, so no project knowledge lives there any more. Editing the files directly with git still works (agents are developers), and the tool reads them back, but the tool is the normal way and the only one that numbers, templates, links and records status changes.

### 0.1 The project structure (in the repository, on the integration branch)

```
docs/
  brief.md                 the project brief (pinned)
  adr/0004-<slug>.md       decisions (ADR-0004)
  design/<slug>.md         designs
  evaluations/0002-<slug>.md   evaluations (EVAL-0002)
  specs/<slug>.md          UX specs
  plans/<slug>.md          plans (phases, workstreams)
  reviews/<date>-<slug>.md reviews (architecture, design, UX)
  features/0007-<slug>.md  features (FEAT-0007, `features-ux.md`)
  retros/<date>-<slug>.md  retrospectives (`retrospectives-ux.md`)
  debt/0012-<slug>.md      tech debt (DEBT-0012)
  limitations/0003-<slug>.md   known limitations (LIM-0003)
  exceptions/0002-<slug>.md    exceptions to a rule or decision (EXC-0002)
  reports/<date>-status.md status reports, weekly or monthly (`project-status-ux.md`)
  <anything else>.md       other docs
```

**The rule (the user, rev 4): persistent in the repository, transitional in the database.** Anything the team will want to read in a year (why we built it this way, what we evaluated, what we knowingly left unfinished, what doesn't work and why, which rules we bent) is a file here. Tasks, bugs, messages, queues, sessions, statuses and other working state are office records: they're acted on, closed and can be deleted, and they never go into the repository. Docs link them by number, and they link docs by id.

### 0.2 How a doc change lands (rev 7: extends `docs/design/projects-git-github.md`, A36 agreed with ada)

- **Never straight to the default branch.** Every doc change, whether a new doc, an edit, an Accept or a status change, is a commit on a branch: an agent's own `task/<task-id>-<slug>` branch, or for changes made in the app a **docs branch** the office creates (`docs/<slug>-<short-id>`). It lands through the project's normal merge flow. For us: an independent review, the reviewer's READY, ada's go, and a merge into `v1`.
- **Docs branches (ada, A36):** for edits made in the app, the office creates `docs/<slug>-<short-id>` branches (owner as author, the editing agent or the app as co-author), with no stand-in tasks. Agents' own edits ride on their `task/…` branches. Docs merge through the **same flow as code**: the project's configured reviewer, the go, then the merge. "Waiting to merge" covers both. This extends `docs/design/projects-git-github.md`, which ada updates.
- **The app shows that honestly.** A changed doc reads "**Waiting to merge** · on docs/payment-step-a7f3 · 1 change" with **View change**. The reader shows the merged text, and a toggle shows the proposed one. An Accept shows as "Accepted · waiting to merge". Lists show the merged state, with a small "1 change waiting" marker.
- **Authorship follows the commit-identity decision:** commits are authored under the owner's name and no-reply address, with the agent as `Co-authored-by`. The app shows the agent who made the change ("ada · committed as you"), read from the co-author trailer, and "you" for changes the owner made.
- **Which branch the Docs tab reads** is the project's integration branch (`v1` for us, `main` elsewhere), a project setting (C13).

1. **Document kinds, one Docs tab:** **Features · Decisions (ADRs) · Designs · Evaluations · Specs · Plans · Reviews · Retrospectives · Debt · Limitations · Exceptions · Reports · Other** (guides, and the brief pinned on top), as listed in §0.1. Every kind is a Markdown file in its folder (§0.1), and the kind comes from the folder.
2. **Markdown with front matter, shaped by a template.** Each doc starts with YAML front matter whose fields come from its kind's template (kind, number, title, status, owner, dates, relations), followed by the template's sections as `##` headings. The UI turns the front matter into a header card and the sections into a navigable page. Agents get the same structure from `org-agent doc schema <kind>`. *(rev 3: replaces the parsed status line.)*
3. **ADR statuses:** **Proposed · Accepted · Superseded · Rejected · Deprecated.** A superseded ADR names its replacement, and both link each other.
4. **Design statuses:** **Draft · In review · Approved · Built · Obsolete.** Evaluations: **Planned · Running · Done** (with a verdict).
5. **An evaluation is its own small doc** with a fixed shape: the question, the options tried, the criteria, the evidence (numbers, links, commits), and the verdict. ADRs link to the evaluations they rest on ("Evidence: Evaluation 0004 · Turso vs SQLCipher"). This is what makes "what tech did we evaluate?" answerable. Today it's buried in ADR prose.
6. **"Why?" is a link on everything.** Tasks, bugs, debt, commits, files and docs show a **Why** section listing the decisions, designs and evaluations they're linked to or that mention them. A decision lists what it led to (tasks, debt, later ADRs).
7. **Tech debt is a persistent doc, not a work item** *(rev 4)*: `docs/debt/0012-<slug>.md`, numbered `DEBT-0012`, with front matter (status, cost, effort, area, taken by, links). It's listed in the project's **Debt** tab and paid back through tasks (database), which link it. Paid debt stays in the repository as history.
8. **Debt is captured at the moment of the shortcut, by whoever takes it,** mostly agents: from the CLI, when finishing a task ("Did you take a shortcut?"), from a review finding left unfixed, from a `TODO(debt)` in code, or from a decision that accepts it. Capturing takes one sentence, and the rest can wait.
9. **Debt is never blocking and never shaming.** It's a record of a trade-off, with who took it and why, not a bug count. The words are neutral: "shortcut", "pay back", "accepted".
10. **Our project comes in with its history.** Bringing in the Agentic Org project (dogfood Flow 1) indexes `docs/` from the product repo and offers ada's v0 ADRs as **Earlier decisions (v0)**, kept apart so the two ADR-0001s never collide (A18).

## 1. Words (glossary additions)

| Term | Means | Don't say |
| --- | --- | --- |
| Decision (ADR) | A recorded architecture decision: context, decision, consequences | Ruling, RFC |
| Design | How something will work, written before it's built | Proposal doc, PRD |
| Evaluation | A comparison or experiment that answers a question with evidence | Spike (the team's word: shown as "Evaluation (spike)"), POC |
| Spec | Eden's UX specs: flows, states, criteria | Requirements |
| Tech debt / Debt | A known shortcut whose proper fix is postponed | Hack, mess, TODO |
| Pay back | Doing the proper fix | Clean up, refactor (in labels) |
| Accepted debt | Debt we decided to live with, and why | Won't fix (that's bugs) |
| Known limitation | Something the product doesn't do or doesn't handle, on purpose or for now, with its workaround | Missing feature, gap, bug |
| Exception | A deliberate, approved departure from a rule, standard or decision, with its scope and when it's reviewed | Waiver, override, hack |
| Why | The decisions, designs and evaluations behind an item | Rationale, context (in labels) |

## 2. Flow 1 · Docs tab, by kind

```
 Docs   [New ▾]  [Search docs]                       ┌───────────────────────────────────────────────┐
 ┌──────────────────────────────┐                    │ ADR-0002 · Storage: SQLite with SQLCipher,     │
 │ 📌 Brief                     │                    │ one writer                                     │
 │ DECISIONS · 3                │                    │ ● Accepted 2026-11-08 by the user · Owner ada  │
 │   ADR-0003 Messaging…  Prop. │                    │ Refines ADR-0001 · Evidence: EVAL-0001 Turso   │
 │   ADR-0002 Storage…    Acc.  │                    │ ─────────────────────────────────────────────  │
 │   ADR-0001 App is a…   Acc.  │                    │ Context …  Decisions …  Consequences …         │
 │   Earlier decisions (v0) · 9 │                    │ ─────────────────────────────────────────────  │
 │ DESIGNS · 4                  │                    │ Led to: ac-12 (task), DEBT-0012, ADR-0003      │
 │ EVALUATIONS · 2              │                    │ Mentioned in: 6 items · docs/adr/0002-….md     │
 │ SPECS · 5                    │                    └───────────────────────────────────────────────┘
 │ OTHER · 3                    │
 └──────────────────────────────┘
```

- **Groups** by kind, each with a count. Inside: number (when the kind has one), title and a status badge (icon + word). Superseded, Rejected, Deprecated and Obsolete docs collapse under "Older (n)".
- **New ▾:** Doc · **Decision (ADR)** · **Design** · **Evaluation**. Each opens the editor with its template (§3) and the next number ("ADR-0004"), and the file name is shown and editable (`docs/adr/0004-….md`).
- **Header** (reader): the header card from the front matter (§3.0): title, status badge, date and by whom, owner, relations as links ("Refines ADR-0001", "Superseded by ADR-0005", "Evidence: EVAL-0001"), and path.
- **Footer:** **Led to** (items and docs linked from this one, or that link it as their Why) and **Mentioned in**.
- **Filters:** Status · Owner · search. **Earlier decisions (v0)** is a read-only group with a note: "From ada's notes before offices. Some are superseded by current decisions." (A18).
- **States:** a doc whose front matter is missing or invalid shows "Details not recognized: <field> <problem>" with **Fix** (opens the Details panel), and the body still shows, and the doc still shows · a numbering clash (two files claim ADR-0004) shows both with "Two decisions use number 0004" and **Renumber…** · everything else as work-items Flow 8 (conflicts, history, git errors).

## 3. Templates: front matter and sections

Every kind has a **template**: the front-matter fields (name, type, required or optional, allowed values) and the sections (headings, with a one-line hint each). New docs start from it, the UI is drawn from it, and the CLI validates against it.

**Decision (ADR)**, `docs/adr/0004-docs-storage.md`:
```markdown
---
kind: adr
id: ADR-0004
title: Docs storage
status: proposed            # proposed | accepted | superseded | rejected | deprecated
owner: ac-staff-engineer-ada  # a member id (agent or person)
created: 2026-11-08
decided: null               # set on accept/reject: { on: 2026-11-09, by: owner }
supersedes: []              # [ADR-0002]
superseded_by: null
refines: [ADR-0001]
evidence: [EVAL-0002]       # evaluations it rests on
related: [ac-17, DEBT-0012] # tasks, bugs, debt
---

## Context
## Options considered
## Decision
## Consequences
## Tech debt accepted
## Not covered here
```

**Design** (`docs/design/`): `kind: design` · `title` · `status: draft | in-review | approved | built | obsolete` · `owner` · `reviewers: []` · `approved: {on, by}` · `adrs: []` · `specs: []` · `related: []`. Sections: Goal · Users and jobs · The flow · States · Open questions.

**Evaluation** (`docs/evaluations/`): `kind: evaluation` · `id: EVAL-0002` · `title` (the question) · `status: planned | running | done` · `owner` · `options: [Turso, SQLCipher]` · `verdict: SQLCipher` · `verdict_reason: "Turso's encryption is experimental"` · `decides: [ADR-0002]`. Sections: Question · Options · Criteria · Evidence (a table: option × criterion) · Verdict · Left open.

**Spec** (`docs/specs/`): `kind: spec` · `title` · `status: draft | for-review | approved | built` · `revision: 3` · `owner` · `created` · `updated` · `requested_by` · `reviewers: []` · `builds_on: []`. Sections: Goal · Decisions · Flows · States · Acceptance criteria · Open questions · Changelog.

**Plan** (`docs/plans/`): `kind: plan` · `title` · `status: active | done | dropped` · `owner` · `period: {from, to}`. Sections: Goal · Workstreams · Finish line · Risks.

**Review** (`docs/reviews/`): `kind: review` · `title` · `subject` (a doc id, branch or commit) · `reviewer` · `verdict: ready | changes-needed | blocked` · `findings: {blocker: 1, high: 7, medium: 19, low: 13}`. Sections: Summary · Findings (each with severity, evidence and fix) · For the user.

### 3.0 How the UI presents a doc

```
 ┌──────────────────────────────────────────────────────────────────────────────┐
 │ DECISION · ADR-0004                                    ● Proposed             │
 │ Docs storage                                                                  │
 │ Owner [A] ada · created 8 Nov · Refines ADR-0001 · Evidence EVAL-0002         │
 │ Related: ac-17 (task) · DEBT-0012                     [Accept] [Reject…] [⋯]  │
 ├──────────────┬───────────────────────────────────────────────────────────────┤
 │ Context      │ ## Context                                                     │
 │ Options      │ …                                                              │
 │ Decision     │                                                                │
 │ Consequences │                                                                │
 │ Debt accepted│  (each line here shows a "Record as debt" chip, §6)            │
 └──────────────┴───────────────────────────────────────────────────────────────┘
```

- **Header card** from the front matter: kind and number, title, status badge, people as avatars (shape + name), dates in words, and relations as chips with the target's status ("EVAL-0002 · Done → SQLCipher"). The front matter itself is never shown raw in the reader.
- **Section rail** from the `##` headings, with the template's sections marked. A template section that's missing or empty shows a quiet hint in place ("No consequences written yet"), never an error.
- **Lists** (the Docs tab groups, Decisions, Evaluations) are drawn from front matter: the Evaluations list shows `title → verdict` and the Decisions list shows status and decided date, with no need to open the file.
- **Editing:** the editor has a **Details** panel (a form generated from the template's fields: selects for the allowed values, member pickers, item and doc pickers for relations) above the Markdown body. Writing YAML by hand is possible (a **Source** toggle), and the form and source stay in sync.

### 3.2 Changing templates (C17)

- Templates are files in the repository: `docs/.templates/<kind>.md`, with the fields described in the front matter as a schema (type, required, allowed values, hint) and the sections as headings with hints. The organization has built-in defaults, and a project's files override them.
- **Settings › Docs › Templates** (project): one row per kind with **Edit**. The editor shows fields (add, rename, reorder, required on/off, allowed values) and sections (add, rename, reorder, hint), plus a preview of a new doc. **Save** commits `docs/.templates/<kind>.md`. **Reset to default** is also available.
- **New kinds:** **Add a doc kind…** with a name, a folder and fields, which becomes a new group in the Docs tab (for example "Runbooks"). Built-in kinds can't be deleted.
- **Agents** can read and change templates the same way (`org-agent doc template show|edit <kind>`). A changed template shows in Activity ("ada added the field `risk` to Decision").
- **Existing docs aren't rewritten** when a template changes. New required fields show "Missing: risk" on older docs with **Fill in**, and removed fields are kept and shown under "Other fields".

### 3.1 The agent's side: `org-agent doc` (C16)

Agents use the tool the way the owner uses the app. Each command writes the file in the right folder from the template (front matter + sections), with the next number, commits it on the agent's branch (owner as author, agent as co-author, §0.2), and prints the number and path:

```
org-agent doc new adr "Docs storage" --project ac          → ADR-0004 · docs/adr/0004-docs-storage.md (proposed)
org-agent doc new design|evaluation|spec|plan|review "<title>"
org-agent doc edit ADR-0004                                 opens $EDITOR on the file, then commits
org-agent doc status ADR-0004 accepted                      sets status and decided {on, by} in the front matter, commits
org-agent doc schema adr                                    prints the fields (allowed values, required) and sections
org-agent doc validate [path]                               checks front matter against the template
org-agent doc supersede ADR-0002 --by ADR-0004
org-agent doc link ac-17 ADR-0004                           the "Why" link
org-agent doc list --kind adr --status proposed
org-agent doc show ADR-0004
org-agent debt add "<what I did instead>" [--task ac-17] [--why …] [--fix …] [--cost low|medium|high]
```

The command words match the app's (decision 3, §1), and every write shows in Activity ("ada proposed ADR-0004 · Docs storage").

## 4. Flow 2 · Decision lifecycle

- **Proposed → Accepted / Rejected:** the reader of a Proposed ADR shows a banner "Proposed by ada · 2 days ago" with **Accept** · **Reject…** (reason required) · **Request changes…** (a message to the owner). There are no permissions in phase 1: anyone can, the app suggests the owner of the organization, and it records who ("Accepted 2026-11-08 by the user") by setting `status` and `decided` in the front matter in a commit on a docs branch. It shows "Accepted · waiting to merge" until the change lands (§0.2).
- **Proposed decisions reach Needs you** for the owner, one row each: "ada proposes ADR-0004 · Docs storage". Agents ask through messages as today. The row is the same thread.
- **Supersede:** on an Accepted ADR, ⋯ › **Supersede with a new decision** opens a new ADR prefilled with "Supersedes ADR-0002" and the context. Accepting the new one marks the old one Superseded, with links both ways.
- **Editing an Accepted ADR** warns once: "Accepted decisions are a record. Supersede it if the decision changed, or edit only to fix a mistake." **Edit anyway** is allowed.

## 5. Flow 3 · "Why?" everywhere

- **Task, bug and debt pages** get a **Why** block in the side column: the ADRs, designs, specs and evaluations linked to the item or mentioning its number, with status badges. **+ Link a decision** links one. Empty: "No decisions linked. **Link one** if this work follows a decision."
- **Commits** (on items and in Activity) show their Why when the message names a doc ("ADR-0002", "EVAL-0001") or an item that has one.
- **Ask "why?"** (⌘K › "Why is…") searches decisions, evaluations and designs by text and shows the matches with their status and date. The copilot version (smart features #13) answers in a sentence with links.
- **Evaluations answer "what did we try?":** the Evaluations group lists every question with its verdict in one line ("Turso vs SQLCipher → SQLCipher: Turso's encryption is experimental"), so the history of tech choices reads like a table of contents.

## 6. Flow 4 · Tech debt (a doc in the repository)

**The file** (`docs/debt/0012-hard-coded-retry.md`):
```markdown
---
kind: debt
id: DEBT-0012
title: Retry count hard-coded in the office client
status: open                 # open | planned | paid | accepted
cost: medium                 # cost of leaving it: low | medium | high
effort: s                    # to pay it back: s | m | l
area: Office server
taken: { by: ac-software-engineer-river, on: 2026-11-08, task: ac-17, commit: 9c1e0b2, branch: task/ac-17-office-client }
because: [ADR-0004]          # the decision that accepted it, if any
payback: []                  # tasks that pay it back, e.g. [ac-40]
accepted: null               # { on, by, reason } when we decide to live with it
---

## What we did
## Why
## The proper fix
```

**Capture (one sentence is enough):**

| From | How | What's prefilled |
| --- | --- | --- |
| An agent, while working | `org-agent debt add "<what I did instead>"` writes the file **on the agent's current branch**, so the debt travels with the code that caused it and the reviewer sees both (C18) | the agent's current task, branch and commit, and the area from the task's team |
| Finishing a task | Marking a task Done asks "Did you take a shortcut?" **No** · **Yes, record it**. Agents get the same question from the CLI (`task done --debt "…"`, or `--no-debt`) | the task, branch, commits |
| A review | A finding the author leaves unfixed → **Record as debt** on the finding (the reviewer's verdict can list "Deferred" findings) | the review, the finding's text, the commit |
| An ADR | each line under "Tech debt accepted" becomes a debt doc whose `because` is the ADR | the ADR |
| Code | `TODO(debt): <text>` or `TODO(DEBT-0012)` in a commit's diff is offered as a draft ("2 possible debt items found") | file and line |
| Anyone | the project's Debt tab › **Record debt** | — |

**Record debt** (slide-over, generated from the debt template's fields, §3.0): **What we did** * (the title, 1–200 characters) · **Why** · **The proper fix** · **Cost of leaving it** Low · Medium · High ("Low: harmless for now" · "Medium: slows us down or risks bugs" · "High: will hurt users, security or data, or blocks a phase") · **Effort to pay back** S · M · L · Area · Links. **Record debt** commits the file (from the app: to the docs branch). Only "What we did" is required. A doc with empty sections shows "Needs details".

**Debt tab** (project, drawn from the files' front matter): grouped by area, sorted by cost, then effort (High cost, S effort first: "quick wins" at the top), with filters Status · Cost · Effort · Taken by · Area. A summary bar: "14 open · 3 High · 5 quick wins · 2 paid this week". Debt still on an unmerged branch shows with a "On task/ac-17-office-client, not merged" badge.

**Statuses:** **Open · Planned · Paid · Accepted.** Creating a payback task (a database task) adds it to `payback` and sets Planned. When all payback tasks are Done, the tool sets Paid in a commit on a docs branch, which lands through the merge flow (activity entry, with Undo as another commit). **Accept…** needs a reason, written to `accepted`, and can name the ADR in `because`.

**Debt page:** the header card (§3.0) · What we did · Why · The proper fix · **Taken by** ("Taken by River in ac-17 · 9c1e0b2") · **Why** block (§5) · **Create payback task** · history (the file's commits).

**Where debt shows up besides its tab:** the task it was taken in ("Debt taken: DEBT-0012"), the commit, the area's header in the Bugs tab ("Payments · 2 open debt"), the project Overview tile ("Tech debt 14 · 3 High"), and Work › **Debt** (the third pill: Tasks | Bugs | Debt | Requests).

**For agents:** the tool asks at the right moment through agent guidance (§6.2), and every agent's manual adds one rule: "When you take a shortcut (skip a test, hard-code, defer a refactor, work around a bug, leave a TODO), record it with `org-agent debt add` in the same session." The task-Done question backs this up.

### 6.1 Known limitations and exceptions (docs in the repository)

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

The tool helps agents work the way the organization wants by **prompting them at set moments**, like the shortcut question. These prompts are a product feature, and the owner shapes them to their own way of working.

| Moment | Default guidance (editable) | Where the agent sees it |
| --- | --- | --- |
| Starting a task | "Before you start {task}, read its Why: {decisions}. Known limitations in {area}: {limitations}." | `org-agent task start`, the inbox adapter's first message |
| Making a design or architecture choice | "If this choice will matter later, record it: `org-agent doc new adr \"…\"`." | when an agent creates a branch or a design doc |
| Taking a shortcut | "Did you take a shortcut (skipped a test, hard-coded, deferred a refactor, worked around a bug)? Record it: `org-agent debt add \"…\"`." | `org-agent task done`, the app's Done |
| Departing from a rule | "This goes against {rule}. Propose an exception: `org-agent doc new exception`." | when a review or check names a rule |
| Finishing a task | the project's checklist (work-items §9.1) and "Is anything here a known limitation? `org-agent doc new limitation`." | `org-agent task done` |
| Asking the owner | "Say what you need, your recommendation and what happens if there's no answer." | `org-agent msg ask --to owner` |
| Waking up (on-call and scheduled agents) | "Read your inbox and backlog, do what's due, record what you did, then stop." (`agent-run-modes-ux.md` decision 6) | at every wake |
| Procedure changed | "New from <retro>: <the change>." (`retrospectives-ux.md` decision 7) | at the agent's next start or wake after the change merges |
| Posting to chat | "Post only what someone must answer, act on or know. Put details in the task or doc and link it." (`chat-ux.md` §5) | `org-agent msg` |

- **Settings › Agent guidance** (organization) and project Settings › Agent guidance (overrides): one row per moment with **On/Off**, the text (with placeholders like `{task}`, `{area}`, `{decisions}` and a picker to insert them), and a **Preview as** an agent and task ("What River will see on ac-17").
- **Add guidance…** adds a moment from a list (task started, task done, bug fixed, branch created, review requested, message to the owner) with its own text, for the owner's own practices ("Always add a screenshot for UI work").
- **Where it's kept:** a project's guidance is persistent, so it's a file in the repository (`docs/.guidance/<moment>.md`, Markdown with front matter: `moment`, `enabled`). The organization's defaults are settings in the office. A change shows in Activity ("You changed the guidance for Taking a shortcut").
- **Agents' answers are recorded:** "No shortcut" or a debt id shows on the task, so the owner can see the guidance working (and turn it down if it's noise).

## 7. Bringing our own project in (dogfood)

When the Agentic Org project is brought in (dogfood spec Flow 1, or later from project Settings › **Index docs**):
- **Converting the status lines:** today's docs carry a bold status line, not front matter. Moving or indexing them offers **Add front matter**, which reads the status line, owner, dates and relations into front matter (shown as a diff per file, **Apply** / **Skip**) and keeps the line in the body until it's removed.
- The app indexes the product repo's `docs/` on `v1`, the integration branch, not `main` (C13). It finds 3 ADRs and 4 designs.
- It offers **Earlier decisions (v0)**: ada's 9 ADRs from her notes, moved to `docs/adr/v0/` with their statuses, shown as their own group so the two ADR-0001s never clash. ada marks which still hold (A18).
- It offers to **move** project documents out of agents' home folders into the repository's structure: my specs → `docs/specs/`, ada's plans → `docs/plans/`, her reviews → `docs/reviews/`, her design drafts → `docs/design/` (as Draft). A review lists each file with where it will go (**Move** / **Skip** each). Moving commits the files to the repo through the normal flow (a branch, the review, ada's go) and leaves a one-line pointer in the old place ("Moved to docs/specs/work-items-ux.md in agentic-org").
- It scans for debt: unfixed findings in ada's reviews, "Not covered here" sections in ADRs, and `TODO`s in code. These are offered as **draft debt** in a review list (**Record** / **Skip** each), never created silently.

## 8. UX acceptance criteria

Criterion ids for debt, limitations and exceptions use **DBT-**, **LMT-** and **EXN-** so they never collide with the records' ids (`DEBT-0012`, `LIM-0003`, `EXC-0002`) *(renamed in rev 7, before any test used them)*.

**Docs and decisions (DEC)**
- DEC-1 · The Docs tab groups docs as Brief, Decisions, Designs, Evaluations, Specs and Other, by folder under `docs/`, each with a count and status badges.
- DEC-2 · *(rev 3)* Each doc's YAML front matter is shown as a header card (kind, number, title, status badge, owner, dates, relations as chips with their status), never as raw YAML, and the section rail follows its `##` headings.
- DEC-3 · New › Decision, Design and Evaluation open their template with the next number and an editable file name, and nothing is committed before Save.
- DEC-4 · *(rev 6)* A Proposed ADR shows Accept, Reject (with a reason) and Request changes. Accept sets `status` and `decided {on, by}` in the front matter in a commit on a docs branch, and the ADR shows "Accepted · waiting to merge" until it lands.
- DEC-5 · Proposed ADRs appear in the owner's Needs you, one row each.
- DEC-6 · Supersede opens a new ADR prefilled with "Supersedes ADR-n", and accepting it marks the old one Superseded, linked both ways.
- DEC-7 · *(rev 3)* Missing or invalid front matter shows "Details not recognized" naming the field, with Fix, and the body still renders. A number used twice shows both docs with Renumber.
- DEC-8 · Superseded, Rejected, Deprecated and Obsolete docs collapse under "Older (n)".

**Why and evaluations (WHY)**
- WHY-1 · Task, bug and debt pages show a Why block with the linked or mentioning decisions, designs, specs and evaluations, with status, and + Link a decision.
- WHY-2 · A doc's reader lists Led to (items and docs that link it) and Mentioned in.
- WHY-3 · The Evaluations group shows each evaluation's question and its verdict in one line.
- WHY-4 · ⌘K "Why is…" finds decisions, evaluations and designs by text.

**Tech debt (DBT)**
- DBT-1 · Record debt needs only "What we did". Missing fields show "Needs details" on the item.
- DBT-2 · Marking a task Done asks "Did you take a shortcut?", and Yes opens Record debt prefilled with the task, branch and commits.
- DBT-3 · *(rev 4)* A debt item is a file `docs/debt/<n>-<slug>.md` with front matter per the debt template, numbered DEBT-<n>, shown with its icon, and sits in an area.
- DBT-4 · The Debt tab groups by area, sorts by cost then effort, and shows the summary bar with open, High, quick wins and paid this week.
- DBT-5 · *(rev 4)* A debt doc becomes Planned when a payback task is linked and Paid when all of them are Done, each written to the front matter in a commit on a docs branch (never the default branch), with an activity entry and Undo.
- DBT-6 · Accept needs a reason and can link the ADR that accepts it.
- DBT-7 · Each line under "Tech debt accepted" in an Accepted ADR is offered as a debt item linked to that ADR.
- DBT-8 · A review finding can be recorded as debt with the finding and commit linked.
- DBT-9 · The task page shows the debt taken in it, and the project Overview shows a Tech debt tile.
- DBT-10 · *(rev 4)* Work has a third pill, Debt, across projects. Debt on an unmerged branch is shown with its branch.
- DBT-11 · Draft debt found by a scan (TODOs, review findings, "Not covered here") is never created without Record.

**Our project (OURDOC)**
- OURDOC-1 · Indexing the Agentic Org project shows its 3 ADRs and 4 designs from `docs/` on `v1`, and ada's 9 v0 ADRs under Earlier decisions (v0), with no number clash.
- OURDOC-2 · *(rev 2)* Indexing offers to move specs, plans, reviews and design drafts from agents' home folders into `docs/specs`, `docs/plans`, `docs/reviews` and `docs/design`, file by file, with a pointer left in the old place.

**Authoring through the tool (AUTH)**
- AUTH-1 · *(rev 3)* Creating a decision, design, evaluation, spec, plan or review in the app or with `org-agent doc new` writes the file in its §0.1 folder from the template, with valid front matter and the next number, and commits it on a branch (the agent's task branch, or a docs branch from the app) with the owner as author and the agent as co-author. Nothing is pushed to the default branch.
- AUTH-2 · *(rev 3)* `org-agent doc status` and the app's Accept/Reject change only the front-matter fields involved (status, decided), and commit.
- AUTH-3 · A doc changed directly with git (outside the tool) shows in the app after it reaches the integration branch, with its author, and no data is lost.
- AUTH-4 · Every create, status change, supersede and link appears in Activity with the actor and the doc's number.
- AUTH-5 · `org-agent doc list` and `doc show` print the same kinds, numbers, statuses and words as the app.
- AUTH-6 · *(rev 4)* `org-agent debt add` writes a debt doc on the agent's current branch with only the sentence, and prefills the task, branch and commit.

**Known limitations (LMT)**
- LMT-1 · Limitations are docs in `docs/limitations/` with front matter, listed in the Docs tab's Limitations group with status Current or Lifted.
- LMT-2 · Report bug shows a matching current limitation with Read it and Report anyway.
- LMT-3 · A bug closed as Won't fix offers Record as known limitation, prefilled from the bug.

**Exceptions (EXN)**
- EXN-1 · Exceptions are docs in `docs/exceptions/` with front matter naming the rule, scope, approver and review date.
- EXN-2 · A proposed exception appears in the owner's Needs you with Approve and Reject.
- EXN-3 · On its review date, an exception appears in Needs you with Keep, Renew and Revoke, and past the date it shows as Expired.
- EXN-4 · The rule's doc lists its approved exceptions.

**Agent guidance (GUIDE)** *(rev 5)*
- GUIDE-1 · Settings › Agent guidance lists each moment with On/Off, editable text with placeholders, and Preview as an agent and task.
- GUIDE-2 · A project's guidance overrides the organization's and is stored in `docs/.guidance/<moment>.md`.
- GUIDE-3 · `org-agent task done` shows the Taking a shortcut guidance, and the answer (no shortcut, or a debt id) is recorded on the task.
- GUIDE-4 · Add guidance adds a moment from the list with the owner's own text.
- GUIDE-5 · Changing guidance appears in Activity, and turning a moment off stops its prompt everywhere (app and CLI).

**Templates (TPL)**
- TPL-1 · Every kind has a template (fields with type, required, allowed values and hint, plus sections with hints), and New fills a doc from it.
- TPL-2 · The editor's Details panel is generated from the template's fields, and the Source toggle edits the YAML directly, with both kept in sync.
- TPL-3 · `org-agent doc schema <kind>` prints the fields and sections, and `doc validate` reports each front-matter problem by field.
- TPL-4 · Settings › Docs › Templates edits a kind's fields and sections with a preview, saves by committing `docs/.templates/<kind>.md`, and Reset to default restores it.
- TPL-5 · Add a doc kind creates a new kind with its folder and fields, which shows as a group in the Docs tab. Built-in kinds can't be deleted.
- TPL-6 · Changing a template never rewrites existing docs. New required fields show "Missing: <field>" with Fill in, and removed fields show under "Other fields".
- TPL-7 · Moving or indexing a doc with a bold status line offers Add front matter with a per-file diff, Apply or Skip.

## 9. Open questions

**For codex-dev**
- ~~C11~~ · *Resolved by the user (rev 3):* YAML front matter is the structure. Status lines are converted on the way in (TPL-7).
- C17 · Templates as `docs/.templates/<kind>.md` with a field schema in their front matter, and built-in defaults in the app: does that fit the library? Does the office validate on write, or only the CLI and app?
- C12 · *(rev 4)* Debt, limitations and exceptions are doc kinds with templates, not work items. The CLI: `org-agent debt add|list|show|accept|pay`, `task done --debt/--no-debt`, and `org-agent doc new limitation|exception`.
- C18 · An agent's debt doc is written on its current task branch and reaches the integration branch with that merge, and the app shows unmerged debt from open branches. OK, or should agents' debt go on its own `docs/…` branch so it can land before the code?
- ~~C16~~ · *Resolved (agy-senior-dev):* the §3.1 commands fit org-agent's noun-verb style. Additions: `doc edit --set <field=val> --section <heading> --body <text|->` for headless agents (besides `$EDITOR`), `--no-commit`, `--as <member>`, and `--json` on every command, returning the front matter plus `path` and `commit`. The text output is the one-line summary (`ADR-0004 · docs/adr/0004-docs-storage.md (proposed)`).
- C13 · Docs are read from the integration branch (`v1` for us), not always `main`: a per-project setting "Integration branch"?

**For ada**
- ~~A36~~ · *Resolved (ada):* office-created `docs/<slug>-<short-id>` branches for app edits (owner as author, agent or app as co-author), with no stand-in tasks. Docs merge through the same review, go and merge flow as code.
- ~~A18~~ · *Resolved (ada):* the product repo's ADR-0001–0003 hold (as amended). Her v0 notes (`knowledge/adrs`, `desktop-app-architecture`) are superseded by the office model: they come in as `docs/adr/v0/` with `status: superseded` and links to their successors, never as current.
- ~~A19~~ · *Resolved by the user (rev 2):* they move into the project's repository and are written through the tool from now on.
- ~~A23~~ · *Resolved by the user (rev 4):* persistent knowledge (decisions, designs, specs, evaluations, reviews, plans, debt, limitations, exceptions) is in the repository. Tasks, bugs, queues, messages and other transitional state are in the database.
- ~~A24~~ · *Resolved (ada):* yes. My specs move to the product repo's `docs/specs/` on an `eden/specs-*` branch through the reviewer and ada's go.
- ~~A20~~ · *Resolved (ada):* yes, agents record knowledge with `org-agent doc new|validate` and `debt add` on their own branch (Remy owns the CLI since agy-senior-dev ran out of credits). Was: the rules for every agent, through HR's templates and AGENTS.md: record shortcuts with `org-agent debt add`, and write decisions, designs, specs, plans and reviews with `org-agent doc`, not in your home folder.

**For the user**
- ~~U14~~ · *Resolved (the user):* yes, as agent guidance the owner can customize (§6.2).
- U15 · *My recommendation:* in phase 1, every proposed decision comes to you in Needs you (they're few), and a setting **Who accepts decisions: You (default) · Leads, in their area** lets you delegate later. Accepted decisions always show in Activity.
