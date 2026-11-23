---
id: doc-15
title: Backend Stream Handover (Tracks A and B)
type: guide
created_date: '2026-11-21T00:16:00Z'
updated_date: '2026-11-22T14:44:00Z'
tags:
  - workstreams
  - backend
  - handover
---
# Backend stream (Track A and B)

Last updated: 2026-11-22 (v1; TASK-79 backend implemented + Pass 1 Neon-reviewed in working tree, awaiting Pass 2 final review & commit)

Scope: Rust crates, the office worker (`apps/agentic-org-office`), host and CLI apps (`apps/agentic-org`, `apps/agent-os`), server side of `apps/agentic-org-web`, server-to-server wire, office store & engines, data-contract/backend design docs, and backend rows of `doc-3` (Feature Checklist). Active tracking lives in Backlog.md (`backlog task list --plain -m m-0`); every remaining `m-0` backend task contains its own self-contained implementation blueprint so any session can pick up directly from Backlog without a separate handover file.

## Status Summary (v1 / Milestone m-0)

| Increment / Task | Status | Evidence (`v1`) | Remaining |
| --- | --- | --- | --- |
| Foundation, Host, Auth, Multi-person, R1 Git Import (`TASK-33`–`TASK-40`) | Done | Merged on `v1` | — |
| HR Hiring & Supervised Agent Sessions (`TASK-41`, `TASK-42`) | Done | Merged on `v1` | — |
| Multi-Harness Support: Claude, Codex & Agy (`TASK-83`) | Done | `a4b56940` | — |
| Chat, Structured Asks, Suspend/Resume, Watcher (`TASK-45`–`TASK-48`) | Done | Merged on `v1` | — |
| Private Task Notes (`TASK-50` / L8) & Shared Git Worktrees/Push (`TASK-51` / C7) | Done | `4011f8c5`, `86801552` | — |
| Multi-org Prefs, Usage/Cost Ledger, CSV Export & Budgets/Holds (`TASK-53` / L1–L3) | Done | `d2d980ad` | Wire PM release-budget auth once `TASK-55` adds release->project catalog |
| Provider Subscription Pools, Model Discovery, Quota Permits & Failover (`TASK-54` / L4) | Done | `fcbeaf9b` | — |
| Session Transcripts (`TASK-43`), Org Processes (`TASK-44`), Typed Tasks (`TASK-49`), Inbox Snooze (`TASK-81`) | Backend Done / UI Pending | `fea5915c`, `75bfddca`, `d9b289e6` | Remaining unchecked ACs are UI (`apps/agentic-org-ui`) screens |
| Subscription Model Usage Collection & API (`TASK-79`) | In Progress (Final Review) | Working tree (Pass 1 Neon fixes applied) | Verify suite + Repowise `exit 0` + Pass 2 fresh final review -> check AC #1/#2 -> commit on `v1`; UI AC #3/#4 for `ui-dev` |
| Second Office Enrollment, `org-sync` Replication & Relay (`TASK-52` / S1 / M3) | To Do | Blueprint in `TASK-52` | Highest-priority next backend task |
| Task Blocking Model, Block History & `agent-os` CLI Parity (`TASK-84`) | To Do | Blueprint in `TASK-84` | Ready immediately |
| Project Docs Tree, Features, Knowledge, Releases & Flags (`TASK-55` / L7 & L11) | To Do | Blueprint in `TASK-55` | Also unlocks release->project PM budget scoping |
| Composable Processes, Issued Doc Versions & Built-in DSS (`TASK-56` / L12–L14) | To Do | Blueprint in `TASK-56` | Depends on `TASK-55` |
| Chief of Staff Agent — Tess (`TASK-80`, `80.1`–`80.4`) | To Do | Blueprints in `TASK-80.*` | Execute `80.1` -> `80.2` -> `80.3` -> `80.4` |
| `os-sandbox` (`TASK-57`), Dedicated Accounts & Org CA (`TASK-58`), Extensions (`TASK-59`), Release/Diagnostics/Copilot (`TASK-60`), Small Follow-ups (`TASK-61`) | To Do | Blueprints in each task | Execute after M3 / L7–L14 / Tess per priority |

## Next Up (Pick-Up Order)

1. **Finish `TASK-79` in working tree:** verify tests + Repowise (`exit 0`) + Pass 2 separate fresh final review -> check AC #1 & #2 -> commit cleanly on `v1`.
2. **`TASK-52` (`S1` — Milestone 3)** or **`TASK-84` (Task blocking model + CLI parity)** -> **`TASK-55` (`L7/L11`)** -> **`TASK-56` (`L12–L14`)** -> **`TASK-80.1..80.4` (Tess)** -> **`TASK-57..61`**.

## Stream Rules & Quality Gates

- Use `/Users/vgmello/.bun/bin/backlog` for all task/doc edits; never hand-edit `.backlog/` files.
- Every backend increment passes `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, affected tests (including `org-office --test engine_boundary` and `org-office-server serve::host`), `python3 -B -m unittest docs/design/organization-data/test_schema.py`, `python3 .repowise/pre_commit_health.py` (`exit 0`), Pass 1 fresh read-only review -> fixes -> separate fresh final review (`VERDICT: APPROVE`, 0 High / 0 Medium; no Codex/Sol per owner rule).
- Never stage `.agent-kit/project-skills/principal-designer.zip`. Commit on `v1`; do not push unasked; never push to `main`.

## For the Other Streams

- **UI (`ui-dev`):** Backend `/api/v1` APIs are live on `v1` for `TASK-43` (session transcripts), `TASK-44` (processes/standards/skills), `TASK-49` (unified tasks/checklists/history), `TASK-53` (multi-org prefs, costs, CSV export, budgets), `TASK-54` (provider pools, bindings, model discovery, readiness, quota), `TASK-81` (inbox snooze/un-snooze), and `TASK-79` (`GET /costs/subscriptions/usage`, `POST /costs/subscriptions/usage/{collect,credentials,credentials/revoke}` once committed).
