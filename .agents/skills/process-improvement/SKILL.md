---
name: process-improvement
description: Measure complete tasks and evaluate one reviewed workflow improvement with quality guardrails and a rollback decision.
---

The lead owns this loop. No monitoring model, synthetic keepalive, external service
or automatic policy tuning. Keep journals/reports on disk, outside routine context.
Load this skill at task registration/closure or when investigating measured waste.

## Register before planning

For every substantive implementation task, save the user's request and agreed
acceptance criteria in a stable contract BEFORE architecture/planning. Keep the
evolving plan separate. Use the SAME contract as the Sol runner's `--task`.
Measure a whole delivery across all slices OR disjoint slices; never sum overlapping
parent/child records. Read-only lookups belong to their parent task's usage.

```bash
python3 .agent-kit/runtime/kit_metrics.py begin --task TASK --category bounded-fix \
  --contract .agent-kit/tasks/TASK-contract.md --environment MACHINE-TOOLCHAIN-LABEL
```

Save the returned journal path in task state. Use `--registration late` if work has
already begun; never backdate. Keep ALL retries/review restarts in this run. On
material scope change, close it as abandoned with the reason and register anew.

## Close and observe

Collect EVERY Sol run, including blocked/rejected ones, before task finalization:

```bash
python3 .agent-kit/runtime/kit_metrics.py collect --run TASK_JOURNAL --review SOL_RUN_FOLDER
```

Recollection replaces the component; it never adds cumulative snapshots. Copy
PILOT_RUN.json to an ignored observations file. Fill only observed WHOLE-task
values, including planner, workers, lookups, all reviews/repairs and human effort.
Reference private evidence with CLI/model/effort versions, cache-write/TTL counters,
index/retrieval overhead and subscription allowance observations when available.
Files are hashed, not copied into the journal. No transcripts or secrets are needed.
Main-session Claude `/usage` is not all-agent usage. Unknown stays null; Sol
components are subsets, never added to whole-task totals. Input includes cached
reads; cached_input_tokens is a subset. Preserve provider reports and any counter
conversion in evidence. Cost is observed actual USD or a documented API-equivalent
estimate with dated prices and ALL billable dimensions. Never convert subscription
allowance to dollars. Cost completeness is independent of token completeness.

```bash
python3 .agent-kit/runtime/kit_metrics.py finish --run TASK_JOURNAL \
  --outcome accepted --review SOL_RUN_FOLDER --observations OBSERVATIONS.json \
  --note "Intent, checks and fresh final output review accepted"
```

Accepted requires the existing current ACCEPTED state, same contract and branch.
Finish rejected/abandoned tasks too, without `--review`, explaining the reason.
Missing model access never permits false acceptance. Register-to-finish wall time
includes waits/checks/parallel work; it is not summed model time or active labor.

Later, `observe --run TASK_JOURNAL --observations UPDATED.json` appends a replacement
observation snapshot, preserving history. Carry forward still-valid measurements.
`quality_window_days` declares the fixed post-finish window for escaped-defect
counts; `observed_through` is the actual observation cutoff with timezone. Count
defects in that declared window, even if reporting later. Unknown is not zero;
compare equal mature windows. Never manufacture future observation cutoffs.

## Evaluate and improve

After ten completed tasks or an evidenced recurring failure, generate a scorecard:

```bash
python3 .agent-kit/runtime/kit_metrics.py report --directory .agent-kit/logs/task-metrics \
  --output .agent-kit/logs/efficiency-snapshot.json
```

Inspect categories, failures, accepted median/p90 latency, cost per accepted task,
missingness, repair regressions, author reversals and useful final-review findings.
Tokens/cache fraction/comment counts alone are not objectives. Small-sample p90 is
descriptive. Missing billing need not prevent an honest latency report.

Choose ONE hypothesis. Before either variant runs, fill EXPERIMENT.json: cases and
trial IDs, immutable contract SHA256s, exact treatments, invariant application paths
(including relevant tests/locks), objective, minimum pairs, quality window, budget
and rollback. Fingerprints separate application inputs from kit treatment files by
your explicit path selection. Review its completeness; hashes cannot prove semantic
equivalence or coverage of omitted dependencies. Both variants retain their declared review gates; a profile comparison records
the final-review model difference as part of the treatment. Stop if budget is absent; there is no scheduled paid benchmark.

Add `--experiment MANIFEST --case CASE --trial TRIAL --variant baseline|candidate`
to begin. Use isolated equivalent application checkouts, alternate/randomize order,
and record machine/toolchain plus warm/cold index/cache conditions. Failures/retries
stay in their task run. The manifest is frozen in each record; changes need a new
ID. Duplicate trials are refused locally and detected across exported worktrees.

Export each checkout before deletion, then combine via repeated `report --input
SNAPSHOT --experiment MANIFEST --output COMPARISON.json`. Include ALL registered
trials. Complete comparisons need every pair, equal application/environment
baselines, quality/human observations and equal mature quality windows. Cost
objectives need complete costs for ALL attempts, on the same USD basis. Fixed
guardrails: no lost baseline-accepted trial, no fewer accepted tasks and no increase in escaped defects, repair
regressions or human intervention. **Data complete** and **objective/guardrails
met** are separate results; neither proves statistical causality.
Latency objectives compare only jointly accepted pairs and require the predeclared
minimum of those pairs; per-variant latency summaries remain descriptive. Inherited
snapshot input errors remain visible and block a complete comparison on re-import.

Use IMPROVEMENT_DECISION.md for independent assessment and a lead-owned adopt,
reject or inconclusive decision. Bind it to the scorecard SHA256 and manifest.
Keep a redacted portable snapshot with the versioned decision before deleting
ignored worktrees. Snapshots include observations and evidence hashes, not the
underlying evidence files; preserve reviewed source evidence separately in an
appropriate private artifact store before cleanup. Review/check the actual policy change, commit it, and retain
rollback version/trigger/owner. Reuse normal change review, not an extra committee.
Fix correctness bugs on reproducible evidence immediately; require measurements
before claiming efficiency gains. No automatic policy promotion occurs.

For memory comparisons use `.agent-kit/templates/HINDSIGHT_PILOT.md`. Freeze the
same approved Git corpus for all paired trials; do not feed earlier answers into
later trials. Include `kit_memory.py usage` events and cold setup/CPU/storage in
whole-task measurements. Verified local memory operations use no paid LLM; returned
context still consumes the agent's allowance. Subscription dollar cost remains
unknown without actual billing evidence. Installation/health probes are not a pilot.
