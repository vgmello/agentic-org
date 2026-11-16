# Independent delivery and dependency boundaries

These are required delivery and boundary rules under
[Architecture Rules](architecture-rules.md#independent-delivery-and-dependency-boundaries).
Keep the detailed policy here; other guidance links to this document.

Apply these requirements to the domain modules and vertical slices defined in
Architecture Rules.
They strengthen the existing architecture and delivery process; they do not add
another review tier or authorize restarting preserved implementation.

1. **Define the delivery before implementation.** Record the usable outcome,
   acceptance criteria, required dependencies, exact file ownership and shared
   interfaces in the slice's stable delivery contract. Each increment is a
   separate acceptance and merge unit. Keep its full production requirements.
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
   invent prerequisites merely to split files. Preserve existing bundles and split
   them at safe handoffs without
   discarding work, author contexts or required coverage.
5. **Escalate unexpected coupling.** When implementation reveals an unplanned
   dependency or an invalid boundary, pause the affected work and return concrete
   source or compiler evidence to the architecture owner. Decide whether to
   deliver a prerequisite or revise the interface before dependent work resumes.
   Material architecture changes retain the existing independent review gate;
   unaffected independent work may continue. Do not import unrelated features,
   invent production capabilities or weaken requirements merely to make a build
   or test pass.
6. **Verify behavior and compatibility.** Acceptance covers the relevant
   authorization, retry/original-result replay, recovery, migration and failure
   behavior, using temporary roots. Historical fixtures represent historical
   schemas independently of the evolving current-schema constructor. Exercise
   actual affected failure boundaries and add meaningful regressions for confirmed
   defects. Passing compilation, lint or isolated helpers does not establish a
   usable production path.
7. **Carry the contract through handoffs and reviews.** Every implementation and
   review assignment references this section and its stable delivery contract,
   including the outcome, dependency boundary, ownership and verification needed.
   Reviewers inspect actual dependencies and production behavior against that
   contract. Keep one overall architecture/integration owner and the existing
   writer limit, author validation, full checks and fresh review gates. Reuse
   concise source-bound receipts rather than repeating routine investigations.
