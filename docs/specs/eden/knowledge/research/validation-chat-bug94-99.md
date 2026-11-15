# Validation · Chat BUG-94..99 (v1 053702ef vs 64a68813) and BUG-123 (v1 d49446c0 vs a65422e9)

Requested by ada (01M777Q4 and 01M77BD6). Checked 12 Nov 2026.
- Chat: Jules's jules/chat-bug94-99, merged as 053702ef (Tier B, fresh Sol zero).
- BUG-123: Avery's avery/fit-tooltip, merged as d49446c0 (Tier B, Sol zero).

**Verdict: met. BUG-94, BUG-95, BUG-96, BUG-97, BUG-98, BUG-99 and BUG-123 are all fixed in both engines, with no regressions.**
- **Sidebar rows (BUG-94):** at 200% and at a 40 px root, the label starts beside its icon in every large-text case (6 rows at 320–1280 px, 200% and a 40 px root, both engines). The icon is capped at 32 px and never shrinks. The count sits on the label's line or under it, aligned to the start.
- **Kind badge (BUG-95):** the icon and the kind word share a line in every case. The metadata group follows the message row's own width at 16, 32 and 40 px roots, with 0 mismatches in 6 sweeps.
- **Attachment clip (BUG-96):** the paper clip sits on the first filename line, 0 to 0.5 px off.
- **Icon colours (BUG-97):** icons take the control's colour.
- **Focus (BUG-98, BUG-99):** reaction chips, the hover toolbar, Attach and Remove show the §17 outline. Removing an attachment moves focus to the next Remove, then Attach.
- **Fit explanation (BUG-123):** it now appears when the chart reports its limit after focus has arrived. That passed 10/10 per engine on after and 0/6 on before; a real resize race passed 5/5 on after and 0/5 on before. Escape is two-stage, 20/20.
- **Nothing lost:** no emoji in the UI, the same accessible names, and reactions, replies, pins and asks all work. The blitz had 0 console or page errors.

**New:** nothing.

**Pre-existing, not caused by these changes, filed so they aren't lost:**
- **S3:** the Chat header scrolls sideways at 320 px.
- **S3:** the Ask card's option buttons overflow the message at 200%.
- **S3:** the "+ New message" and "+ Add channel" icons sit alone on a line at large text.
- **S4:** the Fit explanation runs flush with the viewport edge and covers View.
- **S4:** the Fit explanation disappears once the blocked item is activated.

**Known and excluded:**
- **BUG-119:** at 960 px with a 40 px root the message row is 180 px wide. Bodies overflow, though the metadata drops correctly.
- **BUG-101:** the keyboard-highlighted menu item has no outline. Jules's fix is in progress, and the pin is in §13.
- **BUG-129:** seen once on before, and not on after in 2 runs. That's too few runs to call it fixed.
- **BUG-130:** didn't occur.

**Method:**
- Requirements:
  - ada's BUGS rows BUG-94 to BUG-99 and BUG-123;
  - my §22 pin for BUG-94/95 (one rem threshold local to MessageRow);
  - Avery's BUG-123 pin (`contract-pin.json`);
  - design-system §13, §17, §20 and §22.
- Builds: each before and after ran as its own dev server in e2e mode with sample data ("Acme"). WebKit is the Tauri engine; Chromium is the second engine.
- Three parallel checks:
  - **Chat requirements, behaviour and accessibility:** WebKit and Chromium at 1280/100%, WebKit at 640/200%, plus BUG-96 at 320/100% and 960/200%, on both builds.
  - **Chat layout and reflow:** 2 engines × 320/640/960/1280 × 100%, 200%, a 20 px root and a 40 px root × 2 builds (32 cases per engine), plus 6 threshold sweeps (16, 32 and 40 px roots, 160 to 1440 px in 8 px steps) and long-name stress at 320 and 960 at 200%.
  - **BUG-123:** 2 engines × 320/960 × 100%, 200% and a 20 px root × 2 builds, with 1280/100% as the wide-layout guard (28 cases), plus late-measurement repeats and a real resize race.
- "200% text" means the root set to 200%. A "20 px default" or "40 px root" means the computed root size. WebKit keyboard walks use Option+Tab.

## Requirements (evidence I opened)

