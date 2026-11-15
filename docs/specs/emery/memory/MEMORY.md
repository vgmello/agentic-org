# Memory · Emery

Durable knowledge that must survive between sessions. Keep entries short, dated and specific. Delete anything that is no longer true.

## Conventions and decisions

- 2026-11-11 · I'm a woman (she/her), at the owner's request.
- 2026-11-12 · IA v2 is at PRD rev 4.1 (D1-D42 as of the late 2026-11-12 PRD set) and stories rev 4 (IA-01 to IA-19, IDs stable). ada adopted it as the v1.0 shape. The one-page vision and UX lens for designers and validators is `knowledge/prds/product-vision-and-ux-lens.md`. Keep it in step with the PRD whenever a decision changes. SET-3 (restart to update) is out of v1.0. Spool messages can be read directly: `~/.agency/spool/inbox/ac-product-manager-emery/new/<id>.json`.
- 2026-11-12 · The IA v2 prototype is hosted at https://claude.ai/artifact/JX8BmbmJqkc7PAv47uAshh (private). Republish the same file to update it. The HTML `<title>` sets the artifact name.
- 2026-11-11 · My role has no Bash, so `org-agent msg` (read, send, done) has to go through a helper agent. The syntax is `ORG_AGENT=ac-product-manager-emery ~/.local/bin/org-agent msg send --to <agent> --subject '...' -`, with the body on stdin. Read a message with `msg read <id>`.
- 2026-11-11 · Messages to ada carry only GO requests, merge reports, blockers and answers; FYIs go in my journal (RULES.md #7). Ask ada first and never wait on the user (RULES.md #12).
- 2026-11-11 · The product specs live in the product repo on `origin/v1`: `git -C /Users/vgmello/projects/agentic-consulting/app show origin/v1:docs/specs/<file>`. They're newer than Eden's copies in her home, so cite the repo version. `desktop-app-v1.md` (Tauri era) is the old base layer, not the current IA.
- 2026-11-11 · The product is no longer a Tauri desktop app. v1.0 is a self-hosted server: `agentic-org setup` runs an `org-gateway` web UI in the browser, with HQ and satellite offices and Linux support. The project brief and ARCH still say Tauri.

- 2026-11-11 · **The owner's product direction:** organize the app like a company (org → offices → projects and work → teams of people and agents). Projects, tasks and status come first, plus activity and how members interact. Agents stay, but as a less technical, more advanced area; offices are configuration under organization or settings. **No emoji anywhere**; icons are welcome. The look and feel is fine. My IA v2 is in `knowledge/prds/information-architecture-v2.md`, with the prototype at `knowledge/prototypes/ia-v2/`.
- 2026-11-11 · The UI look is the dark Tailwind Plus style: Inter, indigo primary, amber only for "waiting on the owner", Heroicons outline 24. The design system wins where it and the code differ.

- 2026-11-11 · IA v2 is accepted by ada and sequenced (stories in `knowledge/prds/ia-v2-stories.md`). **AC IDs are stable; send any story edit to Rowan and ada.** Runtime presence (IA-15) is in v1.0. The v1 ETA is about 2026-11-24 to 11-28. Eden owns design-system v2 and the list of rewritten spec IDs.

- 2026-11-13 · **Product review assembly line** (owner via ada): once a feature set is merged on v1, I review it by feature, in batches (not per merge). I check it against its PRD stories and `knowledge/prds/acceptance-tests-v1-prd-set.md`. Eden checks UX in her own batches, and Tomas owns code quality and architecture. Findings become bug tickets for the author (through ada's catalog).
- 2026-11-13 · **Platform (IA D45):** v1 runs in the browser only, against `org-gateway` on localhost, with no desktop shell. Supported: Chrome or Chromium (last 2 majors), Firefox (stable plus ESR) and Safari on macOS (last 2 majors). Tauri is parked, and a Mac status-bar app is future work. The project brief still says Tauri.
- 2026-11-13 · **v1.0 scope (ada, from Rowan's sizing, about 75 points):** Insights and Copilot capture are v1.1, and Codex capture is v1.0. Shared definitions (completion %, durations, day boundaries) stay normative in v1.0. The messaging product rules are in `messaging-asks-v1.md` (AK1-AK10, CP).

- 2026-11-13 · **Offline window:** the owner took the Claude agents offline for about 3 days, and the Codex engineers continue on `v1-offline`. My handover is `handover/OFFLINE.md`. When I'm back, read the decisions the Codex team recorded and confirm or correct them first. The cost visibility rule is in costs Rev 15: budget bars are owner only, a PM sees "in your projects" records, and repo files carry no amounts.

## Lessons learned

- 2026-11-12 · For prototype changes, have the fresh reviewer render the page with Playwright (chromium over file://, 3 widths × both data modes, plus focus flows). Code reading alone missed real defects: a CSS class collision and focus landing on body. Scope any responsive CSS fix to its media query.
- 2026-11-12 · Why we redid the design: the earlier build met its specs but had the wrong shape (machinery first, the human loop buried in Chat, no colleague profile). Validation and review must check fit with the vision, not only the AC. That's the lens doc §2-§3.
- 2026-11-12 · Summaries of decisions (lens pages, briefs) drift from their sources. Quote rather than paraphrase, and give each claim its sequencing caveat (which story makes it true). A fresh source-check review is needed even for docs.
- 2026-11-12 · ada's IA v2 backlog triage lives in `knowledge/prds/ia-v2-backlog-triage.md`, and she references the file directly (no copies). Keep it current.

- 2026-11-12 · The owner asked why I didn't propose project planning and docs myself. Don't wait for requests: keep a proposed-features roadmap (`knowledge/prds/feature-proposals-and-docs-model.md`) and bring new proposals to the owner proactively. Think about the whole product lifecycle (building → live), not only the transactional work.

- 2026-11-12 · Between my own PRDs, cite section, decision and AC IDs, never line numbers: the lines move with every edit, and each review then flags stale citations. Keep line numbers only for product-repo files at a named commit. For a state machine or metric-heavy PRD, write the transition table, permission matrix and metric definitions (population, window, zone, edge cases) first, then the prose.

- 2026-11-12 · **One source of truth for shared rules.** tasks-v1 owns task status moves and Inbox items; costs-and-budgets §12 amends companions explicitly; insights owns metric definitions and the aggregate duration formatter; planning owns release/milestone states. Every fix to one PRD's shared rule needs a cross-doc review pass, or the drift comes back. Tell reviewers "rare combinations are LOW" or the loop never converges.
- 2026-11-13 · **Seed the AC fixture fully before the first review.** The ceremonies PRD took 17 review rounds, mostly over fixture gaps: items, routines, anchors, occurrence numbering, exact message layout. Write exact output blocks (line by line, plain text) and a complete fixture up front.
- 2026-11-13 · **When a release cut moves a PRD (or a story) out of v1.0, first grep every other PRD for citations of it** (AC IDs, §, formatter, periods), and make the ones v1.0 needs normative in one "v1.0 scope" note. Also check for features that depend on a moved story's states, such as In review. The Insights cut took 11 review rounds because these were found one by one.
- 2026-11-13 · **Routines PRD** (`ceremonies-and-retros-v1.md`, D1-D25). "Routine" is the UI word, and `ceremonies` is a CLI alias. v1.0 leads are agents only and only the owner configures. A routine's own Inbox items are two non-dismissible Problems. Eden's retrospectives-ux is superseded per D22.
- 2026-11-12 · The v1 product PRD set (all READY 2026-11-12): tasks-v1, projects-planning-v1, project-insights-v1, away-digest-and-timezones-v1, costs-and-budgets-v1 (D38-D42 in the IA PRD). Build (R&D) is v1; Run (KTLO) is v2, and the v1 model stamps a work type on every item and cost record.

## Stakeholder preferences

- 2026-11-12 · **The owner wants the session kept free:** run research, reviews, drafting and review-fix loops in background subagents, and keep only the product decisions in my context.
- 2026-11-11 · **The owner expects me to make product and design decisions myself**: "if you have 10 years of experience … you should be able to make those decisions". Decide, record the decision and its reason, and move. Don't send the owner lists of open questions; bring only strategy, pricing or legal questions.
