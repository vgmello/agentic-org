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
- Pages: `v6/pages/` (`home.js`, `inbox.js`, `work.js`, `task-composer.js`, `projects.js`, `insights.js`, `chat.js`, `activity.js`, `teams.js`, `agent-profile.js`, `agent-focus.js`, `settings.js`).
- App shell: `v6/shell/` (`sidebar.js`, `router.js`, `cmdk.js`, `keyboard.js`, `dialogs.js`, `actions.js`).

## Fast UX Craft (No Ceremony, Proper Taste)

- Apply `.agents/agent-kit/project-skills/principal-designer/SKILL.md` as the fast design filter (*Simple first*, *Felt safety*, five UX laws, sibling consistency), pulling in **only** the 1 relevant companion file when needed (`layout-and-foundations.md` for page structure/spacing, `components.md` for controls/overlays/rows, `behavior.md` for agent trust/approvals/waiting). Skip `tailwind.md` in plain-CSS `v6/`.
- When prototyping a new product area, skim only its single owning UX spec in `../../../../backlog/docs/specs/` (or `docs/design/`) before editing—never dump whole spec sets or run formal review ceremonies on prototype drafts.
- Iterate directly in `v6/` with live HMR (`bun apps/agentic-org-web-prototype/server.ts`) and verify fast with `smoke.ts <route> <theme>`.

## Backlog Task Tracking & Svelte Handoff

- Whenever prototyping a feature, screen, or interaction that does not yet exist in the production Svelte app (`apps/agentic-org-ui/`), check the parent Backlog repo (`../../../../backlog/`, running `backlog` CLI commands from `/Users/vgmello/projects/agentic-org`) for an existing task.
- If no task covers the new prototype capability, create one (following `backlog instructions task-creation`) capturing the user need, Svelte implementation gap, agreed UX requirements, and testable acceptance criteria. If a task already exists, update its description/notes with the discussion outcomes and prototype changes.
- Keep the task's notes updated as prototype iterations land so that once the owner approves the prototype, the Svelte implementation agent receives a complete task reflecting everything discussed and agreed—not just a diff.
- Reference the Backlog task ID (`TASK-<N>`) in prototype commit subjects (e.g., `feat(prototype): ... (TASK-123)`) and in the `Prototype Release:` handoff marker commit.

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
