---
name: review-cycle
description: Run bounded fresh Sol review and same-session repairs, original-author validation and a profile-bound fresh final code/output review.
---

Keep the ORIGINAL author session alive. If a worker wrote the slice, resume that
exact worker for intent validation; the lead also validates integration. A summary
or invented session ID cannot restore lost intent. Recover missing context explicitly.
Fresh review and repair sessions use `kit_memory.py recall --phase review` for
governing decision/map records only; do not read earlier review learnings as evidence.
The Codex wrapper sets this phase through its environment. Apply it explicitly for
manual Claude final reviews as well. Shared-memory promotion/sync belongs to the lead.

1. Use the stable request/acceptance contract registered in the task journal.
   Keep evolving plans separate. Record original author session/model, agreed base
   and authorized scope. Pause all other checkout writers. Run checks and commit
   the scoped implementation checkpoint without unrelated dirty work. Save the
   evidence-backed assessment and routing from task-routing.
   Ensure project-capabilities is configured/current and record semantic risk tags
   from skill-routing. The runner resolves project role/skill references for review
   and repair; a critical-area match requires a conservative risk assessment.
2. Start the installed shared runner, using actual values and repeating --scope
   and, when applicable, --risk for the catalog's semantic risk tags:

   ```bash
   python3 .agents/agent-kit/runtime/kit_codex.py start --base BASE_SHA \
     --task .agents/agent-kit/tasks/TASK-contract.md \
     --assessment .agents/agent-kit/tasks/TASK-assessment.json --scope src --scope tests \
     --origin-session ORIGINAL_SESSION_ID --origin-model OBSERVED_MODEL_ID
   ```

   Sol High is the default; Xhigh is an explicit escalation for hard/missed issues.
   The runner records the profile and final model/effort. Three rounds by default,
   five maximum; stop when clean or the same findings recur. Fresh read-only review,
   exact-session writable fixes, checks and repair commits, then another fresh review.
   Never use --last, --ephemeral or fork implementation history. No recursive
   delegation, push/merge, scope expansion or verification weakening in these turns.
   Failed checks, auth, malformed output, timeout or exhausted rounds remain BLOCKED.
3. Read HANDOFF.md and EVERY finding, fix, diff, before/after SHA and receipt.
   Validate against original intent and current code. Resolve disputes with evidence;
   do not undo a correct fix merely because it differs. Save a substantive note:

   ```bash
   python3 .agents/agent-kit/runtime/kit_codex.py origin --run RUN_FOLDER \
     --session ORIGINAL_SESSION_ID --decision accept --notes RUN_FOLDER/intent.md
   ```

   Reject when necessary. Further code changes require fresh checks/checkpoint and
   a new bounded run, within the same task budget/journal. AWAITING_FINAL is not accepted.
4. In codex-only mode, run the fresh read-only final Sol pass:

   ```bash
   python3 .agents/agent-kit/runtime/kit_codex.py final-review --run RUN_FOLDER
   ```

   It uses High, or Xhigh when the assessment requires it, and covers actual outputs
   plus the complete change. In hybrid mode, invoke a NEW Claude code-reviewer with
   the recorded Opus Medium/High effort. Supply the contract, active decisions,
   review-skills.json and its required role/skill source references,
   complete diff, current receipt, output/UI evidence and references to Sol artifacts
   and origin validation. Ask it to inspect source/outputs before prior conclusions.
   Record the actual result using FINAL_REVIEW.json and:

   ```bash
   python3 .agents/agent-kit/runtime/kit_codex.py final --run RUN_FOLDER --result RESULT.json
   ```

   Model identity fields are local declarations/request bindings, not authenticated
   provider attestations. Never substitute an unavailable model or fabricate a result.
5. Run `kit_codex.py status --run RUN_FOLDER`. ACCEPTED requires current source and
   checks, matching profile/policy, original author acceptance and a fresh clean final
   review. Issues return CHANGES_REQUIRED; failures remain BLOCKED. Diagnose them
   with the lead, not endless automatic restarts. Both profiles require this gate.
6. Use process-improvement to collect EVERY run (failures included), record whole-task
   observations and finish the journal with its actual outcome. Wrapped Codex final
   usage is included when present; external author/worker/Claude usage still needs
   separate evidence. Missing usage is unknown. Export evidence before cleanup.
