---
id: doc-95
title: 'Research: Backlog.md vs Agentic Org Gap Analysis'
type: other
created_date: '2026-11-23T01:26:00Z'
updated_date: '2026-11-23T01:27:00Z'
tags:
  - research
  - backlog
  - product
  - gap-analysis
  - ax
---
# Backlog.md vs. Agentic Org — Capability Gap Analysis & Adoption Report

> **Purpose:** Evaluate what **Backlog.md (v1.55.3)** does well that **Agentic Org** does not yet have or solve, highlight where the two systems solve the same problem differently (and any risks/watch-outs in our current implementation), and give a concrete adopt / defer / skip matrix for PO triage.
>
> **Evidence base:** Direct inspection of Backlog.md v1.55.3 CLI/guides and Agentic Org's current codebase (`crates/org-office/src/tasks/`, `crates/org-agent-cli/`, `crates/org-proto/`, `crates/org-sync/`, `docs/design/organization-data/schema.sql`) plus `.backlog/docs/` and `.backlog/tasks/` as of `v1` (`ace87b47`).

---

## Executive Summary

Agentic Org is already **architecturally much stronger** than Backlog.md in the hardest areas: multi-office offline-safe IDs (`T-A1` / `T-B1` + UUIDv7), optimistic concurrency (`--if-match`), single serialized office writer + recoverable journal, isolated code worktrees separated from org state, office-enforced authorization (`ADR-0031`), agent-private task notes (`TASK-50`), cost/budget accounting (`TASK-53`), and structured Feature → Milestone → Release hierarchies.

However, **Backlog.md solves several everyday agent- and PO-ergonomics problems cleanly that Agentic Org currently lacks or leaves to v2**. The highest-leverage candidates to evaluate for adoption are:

1. **Task Drafts / Staging (`draft` → `promote` / `demote`)** — critical alongside `ADR-0028` (assigning a task wakes an agent) and **Tess** (`TASK-80`).
2. **Task Archive & Restore Lifecycle** — keeps boards, CLI lists, and agent context windows clean over time without losing history or breaking dependencies.
3. **Phase-Gated On-Demand Workflow Guides** (`create` / `execute` / `finalize` via `agent-os skill read`) — enforces high agent discipline inside the 10 KiB base-prompt cap *(now tracked in `TASK-85.2`)*.
4. **Unified Cross-Entity Agent Search CLI (`agent-os search`) + `--count`, `total`, and `--modified-file` filters** — cuts multi-call token overhead for agents *(CLI task-list filters tracked in `TASK-85.3`)*.
5. **First-Class `final_summary` & Org-Wide Definition-of-Done Defaults** — lighter than full v2 task templates (`TASK-78`), immediate quality boost for reviews and Tess's Executive Brief (`TASK-80.2`).
6. **Parent / Subtask Hierarchy** — currently deferred to v2 (`TASK-73`), worth re-evaluating for v1 given how naturally complex features split (e.g., `TASK-80.1..4`, `TASK-85.1..3`).

---

## 1. Genuine Gaps: What Backlog.md Has That Agentic Org Should Evaluate

### 1.1 Task Drafts / Staging Area (`draft create`, `promote`, `demote`)
- **What Backlog.md does:** Maintains a `drafts/` staging area completely off the active board. Drafts can be created, edited, reviewed, promoted to real tasks (`draft promote`), or demoted back from the board (`task demote`).
- **Agentic Org today:** No general task draft state. Only ephemeral browser form drafts and `details.request_outcome.draft_tasks[]` during request scoping ([doc-42](../specs/doc-42%20-%20Spec-Feature-Requests-and-Intake-UX.md)).
- **Why this matters for us:** Under [ADR-0028](../../decisions/decision-28%20-%20ADR-0028-An-agent-is-always-available-a-persons-message-and-a-task-assignment-also-start-it.md), assigning a real task wakes an idle agent immediately. Having a first-class **Draft** stage lets you (or **Tess** in `TASK-80`) sketch, refine, and bundle a batch of tasks—even with assignees and dependencies—before promoting them onto the active board.
- **Recommendation:** **Strong candidate (v1 / v1.5).**

