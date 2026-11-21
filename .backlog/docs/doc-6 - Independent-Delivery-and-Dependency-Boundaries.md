---
id: doc-6
title: Independent Delivery and Dependency Boundaries
type: guide
created_date: '2026-11-21T00:14:00Z'
updated_date: '2026-11-21T00:14:00Z'
tags:
  - architecture
  - delivery
  - governance
---
# Independent delivery and dependency boundaries

> [!NOTE] Review note (2026-11-21)
> Reviewed against `v1` and Vision (`doc-1`). Current and accurate; serves directly beneath `doc-1` Principle 7 (Vertical Slice & 9/10 Architecture). Relative links to sibling `docs/` files will be updated during `TASK-32`.

These are required delivery and boundary rules under
[Architecture Rules](doc-8%20-%20Architecture-Rules.md#independent-delivery-and-dependency-boundaries).
Keep the detailed policy here; other guidance links to this document.

Apply these requirements to the domain modules and vertical slices defined in
Architecture Rules.
They strengthen the architecture and delivery process; they do not add another
review tier.

1. **Define the delivery before implementation.** Record the usable outcome,
   acceptance criteria, required dependencies, exact file ownership and shared
   interfaces in the increment's short plan, written before code (see the
   [implementation plan](doc-5%20-%20v1-Implementation-Plan.md#for-every-increment)). Each
   increment is a separate acceptance and merge unit. Keep its full production
   requirements.
2. **Keep dependency direction explicit.** Shared foundations expose narrow,
   stable contracts and MUST NOT import feature-specific protocols or depend on
   unfinished consumer implementations. Put each contract with the capability
   that owns its meaning; consumers depend on that contract, and applications
   compose implementations. A common package is not a mandatory home for every
   type. Avoid artificial wrappers, packages or duplicate sources of truth.
3. **Prove the boundary early.** Build and exercise the smallest real production
   path before expanding the slice. Show that the increment compiles and can be
   tested against its declared prerequisites without importing unrelated,
   unfinished features. Focused checks support iteration; they do not replace
   required full checks, production composition or end-to-end acceptance.
4. **Deliver prerequisites deliberately.** Give a genuinely required foundation
   its own bounded contract, checks, review and integration before dependent
   delivery. Label internal foundations honestly; they are not usable features.
   Do not accumulate unrelated features into a bundle that requires later
   extraction. Keep feature-specific supporting code within its slice; do not
   invent prerequisites merely to split files.
5. **Escalate unexpected coupling.** When implementation reveals an unplanned
   dependency or an invalid boundary, pause the affected work and return concrete
   source or compiler evidence to the lead who owns the increment. Decide whether
   to deliver a prerequisite or revise the interface before dependent work
   resumes, and record the decision as the
   [non-negotiables](doc-7%20-%20Architecture-Non-Negotiables-and-Who-Decides-What.md#who-decides-what) require.
   A material architecture change gets an independent review; unaffected
   independent work may continue. Do not import unrelated features,
   invent production capabilities or weaken requirements merely to make a build
   or test pass.
6. **Verify behavior.** Acceptance covers the relevant authorization,
   retry/original-result replay, recovery and failure behavior, using temporary
   roots. Nothing has shipped, so there is no migration or compatibility to test.
   Exercise actual affected failure boundaries and add meaningful regressions for
   confirmed defects. Passing compilation, lint or isolated helpers does not establish a
   usable production path.
7. **Carry the contract through handoffs and reviews.** Every implementation and
   review assignment references this document and the increment's plan,
   including the outcome, dependency boundary, ownership and verification needed.
   Reviewers inspect actual dependencies and production behavior against that
   plan. The lead who owns the increment owns its architecture and integration;
   one writer per file at a time, author validation, full checks and fresh review
   gates still apply. Reuse concise source-bound receipts rather than repeating
   routine investigations.
