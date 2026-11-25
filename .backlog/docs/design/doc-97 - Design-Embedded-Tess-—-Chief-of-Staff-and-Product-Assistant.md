---
id: doc-97
title: 'Design: Embedded Tess — Chief of Staff and Product Assistant'
type: specification
created_date: '2026-11-25T00:13:00Z'
updated_date: '2026-11-25T00:14:00Z'
tags:
  - design
  - architecture
  - ux
  - ax
  - tess
  - chief-of-staff
  - v1
---

# Design: Embedded Tess — Chief of Staff & Product Assistant

> [!NOTE]
> **4-Way Council Consensus:** **PM / PO** · **Systems Architect** · **Principal UX Designer** · **Principal AX Designer**  
> **Authority Chain:** Organization Data (`doc-75`, `doc-78`, `ADR-0014`, `ADR-0029`, `ADR-0031`, `ADR-0032`) · AX Spec (`doc-96`, `AX-1`–`AX-24`) · CLI & Instructions (`doc-27`, `doc-28`, `doc-29`, `doc-39`, `doc-59`)  
> **Backlog Tasks:** `TASK-80` · `TASK-80.1` · `TASK-80.4` · `TASK-80.5` (**`v1 / m-0`**) · `TASK-80.2` · `TASK-80.3` (**`v2 / m-1`**) · `TASK-85.4` · `TASK-103`

---

## 1. Executive Summary & Core Decisions

| Decision Area | Consensus Choice | Why It Wins (What It Gives Us) |
| --- | --- | --- |
| **Runtime & Session Engine** | First-class built-in agent (`employees.builtin_key = 'chief_of_staff'`) on `org-office-sessions` + `org-agent-runtime-{claude,codex,agy}` with warm persistent sessions (`Control::Send`). **Reject** `org-copilot` and any 5th server. | Native parity across Claude, Codex, and Agy; warm follow-ups skip 2–5s cold spawns; honest employee cost attribution (`doc-87`); complies with `doc-7` #7 (strictly 4 programs) & #9 (office authorizes every call). |
| **Tool Transport** | **`agent-os` CLI + `agent-os session attach` ONLY** (zero MCP wrappers per `CLI-12`, no separate `tess` CLI, never human-only `agentic-org`). | Measured on the live 90-command table (298 input fields): MCP `tools/list` injects **11.3k–19.3k tokens/turn** today (**15.1k–25.7k** at 120 commands), and Claude (`ENABLE_TOOL_SEARCH=false`), Codex (`mcp_servers={}`), and Agy (`fail_or_fast_stop`) all ban/disable lazy MCP. Grouped `agent-os` costs **~440 tokens** (~1.75 KiB, 100% KV-cached — **30x–50x smaller**) and pipelines 6 reads in **1 LLM turn (~10–15 ms IPC)**. |
| **Product Knowledge & Org Awareness** | **5-tier static-to-volatile discovery** (`AX-11`) + **7 compiled version-locked product-guide skills** via `agent-os skill list --category \| read` + **`<= 500–800 B` trailing UI viewport envelope**. | Keeps Tier 1 `<= 10 KiB` and byte-identical across all agents (`>= 80%` KV cache hit on turn 2+); `verify_command_references` (`AX-14`) CI-checks every cited command/flag; warm turns use **~600–1,100 uncached tokens** vs. 25k–45k for MCP + org-snapshot dumping. |
| **Native UI Surface** | **Hybrid `⌘K` Fast Launcher + Persistent Collapsible Right-Hand Tess Side Panel (`⌘/` / `✨ Ask`)**. Docks beside `<main>` (`360–420px`) on wide desktop (`>= 1440px`), adapts to modal Sheet below. | Keeps working memory intact while inspecting Projects/Work/Settings side-by-side; persists across route navigation; eliminates cramped 2-tab `⌘K` modal; smart `/chat` de-duplication prevents duplicate Tess panels. |
| **Structured UI Output** | **Tool-backed validated artifacts (`agent-os suggestion create` + `agent-os ui present`) + pure streaming Markdown + canonical entity ref auto-linking**. **Reject** LLM-authored fenced JSON blocks and JSON-only replies. | `org-command` + office run real `--dry-run` preflight and verify entity existence/auth **before rendering a pixel**; zero fence flicker; smaller models self-heal invalid flags inside the turn; untrusted repo/channel text cannot spoof clickable Apply cards via prompt injection. |
| **Safety & Delegation** | Server-side **effective permission intersection (`caller ∩ tess`)** via in-memory `acting_for_caller_id` + **per-caller session partitioning (`tess:ask:<caller_id>`)** + declarative **`MutationPolicy` (`ConfirmRequired` default)** + **Two-Phase `HostChannel` Apply**. | Zero prompt-memorization safety; non-owner PMs stay `COST-7` scoped; Owner context never leaks to another caller; `ADR-0031` private notes & secrets stay unreachable even for Owner; Tier 2/3 mutations execute only when the human clicks **Apply** under their own authenticated `HostChannel` grant. |
| **Offline Fallback** | **Keep deterministic `answerExecutiveQuestion()`**, strictly badged **`Offline quick summary · Local data only`** + *"Connect runtime →"*; kill misleading catch-all KPI dump on unmatched write/how-to queries. | Instant zero-latency utility when offline or before runtime setup, with 100% honesty and zero fake-AI confusion. |

