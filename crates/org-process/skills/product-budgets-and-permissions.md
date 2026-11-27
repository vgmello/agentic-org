# Budgets, spend accounting, permissions, and sandboxes

Agentic Org pairs immutable usage accounting and hard execution holds with least-privilege permission presets and default-on OS sandboxing.

## Usage accounting and budgets

- Every turn records immutable normalized token usage and integer micro-USD cost valuation (or explicitly marks records partial or unpriced rather than fabricating zeros).
- Check your own daily, monthly, and optional task allowance with `agent-os budget status`.
- Operators acting for the owner inspect organization-wide spend with `agent-os cost query`, review versioned model pricing with `agent-os cost prices`, and check provider subscription usage snapshots with `agent-os cost subscription-usage`.
- List active budgets with `agent-os budget list` and inspect one budget by ID or scope with `agent-os budget inspect --help`.
- Budgets apply at task (lifetime), agent (daily or monthly), or release scope, measured in either integer micro-USD or total tokens.

## Threshold notices and execution holds

- Reaching 80% of a budget emits a warning notice while work continues.
- Reaching 100% triggers a hard execution hold: running work stops cleanly (`stop_pending` then `held`), queued wakes pause, and an owner inbox problem appears (`agent-os inbox list`).
- The owner resolves a hold by applying a one-time +20% extension, raising or clearing the budget, keeping the agent paused until period reset, or stopping the task.

## Permission presets, sandbox, and escalation

- Every managed agent runs under an explicit permission preset (`read_only`, `safe_ask`, `workspace_edits`, `autonomous_sandboxed`, or `operator_sandboxed`) with OS sandboxing enabled by default.
- Inspect effective presets, writable roots, denied paths, and allow/deny tool rules with `agent-os grant list` and `agent-os session inspect`.
- Denied office-internal storage (private notes, credentials, accounting, and security vaults) is never readable from an agent workspace.
- Actions outside an agent's preset either fail closed or pause in `waiting_approval` for explicit human escalation.
