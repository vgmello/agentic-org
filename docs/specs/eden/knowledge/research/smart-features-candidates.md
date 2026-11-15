# Smart features: candidates (copilot in the app)

Owner: Eden · 2026-11-08 (revision 2) · For the user to choose from. It refines the candidate table in ada's `ac-staff-engineer-ada/knowledge/designs/drafts/smart-features.md`, which holds the requirement, the principles and the architecture. The copilot runs in the office on the user's agent runtime, is read-only and has no tools, and gets its context from the office.

**What it is:** a helper that drafts, suggests or summarizes. It's not a member of the organization: it has no avatar, no team and no office, and it never shows in the org chart.

## UX rules for every smart feature (they apply ada's principles to the screen)

1. **The copilot drafts, you decide** (ada P1). The output lands in the normal form or a review step, filled in, and you create or send it with the usual button. There's no "accept and send" in one click.
2. **Say what it reads** (P4). Under the button, one line: "Uses this text and checkout's open bugs." Costs show where the runtime reports them, per run in the result, and per month in Settings › Smart features.
3. **Mark what it wrote.** Drafted fields carry a "Drafted" marker (`sparkles` + the word) until you edit or keep them. Suggestions come with a one-line reason ("Same steps as chk-9").
4. **Optional, stoppable, and never in the way** (P5). One button style ("Draft with copilot", secondary, `sparkles`). While it runs: "Drafting… **Stop**", and the form stays editable. **Hidden** when smart features are off or no runtime is set up. **Disabled with the reason** when the runtime is set up but unavailable right now (signed out, busy, timed out), with the manual path unaffected.
5. **One pattern everywhere:** the same button, progress state, Drafted marker and error ("The copilot couldn't draft this: <reason>. You can fill it in yourself."). There's no chat window.

## Candidates

**Value:** what it saves the owner (H/M/L). **Effort:** a rough design-side guess (S/M/L) for engineering to correct, where S means a prompt plus a schema on data the office already has. **Context:** what the office must give the copilot (all read-only, P2). **ada:** the row in her table it refines, or "new". ★ = my suggested first set.

