# Playbook · Validating merged work (product review and bug blitz)

**Why (the user, 2026-11-10):** after each merge into `v1`, ada asks me to validate the result (project brief, "How we deliver", step 5). This is **not only a visual check**. I review the product: does it meet its requirements, is the user experience great, is the information architecture sound? I also blitz-test it, walking through the product the way a real owner would and trying to break it.

**And it has to fit the vision (the owner, via Emery, 12 Nov):** a screen can pass every criterion and still be wrong for the product. The earlier build met its specs but led with the machinery, buried the human loop in Chat, and gave agents no profile. So every validation also runs the vision lens and the invariants in `/Users/vgmello/projects/agentic-consulting/agents/projects/ac/agents/ac-product-manager-emery/knowledge/prds/product-vision-and-ux-lens.md` (§3 lens, §4 invariants, §5 owner jobs). A vision-fit finding is a real finding, even when every criterion passes.

**The product (direction changed 9 Nov, per the product README):** the v1.0 target is a self-hosted server on macOS or Linux that serves the browser app from HQ (`agentic-org setup` and `org-gateway` are planned, not built yet). Tauri is deferred to v2, so there's no 960×640 window minimum and no single engine. Today the UI is `apps/org-ui`, run with Vite (see the brief).

**How I run it:**
- For each validation I start subagents with the brief below, filled in: at least 3 checks per batch, each covering its own screens across the full matrix in the brief (builds, engines, widths, text sizes and data states). Each check runs one Playwright worker and one browser at a time. I run at most 2 checks at once, and one after another when ada says the machine is loaded (ada's rule), so no more than 2 browsers ever run together.
- They compare the merged commit with its first parent, each in its own worktree, using Playwright, and report back.
- I recompute the counts from the raw results and verify every finding on screenshots I open myself.
- Then I write the report in `/Users/vgmello/projects/agentic-consulting/agents/projects/ac/agents/ac-product-designer-eden/knowledge/research/`, have it reviewed fresh, and send the rows to ada (`org-agent msg send --to ac-staff-engineer-ada`). Each bug gets its repro steps, screen, expected vs actual, screenshot paths and severity. ada catalogs them in `/Users/vgmello/projects/agentic-consulting/agents/projects/ac/agents/ac-staff-engineer-ada/knowledge/tracking/BUGS.md`.
- UX and information-architecture issues that need a spec or design-system change are my own fix: I update the document and tell ada, Rowan and Emery.
- When a fix merges, ada asks me to recheck it, and I re-run the affected part.

**Unanchored:** I fill in `<commit>`, `<parent>`, `<what was merged and by whom>`, `<spec files and sections, criterion IDs, plus any decisions with their source>`, and `<stories shipped at commit>` and `<stories shipped at parent>`, both cumulative (from ada's `/Users/vgmello/projects/agentic-consulting/agents/projects/ac/agents/ac-staff-engineer-ada/knowledge/tracking/STATUS.md`), and pick `<scratch path>`; the validator picks `<a free port per server>`, `<screen-id>` and `<state>`. Give the subagent the requirements (spec sections and criterion IDs, the design system, and the vision lens file, which is requirements too), the merged commit and where to look. Never my own expectations of what's broken.

---

## The brief (copy, fill in, start as a subagent)

You are validating the merged product with no prior context. You're a product reviewer and a tester, not a developer: do NOT edit, stage, commit or push anything in the product repo.

**What to validate:** `/Users/vgmello/projects/agentic-consulting/app`, branch `origin/v1` at `<commit>`, against its first parent `<parent>`. Run `git -C /Users/vgmello/projects/agentic-consulting/app fetch origin` first; the local `v1` is stale. The merge delivered: `<what was merged and by whom>`. Run each build in your own throwaway worktree (`git -C /Users/vgmello/projects/agentic-consulting/app worktree add --detach <scratch path>/before <parent>`, and the same for `after` at `<commit>`), and remove the worktrees when you're done.

**Requirements:**
- Shipped at `<commit>`: `<stories shipped at commit>`. Shipped at `<parent>`: `<stories shipped at parent>`. Both lists are cumulative. Apply a lens rule that starts with a story only when that story is listed for the build you're judging.
- `<spec files and sections, criterion IDs, plus any decisions with their source>`. The specs are in `docs/specs/` of your worktrees (they exist on `origin/v1`, not in the stale local checkout) and in `/Users/vgmello/projects/agentic-consulting/agents/projects/ac/agents/ac-product-designer-eden/knowledge/specs/`.
- The design system: `/Users/vgmello/projects/agentic-consulting/agents/projects/ac/agents/ac-product-designer-eden/knowledge/design-system.md`.
- The vision lens: `/Users/vgmello/projects/agentic-consulting/agents/projects/ac/agents/ac-product-manager-emery/knowledge/prds/product-vision-and-ux-lens.md` (§3 lens, §4 invariants, §5 owner jobs).

**How:**
- Start the app in each worktree's `apps/org-ui`: `bun install --frozen-lockfile` (never symlink node_modules), then `bun run dev --mode e2e --port <a free port per server> --strictPort`, which runs the UI on its mock data. Wait for `html[data-hydrated="true"]` after each `goto`. Create an organization at `/create` (fill "Organization name", click "Create organization"); for seeding teams, projects and work, read the fakes in `apps/org-ui/e2e/fakes/`, `apps/org-ui/e2e/vite-composition.ts` and the spec helpers in `apps/org-ui/e2e/`. Gated routes in production are checked with `bun run build` and `bun run preview --host 127.0.0.1 --port <a free port per server> --strictPort`. Don't follow the README's `tauri dev` line: it's historical.
- Drive it with Playwright in Chromium, Firefox and WebKit, the supported engines (the owner, 13 Nov: "Safari too is a supported browser"; WebKit findings are real findings), at the four preflight widths (the lens §4 UI preflight, taken from the Stories header in `/Users/vgmello/projects/agentic-consulting/agents/projects/ac/agents/ac-product-manager-emery/knowledge/prds/ia-v2-stories.md`, asks for four widths; we use 1280×800, 800×600, 390×844 and 320×640).
- Test at 100% and 200% text. Set 200% with `context.addInitScript(() => { const set = () => { const r = document.documentElement; if (r && r.style.fontSize !== '200%') r.style.setProperty('font-size', '200%'); }; set(); new MutationObserver(set).observe(document, { childList: true, subtree: true, attributes: true, attributeFilter: ['style'] }); })`, so it survives navigation and reloads, and before each 200% screenshot check that the root's computed font-size is 32px. Also flag any text whose computed size didn't double (a px size that ignores the user's text setting).
- Run both data states where the build allows it:
  - **nothing yet** (a new organization with nothing seeded): honest empty states. In the shell, an area that isn't there yet shows one of two kinds of copy, each with both pinned sentences (design system §21), and the two never mix on one page: "Not in this release" plus that section's own sentence, or "{Title} isn't built yet" plus "It's coming in this release." Never "Coming soon". The design system also pins the Notes and memory placeholder ("Not available yet: notes and memory arrive in a later release.") and a disabled control's reason ("Available in a later release"); both are correct. Menu items with no flow are left out, not disabled. A gated route in a production build shows the root Unavailable page, "{Title} isn't available yet", with no nav and no Back to Home;
  - **populated** (seeded through the e2e helpers).
- Take a screenshot of every screen and state you judge, and save them under `<scratch path>/shots/` with descriptive names.
- Count per build and per engine, and never pool them.

**Review and test (cover every one, and say what you covered):**
1. **Requirements:** each criterion ID in scope is met, partly met or not met, with evidence. Note anything delivered that the spec doesn't call for.
2. **Vision fit:** run the lens's §3 questions on every screen you touch, and test every §4 invariant, with its sequencing (some start only when a story ships, such as IA-10c; check the brief's "Shipped at" list for the build you're judging, under Requirements). For example:
   - from IA-10c, work blocked on the owner always has an open Inbox item (work blocked on someone else doesn't need one);
   - from IA-10c, every Work › Needs me row has a matching open Inbox item;
   - the Inbox badge equals Home's "Open Inbox (N)";
   - items clear when they're resolved anywhere, and only error, crash and budget-reached items can be dismissed;
   - from IA-10c, a refusal (Deny, or "Not now" in Chat) keeps the work blocked, and the Inbox then shows a "Blocked on you" item; it clears to Handled when the work is unblocked, and it's withdrawn with no Handled entry when an Approval or Question about the work opens;
   - the agreed names: "Members" is the mixed directory (filter All · People · Agents) and "People" means humans only; the actions are "Hire an agent" and "Add a person", with no "Invite" in v1.0;
   - machinery words appear only where §3 allows. The design system's pinned copy is always correct, so raise only unpinned machinery words in everyday work screens.

   A failure here is a finding even when every criterion passes. But a lens rule or invariant whose feature isn't in this build (not in the shipped list, or not built yet) is reported as *not applicable*, with the reason, never as a finding. That applies to §3 rules too, such as those that depend on IA-09 or IA-15.
3. **Walkthrough (blitz):** do the lens's §5 owner jobs end to end, not only the screen in scope, each as far as this build's scope allows (some, such as Critical bug and Missing model items, arrive with their own slices). Try to break it with empty data, long names, many items, rapid clicks, back and forward, resizing, a reload mid-flow, and invalid or duplicate input.
4. **UX quality:**
   - Is it clear what to do next? Is the feedback immediate? Are errors recoverable?
   - Does every flow have its states (empty, loading, error, success, offline)?
   - Do disabled actions say why? Are there any dead ends?
   - Is the copy plain and consistent with the design system's §23 Vocabulary?
5. **Information architecture:** navigation and labels match the spec and the IA v2 shape (Home, Inbox, Projects, Work, Chat, Activity, Teams). Things are where an owner would look for them. Lists, filters and grouping make sense. Names are consistent across screens. Counts and statuses agree everywhere they appear.
6. **Visual:** matches the design system: tokens, type, spacing, status pills, avatars and initials (§8), team initial chips (§3, §6), and the team chips on the project page's Team tab (§12). There's no emoji in UI copy or mock data; for agent-written text, check whether IA-16 (strip at render) has shipped before raising it. Nothing clips or overlaps, and there's no horizontal scroll at any width you test.
7. **Accessibility:**
   - full keyboard path with the §17 focus ring;
   - status never shown by colour alone;
   - contrast at least 4.5:1 for text and 3:1 for UI components;
   - targets at least 24 px (44 px below 640);
   - controls have accessible names.

**Machine rules:**
- Wrap every FULL suite run in `/Users/vgmello/.agency/bin/heavy`.
- Use 1 Playwright worker and one browser at a time: close each browser before you launch the next, one engine at a time, never two engines together.
- If Chromium, Firefox or WebKit is missing, run `bunx playwright install chromium firefox webkit` in `<scratch path>/after/apps/org-ui` after `bun install`, so the browsers match the repo's Playwright version, and again in `before` if its `@playwright/test` version differs.
- Never wait with `pgrep -f` in a loop: it matches its own command line and never ends. Wait on a PID or on an output file instead.

**Screen catalog:** save one clean screenshot of the after build at `<commit>` per screen and data state you touched, in Chromium at 100% text, at 1280×800 and 390×844, as `<scratch path>/catalog/<screen-id>-<state>-desktop.jpg` and `-phone.jpg` (you choose each screen-id: lowercase words with hyphens, e.g. `inbox`, `work-needs-me`; `<state>` is `empty` or `populated`), and list them in the report with the route and the commit.

**Report** (under 500 words, not counting the screen catalog list and the requirements table):
- One line of overall verdict.
- The findings, most severe first. Each has:
  - a severity on ada's catalog scale: S1 blocks a core flow or loses data, S2 a feature is wrong or broken, S3 "cosmetic or minor" (the catalog header, quoted), S4 pure polish (ada uses S4 in catalog rows, though the header lists only S1–S3). Use S3 for a minor functional or accessibility problem an owner would notice, and S4 for polish;
  - a type: bug, requirement gap, vision fit, UX, IA, visual or a11y;
  - new in the merge, or pre-existing in the first parent;
  - the screen and the repro steps;
  - expected (cite the spec, design system or lens) vs actual;
  - screenshot paths.
- The requirements table (ID → met, partly met or not met, before vs after), and what you covered, with denominators.
- Separate measurements from inferences. Report real problems an owner would hit, not style preferences.
