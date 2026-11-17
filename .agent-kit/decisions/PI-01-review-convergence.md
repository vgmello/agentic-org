# PI-01: review convergence (why fixes keep producing the next finding)

- Owner/date/status: Claude lead, 2026-11-17. **PROPOSED, not effective.** Drafted on branch `process/review-convergence`.
  **Effective date:** the skill/catalog/AGENTS/template changes land in `v1-integration` only **after F4 and U6-H are accepted and merged**.
  Each open review run pins the skill-catalog SHA, so landing earlier would mark the F4 and U6-H runs stale and blocked, and would mix the
  treatment into the F4 baseline.
- Scope: how Codex (Sol) review/fix rounds, the fresh final review, slice gates and design records interact. Development is paused at the owner's request
  (F4 and U6-H finish; nothing new starts) so the process can be improved first.
- Independent assessment of draft 1: claude-opus-5-5 code-reviewer, CHANGES_REQUIRED (6 blocking + corrections); this revision resolves them (§7).

## 1. Evidence (F4 gateway browser sessions, base 67860707)

Source: `.agent-kit/logs/v1-f4-sessions/` (ignored; findings copied into Appendix A so the decision stands alone).

- Sol review/fix: **11 runs, 15 Sol rounds**. 6 ended BLOCKED: 2 of those reached the 3-round limit (1528d51a, 4365cd91), and 1 (e0ae0f9d) was an incomplete
  review with no findings because browser evidence was not tied to the commit. 5 were clean at round 1.
  - Sol findings: **12** (2 P1, 9 P2, 1 P3).
- Fresh final review: **5 rounds** after Sol was clean. Findings: **29** (3 P2, 26 P3).
  - The gate requires zero findings, so each round restarted fix → full gates → Sol → origin → final.
- **Total: 41 findings.**
- **Fix-induced** (caused by an earlier fix): **9**. **Missed sibling** (a prior fix covered one path and left the same defect in a sibling): **9**.
  The exact rows are in Appendix A.
- Severity was graded inconsistently: final-1 A2 (P3) and final-4 A1 (P2) are the same sign-out defect class.
- **Sol depth:** Sol reported 5 clean runs, and the final review still found 3 P2s after them.

## 2. Root-cause classes (Appendix A assigns every finding)

| Class | Count | Pattern |
| --- | ---: | --- |
| **A. Lifecycle exit paths not enumerated** | 12 | Each fix covered the cited operation × outcome. The symmetric operation (sign-in/refresh/sign-out) or outcome (success/transient/refusal/timeout/abandoned/crash) appeared in the next round. |
| **B. Timing on a shared resource** | 9 | No written timing model. 7 of the 9 are Ind (caused by an earlier fix; Appendix A). |
| **C. Cross-component contract drift** | 5 | One rule implemented twice (budgets, frame limit, UI fake vs gateway cookies). |
| **D. Refusal causes and logging** | 5 | The same defect at several call sites, found one round at a time. |
| **E. Evidence and doc hygiene** | 8 | Gates were completed mid-slice; docs drifted (FEATURES totals, F3c state, help text). |
| **F. Production-path wiring and platform** | 2 | Both P1s: Linux release-loader gating; the serve entry point. Already governed by docs/independent-delivery.md, but missed. |

Process causes:

1. Fixers fix the instance, not the class. Neither Sol fix turns nor writer briefs asked for a sibling check.
2. Invariants lived in reviewers' heads. GW-01 described mechanisms, not invariants, so each fresh reviewer found a different subset.
3. The final review was shallow and varied: a 16-turn cap meant a 3-way split each round, with new eyes and different depth every time.
   Sol's depth also missed classes A and B.
4. A P3 cost the same loop as a P2, and severity was not graded consistently.
5. Gates were completed during the slice instead of at registration.
6. Premises were not checked against durable formats before deciding. This was seen outside F4: WK-01 D5 assumed it could add projection columns;
   U6 assumed reaping could be verified.
7. There was no escalation rule after repeated rounds.

## 3. Changes

### 3.1 Teach implementers and Codex (classes A-D, F)

