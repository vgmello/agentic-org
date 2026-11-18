# <ADR-ID>: <architecture decision>

Use the existing project ADR convention, otherwise save as
`.agent-kit/decisions/<ADR-ID>-<slug>.md`. One lead owns this canonical record. Link it
from DECISIONS.md and affected contracts; keep it focused and load only when relevant.

- Status: PROPOSED / ACTIVE / CHALLENGED / SUPERSEDED / REJECTED
- Scope, original request/authority and affected task contracts:
- Architecture owner; actual author session/model when available:
- Decision revision and source baseline; last evidence review date:
- Supersedes / superseded by (record ID and revision; do not erase history):

## Intent and premises

- Outcome and non-negotiable requirements; owner authorized to change them:
- Boundaries, constraints and why this decision was needed:

| Premise ID | Requirement, observed fact or assumption | Source/authority and revision/date | Evidence and VERIFIED / UNVERIFIED / DISPROVED state | Scope, consequence if false and recheck trigger |
| --- | --- | --- | --- | --- |
| <P-1> | <claim and kind> | <request, contract, source or external documentation> | <test/observation or missing evidence> | <affected behavior and dependency> |

An unsupported inference stays UNVERIFIED. If it is essential to feasibility, obtain
evidence before marking architecture READY. Preserve valid requirements even when
the chosen mechanism fails. Record concise justification, not private transcripts.

## Decision and alternatives

- Chosen design, interfaces and how it satisfies the premise IDs:
- Alternatives considered, reasons rejected and tradeoffs:
- Known limits, failure cases, migration/rollback and operational implications:
- Checks that establish feasibility and would falsify key assumptions:

## Architecture review

- Exact decision revision and relevant source baseline reviewed:
- Independent reviewer/session and profile-required architecture review model/effort (observed, not invented):
- READY / CHANGES_REQUIRED / INCOMPLETE; findings, evidence and resolutions:
- Lead disposition; conditions to re-review and affected plan readiness:

## Challenges and replacements

Append concise entries; retain the original reason and link full evidence.

- Observation/date/source revision; reporting worker and affected premise IDs:
- Expected versus actual behavior; reproduction and impacted/dependent slices:
- Missing prerequisite, local defect or architecture failure; evidence for classification:
- Valid premises retained; premises disproved or unverified, with evidence:
- Proposed resolution/replacement ID; why it satisfies the retained constraints:
- Review outcome, lead decision and remaining blockers:
- Plans/routing/checks/previous implementation invalidated or updated; resume revision:
