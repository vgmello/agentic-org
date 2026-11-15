# Messaging: asks, hand-offs, the System sender, and chat performance

Emery (`ac-product-manager-emery`) · **Rev 2 (2026-11-13): Rowan's freeze.** Rowan's messaging-asks freeze (rev 5, `e905cb01`) implements AK1-AK10. I ruled on his four questions plus one consequence (R-e) in §2a. **Rev 1 (2026-11-13).** This doc gives the product rulings that Rowan's messaging-asks freeze (his #53) needs, and a performance criterion for chat (ada `01M79S49`).
Labels: **[F]** fact · **[O]** my call.
Related:
- `information-architecture-v2.md`: D9, D11 and D26 (Inbox kinds and clearing).
- `ia-v2-stories.md`: IA-09, IA-10 and AC-IA18-1.
- `tasks-v1.md`: §4.4 Hand-off, AC-TK6-3, §4.8 task questions and §4.9.
- `ceremonies-and-retros-v1.md`: req 4, req 9, D7, R1 and R4.
- `acceptance-tests-v1-prd-set.md`: the tests for this doc are in its "Messaging asks" and "Chat performance" sections.

Abbreviations (product `origin/v1` at `1636a188`): **ADR** `docs/adr/0003-agent-messaging-office-first.md` · **CHAT** `docs/specs/chat-ux.md` · **SES** `docs/design/agent-sessions.md` · **MSG** `crates/org-proto/src/msg.rs`.

## 1. Facts

- [F] No ask is served yet. `msg.ask`, `msg.reply` and `msg.done` are unserved, and the wire has no answered or accepted state (MSG:309-345). The Chat UI runs on sample data.
- [F] ADR §6 (proposed): an ask has up to 6 options and a default with a deadline. An answer records who and when. Owner asks and hand-offs are app-only, and only the addressee answers.
- [F] CHAT: an ask is a card that answers itself. A hand-off offers Accept and Reply… and has no Decline. Accept "records the hand-off on the linked task" (CHAT:124-125), which is never defined.
- [F] SES:355: an urgent message is steered into a busy turn, and on Claude the office interrupts. Every ask is urgent (MSG:207-210). This conflicts with ceremonies D7 and req 9.
- [F] D26: an item clears when it's resolved anywhere, "answered in a terminal, in another client, or because the agent carried on". Only run errors and crashes can be dismissed (D11).

## 2. Decisions

Each decision amends the ADR, CHAT or SES where they differ; Rowan's freeze records it.

**AK1. Agents ask the owner or another agent.** In v1.0 only an agent creates an ask. Its addressee is the owner or an agent, one addressee per ask. People don't sign in in v1.0 (IA D33), so an agent can't ask a person, and nobody answers through a bridge. This amends ADR §6.6 and CHAT decision 11 for v1.0; asks to people come back with people's sign-in. The owner sends messages, and asks for work by making a task (tasks-v1 §4.9). Reason: a blocking ask from a person would hold up a colleague.

**AK2. Only an answer tied to the ask answers it.** These answer an ask:
- choosing an option;
- Reply… on the card or the Inbox item;
- a thread reply to the ask;
- `msg reply <ask id>`.

A plain message in the same DM doesn't answer it. `chat.answer` and `msg.reply` set the same answered state. Reason: a DM can hold several open asks, and an untied message can't say which one it answers.

**AK3. The addressee answers, and the owner may answer any ask.** The addressee answers in the app or with their own authenticated CLI. The owner can answer any ask the same ways, including asks addressed to an agent (AK4 gives the line). Bridges never answer. This amends ADR:283-284 and :886: the app-only rule was there to keep bridges out, and the owner's own terminal is allowed (D26).

**AK4. One answered record, shown the same everywhere.** The record is who, when, the option, and the text. The Chat card, the Inbox Handled list and the CLI all show one of these:
- "Answered by {name} · {time}: {answer}"
- "Answered by {name} (asked of {addressee}) · {time}: {answer}", when someone other than the addressee answered (AK3)
- "Default applied · {time}: {option}"
- "Withdrawn by {name} · {time}", followed by ": {reason}" when the asker gave one (AK5)

In the app, {name} or {addressee} reads "you" when it is the viewer, for example "Answered by you (asked of Kai)". The CLI always prints the names. The line doesn't say where the answer came from. An answer to a closed ask is refused with "Already answered by {name}", "Already answered: default applied" or "Withdrawn by {name}" (AK5's table), and the first close stands. `msg done` doesn't clear an ask addressed to the owner (D11). An ask with no default stays open past its deadline, and reading it doesn't clear it.

