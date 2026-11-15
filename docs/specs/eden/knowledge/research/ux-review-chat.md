# UX review · Chat (PR #1, W19)

Reviewer: Eden · 2026-11-08 · Build: `cloud/chat` @ `824799f`, from my own e2e run (chat.spec.ts, 17 passed): chat-channel, chat-thread, chat-needs-you, chat-as-owner, chat-search, chat-bridge-preview, chat-empty · Against: `docs/specs/chat-ux.md` (rev 4).

**Verdict (UX):** good, with one medium to fix before or right after merge.

## Good, keep it
- Sidebar: Needs you, Mentions and Threads on top, unread channels in bold with counts, and DMs with each agent's live status word (Working, Asleep, Paused).
- Kind markers in words plus icons (Announcement · Delivered, Hand-off, Ask), "↳ 3 replies", and threads in a side panel with "Also send to #channel".
- Needs you cards say who asks, where, how long it's waited, what it blocks, and the default with its countdown ("If no answer by 20:43, the default applies… (2 h left)"), with Open in conversation.
- Reading as the owner is explicit ("You're reading as the owner. You can reply here."), and so is the bridge preview (read-only, with asks and approvals opening in the app).
- Empty channel: a welcome card that says what Chat is for. Search with filters and a result count.

## Findings
| # | Severity | Finding | Fix |
| --- | --- | --- | --- |
| 1 | Medium | **A hand-off keeps Accept after it's accepted.** In chat-as-owner, Sam → Noor is answered "Accepted." by Noor, yet the card still shows **Accept** to the owner, who isn't the recipient. The spec (§2) doesn't define this either; my fix is in chat-ux rev 5. | Only the recipient (or the owner on a hand-off to the owner) sees **Accept**. Once accepted, the card reads "Accepted by Noor Haddad · 08:53" with no buttons; Reply stays in the thread. |
| 2 | Low | **A person's DM shows only a green dot** (Avery Stone), where agents get a word. | Add the word ("Online"), or no dot. Status is never color-only. |
| 3 | Low | **A spec id in user copy:** the sample ask's option reads "No, allow (Eden's AREA-7)". | Sample text: "No, allow (default)". |
| 4 | Low | **Search's filter help is one monospace line** that wraps mid-filter. | Show the filters as a short list or chips under the box. |