---

## 2. End-to-End Architecture & Two-Phase Action Flow

```mermaid
flowchart LR
  subgraph Browser["Browser UI (Svelte 5)"]
    CMDK["Cmd+K Launcher"]
    PANEL["Persistent Right-Hand Tess Panel\n(Context Pill · Activity Pill · Action Cards)"]
    CMDK -- "Ask Tess handoff" --> PANEL
  end

  subgraph Web["agentic-org-web (BFF)"]
    ASK["POST /tess/ask\nPOST /tess/turns/{id}/cancel"]
    SSE["GET /tess/turns/{id}/stream\n(HTTP/1.1 SSE)"]
    APPLY["POST /tess/proposals/{id}/apply\n(Human HostChannel Grant)"]
  end

  subgraph Office["agentic-org-office (Authority & Session Host)"]
    HUB["SessionStreamHub\n(post-redaction · shared w/ TASK-103)"]
    AUTH["RunBindingRegistry\nacting_for_caller_id (caller ∩ tess)\nMutationPolicy Guard"]
    STORE["Org / Work / Process / Suggestion Store\n(--dry-run preflight & HostChannel Apply)"]
  end

  subgraph Runtime["Warm Per-Caller Session (tess:ask:<caller_id>)"]
    HARNESS["Claude -p / Codex / Agy\nOperatorSandboxed Preset (empty 0700 home)"]
    CLI["agent-os session attach\n(~1-2 ms Unix socket · 0 MCP)"]
  end

  PANEL --> ASK --> Office
  HUB --> SSE --> PANEL
  HARNESS --> CLI --> AUTH --> STORE
  STORE -- "ui.block (ActionProposal / ui.present)" --> HUB
  HARNESS -- "text.delta / tool.progress" --> HUB
  PANEL -- "Human clicks Apply" --> APPLY --> STORE
```

---

## 3. Three Live Repo Prerequisites Caught in Audit

1. **Codex `ReadOnly` shell lockout (`crates/org-agent-runtime-codex/src/plan.rs:57-75`, `preset.rs:76-104`):** Codex forces `-c features.shell_tool=false` whenever shell is scoped (`CommandPrefix("agent-os")`) instead of `RuleScope::Any`, locking Tess completely out of `agent-os` on Codex.
   - **Fix (`TASK-80.1`):** Add an **`OperatorSandboxed` / `ChiefOfStaff` preset**: empty repo-less `0700` private home, zero repo writable roots (only `private_home` + `TMPDIR`), `denied_office_globs` intact, `allowed_unix_sockets = [AGENT_OS_SOCKET]`, `ReadOnly` + `CommandPrefix("agent-os")` on Claude/Agy, and narrow Starlark `agent-os` execpolicy allowance on Codex.
