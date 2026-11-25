# Standalone prototypes

For changes confined to `apps/agentic-org-web-prototype/`, use targeted local checks and relevant browser
verification for acceptance. This replaces the repository's code review cycle
for prototype work, including reviewer agents and final review.
Use `bun apps/agentic-org-web-prototype/smoke.ts <route> <theme>` for a quick Chromium check and screenshot.

Commit and push each verified prototype update to `poc/ui-themes-prototype`
with a normal commit subject. When the owner explicitly approves a release,
push a commit with the exact subject prefix `Prototype Release: ` followed by
the approved scope. Use an empty release commit if the approved changes are
already on the branch. The marker commit is the Svelte integration handoff.
Find release markers from the branch history without a path filter so empty
commits are included.

Before writing the marker, compare the last approved snapshot to the new
approved snapshot with `git diff <previous-release-marker> <approved-tip> -- apps/agentic-org-web-prototype/`.
An empty marker has the same snapshot as its parent. For the first marker, use
the agreed integration baseline. Use the diff and the owner's feedback to
describe the final changes by page or behavior. Leave out intermediate
iterations, reversals, and commit-by-commit summaries.

In the marker body, include the comparison range, affected pages, final
behaviors, and any visual details that must match. Call out mock behavior that
needs real product data. The prototype guides the approved behavior and look
and feel. The integration agent should use existing Svelte components and
retain production capabilities beyond the prototype. If approval covers only
part of the range, name the approved changes explicitly.

If a release is in progress before context compaction or handoff, checkpoint the
approval scope, comparison revisions, confirmed final changes, verification status,
and next step in the parent project's handover notes.