1. **Project skill `stateful-protocol-changes`** (`.agent-kit/project-skills/stateful-protocol-changes/SKILL.md`). The catalog entry selects by **path only**
   (`risks: []`): web, host gateway/service/supervisor/lib.rs, host-control, worker `serve.rs` and `serve/`, org-supervise, UI fakes, plus the U6 paths (org-office-sessions,
   org-office execution and hire), which exist on the U6 branch. Delivery is confirmed: `kit_codex` resolves review and implementation packets from
   the scopes plus the changed paths, and the review and fix prompts make Sol read them. The hybrid final review receives them through the lead.
2. **AGENTS.md short rule** (Codex reads it natively), aligned with the `kit_codex` fix contract:
   - fix a sibling only when it shows the same defect, proven by a test that fails first;
   - name out-of-scope siblings in the fix summary (`unaddressed_ids` is for review finding IDs only);
   - record "invariant / siblings / tests" in the fix.json `summary`;
   - if the decision has no Invariants table, derive and state the invariant (its absence alone never makes a review incomplete).
3. **Decision template**: sections for Invariants, the operation × outcome matrix, shared limits/fakes, and **durable-format guards** (which covers change 4).
4. **Premise check against durable formats**: part of change 3, and in H1.

### 3.2 Make review converge

5. **Fix-turn discipline**, delivered through changes 1 and 2 within the runner's existing schema (`summary` field). A kit-level fix.json field is candidate K-02.
6. **Final-review depth**: one deep reviewer with the invariant table and matrix as its checklist. Raise the final reviewer's turn budget, or use one
   reviewer per bounded diff. A split review becomes the exception.
7. **Sol review depth**: the review packet carries the stateful skill (change 1). The Sol review prompt asks for the operation × outcome matrix sweep on
   lifecycle code (local: the skill text; kit: K-02/K-03).
8. **Escalation rule**: after **two** final-review rounds with findings of the same class, the lead stops the loop. The lead then sweeps the whole class
   (or reconciles the design and invariants) in one change, before the next Sol or final round.
9. **Severity policy (H2, separate)**:
   - P0-P2 block.
   - P3 blocks if it is about security, data integrity or user-visible correctness, is a missing required test, or is a contract mismatch.
   - Any other P3 is deferred with an owner in FINAL_REVIEW.json.
   - Includes **class-consistent severity calibration**: the same defect class gets the same severity.
   - Needs an independent assessment and an IMPROVEMENT_DECISION before use.
   - It does **not** claim F4 would have stopped earlier: later P2s (final-4 A1, final-5 A1) were real.

### 3.3 Gates complete from the start (classes E, F)

10. **Tracked gate runner**: the per-run `gates-N.py` becomes one reusable, SHA-bound runner. The slice contract names its required targets at registration:
    both browser engines for touched UI/session specs, dependency fences, native tests, production-path wiring checks (class F), kit.
11. **Doc consistency test**: FEATURES totals equal the rows and FEATURE-AREAS.
12. **Help/doc truth item** in the slice checklist.

### 3.4 Delegation hygiene

13. **Writer sizing**: at least 7 writer sessions in the later part of this delivery hit the 40-turn cap (U4-B3, U6-D1b, U6-A4, U6-H and its
    continuation, F4 final-3 and final-5 repairs); earlier sessions were not counted.
    Receipt-first briefs made each continuation reliable. Size slices to ~25 turns, or pre-plan two sessions.
14. **Writers return unreviewed commits**; the lead's review cycle is the review. This needs the **owner's** confirmation, because the user's global
    "review before commit" rule cannot be overridden by the kit or the lead.

## 4. Evaluation

