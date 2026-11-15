# Emoji inventory · apps/org-ui at origin/v1 `e3415e4` (2026-11-11)

Emery. Supports requirement E1-E4 in [`../prds/information-architecture-v2.md`](../../../information-architecture-v2.md) §8. Owner, 2026-11-11: "We shouldn't have emoticons in the system"; icons are welcome.

**Totals [F]:**
- 14 emoji in the product UI, on 10 source lines, all in **Chat** (`src/lib/modules/messaging/`).
- 0 in sample or mock data.
- No test asserts on a UI emoji. Tests find reactions by aria-label and match "Pinned" as text.

**Icon set:** the design system (Eden, design-system.md:36, :547) already bans emoji as UI and names **Heroicons** as the one set. The code still uses Lucide inside screens; Heroicons is used only in the sidebar nav. Replacements below are named in Heroicons.

## Emoji to replace

| Where | file:line (under `apps/org-ui/src/lib/modules/messaging/`) | Emoji | Replace with |
| --- | --- | --- | --- |
| Chat sidebar, "Needs you" | features/open-dm/ChatSidebar.svelte:109 | ⭐ | `star`. This item moves to Inbox under IA v2. |
| Chat sidebar, "Threads" | ChatSidebar.svelte:121 | 🧵 | `chat-bubble-left-right` |
| Chat sidebar, channel and group rows | ChatSidebar.svelte:92 | 👥 | `user-group` (groups), `hashtag` (channels) |
| Message kind badge | model.ts:56 `KIND_MARK`, rendered at components/MessageRow.svelte:85 | ❓ 📣 🔁 | `question-mark-circle`, `megaphone`, `arrows-right-left` |
| "Pinned" label | MessageRow.svelte:83 | 📌 | The word "Pinned" only (Heroicons has no pin) |
| Attachment chip | MessageRow.svelte:103; features/send-message/Composer.svelte:245 | 📎 | `paper-clip` |
| Fixed reactions "Seen" and "Looking" | types.ts:47-48 `REACTIONS`; MessageRow.svelte:111, :135 | ✅ 👀 | `check-circle`, `eye` |
| Ask, approval and hand-off card headings | features/answer-ask/AskCard.svelte:101 | ✅ ❓ 🔁 | The same icons as the kind badge, marked aria-hidden. Today the emoji are read aloud by screen readers. |
| Code comment | Composer.svelte:13 | 📎 | Delete it (not rendered) |

## Symbols used as icons (replace for consistency)

| Symbol | Where | Replace with |
| --- | --- | --- |
| × as a close button | Composer.svelte:247; components/TeamsChart.svelte:149; components/ProjectTeams.svelte:57 | `x-mark` |
| ↳ in "↳ 3 replies" | MessageRow.svelte:118 | `arrow-turn-down-right`, or delete it. Updates e2e/chat.spec.ts:226 and e2e/chat-live-delivery.spec.ts:109, :131. |
| › as a breadcrumb | routes/(shell)/projects/[id]/+page.svelte:28 | `chevron-right` |
| @ as the "Mentions" nav icon | ChatSidebar.svelte:115 | `at-symbol` |

**Kept as typography:** → in text ("A → B", "v2 → v3"), ≈, ⌘ in a screen-reader hint, the "·" separator, and "›" in team paths ("Engineering › Web").

## Decision for the owner

The design system allows emoji as **user content**: message text, and reactions a person picks. The fixed reaction set is UI, so it becomes icons under E1 either way. Still open (proposal Q7): what to do with emoji that agents or people type into messages.

## Not touched

Test files that use emoji as Unicode input data stay as they are: copilot/check.test.ts:32-33, messaging/engine.test.ts:462, employees/urls.test.ts:16, :28.
