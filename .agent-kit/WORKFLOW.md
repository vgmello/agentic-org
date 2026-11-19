# Shared engineering workflow

- Read the active `.agent-kit/policy/model-routing.json` and relevant project
  standards/ownership. Hybrid uses Claude planning/workers, Sol review and Opus
  final review. Codex-only uses Sol with the profile's effort levels throughout.
  Never choose a mode implicitly from available credentials or skip a required gate.
- The lead uses task-routing before implementation: register substantive work,
  plan once, assess evidence, assign models/efforts and scoped end-to-end slices.
  Small work stays lightweight. Helpers only extract facts; they never implement.
- Use project-capabilities during adoption/maintenance to prepare the actual project's
  role briefs and skill catalog. Use skill-routing before work: select only affected
  guidance, load it and record preflight. Missing/stale knowledge blocks dispatch.
  Full-stack owners retain integration responsibility when delegating independent
  implementation pieces. Critical-area guidance accompanies existing review passes.
- Large/complex architecture needs independent review before detailed planning.
  Use architecture-decisions to preserve premises and rationale. Failed premises
  pause affected/dependent work; review material replacements before resuming.
- One owner per checkout/shared contract. Default one worker, at most two independent
  writers in separate worktrees. Keep original author contexts for intent validation.
- Use Repowise 0.61.0 for index-first discovery, bounded source reads and the project's
  compiler/type checks. Hindsight 1.2.1 supplies semantic recall of reviewed Git
  records with embedded pg0 and local CPU models; no paid memory LLM or RTK.
- Use project-memory for scoped source-backed knowledge. Keep DECISIONS.md as a
  compact index, PROJECT_MAP.md as a revalidated hint and one current task record.
  Commit approved records, then sync with kit_memory.py. Fresh reviewers use
  governing decisions/maps only, excluding earlier review learnings.
  Load only needed skills/records; capture intent, constraints and next steps before
  compaction. A checkpoint hook cannot recover reasoning that was never written.
- Configure `.agent-kit/verification.json` with real commands. Empty checks fail.
  Run `python3 .agent-kit/runtime/kit_hooks.py check` explicitly for direct lead work
  and final acceptance. Checks that declare `paths` run only when a file changed since
  the base matches them (the configured `base`, else an upstream on another branch;
  `--base REF` to choose, `--all` for everything); checks without paths always run, a
  changed configuration or no base at all runs every check, and an unknown base is an
  error. Review runs scope to their review base, the worker stop hooks to the default
  base. Non-blocking warnings appear in the check output, `evidence` and review runs. Hooks only
  provide supported/trusted runtime feedback.
- Use review-cycle: fresh read-only Sol review, exact-session repairs and verified
  commits, fresh recheck after fixes, original-author validation, separate fresh
  final code/output review. Three rounds default, five maximum, stop on repeated
  findings. Pause other writers. No automatic push/merge; no PR is required.
- ACCEPTED needs current source/check evidence and the active profile's review
  gates. Role instructions and local identity records are not authenticated proofs.
  Application CI/merge rules and relevant integration/UI checks still apply.
- Use process-improvement to retain every task outcome and measured quality/usage.
  Inspect ten-task batches or recurring failures, evaluate one controlled hypothesis
  and record an independently assessed adopt/reject/rollback decision. No automatic
  model spending or policy tuning is introduced.
- Use setup-health after adoption, upgrades or tool failures: live doctor probes
  lookup, memory and login without model calls; --checks runs project verification.
- Setup follows `.agent-kit/reference/ADOPT.md`. CONFIGURED, CHECKS_VERIFIED and
  LIVE_VALIDATED are separate claims. Native Codex hooks need project/hook trust;
  hosted/cloud orchestrators may not support local hooks. Explicit checks and review
  wrappers remain mandatory. Missing model access is a blocker, not acceptance.
