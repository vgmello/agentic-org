---
id: doc-98
title: 'Design: Two-Tier Agent Decision Capture and Promotion'
type: specification
created_date: '2026-11-25T00:13:00Z'
updated_date: '2026-11-25T00:14:00Z'
tags:
  - design
  - architecture
  - ax
  - decisions
  - privacy
  - v1
---

# Design: Two-Tier Agent Decision Capture & Promotion

> [!NOTE]
> **Consensus:** **PM / PO** · **Systems Architect** · **Principal AX Designer**  
> **Authority Chain:** `ADR-0031` (Private Notes Boundary) · `ADR-0018` (Dual-Engine Replaceability) · `doc-79` (Doc 05: Private Notes) · `doc-52` (Knowledge & Decisions UX) · `doc-45` (Member Profiles UX) · `doc-96` (AX Spec)  
> **Backlog Tasks:** `TASK-111` (**`v1a / m-0`**) · `TASK-112` (**`v1b / m-0`**) · `TASK-91` · `TASK-55` · `TASK-85.2` · `TASK-85.4`

---

## 1. The 3 Clean Product Primitives (Zero Context Pollution, Zero Git Noise)

| Primitive | Storage & Git Policy | Who Can Read It? | What It Is For |
| --- | --- | --- | --- |
| **1. Private Note** (`kind: note \| plan \| decision`) | **Isolated HQ SQLite DB only** (`<durable-root>/private/notes.db`). **NEVER in Org Git**, never in `office.db`, never replicated to other agents. | **Only that agent + humans (`ADR-0031`)** | Implementation scratchpads (`note`), working plans (`plan`), and tactical/private choices (`decision` with Title, Why, Alternatives, Tags) — zero pollution of other agents' contexts. |
| **2. Task Comment** (`task comment`) | Durable task operational history (shared task scope) | **Everyone on the task** (agents + humans) | Handoffs, status updates, review feedback, task-level conclusions teammates should read. (`note publish` copies a private note here.) |
| **3. Project Decision Doc** (`doc --kind decision` / ADR) | **Organization Git** (`projects/<id>/docs/decisions/<name>.md`) | **Everyone on the project** (agents + humans) | Durable architecture/product choices that future tasks must honor, linked via **Why** edges. (`note promote` graduates a private decision here with zero private provenance.) |

### Why Reject a Standalone `agent-os decision` Resource
- **Visibility truthfulness:** Resource names unmistakably encode audience (`note` = private you+people, `task comment` = task-shared, `doc` = project-shared Git). A standalone `agent-os decision` with a mutable `--private`/`--shared` toggle violates `doc-79:530` and causes either silent multi-agent desync or Git spam.
- **Zero competing primitives:** Reuses the shipped `PrivateNotesStore` (`Sqlite` + `Memory` + `SatelliteRefused`) and `documents(kind = 'decision')` (`TASK-55`) without a second private DB/backup/fencing boundary, keeping `test_schema.py`'s 18-doc invariant intact.
- **Zero `4 KiB` catalogue bloat:** Generalizing `task-note` → **`agent-os note`** (preserving `task-note` and `task note` as pre-normalized `AX-8` aliases) costs **0 net new resources** (+~9 B for `promote`).

---

## 2. Schema Evolution (`docs/design/organization-data/private-notes-schema.sql`)

Clean pre-v1 replacement per `doc-7` #14 (`private_notes` in `<durable-root>/private/notes.db`; keep schema filename `private-notes-schema.sql` for `test_schema.py`):
- **`task_id TEXT` (nullable):** Composite FK `(organization_id, task_id) REFERENCES private_note_tasks(organization_id, task_id)` uses SQLite `MATCH SIMPLE` — passes when `NULL` (non-task/DM decisions), enforces task mirror when attached. `tasks.notes.publish` rejects `task_id IS NULL`.
- **`project_id TEXT` (nullable):** Auto-inferred from `--task`'s project or `AGENT_OS_PROJECT` so standalone decisions filter by project.
- **`kind TEXT NOT NULL DEFAULT 'note' CHECK (kind IN ('note', 'plan', 'decision'))`:** Author-editable on `update` (can reclassify a working note into a `decision`).
- **`title TEXT`:** Auto-derived by the office from the first non-empty line of `body_markdown` (truncated to `120` chars) if omitted on `kind = 'decision'` (`<= 200` chars; required non-null for active `decision` rows; cleared to `NULL` on tombstone).
- **`why TEXT` (nullable, `<= 2,000` chars) & `alternatives TEXT` (nullable, `<= 2,000` chars):** Dedicated columns power compact `~150`-token Turn-40 recall and clean ADR template prefill on `promote`; both cleared to `NULL` on tombstone.
- **Normalized `private_note_tags(organization_id, note_id, tag)`:** Max 8 tags per note matching `task_labels` grammar (`[a-z0-9._:/-]`, `1..64` chars), trigger-guarded against tagging deleted notes, atomically purged on tombstone (`PN-06`).
- **Indexes:** Keep `private_notes_by_task` (`WHERE deleted_at IS NULL AND task_id IS NOT NULL`), add `private_notes_by_author_kind(organization_id, author_id, kind, updated_at) WHERE deleted_at IS NULL` and `private_note_tags_by_tag(organization_id, tag, note_id)`.
- **2-Phase Promotion Receipts (`private_note_command_receipts`):** Extend `command IN ('create', 'edit', 'delete', 'publish', 'promote')`, add nullable `shared_document_id TEXT` (mutually exclusive with `shared_comment_id`, unique partial index, one pending guard), plus private-side `promoted_document_id` on `private_notes` (visible ONLY to author + people; **zero** `private_note_id` or private provenance ever enters shared Org Git!).
- **Log Redaction:** `Debug` impls on `PrivateNoteRecord` / wire structs redact `title`, `why`, `alternatives`, `body_markdown`, and `tags`.

