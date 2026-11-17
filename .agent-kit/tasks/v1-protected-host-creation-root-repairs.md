# F3c root integration repairs

Stable acceptance contract: [v1-protected-host-creation-contract.md](v1-protected-host-creation-contract.md).
Apply [independent delivery boundaries](../../docs/independent-delivery.md).
This remains an internal native foundation; browser onboarding/sign-in/create/reopen is U1.

Root owns architecture, acceptance and integration and is the sole source writer.
The retained native author is terminal/idle, with all contexts preserved. Serial
repair is preferable because authentication, launch and admission share one composition.
The original 96-path author grant is unchanged. Root explicitly owns these repairs:

- `crates/org-host/src/service/mod.rs`: absent Authentication refuses with the existing NotAuthorized-before-params behavior; active authentication is unchanged.
- `apps/agentic-org/tests/host_control.rs`: extend the existing refusal regression to session methods and office.open_channel.
- `crates/org-host-control/src/tests.rs`: remove two impossible release fields from gateway test initializers; the resulting file matches the integration base.
- `apps/agentic-org-office/tests/launch/v3_activation.rs`: require office-channels/1 in two exact ReadyForSecrets expectations, preserving strict assertions under ADR-0012.
- `crates/org-office/src/store/tests/opener_build.rs`: admit only the exact approved cc and local org-release host build dependencies; confine the host caller and pin the local helper manifest/production Rust closure.
- `crates/org-office/src/store/tests/opener_sources.rs`: pass the confined manifest/scope to that inventory validation.
- `crates/org-office/src/store/tests/opener_build_pins.rs`: literal hashes for the unchanged local release-helper manifest and three production modules.
- `crates/org-office/src/store/tests/opener_build_tests.rs`: positive exact-edge coverage and negative metadata/source-confinement/helper-mutation coverage.

Root also owns FEATURES/FEATURE-AREAS and current task/assessment metadata.
These repairs introduce no new runtime dependency, public wire/schema, cryptographic
policy or architecture premise. They represent approved native dependencies in an
existing conservative build-source inventory; arbitrary build helpers remain refused.

Focused evidence: `.agent-kit/logs/v1-protected-host-creation/` contains
`root-auth-refusal-repair-1`, `root-channel-fixture-repair-1` (27 launch tests), and
`root-build-inventory-repair-1` (15 inventory tests plus the actual workspace source
fence). All completed successfully. Full current workspace/kit/app checks, fresh
whole-diff review, exact original-author validation and separate fresh final review
remain mandatory before merging this unit into v1-integration. Existing failures and
checkpoint 1ce54471 are retained; signing setup/authentic release verification is last.


Fresh initial review087e4e8a subsequently confirmed missing-writer-custody replay (P1) and partial marker publication (P2). The runner rejected absolute finding paths; its BLOCKED report is retained. Exact reviewer01a1e747-bc1a-7e33-ac24-63f2230bfb65 performed the same-session bounded repair as sole writer in writer.rs, writer/tests.rs and provisioning/tests.rs; original native author01a1dfb4 remains preserved for intent validation. Root verified the terminal handoff, all three hashes and1492 unchanged nonowned inputs. Original-code RED and current12PASS focused, two Clippy configurations and formatting evidence live in reviewer-writer-repair-1. Missing committed custody now refuses before allocating identity/device, and complete flushed markers publish exclusively without clobbering unsafe entries. Real write-failure/process-kill tests preserve original admission/checkpoint. No authority/schema/wire change; current full gates and fresh reviews remain.


Root also owns the bounded watchdog repair in `crates/org-host/src/service/authentication/tests.rs`: gates5 exposed an eight-second debug-suite socket timeout, while the unchanged compiled test passed in isolation. Both real sign-in behavioral tests now use a bounded30sec test watchdog; production hash strength/deadlines and every sign-in/renew/logout/EOF assertion remain intact. No latency acceptance is claimed, and no new ignore/serialization/production override is introduced. Evidence root-auth-timeout-intake-1; full current gates and reviews remain mandatory.
