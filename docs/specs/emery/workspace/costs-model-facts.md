# Costs and budgets: what exists today (facts)

For `knowledge/prds/costs-and-budgets-v1.md`. Product repo `/Users/vgmello/projects/agentic-consulting/app`, branch `origin/v1` at `4acb522a` (2026-11-12). Every citation is `path:line` at that commit.
Labels: **[F]** fact, read in the code or spec · **[A]** assumption, to verify · **[I]** inference from the facts.

Search used: `git grep -n -i -E 'token|cost|usage|usd|budget' origin/v1 -- crates apps docs/specs`, then narrowed. Most hits for "token" are auth tokens (`apps/org-office-host/src/app_auth.rs`, `crates/org-proto/src/token.rs`, `crates/org-runtime/src/token.rs`) and are not about usage.

## 1. Token usage and cost capture

| # | Fact | Citation |
| --- | --- | --- |
| F1 | **[F]** A normalized `Usage` type exists in the runtime contract crate: model, input (excluding cache reads), output (reasoning included), cache_read, cache_write, reasoning, `cost_usd: Option<f64>`, `cost_basis`, `token_scope`, `cost_scope`. | `crates/org-runtime/src/usage.rs:33-57` |
| F2 | **[F]** Rules on `Usage`: a session emits all of a turn's usage before that turn's `TurnEnded`, "so the office can close the run's numbers when the turn ends". Tokens and cost have separate scopes; Claude's `total_cost_usd` is named as a running total. Cumulative means "since this process started"; the office takes differences. | `crates/org-runtime/src/usage.rs:14-30` |
| F3 | **[F]** A resume with no baseline loses the first turn's usage ("the first turn after such a resume isn't counted"), with a Notice warning. | `crates/org-runtime/src/usage.rs:26-30` |
| F4 | **[F]** `CostBasis` has `ListPriceEstimate` ("The runtime's estimate at list prices; not what the account is billed"), `None` ("the office prices the tokens itself, if at all") and `Unknown`. | `crates/org-runtime/src/usage.rs:81-93` |
| F5 | **[F]** `SessionEvent::Usage(Usage)` is a session event. | `crates/org-runtime/src/event.rs:111-112`; exported at `crates/org-runtime/src/lib.rs:195` |
| F6 | **[F]** Each runtime adapter declares its usual `usage_scope` and its `cost: CostBasis`. | `crates/org-runtime/src/caps.rs:117-120` |
| F7 | **[F]** No real runtime adapter implements the contract on `origin/v1`. The only `impl Session` is a test fake. | `crates/org-runtime/tests/fake_session.rs:136` |
| F8 | **[F]** Nothing outside `org-runtime` consumes `SessionEvent` (no match in `apps/`, `crates/org-office`, `crates/org-supervise`). No table stores usage or cost: no `CREATE TABLE` in `crates/org-office` mentions usage, cost, run or session. | `git grep` over those paths, empty |
| F9 | **[I]** So **token usage and cost are not captured anywhere today**, per session, run, task, agent or project. The contract to capture them exists (F1-F6); the adapters, storage, attribution and roll-ups don't. | from F7, F8 |
| F10 | **[F]** Copilot (smart features) runs return `Usage { input_tokens, output_tokens }` only (no cache, no cost), commented "for the 'Copilot' cost line". | `crates/org-copilot/src/runtime.rs:52, 57-59` |
| F11 | **[F]** The copilot `RunRecord` ("What the office logs per run") carries `usage: Option<Usage>`; it is filled from the run output. | `crates/org-copilot/src/service.rs:138-148, 269` |
| F12 | **[F]** A request outcome's `Estimate` has `effort` and `cost_usd_cents_min/max` (phase 3). The UI formats it as "≈ $40–70". It is an estimate typed by a scoper, not measured cost. | `crates/org-proto/src/work.rs:670-678`; `apps/org-ui/src/lib/work/requests-view.ts:102` |

## 2. Pricing

| # | Fact | Citation |
| --- | --- | --- |
| F13 | **[F]** No price table, price list or per-model rate exists in code. The only "price" hits are `CostBasis::ListPriceEstimate` and Eden's spec. | `crates/org-runtime/src/usage.rs:86-88`; `docs/specs/costs-and-insights-ux.md:121` |
| F14 | **[F]** Eden's spec designs a dated price list per model in Settings › Costs, editable, which "re-estimates past months only on request". | `docs/specs/costs-and-insights-ux.md:121` |

## 3. The agent's current task

