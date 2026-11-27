# Offices, runtimes, model profiles, and provider pools

Agentic Org runs across one Headquarters (HQ) office and optional enrolled satellite offices, coordinating vendor-neutral runtime adapters, model profiles, and provider connection pools.

## HQ and satellite offices

- Every organization has one HQ office that holds primary authority for organization structure, HR, budgets, and pool coordination.
- Additional machines join as enrolled satellite offices with mutual TLS certificates and replicate organization state, work, and messages back to HQ.
- List registered offices with `agent-os office list` and inspect local replication epoch, sequence, and pending outbox queues with `agent-os office sync-status`.
- Satellites keep working locally during transient disconnects and drain pending work and messages automatically once connectivity returns.

## Runtime adapters and execution config

- Agent sessions launch through consumer-owned runtime adapters behind one shared lifecycle, sandbox, and usage contract.
- Inspect an agent's configured model profile, model ID, reasoning effort, permission preset, and system prompt with `agent-os agent execution-get --member <member>`.
- Inspect live session state, stall counters, and token usage with `agent-os session inspect` or list recorded sessions with `agent-os session list`.

## Model profiles, provider pools, and failover

- Model profiles (`agent-os model-profile list`) group one or more ordered provider connections for failover and load sharing.
- Inspect organization provider connections with `agent-os provider list` and active cooldowns or turn permits with `agent-os provider quota`.
- Check whether an agent is ready to run right now (and why each candidate connection is eligible or skipped) with `agent-os agent readiness --member <member>`.
- When a shared connection enters rate-limit cooldown or reaches its concurrency cap, the pool selects the next ready candidate connection automatically.
- Office-local connections remain usable when a satellite is offline so local agents can continue turning without HQ quota round-trips.
