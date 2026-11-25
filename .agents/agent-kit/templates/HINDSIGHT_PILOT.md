# Deferred application memory pilot

Do not run model trials during installation. Prepare this procedure now and obtain
the actual application task/budget when the user schedules the pilot.

1. Record application baseline SHA, kit SHA, profile, actual models/efforts and
   billing mode. Run setup-health with project checks. Snapshot the committed
   `.agents/agent-kit/knowledge/` corpus: record its Git tree SHA and accepted record IDs.
   Use the same baseline in isolated trial worktrees; run each checkout's context
   and memory setup. Never copy setup receipts or index caches.
2. Choose at least three representative stable task/acceptance contracts before
   running trials. Pair comparable work and alternate ordering; acknowledge that
   repeated tasks and caches are confounders. Record a single hypothesis and
   measurable quality/latency thresholds using the existing EXPERIMENT.json and
   process-improvement workflow. Do not invent subscription dollar/token prices.
3. Baseline sessions set `AGENT_KIT_MEMORY_BACKEND=local`. Candidate sessions use
   the default Hindsight backend. Keep models, effort, tools, review gates, budgets
   and baseline source/corpus identical. Both may read only the frozen approved
   corpus; do not promote/sync trial learnings until every paired trial completes.
   Verify the corpus tree SHA before/after each trial and reject contaminated pairs.
   Hindsight filters by each checkout's committed records; prior trial answers
   must never be committed into the next trial baseline.
4. Register every task before planning with kit_metrics.py. Finish every outcome,
   including failures. Collect model/cache usage when actually available, memory
   usage events, task duration, repair/review rounds, checks and output quality.
   Record cold setup/indexing separately and include amortized CPU/storage costs.
   Missing subscription usage is unknown; use accepted-task latency and quality
   when complete dollar cost cannot be observed.
5. Keep final reviewers blind to experiment arm and earlier conclusions. Retain
   source-bound acceptance evidence and watch for escaped defects. Compare all
   attempted work, not only successful recalls. A small pilot is preliminary;
   report uncertainty and inspect subsequent ten-task batches before broad claims.
6. Have the lead assess adopt/reject/rollback against the recorded thresholds.
   A faster recall with worse accepted-task quality fails. Revert the tested policy
   if warranted, preserving Git knowledge and evidence. No automatic policy tuning,
   subscription spending or memory pruning is authorized by this template.
