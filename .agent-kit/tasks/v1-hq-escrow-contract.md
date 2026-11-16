# Protected HQ state and recovery escrow — F3 acceptance contract

Owner: Codex lead; one architecture/integration owner. Base: `eacb505d2398fc473d43f45053522e39269434b5` (`origin/v1-integration`).

Deliver the protected HQ creation/state, key custody, escrow and interrupted-operation recovery required by the existing plan17 F3 and canonical ADRs0007/0011, reusing the original fullstack author's preserved b77cbd6876d72f76cc3b7f1d5fa3c3d96f424c09 implementation. This is an internal prerequisite for U1 onboarding/sign-in and organization create/reopen, not a usable product feature. Preserve accepted F2b durable journal/native replay behavior and interfaces.

Acceptance:
- Confirm governing organization-data/security/ADR authority before implementation. Agree exact source/registration/manifest ownership and material interfaces before dependent code. Preserve all prior source/tests, original authors, branches, worktrees and active workers.
- Protected fresh-owner reservation, HQ creation and retry preserve exact keys, identities and bindings. Never authorize existing-HQ first-owner access without its required proof.
- Persist escrow, protected writer/recovery state and delivery acknowledgement through interruptions/reopen without secret disclosure, identity replacement or silent durability downgrade. Preserve failure ordering, writer fencing and protected filesystem/key custody boundaries.
- Preserve REC-1, physical authentication and production admission requirements. No unsigned production bypass or developer fixture presented as normal production setup. Genuine release-signing setup/authentic signed-release verification remain last.
- Exercise meaningful successful/refusal/exact-retry/interruption/crash/reopen and secret-exclusion tests in temporary directories. Reuse existing independent evidence, rerun current affected checks and complete required workspace/kit checks.
- Preserve complete production readiness, original-author intent validation, fresh independent initial review with bounded same-session repairs and separate fresh final review for the exact current commit. Merge accepted increment --no-ff into v1-integration and push; no release-v1/main merge or external Ada/developer wait.
- Keep architecture/acceptance/integration with the lead and at most two nonoverlapping source writers in separate worktrees. Evaluate useful parallel work; no second writer against unresolved shared bootstrap/custody boundaries.

This stable contract records requested behavior; source feasibility, exact closure and evolving implementation steps belong in separate receipts/plan. Missing prerequisites return to the lead. An independently acceptable foundation can become a separate delivery unit only with its own honest acceptance record; full U1/v1 scope stays intact.