- **H1 (one bundled hypothesis, observational):** changes 1-8 and 10 together reduce review loops without lowering quality.
  - A bundle cannot attribute the effect to a single change. With one historical baseline and no matched pairs, the result is **descriptive, not causal**.
  - Baseline: F4, from Appendix A — 11 Sol runs, 5 final rounds, 41 findings, 9 fix-induced, 9 missed siblings.
    F4's kit_metrics journal is cd0f7eb5; earlier journal f3e39563 was abandoned, with an observation recorded.
  - Candidates: the next 3 stateful slices (U1-A2 web/host, U4 web, U6-D wiring), each registered with kit_metrics before planning.
  - H1 is a before/after observation, NOT the paired `--experiment` baseline/candidate comparison. Its EXPERIMENT.json (filled before the first candidate starts) records:
    - objective: the combined Ind + Sib rate below 0.9 per review round, with the guardrails below;
    - exact treatment file SHA256s: the skill, the catalog, AGENTS.md and the template, at the landing commit;
    - candidate contract SHAs;
    - minimum 3 accepted candidates;
    - quality window 14 days after merge;
    - budget: no paid benchmarks, normal delivery only.
  - Measures:
    - Sol runs and final rounds per accepted slice;
    - fix-induced and missed-sibling findings (classified as in Appendix A);
    - P2+ findings at each final round;
    - escaped defects in the window.
  - Fixed guardrails: no fewer accepted tasks; no increase in escaped defects, repair regressions or human intervention.
  - Rate unit: (Ind + Sib findings) ÷ review rounds, where review rounds = Sol rounds + final-review rounds. F4 baseline: 18 ÷ (15 + 5) = **0.9 per round**.
  - Rollback trigger: any guardrail breached, or the candidates' combined rate is not below 0.9 per round.
    Owner: the lead. Revert the skill, the catalog entry and evidence, the AGENTS rule and the template sections in one commit.
  - Record H1 in IMPROVEMENT_DECISION form (adopt/reject/inconclusive) after the window.
- **H2:** change 9, only after H1 concludes.

## 5. Status of each change

| # | Change | State |
| --- | --- | --- |
| 1-3 | Skill + catalog entry, AGENTS rule, template sections | Drafted on `process/review-convergence`; land after F4 + U6-H are accepted |
| 5-8, 10-12 | Fix discipline, final/Sol depth, escalation, gate runner, doc test, truth item | Proposed (H1 bundle) |
| 9 | Severity policy | Proposed; H2 with an independent assessment |
| 13-14 | Writer sizing, writers' commits | 13 adopted as practice; 14 needs the owner's confirmation |

Kit candidates: `.agent-kit/reference/KIT-CANDIDATE-CHANGES.md`.

Landing hygiene: after rebasing onto `v1-integration` (once F4 and U6-H merge), recapture the catalog evidence hashes for AGENTS.md, Cargo.toml, Cargo.lock and the other
changed evidence files; the template is kit-managed, so a kit upgrade reconciles it (K-01).

## 6. Appendix A: F4 findings, one row each

Tags: Ind(<fix>) = this finding was caused by that earlier fix; Sib(<fix>) = that earlier fix left the same defect in this sibling path.

