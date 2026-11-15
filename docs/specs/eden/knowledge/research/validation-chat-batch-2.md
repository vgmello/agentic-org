# Validation · Chat batch 2 (v1 c1da4893)

Merges: 8fed87b (BUG-62/63 and refresh), be46acfb (65), 9bf3aaeb (66), f972bccc (67/68), 6623c0ac (71), 1c05b7eb (landmark), 39709507 (notices). Before: each merge's first parent. WebKit and Chromium at 1440, 960 and 390, 100% and 200% text. The merges' own specs: 122 of 123 passed; 1 Vite first-load flake passed 3 of 3 on rerun.

**Verdict: all seven met.** New findings are pre-existing in both builds.

| Merge | Result | Evidence (chatval/shots) |
| --- | --- | --- |
| 8fed87b | met. Each history entry has its own title; search adds one entry with `?q=`; Back returns to the conversation, Forward and reload restore; clearing adds no entry; the thread draft survives leaving Chat; BUG-63 controls disabled when the office is stopped | probe-history.json, probe-misc.json, probe-blitz.json |
| be46acfb BUG-65 | met. WebKit, mouse-opened thread at 1440, 960 and 390: Close and Escape return focus to the opener (before: the message list), also after a re-render | probe-focus.json |
| 9bf3aaeb BUG-66 | met. The log is `aria-relevant="additions"` and the pending row sits outside it, so only the delivered message is offered (before: "Sending…" row offered too). DOM evidence only | probe-live.json (I opened it) |
| f972bccc BUG-67/68 | met. No name truncation in the list at 200% (13 before); search 350 of 390 px (224 before); badges inside rows. Mid-word breaks ("platfor/m") in the 256 px list at 200% | probe-layout.json, probe-badge-wrap.json |
| 6623c0ac BUG-71 | met. With the Thread open, one offer from the thread log, and the channel copy is `aria-live=off`; with it closed, one offer from the channel (before: 4) | probe-live.json, live-also-send-after-webkit.png |
| 1c05b7eb | met. Conversation › Message area › Messages (before: two "Messages") | probe-live.json regions |
| 39709507 | met. No role, no live region, decorative icons, gray-300 passes 4.5:1. Nit: the boxes sit flush against the panel edges | callouts-webkit-1440.png |

VoiceOver speech wasn't sampled: AppleScript control of VoiceOver is off, and enabling it needs root and permission prompts. Live-region results are DOM checks.

## New findings (pre-existing, checked by me)
1. **S3 · a11y/layout.** At 200% text with the Conversations button showing (960×640, 390×844), the conversation heading shrinks to 6–12 px wide and shows only "#". I opened probe-title-badges.json: 6 px at 960, 12 px at 390, readable at 1440 (181–401 px). Expected: the title stays readable (WCAG 1.4.4, the intent of BUG-67). Related: at 960 and 200%, the `w-72` nav takes about 576 of 960 px.
2. **S3 · a11y.** Below 1280, Members covers the conversation (inert) with no dialog role. Back with Members open goes to the previous conversation, and focus lands on BODY. Expected (SHELL-28, CHAT-33): close only, focus back on Members. probe-members-thread-back.json.
3. **S3 · a11y.** Back with a Thread open (below 1280) closes it, but focus goes to BODY, not the opener. BUG-65 covered only Close and Escape. Same JSON (`focusIsOpener: false`).
4. **S4.** A refresh delivering 400 messages offers 400 announcements, each ending with the hidden quick-reaction text "✅ 👀". Expected: one summary ("400 new messages"), with reaction buttons out of the announced text.
5. **S4.** "Add channel" opened from the Conversations overlay, then Back: focus goes to Conversations, not Add channel (the overlay has already closed).

Findings 2 and 3 belong with BUG-78 (Back with other modals) as one SHELL-28 fix.