---

## 3. Low-Friction Agent CLI & Turn-40 Recall

```sh
# Fastest valid capture (1 required content flag; --task auto-binds from active run):
agent-os note add --kind decision --text "Use single-flight lock for FTS5 rebuild"

# High-signal structured working decision (still 1 shell call, zero YAML ceremony):
agent-os note add --kind decision \
  --text "Use single-flight lock for FTS5 rebuild" \
  --why "Prevents duplicate rebuilds when concurrent agent wakes hit cold index" \
  --alternatives "Lazy per-query build (races); global office mutex (blocks reads)" \
  --tag concurrency --tag search

# Compact Turn-40 self-recall (< 1 KiB stdout via server-side --fields pruning):
agent-os note list --kind decision --task T-A1 \
  --fields id,title,why,tags,promoted_document_id,revision --format compact --limit 20

# Ranked cross-entity recall across shared ADRs + caller's own private decisions (< 5 ms, TASK-91):
agent-os search query --q "FTS5 rebuild lock" --types decision --types private_decision --limit 10 --format compact

# One-command promotion to shared Org-Git ADR (v1b, TASK-112):
agent-os note promote --id <note-id> --if-match <rev> [--project <ref>] [--feature <id>] [--key ADR-0009]
```

- **Deliberately omitted flags:** Zero `--private` / `--shared` flag (`doc-79:530`), zero `--author` flag on write (`CLI-9`).
- **Search Isolation (`TASK-91`):** Private decision fields (`title` 6x > `tags` 4x > `why`/`alternatives`/`body` 2x) are indexed strictly inside HQ's disposable `0700/0600` `runtime/<office-id>/private/task-notes-search.db` with pre-ranking `author_id == caller` filtering for agents (and people-only `--author` filter), keeping `WHY-4` shared knowledge search 100% free of private rows.

---

## 4. Owner Inspection UI & Anti-Spam AX Discipline

- **Agent Profile → `Decisions` Tab (`/members/[memberId]/decisions`, updating `doc-45` §5 & `tabs.ts`):**
  - Surfaces **every `kind = 'decision'` note that agent made across all tasks and standalone turns** directly from the private DB (zero Git), cleanly separated into two badged sections:
    1. **Working decisions (Private):** Lock badge *"Visible to people and `<agent>`; hidden from other agents"*, strictly read-only for people viewing an agent, showing **Title · Why · Alternatives · Tags · linked Task pill · Promoted ADR badge**, filterable by tag/project/task.
    2. **Project decisions (Shared ADRs):** Globe/doc badge *"Shared in project docs (org Git)"*, populated from `documents(kind = 'decision')` authored/owned by this member (`TASK-112` / `TASK-55`).
  - Keeps `Notes and memory` (`NotesTab.svelte`) untouched (`doc-45:124`) and keeps private rows strictly out of shared `Activity` (`LIVE-2`).
- **Anti-Spam Tier Split (`TASK-85.2` / `TASK-111` / `TASK-85.4`):**
  - **Tier 1 (`instructions.rs`, +~140 net bytes):** 2 calm sentences stating visibility (`note` = private, `task comment` = task-shared, `doc` = project-shared Git) and *"record non-obvious choices, not routine steps"*.
  - **Tier 3 (`tasks-execute` / `tasks-finalize` / `docs-and-decisions`):** Concrete **4-Signal Decision Bar** (non-obvious trade-off, shortcut/scope cut `--tag shortcut`, unverified assumption/timeout default `--tag assumption`, costly-to-reverse cross-turn constraint) + explicit *"Do NOT record"* list (expect **0–3 working decisions/task**).
  - **Empirical AX Gate (`DEC-AX-1..5` in `TASK-85.4`):** Anti-Spam calibration (0 decisions on mechanical task), Meaningful Fork capture (`exit 0`, non-empty `why`/`alternatives`, 0 spurious Git ADRs), Turn-40 Compact Recall (`<= 1` call, `< 1 KiB`), Promotion & `ADR-0031` Firewall (`0` `private_note_id` in Git), and Non-Task Standalone capture.

---

## 5. `v1` vs. `v2` Scope Split

- **`v1a` (`TASK-111`, `m-0` — zero dependency on `TASK-55`):** `private_notes` + `private_note_tags` schema & dual-engine store, `agent-os note` CLI upgrade + aliases, Agent Profile **Decisions** tab (private live + honest shared placeholder), upgraded `PrivateNotesPane.svelte`, `TASK-91` `--types private_decision`, Tier-1/Tier-3 anti-spam guidance.
- **`v1b` (`TASK-112`, `m-0` — lands with `TASK-55`/`TASK-68`):** 2-phase `agent-os note promote` with zero private provenance leaked to Git, Agent Profile **Shared ADRs** live list, `docs-and-decisions` skill, and `DEC-AX-1..5` live evals (`TASK-85.4`).
- **`v2` (Deferred per `doc-52` §7):** Decision approval gates (`DEC-4/5`), configurable guidance moments (`GUIDE-1..5`), dedicated tech-debt register (`DBT-1..11`), offline satellite private-note buffering, hybrid semantic recall (`TASK-110`).
