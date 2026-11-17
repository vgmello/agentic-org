# Active constraints and decision index

The current solo Codex workflow in AGENTS.md governs implementation and integration. Canonical product architecture and data contracts remain authoritative; ADRs stay in docs/adr/.

- Composition: docs/architecture-rules.md and docs/design/project-structure.md; binaries compose libraries, policies own required contracts, no source cycles. Domain moves remain incremental.
- Authority: docs/design/documentation-authority.md and organization-data/README.md own selected target relationships. Draft SQL/acceptance criteria are not shipped migrations or services.
- Storage/key boundaries: ADR-0002 and ADR-0005 retain one office writer and host key custody. Gateway/agent identities do not receive raw office keys. Read primary authorization/acknowledgement/recovery contracts for affected work.
- Coordination: nothing from the earlier coordination tool or its message queue is preserved, imported or cut over (owner, 2026-11-17). The goal is to onboard the people and agents building the product onto the product itself as early as it can carry them. Do not contact offline developers.
- Solo ownership (owner instruction, 2026-11-15): Codex owns all repository areas and uses local kit subagents as needed. Named-owner boundaries, external reviewer waits and Ada agreement/merge approval are superseded. Keep local independent reviews, original-author validation, final review and current checks.
- Owner's integration override (2026-11-15): review completed features locally and then merge into `v1-integration`, the shared integration branch for all v1 features. Do not wait for a PR. The later solo instruction supersedes former area ownership; no release-v1 merge is authorized.
- Owner's blocked-work direction: move to another feature when blocked. Preserve the blocked branch/checkpoint, then start independent work from the integration branch; do not stop all delivery on one unresolved prerequisite.
- UI licensing: Tailwind Plus stays inside apps/agentic-org-ui, never a published kit or crate.
- Adoption: user explicitly selected codex-only on 2026-11-15. Kit 0.10.0 at 01e0d2f8ec338cf84dcbf15fe158ee5181fbd0b1 supplies Sol routing, Repowise 0.61.0 and Hindsight 1.2.1. Separate readiness claims/evidence live in ADOPTION.md.

Product architecture remains governed by the primary documents; task-specific active decisions below select scoped implementation mechanisms. Read applicable full primary documents before dependent work. Preserve replaced decisions/rationale in canonical ADR history.

Task-specific implementation premises: [TH-01](decisions/TH-01-task-history-validation.md)
tracks selected task-history validation and its independent coverage prerequisite;
it does not replace the product decisions in documents 05/12.

[TA-01](decisions/TA-01-explicit-task-assignment.md) preserves the selected complete
assignment-pair domain boundary and its separate service-acceptance obligations.

[ADR-0006 / TAI-01](../docs/adr/0006-selected-task-acceptance.md) revision 5 is
**active for implementation architecture**, after fresh independent review and a focused READY recheck. It covers selected
assignment’s authenticated vertical path and focused private web transport edges, codec 2, snapshot preservation,
recoverable acceptance and UI (adoption is withdrawn); it does not claim implemented support.

[ADR-0007 / REC-ESC-1](../docs/adr/0007-hq-creation-escrow.md) revision 1 is **active for implementation architecture**, following independent READY review and a bounded primitive probe. It fills the missing HQ-creation escrow format prerequisite for ADR-0006 P6/P9: fixed Argon2id/AEAD envelope, high-entropy code, worker-local CA key, protected delivery and idempotent provisioning. It preserves ADR-0006 and REC-1; production setup/recovery acceptance remains unverified.

[ADR-0008 / GW-SBX-MAC-1](../docs/adr/0008-gateway-macos-sandbox.md) revision 1 is **active for macOS adapter implementation**, following fresh independent READY and actual child enforcement/thread/host probes. Fixed parameterized Seatbelt deny-default/exact-rendezvous/deny-bind policy stays in libc-only org-socket; SDK deprecation fails closed and requires rechecks. Linux and full production handoff/browser/code acceptance remain unverified; no acceptance waiver.

[ADR-0009 / RLS-1](../docs/adr/0009-complete-release-verification.md) revision2 is **active for implementation architecture**, after fresh independent READY and exact-version signature/archive plus generated macOS loaded-image probes. It selects build-pinned exact-byte signed complete directory/tar.gz bundles, publisher-bound runtime images/authenticated static assets, host-owned durable staging/pins and a narrow shared org-release verifier. Revision2 preserves the gateway64MiB/4096-entry UI budget including inferred directories and source-backed kernel-loaded image binding; one focused architecture finding was corrected and independently rechecked READY. Production key provisioning and full setup/adoption/browser/code acceptance remain unverified.

- [ADR-0010](../docs/adr/0010-legacy-task-adoption.md): **withdrawn by the owner 2026-11-17.** Nothing has shipped, so there is no legacy work to adopt; no adoption, staging or import is built.

- [ADR-0011 / REC-DEL-1 r1](../docs/adr/0011-protected-recovery-delivery.md): independently READY protected native OS-presence delivery, same-run ACK, durable audit/deletion/reopen and bounded committed public escrow transfer/export. Exact cc1.11.1 SDK bridge, no crypto/key boundary change. Actual physical-auth/production implementation and full feature acceptance remain unverified.

- [ADR-0012 / CH-LIFE-1 r2](../docs/adr/0012-office-channel-lifecycle.md): independently READY bounded8physical/7gateway channel profile, authenticated ownership/refusal/closure and shared permission/receipt liveness. Preserves16pending/512combined listener and64session bounds, exact ACK/revocation/custody. Implementation and production/code/final acceptance remain pending.

- Owner release-phase override (2026-11-16): [release signing deferral](tasks/v1-release-signing-deferral.md). Implement everything else first; signing setup and authentic signed-release verification are final release work. Preserve failclosed signing code and full functional/local review gates; missing signing inputs do not block ordinary implementation/integration.

- Owner delivery-boundary override (2026-11-16): [separate acceptance/merge units](tasks/v1-incremental-delivery.md), plan17. Preserve the oversized assignment branch and active repairs, then reuse bounded source into dependency-driven accepted units from `origin/v1-integration`. First usable unit includes onboarding/sign-in and the minimum first-organization creation/reopen they actually require. Teams, people, daily tasks, assignment and one Codex runtime follow separately. Internal foundations do not count as usable features. Each unit retains current tests, independent/original/fresh-final reviews; no repeated whole-reservoir review or whole-ancestry merge. Root owns architecture/integration, max2 disjoint writers including root; signing remains last.

- [F3b creation/admission ownership boundary r1](tasks/v1-creation-admission-interface.md): ACTIVE after fresh independent SolXhigh READY and original-author exact provider closure. Pure UUID/creation DTOs and organization-owned live owner/canonical bootstrap exclude task-feature imports. All4authority/source-position/recoverable-phase/dependency conditions apply. F3b is worker-provider foundation; production host producer/custody and F4 sessions remain prerequisites for usable U1. No mechanism or security waiver.
