# Validation · Batch 1: focus, Offices and Chat bugs in three engines

Requested by ada on 13 Nov. This is the first batch under the owner's per-milestone rule: findings go to the authors as bugs. I checked it on 13 Nov 2026, following the method in `knowledge/playbooks/product-validation.md`.

**Scope:**
- `62efec16`: BUG-101 and BUG-102 (Jules).
- `29530a93`: BUG-126, 127 and 128 (Avery).
- `34ed1b29`: BUG-150 (Avery).
- A status for BUG-81, 82, 91 and 92 (A-OF1, `64a68813`) and BUG-94 to 99 (Chat, `053702ef`).

**Engines and commits:**
- **Chromium and WebKit:** I validated each slice on 12 Nov, at its own merge against its first parent. The reports are listed under Evidence.
- **Firefox:** it was added on 13 Nov, so I ran it at v1 `f9946e11`. All five fix SHAs are ancestors of that commit.
- Between each slice's merge and `f9946e11`, 9 to 29 commits touched `apps/org-ui/src`. Chromium and WebKit weren't re-run at `f9946e11`.
- I ran one engine and one worker at a time.

**Verdict:**
- All 16 bugs met their criteria: in Chromium and WebKit at each slice's merge (12 Nov), and in Firefox at `f9946e11` with no criterion failure. Firefox ran at that tip only, with no first-parent comparison, so it can't tell new from pre-existing.
- Limits:
  - Chromium and WebKit weren't re-run at `f9946e11`.
  - Firefox used a 32 px root, never the 40 px one (a 20 px default at 200%). At 960 with a 40 px root, A-OF1's `extreme_layout` stays not met: long statuses truncate and the page scrolls sideways. That's deferred to BUG-119 (validation-a-of1.md l.11, BUGS.md l.156).
  - Firefox focus covered 2 widths.
  - BUG-98: Tab reached 44/48. It never reached the pressed chip at 200%.
- In the Status column, "fixed" means met under these limits.
- No new bug rows. N2, the Input focus ring (S3), is open item F-8 under BUG-70; its invalid-state variant is new evidence.
- N1, the Chat Members panel, adds a symptom to BUG-119.
- BUG-143 still reproduces, and it's still open in BUGS.md.

## Status per bug

I recomputed the Firefox counts from the raw files listed under Evidence.

| Bug | Fix | Chromium, WebKit (12 Nov) | Firefox at f9946e11 | Status |
| --- | --- | --- | --- | --- |
| BUG-81 | `64a68813` | met (validation-a-of1) | 60/60 rows: the name contains the visible location; sheet "This Mac (you're here)" 30/30, "the office's machine" 0 | fixed |
| BUG-82 | `64a68813` | met (validation-a-of1) | 12/12 countdown samples (the other 20 of 32 are after the restart): the row's spoken name contains the visible countdown, with no "shortly" and no clock time; 32/32 samples match overall | fixed |
| BUG-91 | `64a68813` | met (validation-a-of1, which accepted an ellipsis; Firefox checked the stricter full word) | 60/60 rows: the full status word inside the row, 320–1280 px, 100% and 200% | fixed |
| BUG-92 | `64a68813` | met (validation-a-of1) | 60/60: icon and word on one line; pill ≤ 48 px at 200% | fixed |
| BUG-126 | `29530a93` | met (validation-offices-followup) | 30/30 pages: one indicator, 2 px indigo-400 inset 2 px; button outline none | fixed |
| BUG-127 | `29530a93` | met (validation-offices-followup) | 60/60: pill › Retry › chevron, chevron flush with the row end | fixed |
| BUG-128 | `29530a93` | met (validation-offices-followup) | 60/60: no line starts or ends with "·"; header and rows share one left edge | fixed |
| BUG-150 | `34ed1b29` | met (validation-bug-150) | 60/60 lines end-aligned. The chevron shares the previous item's line in 55/60. In the other 5 it doesn't fit, so it sits alone, end-aligned, on the next line. That is the "fits" reading Rowan confirmed (validation-bug-150.md l.7, message 01M783JY1F). The 5 rows are the 3 Studio "Status unknown" rows at 320/200 (pill alone) and Unreachable HQ at 960/200 and 390/200. For the HQ rows the probe's `fits` flag is wrong: at 960 the group is 345.6 px and pill + Retry + chevron need 384.8 px | fixed |
| BUG-101 | `62efec16` | met (validation-focus-101-102) | 74/74 items: View, Team ⋯ and Message menus 62 (results2.json), task-card ⋯ 12 (p3 logs). Keyboard outline 2 px indigo-400 at −2 px, 4.03:1; hover shows 0 outlines | fixed |
| BUG-102 | `62efec16` | met (validation-focus-101-102) | 67/67 Buttons by keyboard: 2 px indigo-400 at 2 px, ≥ 4.69:1. 39/39 pointer-focused Buttons have no outline (51 probed, 12 never took focus at 200%). In the dev fixture only: link 24/24, `aria-invalid` 4/4 with the red cue kept. Settled by 200 ms in 48/48 (p3 logs) | fixed |
| BUG-94 | `053702ef` | met (validation-chat-bug94-99) | 152/152 sidebar rows: the icon shares the label's first line, and counts stay inside the row | fixed |
| BUG-95 | `053702ef` | met | 16/16 Announcement badges: icon beside a one-line word | fixed |
| BUG-96 | `053702ef` | met | 8/8: the clip is centred on the first filename line | fixed |
| BUG-97 | `053702ef` | met | 40/40: icon colour equals the control's colour | fixed |
| BUG-98 | `053702ef` | met | 48/48: §17 outline on reaction chips and the toolbar, ≥ 4.28:1. Tab reached 44/48. The pressed chip at 200% (4 widths) was focused by script after 120 Tab presses, so those 4 rows show the outline but don't prove Tab reaches the chip | fixed; Tab reach to the pressed chip unproven at 200% |
| BUG-99 | `053702ef` | met | After Remove, focus moves to the next attachment, then the previous one, then Attach: 24/24 by keyboard, 16/16 by pointer. Outline on Attach and Remove 16/16, ≥ 3.41:1 | fixed |