### 1.2 Task Archive, Restore & Safe Cleanup (`archive`, `restore`, `cleanup`)
- **What Backlog.md does:** Separates *finished on board* from *archived off board*. `archive` makes a task read-only, preserves its ID, history, and links, and still counts a terminal archived task as satisfying downstream dependencies. `restore` brings it back cleanly; `delete` is two-step (archive first) and refuses to delete while subtasks or other references remain. `cleanup` archives terminal tasks older than N days.
- **Agentic Org today:** No task archive, restore, or delete operations exist yet in `crates/org-office-server/src/task_calls.rs` or `crates/org-office/src/tasks/features/ops.rs`. Closed tasks stay in active storage forever (the UI only folds `Done` items older than 7 days behind *"Show all done"*, per [doc-49](../specs/doc-49%20-%20Spec-Work-Items-and-Tasks-UX.md)); deletion is designed as permanent tombstones ([doc-77](../organization-data/doc-77%20-%20Organization-Data-03-—-Features-and-Work.md)).
- **Why this matters for us:** Active multi-agent organizations generate hundreds of tasks per month. Without archiving (especially for cancelled/duplicate/old Done tasks), default CLI queries, UI views, and context windows accumulate noise, while permanent tombstones are too destructive for routine board hygiene.
- **Recommendation:** **Strong candidate (v1.5 / v2)** — especially read-only `archive` + `restore` (projects already have `archived` + `--include-archived`).

### 1.3 Phase-Gated On-Demand Workflow Guides (`instructions overview / task-creation / task-execution / task-finalization`)
- **What Backlog.md does:** Keeps the always-loaded instruction stub tiny and lazy-loads 3 focused lifecycle guides at the exact moment an agent needs them:
  - **Creation:** search first, assess scope/splitting, write durable *why* + testable ACs, *forbid speculative implementation plans at creation*.
  - **Execution:** read full dependency graph first, research live code, record plan, work in short loops, stop on scope creep.
  - **Finalization:** require objective verification evidence before checking ACs/DoD, confirm docs updated, write a concise `final_summary` before marking `Done`.
- **Agentic Org today:** Has a tight **10 KiB** cap on standing instructions (`crates/org-office-sessions/src/instructions.rs`) and `agent-os skill list|read` (`TASK-44`), but ships only 5 terse stubs in `crates/org-process/src/skills.rs`.
- **Why this matters for us:** Fits `agent-os skill read` natively with low cost, stays inside the 10 KiB prompt budget, and dramatically improves agent discipline across Claude, Codex, and Agy.
- **Status / Recommendation:** **Adopted — now tracked in `TASK-85.2`.**

### 1.4 Unified Cross-Entity Search CLI + Agent Query Ergonomics (`search`, `--count`, `--modified-file`, `--exclude-status`, `--unassigned`)
- **What Backlog.md does:**
  - Single `backlog search "query" --type task,document,decision --modified-file <path>` across all knowledge & work entities.
  - `--count` (check result size before flooding context), `--max-count`/`--skip` with explicit `total` + next-page command.
  - `--exclude-status`, `--unassigned`, and `--ready` filters.
  - `--modified-file <path>` substring search ("which tasks previously modified `crates/org-office/src/tasks/core.rs`?").
- **Agentic Org today:**
  - No unified `agent-os search` CLI yet (only web Top-bar Search planned in [doc-39](../specs/doc-39%20-%20Spec-App-Shell-and-Navigation-UX.md) / `FC-006`).
  - `crates/org-agent-cli/src/task_dispatch.rs` drops `TasksListResult.total`, and lacks `--count`, `--exclude-status`, `--unassigned`, and `--modified-file` filters (`--ready` tracked in `TASK-84`, list filters tracked in `TASK-85.3`).
- **Recommendation:** **`task list` upgrades tracked in `TASK-85.3`; add unified `agent-os search` alongside `TASK-55` / `TASK-60`.**

### 1.5 First-Class `final_summary` & Org/Project Default Definition-of-Done Checklists
- **What Backlog.md does:** Separates durable task description from `--plan`, `--notes`, checkable `--ac`, org-wide `definition_of_done` defaults (auto-populated on every new task unless `--no-dod-defaults`), and a structured `--final-summary` written at completion with verification evidence. Also preserves AC check-state when rewording/reordering unchanged items.
- **Agentic Org today:** Has checklists (`agent-os checklist`), a single optional `definition_of_done` text field on `task_details` (no org/project default list), shared comments, and private task notes (`TASK-50`). Full structured task templates are deferred to v2 (`TASK-78`), and required-checklist completion gates (`CHK-5`) are deferred.
- **Why this matters for us:**
  - A concise **`final_summary`** field on task closure (what changed, verification evidence, follow-ups) gives reviewers, future agents, and **Tess's Executive Brief** (`TASK-80.2`) a crisp, structured signal without scraping 40 comments.
  - **Org/project default DoD checklist items** (e.g., *"Docs/specs/ADRs updated"*, *"Tests pass"*) encode team quality bars automatically without waiting for full v2 templates.
- **Recommendation:** **Evaluate `final_summary` + default DoD checklist for v1.5 (or fold into `TASK-78` if keeping v1 lean).**

