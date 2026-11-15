# Active constraints and decision index

The current solo Codex workflow in AGENTS.md governs implementation and integration. Canonical product architecture and data contracts remain authoritative; ADRs stay in docs/adr/.

- Composition: docs/architecture-rules.md and docs/design/project-structure.md; binaries compose libraries, policies own required contracts, no source cycles. Domain moves remain incremental.
- Authority: docs/design/documentation-authority.md and organization-data/README.md own selected target relationships. Draft SQL/acceptance criteria are not shipped migrations or services.
- Storage/key boundaries: ADR-0002 and ADR-0005 retain one office writer and host key custody. Gateway/agent identities do not receive raw office keys. Read primary authorization/acknowledgement/recovery contracts for affected work.
- Coordination: preserve installed org-agent, spool state and pending asks/replies/triage under ADR-0003. Do not use live messaging for routine development or contact offline developers.
- Solo ownership (owner instruction, 2026-11-15): Codex owns all repository areas and uses local kit subagents as needed. Named-owner boundaries, external reviewer waits and Ada agreement/merge approval are superseded. Keep local independent reviews, original-author validation, final review and current checks.
- Owner's integration override (2026-11-15): review completed features locally and then merge into `v1-integration`, the shared integration branch for all v1 features. Do not wait for a PR. The later solo instruction supersedes former area ownership; no release-v1 merge is authorized.
- Owner's blocked-work direction: move to another feature when blocked. Preserve the blocked branch/checkpoint, then start independent work from the integration branch; do not stop all delivery on one unresolved prerequisite.
- UI licensing: Tailwind Plus stays inside apps/agentic-org-ui, never a published kit or crate.
- Adoption: user explicitly selected codex-only on 2026-11-15. Kit 0.10.0 at 01e0d2f8ec338cf84dcbf15fe158ee5181fbd0b1 supplies Sol routing, Repowise 0.61.0 and Hindsight 1.2.1. Separate readiness claims/evidence live in ADOPTION.md.

No new product architecture is selected. Read applicable full primary documents before dependent work. Preserve replaced decisions/rationale in canonical ADR history.

Task-specific implementation premises: [TH-01](decisions/TH-01-task-history-validation.md)
tracks selected task-history validation and its independent coverage prerequisite;
it does not replace the product decisions in documents 05/12.

[TA-01](decisions/TA-01-explicit-task-assignment.md) preserves the selected complete
assignment-pair domain boundary and its separate service-acceptance obligations.