| # | Fact | Citation |
| --- | --- | --- |
| F15 | **[F]** `member_status` holds one row per member: `status_text`, `current_task_id`, `updated_at`, `expires_at`. It is "the local runtime row": one current value, overwritten, **no history**. | `crates/org-office/src/store/status.rs:1, 14-22, 109-110` |
| F16 | **[F]** `work.set_current` sets or clears the bound agent's current task. | `crates/org-proto/src/work.rs:1333-1349`; `crates/org-office/src/status.rs:111-150` |
| F17 | **[F]** Setting it is refused unless the caller is an agent, the item exists, is a **task** (kind check), isn't deleted, and **is assigned to the caller**. A bug can't be a current task; a reviewer can't set someone else's task. | `crates/org-office/src/status.rs:121-140` |
| F18 | **[F]** On read, the stored task is "advisory": it is re-checked and dropped if the item is no longer an undeleted task assigned to the member. | `crates/org-office/src/store/status.rs:128-140` |
| F19 | **[F]** `current_task_id` is shown on the member card and in `who`. | `crates/org-proto/src/directory.rs:148`; `crates/org-office/src/organization/directory.rs:31, 327`; `apps/org-agent/src/office.rs:827` |
| F20 | **[I]** Attributing usage "to the task the agent was working on at that time" needs the current task **at the time of each usage report**. The single overwritten row (F15) doesn't keep that; the attribution must be taken when usage arrives, or the current-task history must be kept. Rowan's call. | from F2, F15 |

## 4. Work items, plans and fields

| # | Fact | Citation |
| --- | --- | --- |
| F21 | **[F]** Item kinds are task, bug, request. No work type field exists. | `crates/org-proto/src/work.rs:175-182, 355-440` |
| F22 | **[F]** No release or milestone exists in `org-proto`'s work model (no match for "release" or "milestone" in `work.rs`). They are specified in `projects-planning-v1.md` (PL-1..7), not built. | `git grep` over `crates/org-proto/src/work.rs`, empty |
| F23 | **[F]** The Insights tab is disabled ("Available in a later release"). | per `project-insights-v1.md` §1 (`apps/org-ui/src/routes/(shell)/projects/[id]/+page.svelte:22, :54`) |

## 5. Existing specs that touch cost (design, not built)

| # | Fact | Citation |
| --- | --- | --- |
| F24 | **[F]** `costs-and-insights-ux.md` rev 5 (Eden, status for-review): the unit is the session; cost is always tokens at API prices in US dollars, plans not shown; Reported / Estimated / Not available. | `docs/specs/costs-and-insights-ux.md:34-38` |
| F25 | **[F]** Its attribution order: the task the session started on, else the agent's current task, else the agent's only In progress task; else "Not linked to a task". A session splits at a task switch. | `docs/specs/costs-and-insights-ux.md:39` |
| F26 | **[F]** Its budgets: monthly, per project only, warn at 80% and 100%, "Nothing stops agents". | `docs/specs/costs-and-insights-ux.md:43, 105-110, 159` (BUD-1) |
| F27 | **[F]** Its runtime table: Claude Code print mode gives cost and usage per run; interactive sessions give tokens per message, no cost; Codex gives tokens; Antigravity unknown (all "to confirm, C15"). | `docs/specs/costs-and-insights-ux.md:112-119` |
| F28 | **[F]** `agent-run-modes-ux.md` makes every wake a run with trigger, duration, cost and outcome (RUN-5), and names one-off agents "one-off R-n" (RUN-12). "R-n" is also the release key in `projects-planning-v1.md` §4.1. | `docs/specs/agent-run-modes-ux.md:53, 57, 113, 120` |
| F29 | **[F]** RUN-15: internal delegates and memory forks are owned work inside an employee's run, "with shared aggregate limits and attributed usage". | `docs/specs/agent-run-modes-ux.md:124` |

## 6. What runtimes report (assumptions, Rowan to verify)

- **[A] Claude Code**, from its public docs: in print / SDK mode (`--output-format json` or `stream-json`), the final `result` message carries `total_cost_usd`, a `usage` object (`input_tokens`, `output_tokens`, `cache_creation_input_tokens`, `cache_read_input_tokens`) and per-model usage. Claude Code can also export OpenTelemetry metrics, including a cost counter and a token counter by type (input, output, cacheRead, cacheCreation), with session and model attributes. The cost is a client-side estimate at list prices (matches F4's `ListPriceEstimate`), not the bill.
- **[A] Codex CLI**: `codex exec --json` reports token usage per turn (input, cached input, output), and the interactive session shows token usage. It does not report a dollar cost, so the office prices its tokens (F4 `CostBasis::None`).
- **[A]** On a subscription plan (Claude Max, ChatGPT plans), the reported or priced figure is what the same tokens would cost on the API, not money billed. Whether the office can tell which plan an agent runs on is unknown.
- **[A]** Antigravity: unknown (as in F27).

## 7. Summary for the PRD

1. Capture: contract yes (F1-F6), adapters, storage and attribution no (F7-F9).
2. Price list: none (F13).
3. Current task: exists, tasks only, assignee only, no history (F15-F18).
4. Work type, releases, milestones: not built (F21-F22).
5. Prior design: Eden's spec conflicts with the owner's 2026-11-12 request on the unit of cost, attribution, plans and budgets (F24-F26).
