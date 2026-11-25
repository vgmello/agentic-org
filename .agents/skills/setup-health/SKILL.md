---
name: setup-health
description: Validate an adopted project after installation, upgrade, restart or suspected tool failure; diagnose and repair setup without paid model calls.
---

Use the current agent session. Discover the project Git root, read effective project
instructions and its adoption report, and inspect the selected mode. Do not change
mode, accounts or project conventions to make checks pass.

Run the installed doctor from that root:

```bash
python3 .agents/agent-kit/runtime/kit_doctor.py --project . --live --json
```

This checks managed files, effective instructions, routing, skills, check commands,
current code and memory lookup through the actual MCP launcher, and Hindsight storage/semantic
recall in an isolated temporary bank that it deletes afterward. It checks the
selected CLI login statuses without opening credential files or invoking models.
It can refresh derived index caches and writes private evidence. Project knowledge
is not modified. A saved login does not prove entitlement, remaining subscription
allowance, client trust or a successful review cycle.

During adoption, and after relevant changes, inspect the configured commands and
run the project checks too. Commands can mutate files; follow project conventions:

```bash
python3 .agents/agent-kit/runtime/kit_doctor.py --project . --live --checks --json
```

Read every failed check and its evidence. Fix the cause within authorized setup:
missing dependencies, stale scoped skills, conflicting effective instructions,
stopped services or a renamed symbol. For memory recovery run `kit_memory.py setup`;
for a stale code probe, read a current symbol and rerun `kit_context.py setup` with
its actual source path. Preserve volumes, project configuration and custom work.
Do not weaken checks, remove required tools or fabricate results. Ask for login
only when the actual login/access blocker needs the user's intervention.

Rerun failed probes after repairs; rerun the full doctor before completion. Inspect
`.agents/agent-kit/logs/health/latest.json` and linked command receipts. Record the tested
revision/time, passed/failed checks and unresolved next steps in ADOPTION.md.
`health_passed`, `services_ready` and `checks_verified` are diagnostic evidence;
`LIVE_VALIDATED` still requires the actual selected-profile model/review cycle.
No paid pilot, continuous polling or recurring model calls are implied.

Fresh review sessions do not run installation/repair probes. They report tool
failures to the lead and continue only within their review contract.
