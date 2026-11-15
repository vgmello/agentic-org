# Messaging asks: product questions for Rowan's freeze (#53, due 2026-11-13 18:00)

Research for Emery, 2026-11-13. Product repo at `origin/v1` = `1636a188`. PRDs in `knowledge/prds/`. FREEZE = Rowan's `ia-10-inbox-read-freeze.md`.

## 1. Current facts (origin/v1 1636a188)

- **No asks exist on the server.** `msg.ask`, `msg.reply`, `msg.done`, `msg.human_needs_you` are `unserved` (`apps/org-server/src/serve/host/operations.json:39,42,44,49`); `msg.send` refuses kind `request` (`crates/org-mailbox/src/mailbox.rs:123`, `crates/org-proto/src/msg.rs:121-123`). `chat.answer` and `chat.accept_handoff` aren't in the operations list at all.
- **The wire has no answered state.** `MessageHeader` has `expires_at`/`ttl_ms` but no answered or accepted fields (`msg.rs:309-345`); `Read` means "read (or answered)" (`msg.rs:244`). `handoff` isn't a wire kind, only a custom tag (`msg.rs:130`). The only reserved sender is `office`, for `delivery_failed` notices (`msg.rs:131-134`).
- **Defaults:** an ask's deadline is 2 min for an agent and 24 h for a person or the owner (`msg.rs:165-169`). After 24 h an ask to the owner stays visible as "Agent timed out waiting, your reply will still be delivered" (ADR-0003:458-461).
- **ADR-0003 §6 (proposed):** Asks carry up to 6 options, a default with a deadline, `blocks` and `approval` (ADR:862). Answering uses `chat.answer`, or `msg.reply` between agents. It "marks the ask answered with who and when", and a default is recorded as answered by `default`, with the reply posted from `office` (ADR:862). `chat.answer`/`accept_handoff` are for "the person it is addressed to". Asks and hand-offs to the owner are **app_token only**, never through a bridge (ADR:283-284, :886). A hand-off carries `linked_ref` (the task or bug) and `accepted_by`, with no `accepted_at` (ADR:893-895).
- **Chat spec** (`docs/specs/chat-ux.md`): An ask is a card that answers itself: answering in Chat unblocks the agent and clears Needs you (:52, :124, CHAT-5 :206). A hand-off has Accept · Reply… only, with no Decline. Accept "records the hand-off on the linked task, if any", and the card then reads "Accepted by <name> · <time>" (:124-125, CHAT-18 :219). An ask to the owner is one item, "answered from wherever the owner sees it" (:129). Messaging an asleep agent wakes it (:54).
- **UI mock** (`apps/org-ui/src/lib/modules/messaging`, sample data only, `api.ts:8-11`): `Answer {by, at, option_id?, text?}`, where `by` can be `default` (`types.ts:66-72`); `accepted_by`/`accepted_at` and `linked_ref` (`types.ts:98-110`). **The owner may answer any ask**, including one addressed to someone else (`engine.ts:419`). Accept is for the addressee only (`engine.ts:229`). A second answer or accept is refused "Already answered/accepted by X" (`engine.ts:227,416`). Accept doesn't touch the linked task (`engine.ts:224-235`). An `office` sender is labelled "Office" (`model.ts:44`). Needs you = an unanswered ask or unaccepted hand-off addressed to the owner (`model.ts:62-63`). People send only `msg`: the Composer has no kind picker.
- **Wake:** Run modes: a message, an assignment or Run now wakes an asleep agent (`agent-run-modes-ux.md:54`, RUN-3 :111); Pause holds messages (:97). Sessions design: a paused agent's triggers wake nothing (`docs/design/agent-sessions.md:342`). A normal message to a busy session waits for the turn's end, **but an urgent one (every ask is urgent, `msg.rs:207-210`) is steered into the turn, and on Claude the office interrupts, then sends** (:355). Not built (ceremonies F12, F13).
- **Work:** a `Question` wait exists (`crates/org-proto/src/work.rs:248,955`). Nothing on the server reads `meta.work_item` today.

## 2. What the PRDs already decide

