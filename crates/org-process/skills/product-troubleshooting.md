# Troubleshooting offices, sessions, limits, and recovery

Use read-only diagnostic commands first to isolate whether an issue is an office sync problem, a session stall or recovery lock, a provider cooldown, a budget hold, or missing credentials.

## Offline or lagging offices

- Run `agent-os office list` and `agent-os office sync-status`.
- Check `initial_snapshot_required`, `restore_pending`, `pending_work_count`, and `pending_message_count`.
- If a satellite is awaiting its initial snapshot or lagging behind HQ's epoch, wait for sync catch-up or advise the owner to inspect host connectivity and office logs before retrying writes.

## Stalled or recovery-locked sessions

- Run `agent-os session inspect` (or pass `--employee-id <id>` when acting for the owner) and `agent-os session list`.
- Look for `state: stall_locked` (`consecutive_stalls`), `state: recovery_required`, `state: waiting_approval`, or a populated `last_error`.
- `waiting_approval` means a human must decide an open escalation in the owner inbox (`agent-os inbox list`).
- `stall_locked` or `recovery_required` indicates repeated turn failures or interrupted teardown; diagnose the root cause from `last_error` and readiness before proposing a reset or restart.

## Rate limits, readiness, and budget holds

- Run `agent-os agent readiness --member <member>` to see exact readiness codes (`NO_EXECUTION_CONFIG`, `EXECUTION_DISABLED`, `PROFILE_ARCHIVED`, `MODEL_UNVERIFIED`, `BUDGET_HELD`, `POOL_EXHAUSTED`, `NO_READY_CONNECTION`).
- For `POOL_EXHAUSTED` or `NO_READY_CONNECTION`, inspect `agent-os provider list` and `agent-os provider quota` to identify connections in cooldown or at concurrency capacity.
- For `BUDGET_HELD`, inspect `agent-os budget list` and `agent-os inbox list` to see which daily, monthly, or task cap triggered the hold.

## Credential enrollment and safe remediation

- Missing or expired provider credentials show up as `auth_expired` in readiness diagnostics; enrollment and credential management are strictly human-only and never run by agents.
- When remediation requires a durable change (pausing/resuming an agent, updating a task, or adjusting team setup), prepare a validated proposal with `agent-os suggestion create --help` instead of mutating state directly.
