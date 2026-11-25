# Standalone prototypes

For changes confined to `apps/agentic-org-web-prototype/`, use targeted local checks and relevant browser
verification for acceptance. This replaces the repository's code review cycle
for prototype work, including reviewer agents and final review.
Use `bun apps/agentic-org-web-prototype/smoke.ts <route> <theme>` for a quick Chromium check and screenshot,
or `bun apps/agentic-org-web-prototype/smoke.ts --all` to run all 5 behavioral checks plus a 27-route / 6-theme zero-error sweep in one browser pass.

## v6 Prototype Layout (Edit Directly — Zero Build Config)

- Shell: `v6.html` (links stylesheets in cascade order + `<script type="module" src="./v6/main.js">`).
- Styles: `v6/styles/` (`base.css`, `home.css`, `theme-ocean-classic.css`, `theme-report.css`, `theme-graphite.css`), plus shared `operations.css` and `cozy.css`.
- Core JS: `v6/data.js`, `v6/state.js`, `v6/ui.js`, `v6/main.js`.
- Pages: `v6/pages/` (`home.js`, `inbox.js`, `work.js`, `projects.js`, `insights.js`, `chat.js`, `activity.js`, `teams.js`, `agent-profile.js`, `settings.js`).
- App shell: `v6/shell/` (`sidebar.js`, `router.js`, `cmdk.js`, `keyboard.js`, `dialogs.js`, `actions.js`).
- Keep `v5.html` untouched (historical snapshot).

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

If a release is in progress before context compaction or handoff, update
`.agents/agent-kit/logs/prototype-release.md` with the approval scope, comparison
revisions, confirmed final changes, verification status, and next step.