| Req | Before | After | Result |
| --- | --- | --- | --- |
| BUG-94: label beside its icon at large text | The label drops below the icon at 200% and at a 40 px root, and the Needs you count falls under the icon | Beside the icon in every large-text case (6 rows at 320–1280 px, 200% and a 40 px root, both engines). Labels wrap between words, and the count goes on the label line or under it, start-aligned (`B/shots/webkit-after-320-r40-sidebar.png`) | met |
| BUG-94: icon ≤32 px, never shrunk | 48 px at 200%, 60 px at a 40 px root | 32 px SVG, never shrunk | met |
| BUG-95: kind icon and word together | Orphaned in 10 cases | 0 orphaned | met |
| BUG-95: metadata flip follows the row's own width | No flip at all | The flip happens at row width ≤ 10.765625rem (172.25, 344.5 and 430.63 px), with 0 mismatches in 6 sweeps. At 32 and 40 px roots it flips back and forth as the shell changes the column width, which shows it follows the row and not the viewport | met |
| BUG-96: clip on the first filename line | 20 px low at 320/100% and 140 px low at 960/200%; 28 of 32 layout cases fail | 0 px (0–0.5 px), 0 of 32 fail (`A/out/after-webkit-960-200-h1600/96-sent-attachment.png`) | met |
| BUG-97: icon colour | gray-400 everywhere (Needs you current row 4.35:1) | Inherits: white on the current row (11.31:1), the chip's indigo when pressed (8.93), gray-300 in kind badges (9.96), and Attach turns white with its text on hover (14.67) (`A/out/after-webkit-1280-100/97-needs-you-row.png`) | met |
| BUG-98: chips and toolbar focus | Chips: static 1 px white/10. Toolbar: a faint box-shadow ring | Solid 2 px indigo-400 with offset 2 and no ring, on `:focus-visible` (`A/out/after-webkit-1280-100/98-React-Seen.png`) | met |
| BUG-99: focus after Remove | Focus goes to `body`, and later Enters do nothing | Middle removed → next Remove; last removed → previous Remove; only one removed → Attach | met |
| BUG-99: Attach and Remove outlines | The browser's default ring | §17 outline | met |
| BUG-123: explanation on focus and hover | Shown when the measurement arrives first | Shown in both cases, text exact, 0 of 5 points covered, 20.13:1 contrast | met |
| BUG-123: measurement arrives after focus | 0/6 per engine; real race 0/5 | 10/10 per engine; real race 5/5 (3 of 5 actually raced) | met |
| BUG-123: one item, `aria-disabled`, `aria-describedby` set to the reason | yes | yes | met |
| BUG-123: two-stage Escape, outside click | yes | 1st Escape closes the explanation and focus stays on the item; 2nd closes the menu and focus returns to View (20/20); an outside click closes everything | met |
| BUG-123: Fit when available | yes | Fits, and the label toggles to "Actual size". At 200% and a 20 px root the chart already reaches 12 px by scaling, so Fit is available there | met |
| No emoji, typed emoji kept | 0 | 0 in 7 states; typed emoji kept | met |
| Accessible names | — | ariaSnapshot identical apart from a clock string | met |
| Reactions, replies, pins, asks, blitz | ok | ok; 0 console or page errors | met |

## Findings (verified on the screenshots myself)

