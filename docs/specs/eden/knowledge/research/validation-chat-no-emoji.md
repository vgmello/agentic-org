# Validation · Chat no-emoji (v1 ef0ce00b vs cd1c2ce8)

Requested by ada (01M76EH3). Jules, 14 Chat emoji and Chat pseudo-icons → Heroicons, plus the no-emoji guard. Checked 12 Nov 2026.

**Verdict: met, with one S3 regression at 200% text.**
- All 14 Chat emoji and the Chat pseudo-icons are gone from the rendered UI in both engines.
  - A text scan found 206 hits before and 0 after. The only thing left is the "@ mention" keyboard hint, which is typography.
- Every new icon is Heroicons v2, hidden from screen readers, and sits next to a visible word or inside a named control.
- Accessible names are unchanged apart from the changes the contract asked for: card headings no longer read ❓✅🔁 aloud, and the replies button no longer starts with "↳".
- Emoji a person types are untouched.
- Reactions, attach, remove, asks and pins all still work by keyboard and pointer, with no console errors.

**New:**
- **One S3:** at 200% text, the Chat sidebar's "Mentions" and "Needs you" labels drop below their icons, and the Needs you count falls onto its own line.
- **Three S4s:**
  - the Announcement icon is orphaned from its word at 200%;
  - a sent attachment's paper clip is centred on a wrapped filename;
  - icons stay gray where §22 asks for the context colour.

**Pre-existing (not caused by this change), filed so they aren't lost:**
- the reaction chips and the message toolbar have no visible focus (S3);
- removing an attachment drops focus to the page (S4);
- Chat still mixes Lucide with Heroicons (deferred by the contract).

**Method:**
- Requirements:
  - my mapping to Jules (bell for Needs you; micro 16 for chips, badges and reactions; aria-labels; keep "Ask");
  - PRD §8 E1–E3;
  - design-system §17, §18, §20 and §22.
- Builds: before is cd1c2ce8 and after is ef0ce00b, each run as its own dev server in e2e mode with sample data ("Acme").
- Coverage: 3 parallel checks.
  - Icon mapping and names: Chromium and WebKit at 1280.
  - Layout and reflow: 2 engines × 320/640/960/1280 × 100/200% text × 2 builds = 64 runs.
  - Accessibility, keyboard and blitz: WebKit at 1280 and 640, each at 100% and 200%, plus Chromium at 1280/100.
- 200% text means the root font size set to 200%.
- WebKit is the Tauri engine; Chromium is the spot-check.

## Requirements (evidence I opened)

| Where | Before | After | Hidden from AT | Result |
| --- | --- | --- | --- | --- |
| Needs you | ⭐ | `bell`, outline 24 | yes | met |
| Threads | 🧵 | `chat-bubble-left-right`, outline 24 | yes | met |
| Mentions | @ | `at-symbol`, outline 24 | yes | met |
| Group rows | 👥 | `user-group`, outline 24 | yes | met |
| Channel rows | # | `hashtag`, outline 24 | yes | met |
| Kind badges Ask, Announcement, Hand-off | ❓📣🔁 | `question-mark-circle`, `megaphone`, `arrows-right-left`, micro 16; the words stay, "Ask" kept | yes | met |
| Pinned | 📌 Pinned | the word "Pinned" | n/a | met (see note) |
| Attachment chips | 📎 | `paper-clip`, micro 16 | yes | met |
| Reactions Seen, Looking | ✅👀 | `check-circle`, `eye`, micro 16; names "Seen, 1", "React Seen" unchanged | yes | met |
| Ask, Approval, Hand-off card headings | ❓✅🔁, read aloud | micro 16 icons; headings read the words only | yes | met |
| Composer Attach | Lucide paperclip, 14 px | `paper-clip`, mini 20 | yes | met |
| Remove attachment | × (8×20 px) | `x-mark`, mini 20 (20×21 px); name "Remove {file}" | yes | met |
| Replies line | ↳ 3 replies | "3 replies · …" | n/a | met |
| E3 guard | — | the no-emoji unit guard runs in the suite: `Tests 12 passed (12)` | — | met |
| Typed emoji | shown as typed | "🚀 👍 ✅ ⭐ ❤️ 🇧🇷 1️⃣ 👍🏽" kept code point for code point | — | met |

- **Sizes:** icon sizes match §22 in all 64 layout runs.
  - The icon sits within 0.5–2.2 px of the text centre wherever icon and label share a line.
  - No icon clips, and none overlaps its label.
  - No sideways scroll in any run.
- **Contrast:** every icon is at least 3:1 against its surface.

| Surface | Icon contrast |
| --- | --- |
| Sidebar | 5.95 (4.14 on the current row) |
| Cards | 5.1 |
| Reactions | 4.2 |
| Chips | 4.14 |
| Attach | 5.64 |

- **Blitz:** results were consistent on both builds, with no console or page errors.
  - 15 rapid Seen toggles.
  - Back and forward across Threads, Mentions, #general and Needs you.
  - Reload with a thread open.
  - In offline mode (`?chat=offline`), the reaction controls are disabled.
- **Note on Pinned:**
  - §22 maps "pinned" to `bookmark` when an icon is used.
  - The contract chose the word alone, which carries the meaning without an icon.
  - I accept that, so no row.

## Findings (verified on the screenshots myself)