2. **Launch catalogue saturation (`crates/org-command/src/help.rs:15`, `instructions.rs:92`):** Flat 90-command `catalogue()` is **4,054 / 4,096 B** (42 B left) and `render_command_catalogue()` is already **5,256 B** (1,160 B over `CATALOGUE_LIMIT`).
   - **Fix (`TASK-85.3`):** Switch to grouped resource-level catalogue (`~1.65–1.95 KiB` / ~440 tokens).
3. **Inverted KV prefix order (`crates/org-office-sessions/src/instructions.rs:133-205`):** `# Agent Identity` is currently Part 1 instead of static behavioral rules + catalogue (`AX-11`).
   - **Fix (`TASK-85.2`):** Reorder strictly static-to-volatile so Tiers 1–2 remain byte-identical across all agents and turns.

---

## 4. Token-Efficient Product Knowledge & 5-Tier Discovery

| Tier | Surface & Owner | Hard Budget | Contents for Tess | KV-Cache Scope |
| --- | --- | --- | --- | --- |
| **Tier 1 (Base)** | `BUILTIN_BEHAVIORAL_RULES` (`instructions.rs`) | `<= 10,240 B` | Identical to all agents: turns, untrusted-data fencing, `agent-os` mental model, `--help`/`--schema`, `skill list\|read`. Zero Tess-specific prose here. | All agents, all orgs, all turns |
| **Tier 1b (Role)** | `CHIEF_OF_STAFF_GUIDANCE` (Pos 5, parallel to `HR_LEAD_GUIDANCE`) | `<= 2,048 B` | Executive voice, caller-scope rule, never mutate silently (use `suggestion create`), `session attach` batching, and a **~350 B 7-line Product-Guide Topic Index**. | All turns of Tess |
| **Tier 2** | Grouped `CommandTable::catalogue()` (Pos 2) | `<= 4,096 B` (~1.8 KiB) | Every active resource + verb + gloss, including Tess operator resources (`suggestion`, `ui`, `office`, `provider`, `budget`, `session`, `hr`, `role`, `process`, `skill`). | All agents, all orgs, all turns |
| **Tier 3** | On-demand `agent-os skill list [--category ...] \| skill read --name <key>` | `0 B` standing; ~1.5–2.5 KiB on demand | Extend `BuiltinSkill` (`crates/org-process/src/skills.rs`) with `category: workflow \| product_guide \| chief_of_staff`. Compiled, `< 1 ms`, offline, verified by `verify_command_references` (`AX-14`). | Transient on topic hit |
| **Tier 4** | Offline `--help` / `--schema` + batched `session attach` live reads | `0 B` standing; pruned via `--fields` / `--format compact` | Exact flags/mutation policy + live org state fetched strictly on demand (~10–15 ms for 6-read batch). | Transient per turn |
| **Volatile Turn Envelope** | Appended LAST in turn input | `<= 500–800 B` | `mode`, `caller_id`, `caller_role`, active `route`, optional `selected_ref` + compact 1-line summary header for `"Ask about this"`, high-level org counts. User prompt fenced as untrusted data (`INSTR-5`). | Uncached (strictly trailing) |

### 7 Compiled Version-Locked Product-Guide Skills (`crates/org-process/src/skills.rs`)
1. `product-org-and-teams` — Organizations, team hierarchy, accountable vs contributing teams, people vs agents, roles, `may_hire`, HR hire flow (`ADR-0029`), team `max_members`.
2. `product-projects-and-work` — Projects, repos, per-task worktrees, tasks, checklists, private notes & decisions (`ADR-0031`), DSS vs Kanban process packages.
3. `product-processes-and-standards` — Single-chain org → team → project inheritance, `overridable: false` precedence, `--why` resolution chain.
4. `product-offices-and-runtimes` — HQ vs satellite offices, Claude/Codex/Agy adapters, model profiles, provider pools, failover, offline office recovery.
5. `product-budgets-and-permissions` — Daily/monthly/task budgets, holds, permission presets, default-on sandbox, escalation rules (`ADR-0032`).
6. `product-troubleshooting` — Diagnosing offline offices, stalled/recovery-locked sessions, rate limits, credential enrollment, safe remediation proposals.
7. `tess-workflows` — Executive Brief read-only rules, `session attach` batching recipes, `suggestion create` preflighting, and `ui present` discipline.

