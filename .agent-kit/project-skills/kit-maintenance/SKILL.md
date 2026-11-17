---
name: kit-maintenance
description: Maintain Agentic Org's Codex-only starter kit, checkout-bound services, capability evidence and adoption readiness.
---

Read reference/ADOPT.md and the affected installed project-capabilities, indexed-context, project-memory, setup-health or review-cycle skill. Preserve managed runtime/adapters unless the owner explicitly requests an integration change; record such local adaptations and upstream/current hashes in the manifest. Put other specialization in local role/skill briefs, maps, catalog and verification. Propose runtime bugs upstream rather than silently patching them.

Codex-only uses Sol, Repowise 0.61.0 and Hindsight 1.2.1, run as hindsight-embed with native hindsight-coding-agents wiring for Codex only. The daemon uses existing Codex OAuth for extraction in the native bank; the separate kit reviewed-record bank retains chunks without model calls. Native memory is opted into this original checkout only. Use the exact pinned install/restart commands in ADOPTION.md; preserve provider configuration on native hooks/MCP so daemon cold starts cannot select another assistant. Preserve native tools and logins. Prepare each checkout independently; never copy caches/receipts. Hooks require exact-definition trust; explicit checks remain mandatory.

Use real non-watching checks within 300 seconds; retain required full Rust and affected frontend/browser checks separately. Inspect command effects. Validate catalog, representative critical packets/preflight and disposable failure/recovery, freshness and metrics fixtures.

From the repository root run `python3 .agent-kit/runtime/kit_memory.py audit`, `python3 .agent-kit/runtime/kit_skills.py validate`, `python3 .agent-kit/runtime/kit_policy.py --assessment .agent-kit/tasks/ASSESSMENT.json` and `python3 .agent-kit/runtime/kit_doctor.py --project . --live --checks --json`. Record exact blockers and separate CONFIGURED, CHECKS_VERIFIED and LIVE_VALIDATED. Saved logins/probes do not establish model entitlement. Unknown cache/usage/billing remains unknown; no synthetic warming or savings claims.

Kit changes follow AGENTS.md and docs/processes/branch-and-merge.md like any other change: local review gates, then a merge into v1-integration. Reassess material stack changes before recapturing evidence.

If a never-approval review cannot call the kit MCP, preserve its read-only shell and approve only `starter_kit_context.tools.lookup` and `.recall` through per-tool `approval_mode = "approve"` in project .codex/config.toml, after verifying the server still exposes only those bounded retrieval tools. Keep these tables outside the generated context block; do not grant blanket server/tool or filesystem access. Official configuration reference: https://learn.chatgpt.com/docs/extend/mcp?surface=cli .
