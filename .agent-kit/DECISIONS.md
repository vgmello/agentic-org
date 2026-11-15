# Active constraints and decision index

Repository ownership and architecture remain authoritative. Canonical ADRs stay in docs/adr/; new architecture decisions follow ada's process.

- Composition: docs/architecture-rules.md and docs/design/project-structure.md; binaries compose libraries, policies own required contracts, no source cycles. Domain moves remain incremental.
- Authority: docs/design/documentation-authority.md and organization-data/README.md own selected target relationships. Draft SQL/acceptance criteria are not shipped migrations or services.
- Storage/key boundaries: ADR-0002 and ADR-0005 retain one office writer and host key custody. Gateway/agent identities do not receive raw office keys. Read primary authorization/acknowledgement/recovery contracts for affected work.
- Coordination: ADR-0003 live Phase A section and AGENTS.md; preserve installed org-agent, spool state, pending asks/replies/triage.
- Ownership/merge: AGENTS.md and docs/processes/branch-and-merge.md; one unmerged branch per agent, independent own review, reviewer READY for exact commit, required checks and ada's go.
- Owner's integration override (2026-11-15): review completed features locally and then merge into `v1-integration`, the shared integration branch for all v1 features. Do not wait for a PR. This does not change area ownership or authorize a release-v1 merge.
- UI licensing: Tailwind Plus stays inside apps/agentic-org-ui, never a published kit or crate.
- Adoption: user explicitly selected codex-only on 2026-11-15. Kit 0.10.0 at 01e0d2f8ec338cf84dcbf15fe158ee5181fbd0b1 supplies Sol routing, Repowise 0.61.0 and Hindsight 1.2.1. Separate readiness claims/evidence live in ADOPTION.md.

No new product architecture is selected. Read applicable full primary documents before dependent work. Preserve replaced decisions/rationale in canonical ADR history.

Task-specific implementation premises: [TH-01](decisions/TH-01-task-history-validation.md)
tracks selected task-history validation and its independent coverage prerequisite;
it does not replace the product decisions in documents 05/12.