- **The Inbox shows what waits on the owner, actionable in place:** "Questions: Reply inline (messaging exists)" (IA D9). Questions = "agent asks… They clear when answered" (D11).
- **Clearing anywhere** (D26): "an item clears when it's resolved **anywhere**: answered in a terminal, in another client, or because the agent carried on." Nothing but run error and crash is dismissable (D11, D26).
- **Kinds** (D26 rows; Emery's msg21): An agent ask, or a question about a work item, is a **Question** that clears when "answered anywhere". An unaccepted hand-off to the owner is an **Approval (Accept)**, which clears when "accepted by any path (Chat's accept_handoff included)". Review requested (v1.0) = an ask with a review marker. Approve sends "Approved".
- **Inbox stories** (IA-09): The first source is chat asks: "Reply works inline (D9), and the item moves to Handled when answered" (AC-IA09-3). Each card shows who · what · why · actions · expiry (AC-IA09-2). The badge and Home's Needs you share one count (AC-IA09-4). Clearing is live within 2 s of acting anywhere (AC-IA10-2).
- **Work › Needs me** (AC-IA18-1): It lists the five waits of `tasks-v1` §4.1: blocked on the owner, reviewer, open Hand-off, open question to the owner, budget. "Chat hand-offs arrive with IA-10b… only one that names its work item is listed here, and one without a work item shows in the Inbox and Chat only." "An unaccepted hand-off is also a 'Hand-off' Inbox Approval."
- **Task Hand-off** (`tasks-v1` §4.4, AC-TK6-3): It opens when someone other than the owner assigns a task to the owner. "Accept counts as the owner's start: To do → In progress… 'Picked up by' is the owner." It clears on any owner move, `assign --none`, reassignment away, or cancel. "The owner declines a hand-off by reassigning the task to someone else or cancelling it."
- **Task questions:** a question to the owner on a task is a question comment; deleting it or cancelling the task clears it (`tasks-v1` §4.8). "If a chat message asks for work, the agent makes it a task" (§4.9).
- **Ceremonies** (`ceremonies-and-retros-v1.md`): Every M1-M4 shows the sender "System". An agent's reply is refused with "System messages can't be replied to.", and no member can send as System (AC-CER3-2; req 4). Messages go to agents only, and the owner sees Activity (D3, D6, §5.6 :162). **Req 9 / D7:** an idle or asleep agent is woken. A busy one gets the message at the end of its turn and is never interrupted. A paused one has it queued, not woken (costs req 20). Tests: AC-CER10-1..4 (a wake within 1 min). Open: R1 (how System is represented) and R4 (agents run by hand).

## 3. Open product questions

**Q1. Who may create an ask, and to whom?**
- Why it matters: the freeze pins who may call ask creation. The mock lets anyone send `request` (`engine.ts:348`). The ADR lets app-bound people call `msg.ask` (ADR:273).
- Options: (a) agents only; (b) agents and people; (c) anyone, the owner included.
- **Recommend (a): agents ask, and anyone (owner, person, agent) can be asked, one addressee per ask.** People talk to agents in messages and ask for work as tasks (§4.9). A blocking ask from a person would hold up a colleague for no reason. An undirected channel Question (chat rev 6) covers "anyone?".

**Q2. Does a plain chat message answer an ask, or only an answer tied to it?**
- Why it matters: it decides what sets the answered state.
- Options: (a) only an answer tied to the ask: an option, Reply… on the card or in the Inbox, a thread reply to the ask, or `msg reply <ask id>`; (b) also the owner's next plain message in that DM.
- **Recommend (a).** A DM can hold several open asks, and an untied message can't say which one it answers, so guessing breaks "clears when answered". Eden may add a "Replying to Kai's ask" composer chip. Either op (`chat.answer` or `msg.reply`) must set the same answered state.

**Q3. Who may answer, and from where?**
- Why it matters: D26 says "answered in a terminal" and the mock lets the owner answer anyone's ask, while ADR:283 and :886 allow app_token only and the addressee only.
- Options: (a) the addressee only, owner asks in the app only; (b) the addressee, plus the owner on any ask, from the app (Chat or Inbox) or the owner's own authenticated CLI, never a bridge.
- **Recommend (b).** The owner is the final authority (tasks-v1 T4). The ADR's "app only" rule was written to keep bridges from acting as the owner, not the owner's own terminal. Show "Answered by you (asked of Kai)".

**Q4. What does an answered ask record and show, and can the owner close an ask without answering?**
- Why it matters: the freeze needs the who/when fields and the clearing paths. FREEZE:70 adds "or the owner marks it done" (`msg.done`), which D11 forbids ("Nothing else is dismissable").
- **Recommend:** Record who, when, the option and the text. Show the same line everywhere (Chat card, Inbox Handled, CLI): "Answered by {name} · {time}: {answer}", "Default applied · {time}: {option}" or "Withdrawn by {name} · {time}" (Q5). Don't show which surface was used. A second answer is refused with "Already answered by X". `msg.done` doesn't clear an owner ask. The owner can answer "Not now" with a reason, which unblocks the agent with an answer.

**Q5. Can the asker withdraw an ask?**
- Why it matters: D26 clears "because the agent carried on", but FREEZE:70 says a follow-up never clears, and nothing defines withdrawal.
- Options: (a) no withdrawal; (b) an explicit withdraw by the asker, with an optional reason; (c) any later message from the asker clears it.
- **Recommend (b).** The item moves to Handled as "Withdrawn by Kai" and the card reads the same; withdrawing an answered ask is refused. An agent that solved it itself shouldn't leave the owner a stale question. An explicit act keeps the clearing testable.

