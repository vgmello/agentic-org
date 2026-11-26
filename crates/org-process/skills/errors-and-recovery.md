# Errors and recovery

A failed call prints `ok: false` and an `error` with `code`, `message`, `hint` and sometimes `retry_with`. Read the hint first: it says what to do next. `retry_with` lists the flags to change for a retry that can succeed.

Exit code 2 means the command line itself could not be read: fix the flags (`--help` shows them). Exit code 1 means the office refused or failed.

## The codes

The set of codes is closed. What each one asks of you:

- `invalid_request`, `invalid_name`, `too_large`: what you sent is wrong. Fix it; repeating it does not help.
- `not_found`, `unknown_recipient`: the thing you named does not exist or is gone. Look it up again.
- `already_exists`: it exists already. Use the existing one.
- `conflict`: the record changed since you read it, or it is in the wrong state. Read it again, then decide.
- `not_authorized`: you may not do this. Do not try another way; ask someone who may.
- `checklist_incomplete`: required checklist steps are not ticked.
- `cycle`, `not_empty`, `not_a_member`, `owner_protected`: the change would break the organization's structure.
- `unavailable`, `not_ready`, `timeout`, `storage_error`: a temporary failure. Retry later with the same idempotency key.
- `rate_limited`: wait before you retry.
- `unsupported`: this command or option is not available here, such as `--dry-run` on most writes.
- `operation_reused`: you reused an idempotency key for a different change. Use a new key.
- `expired`, `clock_skew`, `snapshot_required`, `unsupported_version`, `unknown_method`, `not_bound`, `cancelled`, `unknown`: tell a person if it repeats.

## Optimistic concurrency

Writes to records with a revision take `--if-match <revision>`, and some require it:

```bash
agent-os task inspect --task <task> --fields task.revision
agent-os task transition --task <task> --status done --if-match <revision>
```

On `conflict`, read the record again, check that your change still makes sense against what changed, then retry with the new revision. Never retry blindly with a fresh revision.

## Retrying safely

Every write has an idempotency key, yours (`--idempotency-key <key>`) or one the command made and reported in `meta.operation_id`. Retrying with the same key cannot apply the change twice, so after a timeout or a lost answer, retry with that key before you assume anything.

## When your session breaks

- **Stalled**: if a turn stops making progress, the office ends it and reaps its background work. Reassess, report where you are, and continue from durable state: the task, your notes and your branch.
- **Restarted**: if the office restarts during your turn, a later turn may tell you which `agent-os` calls were already accepted. Do not repeat a write you are unsure of: read the record first, or retry with the same idempotency key.
- **Recovery required**: after some failures the office holds your session as `recovery_required` until a person reviews it. Nothing you do clears it; start nothing new and wait.

Write durable state as you go (notes, comments, pushed commits), so a reset costs a few minutes and not the work.