Firefox coverage:
- Offices: 5 widths, 100% and 200%, 3 office states (30 pages, 60 rows).
- Focus: 1280 and 390, 100% and 200%.
- Chat: 1280, 960, 390 and 320, at 100% and 200% (the per-bug counts are in the table). Focus ran at 1280 and 390, and neither covered 800×600.
- Out of scope: the playbook's vision-lens sweep and screen catalog. This batch re-checks named bugs only.
- The root was 32 px in every 200% run.
- I measured horizontal scroll in Offices (0/30 pages, 0/60 rows) and Chat (0/32), and emoji in Chat (0/16). The focus runs measured neither.

## Findings

**N1 · S3 · a11y, layout · origin not determined · Chat › Members panel, proposed for BUG-119 (re-pointed to IA-01)**
- Repro: Firefox, 960×640, 200% text. Open #general, then Members.
- Expected: no clipping, and no word broken mid-letter (design system §20 and §14).
- Actual:
  - The text column is about one character wide.
  - "· Offline ·" stacks one letter per line.
  - No name is visible.
- Evidence: `chat/shots-run1/bug98-toolbar-960-200.png`, a single capture, cropped to the panel, from an earlier run. The run's config was 960×640 at 200%, but the crop alone doesn't show that, and the results file doesn't record the panel opening. I haven't checked Chromium, WebKit or the first parent.
- Also related: BUG-40 (BUGS.md l.49, closed): at 960×640 the Members panel squeezed the conversation. N1 may be a regression of it. BUG-76: the status word breaking letter by letter.
- Likely cause, inferred and not measured: BUG-119's rem-sized sidebar. BUGS.md l.191/194 already record a different Members symptom under BUG-119: a 24 px outline clip at 960 with a 40 px root and with the 20 px default at 200%. N1 is text collapse. With this run's 32 px root, `w-72` is 576 px, which leaves a 384 px main column. Add the Members panel as a new symptom there, not as a new row. BUG-119 already records 960/200 (New task, P3).
- The same earlier run (`chat/results-run1.jsonl`) had 5 BUG-98 failures at 960/200: 4 toolbar items and the pressed chip were never focused after 120 Tab presses. That was the run with Members open by mistake, so I discounted it and re-ran. See the BUG-98 row for the pressed chip in the final run.

**N2 · S3 · a11y · adds to BUG-70 F-8 (bug-70-per-screen-gaps.md l.49, ON-5 l.64, DL-4 l.204) · New task dialog › Title (Input primitive)**
- Repro: open New task and submit with the Title empty. Press Shift+Tab, then focus Title; the probe used `title.focus()`, with `:focus-visible` true.
- Expected: the Input focus outline from §11 and §17, which is 2 px indigo-400 inset (`-outline-offset-2`), with the §11 error outline `outline-red-300` kept as the invalid cue.
- Actual: no outline in 4/4 configurations. Only the old shadcn ring shows: a 3 px red shadow at 40%. `input.svelte` at `f9946e11` still uses `focus-visible:ring-3` and no outline. This is pre-existing: BUG-102 covered Buttons only. F-8 already tracks it, and the new evidence is the invalid state.
- Evidence:
  - `focus/shots/firefox-{1280,390}-{100,200}-newtask-invalid.png`
  - `focus/p3.log` l.21 and `focus/p3b.log` l.10, 20 and 30: "product invalid field none 3px".

**BUG-143, still open (no new row):** at 200% the New task dialog can't scroll in Firefox.
- The top is cut off by −135 px at 1280 and −228 px at 390, and Create task sits below the screen.
- At 390 the dialog is also clipped on the right. That sideways overflow belongs to BUG-144, not BUG-143: the Create task box is 571 px wide in a 390 px viewport (p3b). Add 390/200 to BUG-144, which records only 320.
- The Chromium 1280/200 screenshot looks the same. I didn't measure Chromium.
- Evidence:
  - `focus/shots/firefox-390-200-newtask-dialog.png`
  - `focus/shots/chromium-1280-200-newtask-dialog.png`
  - p3 logs
- Add 1280/200 and 390/200 to its ranges.

**Not findings:**
- The Needs you count wraps under its label at 200% but stays inside the row. I accepted that for BUG-94 on 12 Nov.
- The Button outline's first frame is partly faded: 2–3 px at about 50% opacity, 3.88–3.92:1. It settles by 200 ms. This is the 0.15 s transition that BUG-120 records.

## Evidence

- **Reports from 12 Nov:**
  - `validation-a-of1.md`
  - `validation-offices-followup.md`
  - `validation-bug-150.md`
  - `validation-focus-101-102.md`
  - `validation-chat-bug94-99.md`
- **Raw Firefox data from 13 Nov**, under `/private/tmp/claude-501/-Users-vgmello-projects-agentic-consulting-agents/bee357c5-98c7-42b2-984f-cfd50ca7c83b/scratchpad/b1ff/`:
  - `offices/results.jsonl` and `offices/shots/`
  - `focus/results2.json`, `focus/p3.log`, `focus/p3b.log` and `focus/shots/`
  - `chat/results.jsonl`, `chat/results-run1.jsonl`, `chat/shots/` and `chat/shots-run1/`
- **Process note:** one focus follow-up run started at load 86 without the load-40 wait. Its results agree with the other runs.