| Id | Sev | Where | What I saw | Fix |
| --- | --- | --- | --- | --- |
| N1 | S3, regression | Chat sidebar, Needs you and Mentions, 200% text, every width, both engines | The row gained `flex-wrap`, and with the 24 px icon (48 at 200%) "Mentions" wraps under its icon and the Needs you count badge drops to its own line. Row height grows 72→120 px (Mentions) and 120→160 px (Needs you). Before, "@ Mentions" stayed on one line. §20: 200% must not reflow a label away from its icon. `shots/sidebar-1280-200-chromium-after.png` vs `-before.png` | **Superseded 12 Nov (BUG-94 pin with Jules; design-system §22):** the original fix here, `break-words` on the label, split words in the middle at 200%, so I withdrew it. The row is now the icon (`shrink-0`, capped at `max-w-[32px] max-h-[32px]`) followed by a content column (`min-w-0 flex-1`). The label wraps only between words, and the count wraps under the label, aligned to the start. This applies to channels, groups and DMs too. |
| N2 | S4, regression | Message kind badge (Announcement), 960/200% in both engines | The megaphone sits alone on a line above "Announcement". §22 says the icon stays with its word. `shots/kindbadge-960-200-chromium-after.png` | Wrap the icon and the kind word in one `inline-flex items-center gap-x-1 whitespace-nowrap` span; the rest of the meta line can still wrap. **Update 12 Nov (BUG-95):** at 320/200% the unit is 8.4 px wider than its column even on a line of its own. It also overflows at 960/200%. So the metadata group wraps by available width onto a full-width row under the avatar (design-system §22). |
| N3 | S4, regression | Attachment chip on a sent message: 320/100%, and 320/640/960 at 200% | `inline-flex items-center` centres the clip on the whole wrapped filename (10–60 px low). Before, it sat on the first line. `shots/sentchip-320-100-chromium-after.png` | `items-start` on the chip, with the icon nudged to the first line's centre (`mt-0.5` at micro 16 on `text-xs/5`). |
| N4 | S4 | Icon colour (§22: the icon takes the control's colour, `text-white` when current or pressed) | The `HeroIcon` default `text-gray-400` wins over the context colour. On the current Needs you row the icon is gray beside white text. On a pressed reaction chip the icon is gray beside the indigo count. In the kind badge it's gray-400 beside gray-300 text. `shots/after-webkit-announcement-pinned-reacted.png` | Let the icon inherit (`text-current`, or no colour class on the component) and colour the control. |
| P1 | S3, pre-existing | Reaction chips ("Seen, 1") and the hover toolbar (React Seen, React Looking, Reply, More), both builds | Keyboard focus on a chip shows no change: the static `outline-1 outline-white/10` stays. The toolbar shows the faint shadcn `ring-ring/50` (about 2:1). §17 asks for the indigo-400 2 px outline. `out/after-webkit-1280-100/focus-seen-chip.png` | Apply the §17 focus classes (`focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-indigo-400`) to both. |
| P2 | S4, pre-existing | Composer, Remove {file} | After Enter on Remove, focus drops to `<body>`. Attach and Remove use the browser's default ring instead of §17's. | After removal, move focus to the next chip's Remove, else the previous one, else Attach. Apply §17 focus. |
| F1 | follow-up | Chat composer, toolbar and headers | Lucide is still used for Add channel, New message, Reply, More, Send, Search and close; §22 says don't mix sets on one screen. The contract deferred these. In the hover toolbar, 16 px Heroicons sit beside 12 px Lucide. | A second pass: `plus`, `arrow-uturn-left`, `ellipsis-horizontal`, `paper-airplane`, `magnifying-glass`, `x-mark`. |
| F2 | info, pre-existing | Below 640 px | Reaction pills are 24 px tall and Attach 20 px, both below the §20 44 px target, on both builds. Remove got better (8×20 → 20×21). | Give them a 44 px hit area below 640 px (`relative` plus an `absolute -inset-…` span, as in §20). |

Not findings:
- The hover toolbar uses micro 16 for the React buttons, while my mapping said mini 20 for control buttons. §22 allows micro in dense toolbars, so I accept micro 16 there.
- A zero-width-joined emoji (👨‍👩‍👧) renders as three faces on both builds. That was already the case, and it's outside this change.

## Evidence

Scratch: `/private/tmp/claude-501/-Users-vgmello-projects-agentic-consulting-agents/bee357c5-98c7-42b2-984f-cfd50ca7c83b/scratchpad/chatnoemoji/`.

| Check | Folder | Contents | Final line |
| --- | --- | --- | --- |
| Icon mapping | `A/` | `scan.mjs`, `badges.mjs`, `zwj.mjs`, `results.json`, `aria-dump.txt`, about 80 shots | `done before-chromium: hits=206 …; after-chromium: hits=7 …; before-webkit: hits=206 …; after-webkit: hits=7` (the 7 are the @ keyboard hint) |
| Layout | `B/` | `measure.mjs`, `analysis.txt`, 164 shots | `done chromium: 2 combos, 0 with errors`; `done webkit: 2 combos, 0 with errors` (re-run of 320/200 after a harness wait fix) |
| Accessibility and blitz | `C/` | `run.mjs`, the `out/{before,after}-*` folders, 18 catalog shots | `RESULT … ok, 14 keys, 0 errors` on all 10 runs; `CATALOG ok: 18 screenshots` |

- All worktrees were removed and every server stopped.
- Nothing was edited in the product repo.