---

## 5. Authorization, Zero-Memorization Safety & Operator CLI Surface

### A. Server-Side Dual-Intersection Auth & Hard Invariants
- **Effective Permission Intersection (`effective = caller_permissions ∩ tess_capabilities`):** Interactive `chief_of_staff.ask` turns bind `acting_for_caller_id = Some(caller_id)` in-memory on `RunBindingRegistry` (never CLI flags). Every RPC re-evaluates the caller's live permissions (`COST-7`/`COST-9` PM spend scoping, owner-only inbox).
- **Per-Caller Session Partitioning:** Warm interactive sessions are keyed as `tess:ask:<caller_id>` (separate `runtime_session_id` per caller; rotated at 60% context window or 20 turns) so Owner context can never bleed into another member's LLM window.
- **Hard Non-Bypassable Denies (Even When Caller == Owner):**
  - Other employees' `ADR-0031` private notes (`private/notes.db`) — Tess is `kind = 'agent'`, sees only her own notes.
  - Non-member 1:1 DMs (`chat.read_as_owner` stays forbidden for agents).
  - `CredentialVault`, OAuth enrollment tokens, wrapped CA keys, TLS certs, raw SQLite/journal files (`denied_office_globs`), `AGENT_OS_SESSION_TOKEN`.
  - Self-escalation (`may_hire`, `permission_preset`, `builtin_key`), offboarding Tess or built-in HR while org exists, or any `bypassPermissions` mode.

### B. Declarative `MutationPolicy` & Two-Phase Human Commit
- **`AutoSafe`:** `suggestion create|withdraw`, `ui present`, `status set|clear`, Tess's own private `note`, replying to caller.
- **`ConfirmRequired` (Default for ALL durable org/work/config mutations in v1):** Attempting a direct write in Chief-of-Staff mode fails closed with `ErrorCode::PolicyViolation` + exact `error.hint` steering directly to `agent-os suggestion create` (1-turn self-healing on smaller models).
  - `suggestion create` validates against `CommandTable`, executes real server-side `--dry-run` preflight, pins `expected_revision` (`--if-match`), persists the `Suggestion` (`SUG-42`), and emits a verified `ui.block` (`kind: "action_proposal"`) on the SSE stream.
  - Clicking **Apply** sends `POST /api/v1/organizations/{org}/tess/proposals/{id}/apply` over the **human caller's authenticated `HostChannel` grant (`via_grant: true`)**, re-verifying caller, live permissions, expiry, and `if_match`, with dual attribution (`actor = caller`, `proposed_by = tess`).
- **`ForbiddenForAgent` (Nav Only — Never on `agent-os`):** Host admin/reset/TLS, owner handover, account invites/access removal, satellite PKI enrollment/removal, OAuth credentials, direct budget/session approval overrides.

### C. Operator CLI Delta (`crates/org-agent-cli`) & HTTP/SSE Endpoints
- **Added to `agent-os` (server-gated to `chief_of_staff` + `acting_for_caller_id`):** `project add-team|remove-team`, `cost query|prices|subscription-usage`, `budget list|inspect`, `session list`, `agent execution-get|readiness`, `provider list|quota`, `model-profile list`, `office sync-status|list`, `inbox list` (owner-only), `channel create`, `suggestion create|list|inspect|withdraw`, `ui present`.
- **HTTP/1.1 + SSE Bridge (`apps/agentic-org-web` ↔ `org-office-server`):**
  - `POST /api/v1/organizations/{org}/tess/ask` → `{ turn_id, status: "streaming" | "fallback", stream_url, fallback_answer?, unavailable_reason? }`
  - `GET /api/v1/organizations/{org}/tess/turns/{turn_id}/stream` (SSE with `Last-Event-ID`, 15s heartbeat; events: `turn.started`, `tool.progress`, `text.delta`, `ui.block`, `turn.completed`; fed post-redaction from shared `SessionStreamHub` in `org-office-sessions`)
  - `POST /api/v1/organizations/{org}/tess/turns/{turn_id}/cancel` (`Control::Interrupt`, shared with `TASK-103`)
  - `POST /api/v1/organizations/{org}/tess/proposals/{proposal_id}/{apply,dismiss}`
  - `POST /api/v1/organizations/{org}/tess/session/reset`