### 1.6 Parent / Subtask Hierarchy (`parent_task_id`)
- **What Backlog.md does:** First-class parent/subtask tree (`-p TASK-80` → `TASK-80.1..4`), `--parent` filtering, parent-child completion discipline, and deletion protection while subtasks exist.
- **Agentic Org today:** Explicitly deferred to v2 (`TASK-73`, `FC-158`). In v1, hierarchy is `Project → Feature → Tasks` plus flat `depends_on` edges (`TASK-77`).
- **Why worth evaluating:** A `Feature` + `depends_on` covers many cases, *except* projectless team tasks or breaking one mid-sized task into 3–4 vertical slices (as we did with `TASK-80` → `TASK-80.1..4` and `TASK-85` → `TASK-85.1..3`). Lightweight `parent_task_id` (without a heavy cascade engine) is much cheaper than full `TASK-73`.
- **Recommendation:** **Keep full cascade engine in v2 (`TASK-73`),** or consider lightweight `parent_task_id` link if needed during dogfooding.

### 1.7 Inline Transitive Dependency Tree on `task inspect` & Bulk Multi-ID Operations
- **What Backlog.md does:** `backlog task view` prints the full transitive `Depends on` and `Dependents` tree inline (flagging unknown/ambiguous IDs), and `backlog task edit TASK-1 TASK-2 ...` supports bulk updates across multiple IDs.
- **Agentic Org today:** `task inspect` returns direct `depends_on_tasks` / `dependent_tasks` summaries; transitive graph uses `agent-os task deps`. Bulk writes use `session attach` pipelining.
- **Recommendation:** **Small nice-to-have** (low priority; `task deps` + `session attach` already cover most of this).

---

## 2. Problems Solved Differently (Where Agentic Org Is Stronger — Plus Watch-Outs!)

| Problem Area | How Backlog.md Solves It | How Agentic Org Solves It | Assessment & Watch-Outs |
| --- | --- | --- | --- |
| **Concurrent edits & ID collisions across branches/worktrees** | Git-native Markdown files edited across developer branches; relies on `checkActiveBranches`, remote snapshots, `.locks/`, and post-hoc `backlog doctor --fix` to repair duplicate IDs & merge conflicts. | Separates org state (`durable/state/`) from code Git (`TASK-51`). Offline-safe office-prefixed keys (`T-A1`, `T-B1` + UUIDv7, `ADR-0019`), optimistic concurrency (`--if-match`), single serialized office writer + acceptance journal (`ADR-0006`). | **Agentic Org is fundamentally better.** Eliminates separate-clone ID collisions and branch merge conflicts. **Watch-out:** Multi-office sync transport (`crates/org-sync/`, `TASK-52`) isn't wired yet—verify conflict/replication paths thoroughly when `TASK-52` lands. |
| **Integrity checking (`backlog doctor`)** | Post-hoc repair CLI (`backlog doctor --fix`) fixing duplicate IDs, cyclic dependencies, and misplaced archive files after Git merges. | Prevents most issues at write time (UUIDv7, BFS + SQLite cycle triggers, automatic SQLite rebuild from canonical files). Instance `agentic-org doctor` planned in `TASK-60`. | **Watch-out:** Once human/Git-edited org docs & ADRs land ([doc-52](../specs/doc-52%20-%20Spec-Knowledge-Decisions-and-Tech-Debt-UX.md) `DEC-7`), bad front matter, broken cross-links, and display-key clashes *can* happen externally. Make sure `agentic-org doctor` / `agent-os doc validate` covers org-state cross-reference integrity, not just host/daemon health. |
| **Human-readable Git Markdown vs. JSON store** | Every task/doc/decision is plain Markdown + YAML front matter in Git. | Spec ([doc-77](../organization-data/doc-77%20-%20Organization-Data-03-—-Features-and-Work.md)) specifies Markdown + YAML (`task.md`, `comments/<id>.md`), **but current Rust code (`crates/org-office/src/tasks/records.rs`) writes JSON (`task.json`, `comments/<id>.json`)**. | **Important watch-out!** Reconcile `records.rs` back to `doc-77`'s Markdown files or update `doc-77` to ratify JSON before `TASK-52` (multi-office replication) and `TASK-78` (Markdown template sections). |
| **Agent visibility & private scratchpads** | All files are shared; no authorization boundary and no private agent notes. | Office-enforced caller authority (`ADR-0031`), isolated HQ-only **Private Task Notes** (`TASK-50`, invisible to other agents, visible to author + people), explicit `publish` to shared comments. | **Agentic Org is much stronger.** Keep this separation strictly intact. |
| **Live updates & agent triggering** | `backlog task list --json --watch` streams replacement snapshots to a polling client. | Browser uses SSE (`/api/v1/events/stream`); agents wake automatically on message/assignment (`ADR-0028`) and use `msg wait` / `session attach` (2.5 ms/cmd). | **Agentic Org is better.** Event-driven wakeups avoid burning idle processes polling task lists. |
| **MCP Server (`backlog mcp start`)** | Exposes Backlog operations via stdio MCP server. | Intentionally bans MCP for product primitives ([doc-59](../specs/doc-59%20-%20REQ-08-agent-os-CLI-and-Kernel-Primitives.md)); standardizes on `agent-os` CLI + `--schema` + `skill read`. | **Keep Agentic Org's approach.** Faster, uniform across `claude -p`, Codex, and Agy, and avoids MCP tool-schema context bloat. |
| **Knowledge, ADRs, Milestones & Releases** | Lightweight flat `docs/`, `decisions/`, `milestones/` with simple create/archive/rename/remove CLI. | Richer domain model (`Feature` → `Milestone` → `Release` → `Deployment` with derived progress views; versioned `docs-history/` + code links + `--realizes <doc>@<ver>`). | **Watch-out (implementation lag):** Project `documents`, `decisions`, `features`, `milestones`, and `releases` aren't wired into `agent-os` or `org_routes.rs` yet (`CreateTask` stubs `feature_id`/`milestone_id` to `None`). Prioritizing `TASK-55`/`TASK-56` closes this gap. |