| # | Feature | Where | In → out (draft) | Where you confirm | Context from the office | Value | Effort | ada |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 ★ | **Report a bug from a sentence** | Report bug (top: "Describe it in your own words"), ⌘K | A sentence, pasted text or a log → title, what happened, expected, steps, area and severity, each marked Drafted | The filled Report bug form → **Report bug** | the project's areas and teams | H | S | Bug from a description |
| 2 ★ | **Duplicate finder** | Report bug (replaces the title match) and the triage panel | The bug → up to 3 likely duplicates, each with a reason | **This is the same bug** / ignore | the project's open bugs (titles and summaries) | H | S | in Bug from a description and Triage helper |
| 3 ★ | **Triage suggestion** | Triage panel: "Suggest" | The bug → area, severity, priority and assignee, each with a reason | **Mark as triaged** (any field can be changed first) | areas, teams, members, recent similar bugs | H | M | Triage helper |
| 4 ★ | **Draft or improve a text** | Every long text field: bug and task descriptions, definitions of done, docs, the brief, messages, people's notes | Text + intent (Shorter · Clearer · Plain language · Fix spelling · Custom…) → a rewrite as a diff | **Use this** · **Use selected parts** · Discard | none beyond the field | H | S | Improve / draft a text |
| 5 ★ | **Hire from a description** | Hire panel: "Describe who you need" | "Someone to check our UI for accessibility" → title, role summary, responsibilities, focus, suggested team and reports to, and a name | The usual Hire review (the dry run), then **Hire** | the role catalog, teams, the org's names | H | M | Hire helper |
| 6 | **Definition of done from a task** | New task, the Assign dialog | The task title (+ description) → a one- or two-sentence checkable definition of done | The field, then **Create task** | none | H | S | new |
| 7 | **Task from a description / break down a task** | New task; task page ⋯ › Break down | A goal → 1 task, or 3–7 tasks with definitions of done, order and suggested assignees | A checklist of drafted tasks → edit → **Create 4 tasks** | the project's teams and members | M | M | Task from a description / split |
| 8 | **Fix task from a bug** | Bug page › Create fix task | The bug → a fix-task title, description and definition of done | The prefilled New task form | the bug, its area. Repo paths only if the office provides them (P2) | M | S (M with repo context) | new |
| 9 | **Suggest a reply** | Needs you items and agents' questions | The agent's question and its last turns → 2–3 short replies ("Yes, drop the Safari fix", "Not yet: …") | Pick one, edit, **Send** | the session's last turns, as data (P3) | M | S | new |
| 10 | **Summaries** | Home ("Since you were away"), project Overview, a long bug or message thread | The activity or thread → 3–5 lines with links | Read-only, and links open the items | the activity log, the thread | M | M | Summaries |
| 11 | **Draft a doc or the brief** | Docs › New doc ("Draft from a topic"), an empty Brief | The project + a topic or a few answers → a Markdown page | The editor, then **Save** (a commit) | the project, its teams and items, and existing doc titles | M | S | Project docs draft |
| 12 | **Release notes** | Project ⋯ › Release notes | Done tasks and Verified bugs in a date range → user-facing notes | A new doc in the editor → **Save** | the items in the range | L | S | new |
| 13 | **Ask the organization** | ⌘K › "Ask…" | A question ("who works on payments?", "what blocks checkout?") → an answer with links | Read-only | a search over the org's records, done by the office first (P2) | M | L | new |
| 15 ★ | **Staff a team from requirements** (the user) | Hire panel › "Build a team", Org chart › New team | Requirements ("a team to build a payments app: Rust backend, Svelte UI, QA") → a proposed team: name, lead, 3–6 roles with titles, focus, runtime and model each, reporting lines, and a cost range per month at API prices | A review listing every hire (edit, drop or add one), then **Hire 5 agents** (each through the normal hire review) | role catalog, existing teams and members, past costs per role | H | M | new |
| 16 ★ | **Route a request** (the user) | New request panel (⌥⌘N) | The request text + projects, members' roles, recent requests → a suggested project, kind and the 3 best members to scope it, each with a reason | Pick one, @name, or Just save, then **Send** | projects, roles, members, recent requests | H | S | new (W16, rule fallback without a runtime) |
| 14 | **Team suggestion** | Org chart › New team, Add member | Members' roles and projects → "These 3 agents work on checkout. Make a Checkout team?" | The New team form, prefilled | members, roles, projects | L | M | new |

**My suggested first set:** #1, #2, #4, #6 and **#15** (the user's own idea: staffing a team from requirements) (all S, they cover the user's three examples except hiring, and #1 and #2 make the phase-1 bug flow much better), then #3 and #5. ada's plan builds "the first two features" first: I'd pick **#4 Draft or improve a text** (one component, every text field) and **#1 Report a bug from a sentence** (the user's own example, and it exercises the full draft → form → confirm path).

## Not recommended (for now)

- **Acting on its own** (auto-triage, auto-assign, auto-closing duplicates): this breaks P1, and with no permissions in phase 1, its mistakes would be hard to see. Revisit with permissions and an undo log.
- **A chat sidebar on every screen:** it would be a second way to do everything, with no clear home. #4 and #13 cover the need.
- **The copilot as an org member** (in the chart, assignable): this confuses the copilot with agents, the product's core idea.
- **Reading the repository directly** for #8 and #13: tools break P2. If repo context is wanted, the office should prepare it (paths, file excerpts) as data.

## Open questions (for the user)

- Cost: show it per run (when the runtime reports it) and as a monthly total in Settings? Or is a monthly total enough?
- Which features run on which runtime when the user has two (Claude Code and Codex)? I'd keep one choice for all features in Settings.