---

## 6. Native Embedded UX & Wireframe (`TESS-UX-1..12`)

```text
┌────────┬──────────────────────────────────────────────┬─────────────────────────┐
│ Org  ▾ │   [ 🔍 Search, run a command, or ask Tess… ⌘K | ✨ Ask ]  [+ New N]   │
├────────┼──────────────────────────────────────────────┼─────────────────────────┤
│ Home   │  <main> (≥ 720px, navigates independently)   │ ✨ Tess · Ready  [▾][↗][×]│
│ Inbox• │                                              ├─────────────────────────┤
│ Work   │  Projects / Work / Inbox / Settings          │ You                     │
│ Chat   │  (remains live while conversing with Tess    │ Create an iOS project   │
│ Activ. │   and reviewing Action Proposal Cards)       │ with Kanban for Mobile  │
│ Proj.  │                                              │                         │
│ Teams  │                                              │ ▸ Checked 3 teams ·     │
│ Offices│                                              │   Checked processes ▾   │
│ Sett.  │                                              │                         │
│        │                                              │ I've prepared the new   │
│        │                                              │ project setup:          │
│        │                                              │ ┌─────────────────────┐ │
│        │                                              │ │ Create project      │ │
│        │                                              │ │ Reversible          │ │
│        │                                              │ │ Name:    iOS App    │ │
│        │                                              │ │ Process: Kanban Flow│ │
│        │                                              │ │ Team:    Team Mobile│ │
│        │                                              │ │ [Create project]    │ │
│        │                                              │ │ [Edit]   [Cancel]   │ │
│        │                                              │ └─────────────────────┘ │
│        │                                              ├─────────────────────────┤
│        │                                              │ [📁 Viewing: Projects ×]│
│        │                                              │ ┌─────────────────────┐ │
│        │                                              │ │ Ask Tess anything…  │ │
│        │                                              │ │               [Send]│ │
│        │                                              │ └─────────────────────┘ │
└────────┴──────────────────────────────────────────────┴─────────────────────────┘
```

- **Adaptive Container:** Docked non-modal right-hand `<aside data-tess-panel>` (`360–420px`, `<main> >= 720px`) on wide desktop (`>= 1440px` expanded sidebar or `>= 1280px` collapsed rail); modal right-hand Sheet below docking width; full-width `100dvh` Sheet `< 640px` (`registerModalBack`, `320px` & `200%` text zoom safe).
- **Zero Terminal Dump Primitives:**
  1. **Ambient Context Pill:** Dismissible route/entity chip above composer (`Viewing: Project · Agentic Org v1.0 ×`).
  2. **Calm Tool-Progress Pill:** Translates `tool.progress` to plain product language (*"Checking projects…"* → *"Still checking…"* after 2s) + in-place **Stop** button; collapses settled calls into `Checked 4 projects · 1.4s ▾`.
  3. **Clickable Entity Mini-Rows & Follow-Ups:** Native `--exec-*` rows navigating `<main>` while docked panel stays open; max 3 follow-up chips on latest turn only.
  4. **Action Proposal Cards (`TessActionProposalCard.svelte`):** Dry-run parameter table, risk/reversibility badge (icon + word), consequence sentence, verb-object CTA (`aria-busy`), **`Edit in form`** escape hatch, **`Cancel`**, and immutable server-confirmed receipt.
  5. **Coherent Identity & Badged Offline Fallback:** Home `ExecutiveBriefCard` stays strictly read-only; `⌘/` on Tess's `/chat` DM focuses the DM composer instead of duplicating Tess; local `answerExecutiveQuestion()` renders strictly badged **`Offline quick summary · Local data only`** and replaces its unmatched catch-all with an honest capability message + manual shortcut.