---

## 3. Prioritized Decision Matrix for PO Review

| # | Capability from Backlog.md | Impact | Effort | Suggested Timing | Status / Notes |
| --- | --- | --- | --- | --- | --- |
| **1** | **Phase-gated task lifecycle guides** (`create` / `execute` / `finalize`) via `agent-os skill read` | High | Low | **Adopt now (v1)** | **Tracked in `TASK-85.2`.** Zero schema change; fits 10 KiB prompt budget across all 3 runtimes. |
| **2** | **`agent-os task list` query upgrades** (`total` / `--count`, `--exclude-status`, `--unassigned`, `--modified-file` filter) | High | Low | **Adopt now (v1)** | **Tracked in `TASK-85.3`.** Saves agent context tokens and makes `TASK-77`'s `modified_files` queryable. |
| **3** | **Task Drafts / Staging (`draft` → `promote` / `demote`)** | High | Medium | **Adopt (v1 or v1.5)** | Prevents half-baked tasks or Tess proposals from waking agents (`ADR-0028`) or cluttering active boards. |
| **4** | **First-class `final_summary` + Org/Project Default DoD checklist** | High | Medium | **Adopt (v1.5 / fold into v1)** | Structured closure receipts power reviews and Tess's Executive Brief (`TASK-80.2`); default DoD enforces docs/tests without waiting for `TASK-78`. |
| **5** | **Task Archive & Restore lifecycle** (+ optional age-based cleanup) | Medium–High | Medium | **Adopt (v1.5 / v2)** | Essential for long-running org hygiene without destructive tombstones; preserves IDs and satisfied dependencies. |
| **6** | **Unified cross-entity `agent-os search`** (tasks + docs + decisions) | High | Medium | **Adopt alongside `TASK-55` / `TASK-60`** | High agent value once docs/decisions CRUD lands in `agent-os`. |
| **7** | **Broaden `agentic-org doctor` / `doc validate`** for org-state & cross-reference integrity | Medium | Low–Medium | **Fold into `TASK-60` / `TASK-56`** | Catches broken links / external Git edits once Markdown docs & multi-office sync ship. |
| **8** | **Lightweight `parent_task_id` subtasks** | Medium | Medium | **Evaluate (currently v2 `TASK-73`)** | Revisit if `Feature` + `depends_on` feels too flat during dogfooding. |
| **9** | **Backlog.md-style Git-branch scanning / MCP server / `--watch` polling** | Low / Negative | — | **Skip** | Agentic Org's separate org-state store, `agent-os` CLI, SSE, and `ADR-0028` wakeups are strictly superior. |

---

## 4. Two Architectural Watch-Outs Discovered During Audit

1. **Storage format drift (`task.json` vs. `doc-77` `task.md`):** `crates/org-office/src/tasks/records.rs` currently persists JSON files under `durable/state/work/tasks/<id>/task.json`, whereas [doc-77](../organization-data/doc-77%20-%20Organization-Data-03-—-Features-and-Work.md) specifies Markdown + YAML front matter (`task.md`). Reconcile (either update code or update `doc-77`) before `TASK-52` (multi-office replication) and `TASK-78` (Markdown template sections).
2. **`CreateTask` stubs & missing doc/decision/milestone CLI:** `feature_id` and `milestone_id` on `CreateTask` are currently stubbed to `None`, and project `documents`/`decisions`/`milestones`/`releases` aren't wired into `agent-os` yet (`TASK-55` / `TASK-56`).