| # | Source | Sev | Finding | Class | Tag |
| ---: | --- | --- | --- | --- | --- |
| 1 | Sol a40d361d r1 | P1 | Release-loader Linux gating | F | |
| 2 | Sol a40d361d r1 | P2 | Gateway connection vs host per-connection session limit | C | |
| 3 | Sol 6e6241e4 r1 | P2 | Host session cleanup blocked the async path | B | Ind(lead reap fix 00c93089) |
| 4 | Sol 6e6241e4 r1 | P2 | Running published before HTTP init | A | |
| 5 | Sol cfde0832 r1 | P1 | Gateway startup not on the contracted serve path | F | |
| 6 | Sol 1528d51a r1 | P2 | Bindings not evicted after host denial | A | |
| 7 | Sol 1528d51a r2 | P2 | Revoked bindings not reclaimed at capacity | A | Sib(e19de943) |
| 8 | Sol 1528d51a r2 | P3 | Help text promised the serve launcher | E | |
| 9 | Sol 1528d51a r3 | P2 | New host session not ended when prep fails | A | Sib(b7eb2155) |
| 10 | Sol 4365cd91 r1 | P2 | Expiry checked after claim | B | Ind(57ada669) |
| 11 | Sol 4365cd91 r2 | P2 | Capacity scan lost progress | B | Ind(163bbcef) |
| 12 | Sol 4365cd91 r3 | P2 | Frame limit differs from the host's | C | |
| 13 | final-1 A1 | P2 | Shared scan deadline closes control | B | Ind(b7eb2155) |
| 14 | final-1 A2 | P3 | Sign-out removes only the presented key | A | (same class as #33, graded P3) |
| 15 | final-1 A3 | P3 | Unrelated cookies deny the session | C | |
| 16 | final-1 B1 | P3 | Browser sessions can exhaust the global cap | C | |
| 17 | final-1 C2 | P3 | Dependency exclusion only manual | E | |
| 18 | final-2 A1 | P3 | Queue wait counts toward the fatal deadline | B | (pre-existing, exposed by 163bbcef) |
| 19 | final-2 A2 | P3 | Capacity sign-in exceeds the backend timeout, leaving an orphan binding | A | Ind(163bbcef) |
| 20 | final-2 B1 | P3 | Restart count never resets | A | |
| 21 | final-2 C1 | P3 | Browser evidence not tied to the reviewed commit | E | |
| 22 | final-2 C2 | P3 | Refusal message names the office server | D | |
| 23 | final-3 A1 | P3 | Abandoned sign-in binds after the check | A | Ind(a49e1cc2) |
| 24 | final-3 A2 | P3 | Near-deadline job written, then closes | B | Ind(57ada669) |
| 25 | final-3 A3 | P3 | Timeout closes even though the reply was delivered | B | Ind(57ada669) |
| 26 | final-3 A4 | P3 | Reply id mismatch not treated as desync | B | |
| 27 | final-3 A5 | P3 | Stale comment | E | |
| 28 | final-3 B1 | P3 | Refusal logged with the wrong cause | D | Sib(f0f59acf) |
| 29 | final-3 B2 | P3 | Restart-reset test gap | E | |
| 30 | final-3 C1 | P3 | FEATURES totals stale | E | |
| 31 | final-3 C2 | P3 | UI fake refuses nameless cookies | C | Sib(2001c033) |
| 32 | final-3 C3 | P3 | Dependency evidence not tied to HEAD | E | |
| 33 | final-4 A1 | P2 | Sign-out keeps the pair on a transient end failure | A | Sib(0d7f7e7d) |
| 34 | final-4 A2 | P3 | 50 ms reply grace closes a healthy connection | B | Ind(5851ecae) |
| 35 | final-4 A3 | P3 | Reply built after commit | A | |
| 36 | final-4 B1 | P3 | Admission causes inside start discarded | D | Sib(9f9d5782) |
| 37 | final-5 A1 | P2 | Undelivered refresh not rolled back | A | Sib(5851ecae) |
| 38 | final-5 A2 | P3 | Sign-out refusals return no cleared cookie | A | Sib(16760bb9) |
| 39 | final-5 B1 | P3 | Re-admission cause lost | D | Sib(98415839) |
| 40 | final-5 B2 | P3 | Pre-spawn refusal not escaped | D | |
| 41 | final-5 C1 | P3 | F3c still called unaccepted | E | |

Totals: A 12, B 9, C 5, D 5, E 8, F 2 = 41. Ind 9 (#3, 10, 11, 13, 19, 23, 24, 25, 34). Sib 9 (#7, 9, 28, 31, 33, 36, 37, 38, 39).
The lead's classification is a judgment; the H1 candidates are classified the same way.

## 7. Resolution of the independent assessment of draft 1

| Finding | Resolution |
| --- | --- |
| 1. Counts did not match the evidence; P1s unclassified | §1 corrected; Appendix A added; class F added |
| 2. The H2 claim was false | Removed; severity calibration added to change 9 |
| 3. Landing the catalog change blocks in-flight runs | Effective date set after F4 + U6-H are accepted |
| 4. Conflict with the `kit_codex` fix contract | AGENTS rule and skill aligned (test-proven siblings only; `summary` field; the runner owns gates) |
| 5. Missing tables would make reviews incomplete | "If present; otherwise derive it; absence alone is not incomplete" |
| 6. Catalog paths and risks text | lib.rs added; U6 paths noted as branch-only; path-only selection stated |
| Re-assessment (P2: unaddressed_ids, serve.rs coverage, the rate unit; P3: EXPERIMENT fields, Ind wording, the e0ae0f9d note, landing hygiene) | All applied; Ind/Sib commit SHAs verified to exist by the lead. Third check: READY (one wording nit applied) |
| Other corrections | Skill text on constant ownership and refusal causes; change 4 folded into the H1 template; H1 completed (observational, EXPERIMENT outline, guardrails, rollback owner/trigger, IMPROVEMENT_DECISION); Sol depth (change 7) and escalation (change 8) added; K-09 and K-10 reworded |
