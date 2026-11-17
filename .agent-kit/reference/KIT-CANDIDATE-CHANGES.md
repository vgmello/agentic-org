# Agent-kit candidate changes (from Agentic Org process improvement)

Purpose: changes tried in this project that may belong in the starter kit. **Nothing here is applied to the kit yet.** A later, separate agent
applies an item only after its project evidence (PI decision) shows it worked. Each item names its source decision, the kit files it would touch,
and the evidence needed to promote it.

| ID | Candidate | Kit files likely touched | Source | Promote when |
| --- | --- | --- | --- | --- |
| K-01 | Decision template: **Invariants** section + **operation × outcome matrix** + **durable-format guards** (success/transient/definitive/timeout/abandoned/crash) for stateful or concurrent work; architecture review checks completeness | `templates/ARCHITECTURE_DECISION.md`, `architecture-decisions` skill, `plan-review` checklist | PI-01 changes 3-4 | H1 shows fewer fix-induced findings. Note: the template is kit-managed (`.agent-kit/.starter-kit-manifest.json`), so a kit upgrade must reconcile this project's local edit |
| K-02 | **Fix-the-class rule** for every fix turn (Sol and writers): state the invariant, check siblings, fix a sibling only with a test that fails first, report out-of-scope siblings, record invariant/siblings/tests (kit: a fix.json field) | `review-cycle` skill, `kit_codex.py` fix prompt + `fix.json` schema field, fullstack-engineer brief template | PI-01 change 5 | H1 |
| K-03 | A generic **stateful-protocol** skill pattern (shared-resource close rules, timing model, single-owner constants, fake conformance, refusal causes) that projects specialize | new kit reference skill + `project-capabilities` guidance | PI-01 changes 1, 7 | H1 |
| K-04 | **Final-review depth**: the final reviewer's turn budget scales with diff size (or one reviewer per bounded diff); a split review is an exception; the invariant table is the checklist | `code-reviewer` agent definition (`maxTurns`), `review-cycle` skill | PI-01 change 6 | H1 |
| K-05 | **Severity policy**: P0-P2 block; P3 blocks only for security/data-integrity/user-visible correctness/missing required test/contract mismatch; other P3 are deferred with an owner in FINAL_REVIEW.json | `kit_codex.py final` schema + acceptance rule, `templates/FINAL_REVIEW.json`, `review-cycle` skill | PI-01 change 9 | H2 with an independent assessment |
| K-06 | **SHA-bound gate runner** driven by `verification.json`; the slice contract names its required targets at registration (incl. both browser engines for UI/session work) | `kit_hooks.py` or a new `kit_gates.py`, `templates/SLICE_CONTRACT.md` | PI-01 change 10 | H1 |
| K-07 | **Premise check against durable formats** (frozen schemas, wire versioning) before a storage/wire decision | `architecture-decisions` skill | PI-01 change 4, WK-01 r3 | Observed twice (WK-01, U6 reaping); promote with H1 |
| K-08 | Writer sizing: receipt-first briefs (keep); slices sized to ~25 turns or pre-split; continuation briefs from the receipt | `task-routing` skill, delegation guidance | PI-01 change 13 | Observational; promote after 10 tasks |
| K-09 | Lead owns review; writers return unreviewed commits. A user's global "review before commit" rule can only be relaxed by that user; the kit documents the lead's cycle as the review and asks for the owner's confirmation | `review-cycle`, fullstack-engineer agent definition | PI-01 change 14 | Owner confirmation only |
| K-10 | Known kit limitations seen here: a review is BLOCKED when Sol cites absolute paths; the metrics `begin` is refused while a review holds the lock (needs `--registration late`); code-reviewer 16-turn cap | `kit_codex.py`, `kit_metrics.py`, agent definitions | Lead handoff notes | File as kit bug reports with reproductions; fix as bugs |
| K-11 | **Escalation rule**: after two final-review rounds with findings of the same class, the lead sweeps the class or reconciles the design before another loop | `review-cycle` skill | PI-01 change 8 | H1 |
| K-12 | **Severity calibration**: the same defect class gets the same severity across rounds and reviewers | `review-cycle`, review prompts | PI-01 change 9 | H2 |

**Promotion thresholds (H1):** promote an H1 item only if, across ≥3 accepted candidate slices, fix-induced + missed-sibling findings per review round fall
below the F4 baseline: (Ind + Sib) ÷ (Sol rounds + final rounds) = 18 ÷ 20 = 0.9 per round, with no guardrail breach (accepted tasks, escaped defects, repair regressions, human
intervention) in the 14-day window. Results are descriptive, not causal.