| Id | Sev | Where | What I saw | Fix (outcome) |
| --- | --- | --- | --- | --- |
| P1 | S3, pre-existing | `/chat?c=c-general` at 320 px, both engines | The Conversation section scrolls sideways: by 2 px at 200% (322 vs 320) and by 82 px at a 40 px root (402 vs 320). The header's title and its "Conversations" and Search controls don't reflow. `B/shots/webkit-after-320-r40-sideways.png` | The Chat header reflows inside its column at 320 px at every text size, with no sideways scroll (§20). |
| P2 | S3, pre-existing | Ask card in `#atlas` and Needs you, at 320/640/960 at 200%, and 1280 at a 40 px root | The option buttons run past the card: "No, allow (def…" is clipped. `B/shots/chromium-after-320-200-kind-Ask.png`. Also, in Chromium only at 320 with a 40 px root, the Announcement's body link "the branch" overflows the row by a few px | The option buttons wrap inside the card and each label shows in full (§20). |
| P3 | S3, pre-existing, partly improved | Chat sidebar "+ New message" and "+ Add channel", at 200% and a 40 px root | The plus sits alone on a line above its words (§22). After the change, Add channel is fixed at 200% but still orphaned at a 40 px root; New message is orphaned in every case. `B/shots/addrow-webkit-after-320-200pct-New-message.png` | The same icon-plus-content-column layout as the other sidebar rows (§22, BUG-94). |
| P4 | S4, pre-existing | People view menu, Fit explanation, 320/960/1280, both engines | The tooltip sits flush with the viewport's right edge with no gutter ([0,320] at 320, [960,1280] at 1280), and it covers the View trigger. `C/catalog/org-chart-view-menu-desktop.png`, `C/shots/chromium-after-320-100-k-tip.png` | The explanation keeps at least a 16 px gutter from the viewport edge and doesn't cover the trigger the menu came from (§13). |
| P5 | S4, pre-existing | People view menu, the blocked "Fit to width" activated with Enter or a click | The menu stays open but the explanation disappears. Moving focus away and back brings it back. The code comment says the description should stay available after activation. `C/shots/*-k-after-enter.png` | Activating the blocked item keeps its explanation visible: that's the moment the person needs it. |

Not findings:
- **The toolbar focus outline** fades in over the shared Button's `transition-all` (0.15 s; exact indigo-400 by 250 ms). That's BUG-120, which ada classified as a test-sampling issue with no production change. The settled outline meets §17.
- **The DM avatar** is 48 px at 200% (60 px at a 40 px root). §22's 32 px cap is for icons. An avatar is a person's identity and scales with the text, so it stays as it is.
- **Channel names** such as "engineering" and "onboarding-kit" break inside the word at 320 px with a 40 px root. Each is a single word wider than the whole column, and §6 lets names people typed break anywhere, so this is the right fallback.
- **Long filenames** wrap as "attachment / -filename-" (`overflow-wrap:anywhere`). §6 allows that for names people typed.
- **The People menu item** highlighted by keyboard shows the background only. That's BUG-101, which Jules is fixing to the §13 pin.
- **BUG-129** (WebKit, 20 px root, 320: Escape sends focus to `body`) was seen once on before and wasn't seen on after in 2 runs. It needs 20 or more runs before anyone calls it fixed.

## Evidence

Scratch: `/private/tmp/claude-501/-Users-vgmello-projects-agentic-consulting-agents/bee357c5-98c7-42b2-984f-cfd50ca7c83b/scratchpad/chat94/`.

| Check | Folder | Contents | Final lines |
| --- | --- | --- | --- |
| Chat requirements, behaviour, accessibility | `A/` | `scripts/`, `out/` (16 run folders with `result.json`, aria YAML and PNGs), `summary.txt`, 3 catalog shots | `DONE {before,after}-{webkit,chromium}-1280-100: steps=16 stepErrors=0 console=0 pageErrors=0`; `DONE {before,after}-webkit-640-200: steps=16 stepErrors=0 console=0 pageErrors=0`; `CATALOG DONE: desktop badgeVisible=true attached=2` |
| Chat layout and reflow | `B/` | `results/matrix-*.json`, `threshold.json`, `stress.json`, `summary.txt`, 170+ shots | `DONE webkit: 32 cases, 22 flagged`; `DONE chromium: 32 cases, 22 flagged` (excluding 960/40, 15 cases per build per engine: 13 flagged before, 7 after; every after-flag is the DM avatar, P1 or P2); `DONE threshold: 6 sweeps, 0 samples disagree with containerWidth <= 10.765625rem`; `DONE stress: 8 cases, 8 flagged` (after: DM avatar and the P1 header only); `DONE addrows: 32 checks, 28 icon orphaned` |
| BUG-123 | `C/` | `results-matrix-*.json`, `results-late*.json`, 214 shots, 2 catalog shots | `matrix {chromium,webkit} {before,after}: 7 cases, 0 errors`; `late {chromium,webkit} after: 10/10 pass, esc2 to trigger 10/10`; `late {chromium,webkit} before: 0/6 pass`; `late-natural {chromium,webkit} after: 5/5 pass, raced 3/5`; `late-natural before: 0/5 pass` |

- Each worktree got its own `bun install --frozen-lockfile`.
- All servers are stopped and all worktrees removed. Nothing in the product repo was edited.
