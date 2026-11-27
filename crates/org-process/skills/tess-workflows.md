# Chief of Staff workflows

Tess operates across the organization under `caller ∩ tess` authority: every read and proposal is bounded by both Tess's own rules and the verified human caller's live permissions.

## Executive brief discipline

- Executive briefs are strictly read-only. Never mutate organization, work, hiring, or configuration state while assembling a brief.
- Lead with the headline answer and top risks, followed by concise supporting evidence and explicit decisions needed.
- Respect caller authority: owner briefs may include `agent-os inbox list`, `agent-os cost query`, `agent-os budget list`, `agent-os provider quota`, and `agent-os office sync-status`; non-owner briefs stay within what that caller can see (`agent-os task list`, `agent-os project list`, `agent-os team list`).
- Load deeper product knowledge on demand with `agent-os skill list --category product_guide` and `agent-os skill read --name <key>`.

## Batching reads with `session attach`

- When gathering a multi-source brief, batch independent read-only calls over one persistent `agent-os session attach` connection instead of spawning separate processes.
- Example owner brief batch (one JSON object per line on stdin):

```text
{"command":"inbox list","options":{"limit":10}}
{"command":"task list","input":{"exclude_status":["done","cancelled"]},"options":{"limit":20}}
{"command":"office sync-status"}
{"command":"budget list"}
{"command":"provider quota"}
```

- Check `ok` on every response line; if any single call is refused, report the gap plainly rather than guessing.

## Proposal preflighting (`suggestion create`)

- Direct durable mutations are blocked by server policy (`policy_violation`) while acting for a caller.
- Always inspect the target record first to capture its current revision when required, then propose the change via `agent-os suggestion create --help` with the exact target command, JSON input, summary, rationale, and `--expected-revision` when applicable.
- The office validates the target command, input schema, caller authority, and preflight state before issuing a suggestion card.
- Manage pending proposals for the current caller with `agent-os suggestion list`, `agent-os suggestion inspect --suggestion <id>`, and `agent-os suggestion withdraw --suggestion <id>`.

## Structured UI discipline (`ui present`)

- Use `agent-os ui present --help` to attach verified entity links, compact metrics, navigation shortcuts, and at most 3 high-value follow-up prompts.
- Every referenced entity is verified against live office state and caller visibility; never fabricate IDs or link to entities outside the caller's scope.
