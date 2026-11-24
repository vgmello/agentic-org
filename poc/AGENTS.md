# Standalone prototypes

For changes confined to `poc/`, use targeted local checks and relevant browser
verification for acceptance. This replaces the repository's code review cycle
for prototype work, including reviewer agents and final review.

Commit and push each verified prototype update to `poc/ui-themes-prototype`
with a normal commit subject. When the owner explicitly approves a release,
push a commit with the exact subject prefix `Prototype Release: ` followed by
the approved scope. Use an empty release commit if the approved changes are
already on the branch. The marker commit is the Svelte integration handoff.
Find release markers from the branch history without a path filter so empty
commits are included.