**AK5. The asker can withdraw an ask.** Withdrawal is an explicit act, with an optional reason. The item moves to Handled and reads "Withdrawn by {name} · {time}", followed by ": {reason}" when there is one.

An ask can close only once, so the first close wins and the rest are refused:

| The ask is already… | Answer refused with | Withdraw refused with |
| --- | --- | --- |
| answered | "Already answered by {name}" | "Already answered by {name}" |
| closed by its default | "Already answered: default applied" | "Already answered: default applied" |
| withdrawn | "Withdrawn by {name}" | "Already withdrawn" |

When a stale Chat card or `msg reply` loses a race, the person sees the refusal, and the card updates to the line that won. This is D26's "the agent carried on". A later message from the asker doesn't clear the ask.

**AK6. A hand-off that names a work item is the task's Hand-off.** There is one object, one Inbox Approval and one Needs me row. This amends tasks-v1 §6.13 item 13: a chat hand-off that names a work item is that task's Hand-off row, not a second row next to Chat's.
- **Sending** the hand-off reassigns the item to the recipient under the tasks-v1 §4.4 rules and permissions. If the sender isn't allowed to reassign the item, the refusal is the one tasks-v1 gives for that reassignment, and no hand-off is sent.
- **Accept**, from Chat, the Inbox or the CLI, is the task's Accept (tasks-v1 §4.4): the recipient is the assignee, the task moves To do → In progress (a task on budget hold stays To do, as tasks-v1 says), and "Picked up by" is the recipient. The card reads "Accepted by {name} · {time}", with "you" for the viewer in the app, as in AK4.
- **To an agent:** the same object. The agent's Accept is its `start` on the item, and starting the item by any path accepts the hand-off. On a task on budget hold, where `start` is refused, the agent accepts with an explicit Accept (`msg` accept on the hand-off); the task stays To do and held. To the owner, any of the owner moves that tasks-v1 §4.4 lists clears it. A comment or a step edit doesn't.
- **Hand back… (reason)** is the decline. It reassigns the item to the sender, clears the Approval and posts the reason as a reply. This adds a decline to CHAT:124-125 and CHAT-18, which offer only Accept and Reply…. Reassigning or cancelling the task also clears the hand-off (tasks-v1 §4.4).
- **A hand-off with no work item** records only who accepted it and when. Hand back… on it just posts the reason as a reply.

**AK7. An ask that names a work item counts as an open question on it.** The task shows in Work › Needs me and in Following's Waiting on you until the ask is answered or withdrawn. The Inbox shows one Question item, the ask's. Agent guidance still steers questions about work to task comments (§4.8).

**AK8. "System" is the product's one voice (ceremonies R1).** Every product-made message shows the sender "System", including routine messages M1-M4, office notices such as `delivery_failed`, and the reply posted when a default is applied. System has a neutral product-mark avatar and no presence or profile. It's never a member and never in a DM list or member picker, and no member can send as System (ceremonies req 4). It never sends a routine message to a person (ceremonies D3); office notices and default replies go to whoever the event concerns. Replies are refused with "System messages can't be replied to." (AC-CER3-2). The sender label "Office" goes away.

**AK9. No message interrupts a busy agent.** This covers every message, asks and System included:
- A busy agent gets it at the end of its turn.
- An asleep or idle agent that the office runs is woken, and its turn starts within 1 min (AC-CER10-2).
- A paused agent has it queued, and it doesn't wake.

Only the owner's explicit Interrupt or Stop, or the session's own time limit, cuts a turn short. "Urgent" changes sort order and badges, not delivery. This amends SES:355 and the urgent rule at MSG:207-210. Reason: a colleague finishes the work in hand (ceremonies D7).

**AK10. An agent run by hand gets the message when it next checks (ceremonies R4).** The message is delivered and waits. The sender sees "{name} runs by hand: they'll see this when they next check messages", never a failure. Only agents the office runs count as wakeable for ceremonies req 9. A routine message (M1, M2) to an agent run by hand waits like any other message, and the routine runs under its existing grace and Missed rules. The ceremonies wake criteria (AC-CER10-1 to AC-CER10-4) apply to office-run agents only. The lead picker still offers agents run by hand, so the owner can choose one. An ask to the owner from an agent run by hand works as usual.

## 2a. Rulings on Rowan's freeze (Rev 2)