- **6-Theme & A11y Parity:** Uses `--side-panel-surface`, `--exec-*`, `--tone-*`, `--field-border`, `--focus` across `ocean`, `classic`, `graphite`, `cozy`, `report`, `report-dark`; milestone-only `.sr-only` `aria-live="polite"`; `role="alert"` inline errors without duplicate toasts; `prefers-reduced-motion: reduce`.

---

## 7. Multi-Runtime Empirical AX Eval Gate (`TESS-AX-1..6` in `TASK-85.4`)

Executed against a real disposable office + `agent-os` across `claude -p`, `codex`, and `agy` (flagship + cost-effective models):
1. **`TESS-AX-1` (Product Q&A & Progressive Discovery):** Calls `skill read` in `<= 2` hops, `<= 2,500` uncached tokens, cites real flags, zero hallucinated flags.
2. **`TESS-AX-2` (Batched Org Synthesis & Read-Only Brief):** Uses `session attach` (`< 50 ms` IPC) or `<= 3` pruned reads; Brief emits 0 inline action cards and never prints `"Tactical Execution & Squad Supervisor"`.
3. **`TESS-AX-3` (Action Proposal & Self-Healing):** Zero silent live mutations; `>= 90%` 1-turn recovery from `policy_violation` to `suggestion create`; 100% preflight-valid proposals; 0 raw JSON fences.
4. **`TESS-AX-4` (Caller Scope, `ADR-0031` Private Notes & Prompt Injection):** `0` cross-caller leaks, `0` private-note reads, `0` secret/token leaks, prompt injection ignored.
5. **`TESS-AX-5` (Stale Revision Conflict Recovery):** `>= 90%` next-turn recovery from bounded `ConflictDetail` (`<= 8 KiB`) on `suggestion create`.
6. **`TESS-AX-6` (3/3 Runtime & Cost-Effective Parity):** Identical pass across Claude, Codex (with `OperatorSandboxed` shell), and Agy; `>= 80%` turn-2+ KV cache hit; `<= 5%` CLI usage error rate; `0` MCP servers.

---

## 8. `v1 (m-0)` vs. `v2 (m-1)` Board Roadmap

| Task | Milestone | Scope | Key Dependencies |
| --- | --- | --- | --- |
| **`TASK-80.1`** | **`m-0` (v1)** | Identity (`builtin_key='chief_of_staff'`), `OperatorSandboxed` preset (fixing Codex shell lockout), per-caller session partitioning, `caller ∩ tess` dual-intersection security, `CHIEF_OF_STAFF_GUIDANCE` + 350 B topic index | `TASK-42`, `TASK-45`, `TASK-83`, `TASK-85.2`, `TASK-85.3` |
| **`TASK-80.4`** | **`m-0` (v1)** | Operator CLI delta on `agent-os`, `MutationPolicy` + server `--dry-run`, `suggestion create` & `ui present` tools, 7 compiled `product-*`/`tess-workflows` skills, `SessionStreamHub` + SSE streaming + Two-Phase `HostChannel` Apply | `TASK-80.1`, `TASK-103` |
| **`TASK-80.5`** | **`m-0` (v1)** | Embedded Tess Side Panel UI (`⌘K` fast launcher + persistent right-hand drawer, context pill, calm tool progress, clickable entity rows, Action Proposal Cards, badged offline fallback — `TESS-UX-1..12`) | `TASK-80.4` |
| **`TASK-85.4`** | **`m-0` (v1)** | Adds `TESS-AX-1..6` live multi-runtime evals across `claude -p`, `codex`, and `agy` | `TASK-80.1`, `TASK-80.4` |
| **`TASK-80.2`** | **`m-1` (v2)** | Scheduled / LLM-Synthesized Executive Brief on Home (Today, 7-Day, Board lenses — v1 already ships deterministic zero-token `deriveExecutiveBrief`) | `TASK-80.1`, `TASK-80.4` |
| **`TASK-80.3`** | **`m-1` (v2)** | Proactive Background Monitoring & Suggestions Queue in Owner Inbox (directly reuses `TASK-80.4`'s `Suggestion` & `suggestion.apply` pipeline) | `TASK-80.1`, `TASK-80.4` |