**Q6. Is a chat hand-off that names a work item the same thing as the task's Hand-off?**
- Why it matters: this is the freeze's `meta.work_item` rule and what Accept does to the work item. Today Accept "records the hand-off on the linked task", which is undefined (chat-ux:125).
- Options: (a) one hand-off; (b) two objects, each cleared separately.
- **Recommend (a): one hand-off, one Inbox Approval, one Needs me row.** Sending a hand-off that names a work item reassigns that item to the recipient, under the §4.4 rules. Accept from Chat, the Inbox or the CLI is the task's Accept: the recipient becomes the assignee and the task moves To do → In progress, "Picked up by" them (AC-TK6-3). Declining is **Hand back… (reason)**: it reassigns the task to the sender (AC-TK6-3's "reassign to someone else"), clears the Approval and posts the reason as a reply. A hand-off with no work item records only who accepted it and when. Hand back… just replies. The same rules hold for a hand-off to an agent.

**Q7. Does an ask that names a work item count as "a question to the owner is open" on that task?**
- Why it matters: AC-IA18-1 lists only hand-offs from chat, and §4.8 defines a task question as a comment.
- **Recommend yes.** It shows in Needs me, in Following's Waiting on you and as one Inbox Question, the ask's. The owner is waiting to answer either way. Agent guidance keeps steering work questions to task comments.

**Q8. How is the System sender shown, and does it cover office notices?**
- Why it matters: this is R1. ADR:862 posts default replies and `delivery_failed` from `office`, which the mock shows as "Office" (`model.ts:44`).
- **Recommend:** The name is "System", with a neutral product-mark avatar: no presence, no profile, not in DMs or the member pickers, never a member. Replies are refused (AC-CER3-2), and no message to a person (D3). Every product-made message, office notices and default replies included, shows as System. That gives the product one voice, and "Office" stops looking like a colleague. A default-applied answer still reads "Default applied".

**Q9. Does an urgent message or an ask interrupt a busy agent?**
- Why it matters: `agent-sessions.md:355` steers or interrupts on urgent, and every ask is urgent, while D7 and req 9 say never interrupt.
- **Recommend: never.** Every message, asks and System included, reaches a busy agent at the end of its turn. Only the owner's explicit Interrupt or Stop (`agent.interrupt`, app only) cuts a turn short. Asleep or idle agents wake, and a wake starts within 1 min (AC-CER10-2). Paused agents get it queued, with no wake. "Finish the work in hand" is the colleague model.

**Q10. What happens to a message for an agent run by hand, which the office can't wake (R4)?**
- Why it matters: the freeze pins the wake semantics, and today every agent is run by hand (F13).
- **Recommend:** The message is delivered and waits. The sender sees "{name} runs by hand: they'll see this when they next check messages", never a failure. Only office-run agents count as wakeable for req 9, so routines need an office-run lead and participants. Owner asks to a hand-run agent are unaffected.

## 4. Acceptance tests (messaging asks)

**IA-09 (asks part)**
1. [integration] Given Kai's open ask to the owner, when the owner answers it with an option in the Inbox, then the ask is answered with by = owner and a time, Kai gets the reply, and the item moves to Handled (AC-IA09-3, D11).
2. [integration] Given an open ask, when the owner answers it in Chat, then the Inbox item clears and the badge drops within 2 s, with no Inbox action (AC-IA10-2, D26).
3. [integration] Given an ask past its 24 h deadline with no default, when the Inbox is listed, then it's still open, and reading it doesn't clear it (FREEZE:70, ADR:458-461).
4. [integration] Given an ask with a default, when its deadline passes, then it's answered by `default` and moves to Handled showing the default (ADR:862, D26).
5. [UI e2e] Given open asks, then the sidebar badge, Home's Needs you and the Inbox Questions tab show the same count, and each card shows who · what · why · actions · expiry (AC-IA09-2, AC-IA09-4).
6. [integration] Given an answered ask, when a second answer arrives from any path, then it's refused with "Already answered by …" and the first answer stands (after Q4).

**AC-IA18-1 (asks and hand-offs part)**
1. [integration] Given Kai reassigns T-12 to the owner, then T-12 is in Needs me and the Inbox has one "Hand-off" Approval; when the owner accepts it, T-12 is In progress, "Picked up by" the owner, and both clear (AC-IA18-1, AC-TK6-3).
2. [integration] Given a chat hand-off to the owner with `meta.work_item` T-12, then T-12 is listed in Needs me; given one with no work item, then it's in the Inbox and Chat and not in Needs me (AC-IA18-1).
3. [integration] Given a chat hand-off naming T-12 to the owner, when it's accepted in Chat, then the Inbox Approval and the Needs me row clear within 2 s, and the card reads "Accepted by you · {time}" (D26, CHAT-18, AC-IA10-2) (after Q6).
4. [integration] Given an open Hand-off on T-12, when the owner reassigns T-12 to Sage or cancels it, then the Hand-off and its Inbox item clear (AC-TK6-3).
5. [UI e2e] Given Needs me is the default view with only one open hand-off, when the owner accepts it inline, then the view shows "Nothing needs you in Work" (AC-IA18-1, AC-IA18-3).
6. [integration] Given Kai's ask to the owner naming T-12, then T-12 is in Needs me until the ask is answered (after Q7).