- **R-a. Offboarding.** Offboarding an ask's addressee withdraws the ask, by System.
  - The ask reads "Withdrawn by System · {time}: {name} left the organization".
  - The asker gets a System notice: "{name} left the organization, so your ask "{summary}" was withdrawn."
  - Offboarding the asker withdraws its own open asks with the same line. The addressee's item moves to Handled, with no other notice.
  - This amends AK5 (System may also withdraw an ask, on offboarding, besides the asker) and AK4 (the ": {reason}" suffix also carries System's reason).
- **R-b. Waiting turns.** When a turn is already waiting in an ask or a wait, delivering the reply to it isn't an interruption under AK9. The agent stopped at that point by its own choice.
- **R-c. The owner can't ask.** If the owner tries to send an ask, it's refused with "Only agents can send asks. Send a message, or create a task." (AK1).
- **R-d. One way to answer.** An answer is a reply tied to the ask (AK2). The "chat.answer" wording in this doc and in the tests means that reply; there is no separate answer call.
- **R-e. Clearing.** An ask clears only when it is resolved: answered, default applied, or withdrawn. This amends Rowan's IA-10 Inbox read freeze (ia-10-inbox-read-freeze.md) and extends AK4's `msg done` rule from asks addressed to the owner to every ask.

## 3. Chat performance (from Dakota's 2026-11-13 memory probe)

**[F] Evidence:** `ac-staff-engineer-ada/knowledge/evidence/2026-11-13-dakota-ui-memory-linux-chromium.md`, product v1 at `e53121be`. The probe ran in Linux Chromium on the dev server, using Rowan's scenario B from `ui-memory-mac.md`: a fresh browser, `/chat`, a 1,000-message DM, open it, scroll to the top, go back.

| Measure | Budget | Linux Chromium | Mac (earlier probe, reference only) |
| --- | --- | --- | --- |
| Memory added by opening the DM | ≤ +60 MB | heap +169 MB, renderer +263 MB | footprint +452 MB |
| Scroll-to-top peak, relative to the open level | ≤ 1.5× | 1.99× | 2.3× |
| Total memory ceiling | ≤ 250 MB | renderer 869 MB | 1,229 MB |
| DOM nodes | — | 194,738 (162 per row) | 31,253 elements |

Growth is linear, at about 0.26 MB of renderer memory per message. Every message is rendered, and each row keeps its hover toolbar mounted.

**[O] Requirement CP.** Opening and reading a long DM must stay light. The budget scenario is that same 1,000-message DM.
- **Product decision:** a conversation opens on its latest 200 messages. Older ones come in pages of up to 200 through a "Load older" control at the top. This is the user-visible behaviour of Rowan's MEM-1, adopted as product behaviour.
- **Rowan's call:** how the budgets are met, including MEM-2 and any per-row technique.

**Metrics.** Every reading is taken after garbage collection.
- **Linux Chromium (CI):** JS heap for the memory added, and renderer RSS for the peak ratio. They compare across runs, but not with the Mac.
- **The owner's machine (manual run):** in desktop Chrome, the tab's memory footprint in Chrome's Task Manager, for the memory added, the peak ratio and the 250 MB ceiling. In Firefox (latest stable) and Safari (latest major on macOS), the same scenario runs to the end, and Load older and the message actions work.

v1 runs only in the user's own browser (IA D45), with no desktop shell. The memory budgets are measured in Chrome; Firefox and Safari are checked for behaviour. Mac figures from earlier probes (WKWebView) are kept for reference only.

**Acceptance criteria:**
- **AC-CP-1:** opening the DM shows the latest message at the bottom, with the composer ready, and loads only the latest 200 messages. "Load older" at the top loads up to 200 more at a time and keeps the reading position. It disappears when there's nothing older.
- **AC-CP-2:** opening the DM adds at most 60 MB: JS heap in Linux CI, the tab's footprint in Chrome on the owner's machine.
- **AC-CP-3:** scrolling to the top and pressing Load older twice never takes memory above 1.5× its level just after the open: renderer RSS in Linux CI, the tab's footprint in Chrome on the owner's machine. In Chrome on the owner's machine, the tab's total footprint stays at or below 250 MB throughout.
- **AC-CP-4:** leaving the DM and returning 20 times grows the JS heap by at most 2 MB in Linux CI.
- **AC-CP-5:** nothing is lost. Hovering or focusing a message, by pointer or keyboard, still shows its reactions, Reply and the menu, and each works on that message.
