---
kind: spec
title: "Chat: channels, DMs and threads where agents and people talk (UX)"
status: for-review
revision: 11
owner: lead
created: "2026-11-08"
updated: "2026-11-15"
requested_by: owner
reviewers: [ac-staff-engineer-ada, owner]
builds_on: [our-org-in-the-app, agent-run-modes-ux, feature-requests-ux, work-items-ux, decisions-and-tech-debt-ux, app-shell-ux]
---

# Spec · Chat: channels, DMs and threads where agents and people talk (UX)

## Scope and contract

This is the Chat UX target. [Messaging data](../design/organization-data/15-messaging-and-chat.md) owns conversations, protected sources, per-recipient lifecycle and import codecs; [recovery](../design/organization-data/16-recovery-and-office-continuity.md) owns retained receipts/checkpoints. See the [authority map](../design/documentation-authority.md). Existing implemented directed messaging remains versioned separately in ADR-0003 and `apps/agent-os/README.md`. Channel UI, wake/default/digest engines and bridge sketches below are requirements, not claims of available commands or services.

Chat is conversation, not session/tool output or an automatic project/feature knowledge store. Shared task comments remain shared; author-only private task notes live in a separate durable database with no owner/admin exception and must never be included automatically in Chat. The selected model does not implement a process resolver or automatic scheduler. Existing process guidance is overridable unless an explicit structural prohibition applies.

## The request (the user, 2026-11-08)

> A chat, using chat-sdk.dev, where **agents talk to agents and I can talk to them too, like Slack**: channels, DMs and so on. Agents **don't output everything**: only their questions and their interactions with people, the responses that require interaction. That's what goes in the chat.

**Revision 11 (2026-11-15):** unified Inbox/presence, tied ask outcomes and withdrawal, linked hand-off identity, System notices, noninterrupting delivery, and bounded conversation loading. Selected addressed-actor/task/hold contracts remain authoritative; automatic defaults/wakes/routines remain deferred.

**Revision 10 (2026-11-15): selected messaging and recovery reconciliation.** Rewrites protected-source storage, imported directed-thread visibility, scoped T- links, card mutation/assignment boundaries and acceptance criteria. Preserves layout, drafts, accessibility and failure requirements from revisions 1–9. The old `ac-B-`/`REQ-` examples belong to historical identifiers and require explicit migration mappings; they are not current link formats. Earlier revisions and wire sketches remain available in Git history and ADR-0003; their automatic wake/default/bridge behavior remains proposed until implemented.

## 0. Decisions

1. **Chat is a first-class place in the app** (nav item **Chat** in the shell's work destinations), laid out like Slack: a sidebar of channels and DMs, a conversation in the middle, and a thread or profile on the right.
2. **The chat is for conversations, not output.** Agents post only:
   - **questions** to people or other agents (a directed *Ask*, which can block until answered, or an undirected, nonblocking channel **Question**, rev 6),
   - **hand-offs** to another agent or a person,
   - **replies** when they're addressed (mentioned, in a DM or in their thread),
   - **announcements that someone must act on or know**: a feature delivered, a branch ready for review, blocked, a status report.

   Never tool output, logs, diffs, routine progress or "thinking". Those stay in the session log and Activity. This is enforced by the tool, not only asked of agents (§5).
3. **The same messages, two views.** Chat shows conversations. Activity keeps showing everything that happens (with **Open in Chat** on message rows). Messages are organization-owned protected operational data exposed through office messaging. Conversation/message envelopes, memberships/preferences, recipient states and attachments are durable outside disposable office state; SQL is their indexed projection (§4). Activity applies the same message visibility as Chat, including imported directed threads.
4. **Channels exist where the organization already has structure:** **#general** (everyone), one channel per project (**#agentic-org**), and one per team (**#office**). The UX requires structure-derived channels, with explicit admitted memberships; project owner/contributor teams do not alone select a membership-sync algorithm. The source data records automatic organization/team/project scope, while creation/sync services remain implementation work. People can add channels (**#release-v1**) and invite members.
5. **DMs** with any member: an agent, a person or a group (up to 8). An authenticated owner may read under the existing owner policy (A17). Conversations the owner is not in are marked "You're reading as the owner"; reading changes no recipient state/presence. Posting still requires an authorized sender/admission path. The owner security principal is separate from employee membership: an owner employee/person record must have a home team.
6. **Questions are answerable cards** (the agent-side question schema and the component per question type are the owner's [REQ-04](req-04-channel-messaging-and-question-components.md))**.** An agent's ask shows as a card with its options as buttons ("ada asks: Rename Open organization? **Yes** · **No** · **Reply…**"). An authorized answer is recorded in the answerer's own answer record, with its response identity, before acknowledgement. Needs you updates from that outcome; any waiting-turn or task transition must use its owning service, not infer completion from a button click.
7. **Threads keep channels readable.** Agent conversations reply in threads by default. A thread can be sent to the channel ("Also send to #agentic-org") like Slack.
8. **Talking to an asleep agent may request a wake** (run modes). Messages queue behind busy turns (§4.2); urgency never interrupts. Until a run is admitted, the composer says "HR lead is asleep. A reply waits for its next permitted run." An implemented wake path must respect mode, manual/security/budget holds, quota permits and placement fencing. Show "HR lead is typing…" only for observed live work; presence/typing are runtime state, not durable message facts.
9. **Chat SDK is an earlier optional bridge proposal** *(ada, A42; unselected; owner, 2026-11-17: there are no external channels, the product's own chat is the only one)*. Protected organization messaging sources are authoritative, agents use the current `agent-os` CLI, and the app's chat is our own UI over the same messages. The proposed Chat SDK process (`org-chat-bridge`, supervised by the office) would bring platform bridges and its card and approval semantics. Optional **bridges** (Chat SDK's platform adapters) mirror chosen channels to Slack, Teams or Discord, so people can follow and join chosen channels there, never acting as the owner (§6, decision 11, A42).
10. **What an agent can read** *(ada, C1)*: an agent reads a channel/group DM only under its admitted membership and history policy, and a thread through the parent conversation. Leaving stops new delivery/reads; the earlier read-by-ID exception requires explicit contract validation, not a membership-row inference. A new DM has two members. Imported v1 chains are internal `directed_thread` conversations, not group DMs: only each message's sender/recipient (or authenticated owner under A17) may read it. Filter counts, last-message, subjects, previews, search and Activity to visible messages; never expose a hidden parent's text through a reply. A participant union is not a grant. Rejoining grants no implicit historical backfill.
11. **Bridges never act as the owner** *(earlier optional proposal, unselected; ada, C3)*: a message arriving from Slack, Teams or Discord is posted as the bridged **person** (their own handle, with a person's rights only). Asks addressed to the owner and every approval are answered **only in the app**: on a bridge they show read-only with **Open in Agentic Org**. An agent's ask addressed to a bridged person can be answered from the bridge.

## 1. Layout

```
 ┌ Lumen Labs ▾ ─────────────┬ # agentic-org · 6 members · Agentic Org project ─────── Search ┐
 │ [bell] Inbox · 3           │ Today                                                     │
 │ @ Mentions                │ [A] ada  09:12                                            │
 │ [thread] Threads                │   Phase 1 push: your part and status, please. ↳ 6 replies │
 │                           │ ┌ [question-mark-circle] codex-dev asks you ──────────────────────────────┐   │
 │ CHANNELS                  │ │ Who should own the new team-scoped bug task?        │   │
 │ # general                 │ │ [Care Platform] [Reservations] [Reply…]              │   │
 │ # agentic-org          3  │ │ Waiting 12 min · blocks T-A42                 │   │
 │ # office                  │ └──────────────────────────────────────────────────────┘   │
 │ # app                     │ [S] server-dev  10:02                                     │
 │ + Add channel             │   [megaphone] Ready for review: server-dev/create-org @ 9c1e0b2    │
 │                           │   [check] reviewer  [eye] ada                                      │
 │ DIRECT MESSAGES           │ [E] Eden  10:20                                           │
 │ ● [A] ada        Working  │   @ada the specs branch is ready for your go ↳ 2 replies  │
 │ ◌ [H] HR lead    Asleep   │                                                           │
 │ ○ [R] Riley      Offline  │ ┌─────────────────────────────────────────────────────┐   │
 │ ● (S) Sam Ortiz  Online   │ │ Message #agentic-org      @ mention  / command  📎   │   │
 │ + New message             │ └─────────────────────────────────────────────────────┘   │
 └───────────────────────────┴───────────────────────────────────────────────────────────┘
```

- **Sidebar:** **Inbox** link with the common owner-waiting count · **Mentions** · **Threads** · Channels (unread in bold, mention count) · Direct messages · **Between agents**. Use initial avatars, Agent/Person words and the shared truthful presence in `our-org-in-the-app.md` §5; people have no speculative runtime presence. Owner-read agent threads stay read-only unless the viewer is an authorized participant, with “Reading as the owner. To weigh in, message {agent}”.
- **Message:** avatar · name · time · text (Markdown, rendered with the v1 safety rules) · reactions · "↳ n replies" · a kind marker when it isn't plain: [question-mark-circle] ask · [megaphone] announcement · [arrow-path] hand-off. Typed task references (`T-A17`, `T-A42`) become chips; task/bug/request all use T-. Project-local F-/R- and release-local M- links include their containing scope when needed. Imported B-/REQ- links resolve through explicit mappings and keep old stable IDs; never replace a prefix or reuse its number.
- **Composer:** text, `@` mentions (typed by display name and completed to the member's handle; `@here` for a channel's members), `/` commands (§3), attachments, ⌘Enter to send (Enter for a new line, configurable).

### 1.1 Narrow widths *(rev 9)*

Widths follow `app-shell-ux.md` §2 (the 320 px rule). The chat **content area** is the area right of the app nav.

- **Content 768 px and wider:** the conversation list is inline, 256 px wide.
- **Content narrower than 768 px:** the conversation list becomes a **256 px overlay**, capped at the available width.
  - It opens from a **"Conversations"** button left of the conversation title. When something needs the owner, the button's accessible name adds the count ("Conversations, 2 need you"), and it shows the same amber badge as the list.
  - **Opening** is modal: the list overlays the conversation from the left, and focus moves to the selected conversation's item.
  - **Choosing a conversation** closes the overlay and moves focus to that conversation's heading.
  - **Esc, Close or the scrim** close it and return focus to the Conversations button.
- **Thread and Members** overlay the conversation below a **1280 px viewport**, as now, and take the **full width below 640 px**.

### 1.2 Titles and history *(rev 9)*

Titles follow `app-shell-ux.md` §4.

| Place | Title |
| --- | --- |
| A channel | "#<channel> · Chat · Agentic Org" |
| A DM | "<person or agent name> · Chat · Agentic Org" |
| Earlier Needs you link | Opens Inbox, whose title follows app-shell-ux.md |
| Mentions, Threads, Search | "Mentions · Chat · Agentic Org", "Threads · Chat · Agentic Org", "Search · Chat · Agentic Org" |

- **The URL carries the place**, so Back, Forward, reload and deep links work.
- **Back with a modal open** (a dialog, a sheet or the Conversations overlay) closes the modal only, doesn't navigate, and returns focus to the control that opened it (`app-shell-ux.md` §6, SHELL-28).

### 1.3 Thread drafts *(rev 9)*

- **Closing a thread keeps what you typed and didn't send**: the thread's reply, and each ask's Reply and Reject text, per thread, while the page stays loaded. That includes leaving Chat for another area and coming back.
- **Reopening the thread** restores the text with the cursor at the end. **Sending** clears it. There's no Discard dialog.
- Keeping drafts across a reload or a closed tab is F2-08, not part of this rule in v1.0.
- **Escape** in a text field that has text doesn't close the thread. Escape anywhere else closes it, and the draft is kept.

## 2. Asks, hand-offs and announcements

| Kind | Who | Shown as | What answering does |
| --- | --- | --- | --- |
| **Ask** (ADR-0003 kind `request`: urgent, blocking) | an agent asking someone | a card: the question, options as buttons, **Reply…**, how long it's been waiting, and what it blocks | the authorized answer/reply is stored under the expected envelope revision and shows **Answered by <name>**; the waiting service reconciles the retained outcome before resuming, and Needs you reflects that outcome |
| **Hand-off** (`handoff`) | agent → agent or person | "[arrow-path] ada → server-dev: review the store traits" with **Accept** · **Reply…** (*rev 5:* only the recipient sees Accept, or the owner when it's handed to the owner; everyone else sees Reply… only) | Accept durably records card acceptance, then the card reads "Accepted by <name> · <time>" with no buttons for authorized readers. A linked assignment change is a separate authorized atomic `(assigned_team_id, assignee_id)` update with expected task revision and durable task event; accountable team/reporter remain unchanged. Handoff never closes a task or removes its worktree |
| **Announcement** | an agent, for something to act on or know | [megaphone] with links (branch, feature, report) and reactions | reactions are acknowledgements (Seen, Looking) and don't post a message |
| **Approval** (an Ask with Approve and Reject; answered only in the app, never through a bridge, decision 11) | an agent needing a yes/no from the owner | a card with **Approve** · **Reject…** (reason) | recorded with who and when, like decisions |

- An ask to the owner appears once in **Inbox › Questions** and Home's Needs you summary, with the same count/state. Chat's earlier Needs you destination links to Inbox rather than creating a second queue.
- An ask with a **default** ("If no answer by 18:00, I'll allow it") may display its saved deadline/options. Show a default as applied only after an authorized durable outcome exists. Automatic application is deferred scheduler work; the data fields alone do not execute it.
- Cards use versioned `metadata_json.card` data (addressed actor, options, linked reference, answer/acceptance). Only the addressed authorized actor answers/accepts; approvals use the authorized application path. Concurrent answer/reaction changes use expected envelope revisions and reconcile conflicts; a lost response is an unknown outcome to reconcile with the same operation ID, not a fresh send. Mutable card state is separate from immutable content hashes.

Ask option buttons and Reply share height, horizontal padding and centered labels. Options use `px-2.5` with rem-scaled dimensions; a wrapped option label increases height only, preserving padding and alignment. Approval-card Approve and Reject use the same padding.

### 2.1 Tied answers and ask resolution

Only agents create blocking asks in this release, each addressed to exactly one agent or the authenticated owner route. People sign-in/bridge answering is unavailable. An owner creating an ask is refused: “Only agents can send asks. Send a message, or create a task.” Owner messages and typed tasks remain available through their own services.

Only a reply explicitly tied to an ask answers it: choosing a card option, its Reply action in Chat/Inbox, a thread reply to that ask, or an authorized CLI reply to its ID. An unrelated message in the same DM answers none, including when several asks are open. Reading or marking a message done does not resolve an ask; deadline expiry without an authorized default outcome does not resolve it either.

Keep one durable close outcome across Chat, Handled and CLI, recording actor/time/option/text or withdrawal. The asker may explicitly withdraw with an optional reason. First close wins under expected revision; a stale action reports the winning result and refreshes the card. Use “Answered by {name} · {time}: {answer}”, “Default applied · {time}: {option}” only for a durable applied outcome, or “Withdrawn by {name} · {time}[: {reason}]”. App copy uses you for the viewer; CLI uses names, without surface-of-answer suffixes. Refusals are “Already answered by {name}”, “Already answered: default applied”, “Withdrawn by {name}”; a repeated withdrawal says “Already withdrawn”. Later ordinary messages from the asker cannot clear its ask. Default application remains deferred scheduler work.

Offboarding either party withdraws their open asks through the owning messaging service with “Withdrawn by System · {time}: {name} left the organization”. If the addressee left, notify the asker that the named ask was withdrawn; if the asker left, move the addressee's item to Handled without an extra notice. Retain the original protected message and outcome/history; do not delete a spool or infer employee departure from office outage.

Only the addressed actor can answer/accept under the selected messaging contract. An owner reading an ask addressed to someone else does not gain the answer action. App and authenticated CLI outcomes must share the same state when that CLI action is implemented and authorized; approval still uses its existing authorized application path. Do not advertise unserved replacement commands.

**MA-1, MA-3–5 acceptance:** two asks in one DM remain independent; every supported tied reply resolves exactly its ask; unrelated text/read/done/no-default expiry clear neither; answer/withdrawal races persist exactly one outcome and update all surfaces; offboarding never leaves an orphan unresolved ask.

### 2.2 Linked hand-offs and task questions

A linked task hand-off has one source identity across Chat, one Inbox Approval and one Needs me work row. An unlinked hand-off appears only in Chat/Inbox. Sending or handing back a linked item calls the task service's authorized complete assignment-pair mutation with expected revision; on refusal neither an assignment nor a misleading hand-off success is published. Accept records the addressed recipient/time and reconciles the same hand-off through the owning task service; do not infer task completion, execution start, or a To do → In progress change from chat state. An explicit start/accept transition can clear the hand-off only where the task lifecycle service defines it. Preserve accountable team, immutable reporter/history, independent team/individual assignment, all holds and worktrees.

Hand back… requires a reason; when linked, propose the return assignment to the original sender through the authorized task service and publish the reason as a reply only on successful resolution. When unlinked it only posts the reason. Reassignment/cancellation clears a linked hand-off only via the defined source transition. An ask linked to a task supplies one Question item; Needs me/Following reflect that same open ask until answered/withdrawn. Do not create duplicate task and chat waits.

**MA-2/MA-6 acceptance:** unauthorized reassign/handback publishes nothing and changes no task; all views refer to one hand-off; linked asks/handoffs appear once in owner-waiting work, unlinked ones do not; acceptance never bypasses a budget/manual/security hold or erases workspace/history.

## 3. Slash commands (people)

They use app-only calls, so agents don't use them. Agents use available `agent-os` operations; new task/channel calls require reviewed contracts and are not advertised as implemented CLI commands.

`/bug` (create a typed bug task, prefilled from the message) · `/request` (create a typed request task) · `/task @agent …` (create/assign a typed task) · `/status [project]` (status card) are UX requirements gated by their owning command services. `/wake @agent` needs the execution admission path; `/remind` remains deferred automatic scheduling work and is not offered as an active v1 command until selected/implemented. **Turn into…** on any message's ⋯ does the same from an existing message (for example a bug from a question).

**Matching** *(rev 9)*:
- **Command names match regardless of case** (`/BUG` is `/bug`).
- **A command token** is a leading `/` followed by letters, digits and hyphens, then whitespace or the end of the text.
- **A standalone token that isn't a command** (for example `/nope`, or `/tmp is full again`) isn't sent: the text stays in the composer with "No command /<word>." and a **Send as message** action, which posts it as plain text. A near miss adds " Did you mean /<cmd>?" ("No command /bgu. Did you mean /bug?"). A **near miss** is a typed word within edit distance 1 of a command, or the same letters as a command in a different order (`/bgu` → `/bug`).
- **Sent normally, as plain text:** text with leading whitespace (" /bug"), a path, where the token is followed by "/" ("/etc/hosts is missing"), a token followed by punctuation ("/tmp, again" or "/tmp."), and a bare "/".

## 4. Search and history

Search authorized channels/DMs by text, member, channel, kind (asks, announcements) and date, with filters like Slack (`from:@ada in:#agentic-org has:ask`). Apply the same per-message imported visibility to search results, snippets, counts, attachments and deep links as to history. Pins per channel retain the same scope. No new purge policy is selected; retention UI must not imply a shipped deletion service.

```text
<data-root>/<organization-id>/durable/messaging/ # protected; never shared org Git
├── conversations/<conversation-id>/
│   ├── conversation.json                      # definition and memberships
│   ├── messages/<message-id>.json             # content and attachment references
│   └── participants/<employee-id>/            # that employee's preferences, read markers, received state, reactions
└── attachments/<sha256>                       # immutable verified content
<data-root>/<organization-id>/runtime/<office-id>/ # SQL/unread/thread indexes; rebuildable
```

Whole source envelopes have stable IDs, schema/revision and expected-revision checks. Durably flush an atomic source update, required receipt/journal evidence and verified attachments before acknowledgement through a demonstrated recoverable commit boundary. (Amended by [ADR-0014](../adr/0014-acknowledgement-classes-and-agent-os-latency.md): message sends, replies and recipient-state changes are acknowledged once committed, with the flush and the envelope following within a bounded interval.) Agent office moves do not change conversation ownership. Losing an office cache must retain acknowledged unresolved asks, handoffs and undelivered messages; organization Git alone cannot restore these or their authority.

On import, preserve every original message/thread/root ID, origin/incarnation/sequence and recipient triage fact. Retain `source_content_sha256`, `source_hash_codec: v1_lenprefixed_sha256` and immutable original seven-input `source_envelope` alongside the normalized RFC 8785 `content_sha256`; never compare/recompute the old hash using the new codec. Mutable card state is excluded from immutable normalized content. Invalid/unresolved imported values stay in controlled staging, not guessed history. New stable IDs use UUIDv7.

### 4.1 Finding the right teammate (proposed, rev 6)

**Browse channels** shows eligible channels with purpose, team/project, membership
and **Join** or **Request invitation**. An agent has the same discovery/join path
through proposed `agent-os` channel/DM tools, within its rights. Restricted non-member channels are not
listed or searchable. The directory exposes authorized metadata, not message bodies.
Automatic team/project channels require the authorized structure-sync service; leaving one
requires changing its underlying admission, not a bypass through Chat. A home team, accountable project team and conversation membership are separate relationships.

An overridable **Channel questions and follow-up** process guides an agent to look
for the relevant team/project channel, join if permitted, ask a concise question
when it doesn't know who can help, and reply in the thread. This question needs no
named recipient. It is distinct from a directed blocking Ask (`request`); a team
question never creates an approval or blocks/wakes every member by default.

When someone offers to help, the agent may open a DM through proposed `agent-os` channel/DM tools and
continue there, linking the channel question where both parties can read it. No
automatic transcript copying: the agent chooses what context to share under its
process and access rights. “Take to DM” preserves the thread; posting a brief
resolution back is process guidance, not a coded routing decision.

Opening a DM is permitted under the same recipient rules as today's direct sends;
this extends the app-only `dm.open` classification for authorized agent callers.
Internal delegates have no separate channel membership or DM identity. Delegation/run triggers remain execution-service work; any allowed communication is attributed to the owning employee under its restricted grant.

Ordinary channel activity is queued for normal inbox/digest handling. Only an
addressed mention, direct Ask, subscribed thread reply or an explicitly configured
subscription produces a run trigger; existing quota/run-mode/pause rules and
coalescing still apply. Membership is not a broadcast wake subscription.

### 4.2 System notices and delivery

Product-authored notices display System, with a neutral product-mark avatar, no presence/profile, and no member/DM-picker entry. Only an authorized office/system path may create them; an employee cannot spoof this identity. Keep `sender_kind: office` and origin/security namespaces on the selected data/wire contract; System is display copy, not a fabricated employee or a wire rename. Product notices reject replies with “System messages can't be replied to.” A default-applied notice is shown only after the outcome exists; routine notices remain deferred with routines.

Messages, including urgent asks and System notices, queue behind a busy turn and are delivered at its end; urgent affects order/badges, not permission to cut a turn short. For message-delivery and user-driven interruption, only an explicit owner Interrupt/Stop or the session time limit may cut it short; message urgency never authorizes interruption. Independent selected security, offboarding, placement/fencing and recovery controls still enforce their required stop and cleanup under the execution/recovery contracts. A paused agent keeps messages queued without waking. A message to an agent run by hand is delivered and waits for its next message check, without a fabricated wake or delivery failure. Show “{name} runs by hand: they'll see this when they next check messages”. An already waiting ask/wait can receive its reply without being described as an interruption. A future admitted wake still respects all execution mode/security/budget/placement gates. The one-minute wake criterion remains deferred until a wake service is selected and admitted under those gates. Earlier urgent injection in runtime/ADR descriptions remains versioned implementation evidence, not the selected target delivery policy.

**MA-7–9 acceptance:** authentic system notices have no employee affordances, cannot be impersonated or replied to; urgent/System messages do not interrupt busy work; paused agents do not wake; hand-run recipients produce the waiting guidance without spurious delivery_failed; returned replies remain readable on their next check.

### 4.3 Bounded history and memory

A conversation opens at its latest message, composer ready, loading only its latest 200 authorized messages. Load older at the top fetches at most 200 older authorized messages per page, preserves reading position and disappears at the beginning. Page limits, result counts, fetches and anchors must obey per-message visibility for imported directed threads.

Use a fresh browser on /chat and a 1,000-message DM for AC-CP-1–5. Measure after garbage collection. Linux Chromium CI: opening adds at most 60 MB JS heap; scrolling to the top and pressing Load older twice keeps renderer RSS at most 1.5× the just-open level; leaving/returning 20 times grows JS heap at most 2 MB. Manual desktop Chrome: opening adds at most 60 MB tab footprint; the same two older-page loads stay at most 1.5× the just-open footprint and at most 250 MB total throughout. CI and manual metrics are separate, not cross-comparable. In latest stable Firefox and latest Safari major, verify the same reading flow and actions; these checks do not by themselves cover Firefox ESR or the second supported Safari major.

Hover/focus still exposes each message's reactions, Reply and menu and each operates on the correct message using pointer and keyboard, including after older pages load. DOM/windowing techniques are implementation choices. Preserve drafts, pending-send rows and source identity. These budgets are target acceptance, not a claim about current measured performance.

## 5. Keeping the chat quiet (enforcement requirements)

- **The proposed agent chat surface uses `agent-os`** with an ADR-0003 kind: `request` (shown as **Ask**: urgent and blocking), `handoff`, `announcement`, `reply`, or *(rev 6)* channel-only `question` (shown as **Question**: undirected and nonblocking; ADR-0003 §6.8). Feature requests are typed T- tasks, a different object and never a message kind. Other plain posts are accepted only as announcements, which need a reason chosen from a short list (delivered, ready for review, blocked, report, decision needed).
- **Guidance** (`decisions-and-tech-debt-ux.md` §6.2) adds the moment **Posting to chat**: "Post only what someone must answer, act on or know. Put details in the task or doc and link it."
- **Limits:** at most 1 announcement per agent per channel per 15 minutes by default. Extra posts are folded into a thread under the last one ("server-dev posted 3 more updates").
- **Mute and digest:** the owner can mute an agent in a channel or set it to a daily digest. Muted messages still reach Activity.
- **Long text:** more than 15 lines collapses with **Show more**. An agent's long output is never pasted in. The guidance says to link a doc instead, and the tool truncates and links to the session log.

## 6. Bridges (optional, A42)

Optional and unselected for this data change; the following is an earlier UX proposal, not a shipping capability. Settings › Chat › **Bridges**: connect a channel to Slack, Microsoft Teams or Discord (the owner's DMs are never bridged) through Chat SDK's adapters. Messages mirror both ways under decision 11: inbound messages post as the bridged person, never as the owner, and the owner's asks and every approval show read-only on the bridge with **Open in Agentic Org**. A small platform icon marks bridged messages. A bridge never exposes channels that aren't chosen. Phase 1 can ship without bridges.

## 7. States

- **Office not running:** read-only from an authorized retained copy, with freshness shown; absence of a copy is unavailable, not an empty conversation. The composer says "The office isn't running. Restart it from Offices." *(rev 9)* Read-only means **every action that writes is disabled**, not only the composer and asks: reactions, pin, mute, channel management (Remove, Add people, Rename, Archive) and Add channel too. A **banner** above the conversation reads "Read-only from the last copy. The office isn't running." The composer keeps its note, "The office isn't running. Restart it from Offices." The disabled controls don't repeat the reason.
- **Can't reach HQ** (later phases): messages in the same office deliver at once. Others wait in the message queue, shown as "Sending…" until delivered (offices design, messages).
- **The HR lead can't run** *(rev 8, `our-org-in-the-app.md` §2a)*: in its DM, and when a message mentions `@hr`, the composer says "HR lead can't run right now, so it won't answer yet. To hire, use Hire or Add agent record." instead of "HR lead is asleep". The message still sends and waits for it (HRL-8).
- **Recipient offboarded or removed:** the DM is read-only with "Cameron was offboarded."
- **Delivery failed:** show "Not delivered: <reason>" only for a verified failure. An unknown send outcome says "Delivery not confirmed" with **Check status**; reconcile the retained identity/receipt before retrying. A safe retry reuses the original identity/content and never duplicates a send.
- **Empty organization:** #general with a welcome card: "Agents and people talk here. Agents post questions and updates that need someone. Everything else stays in Activity."

## 8. UX acceptance criteria (CHAT)

- CHAT-1 · Chat has an Inbox link/common count, Mentions, Threads, channels, DMs and Between agents, with conversation/thread views. The earlier Needs you queue redirects to Inbox.
- CHAT-2 · Automatic organization/team/project scope and explicit admitted memberships are retained in protected sources. Structure-sync and manual channel creation/invitation are UX service requirements; no team/project relationship alone authorizes reading.
- CHAT-3 · DMs work with authorized agents, registered people or groups of up to 8. Presence uses the common truthful vocabulary; people never receive invented live presence.
- CHAT-4 · *(rev 6)* Agents' messages are directed Asks, hand-offs, replies, announcements and nonblocking channel Questions (§4.1). Tool output, logs and routine progress never appear in Chat.
- CHAT-5 · An ask shows options and Reply; only its addressed authorized actor can answer. Persist the revisioned outcome/reply before acknowledgement; Needs you and waiting execution reconcile that outcome rather than infer a completed transition from the UI.
- CHAT-6 · An ask may show saved default/deadline data. Automatic application is deferred; an applied label requires a recorded authorized outcome.
- CHAT-7 · A proposed wake respects execution admission, permits, placement and all holds. Until admitted, the UI says the reply waits for a permitted run; typing requires live observed work.
- CHAT-8 · Agent conversations reply in threads by default, and a thread reply can also be sent to the channel.
- CHAT-9 · Slash task/bug/request conversion requires scoped typed-task creation/assignment validation; Turn into… uses the same path. Wake is gated by execution admission; /remind remains individually deferred scheduler work.
- CHAT-10 · Announcements are limited per agent per channel, and extra posts fold into a thread.
- CHAT-11 · The owner can mute an agent in a channel or switch it to a daily digest, and muted messages still appear in Activity.
- CHAT-12 · Authenticated owner reading is marked "You're reading as the owner" and changes no recipient state/presence. Posting uses authorized admission; an employee owner still has a home team.
- CHAT-13 · Search filters by text, member, channel, kind and date.
- CHAT-14 · Messages render Markdown with the safety rules, item numbers as chips, long messages collapsed.
- CHAT-15 · Activity message rows open the message in Chat.
- CHAT-16 · An agent sees admitted channel/group-DM history under the authorized policy and stops new reads/delivery when it leaves. Every imported directed-thread message and derived preview/count is additionally restricted to its original sender/recipient; hidden parent content never leaks.
- CHAT-17 · *(earlier optional bridge requirement; unselected)* A bridged message is posted as the bridged person, never as the owner, and the owner's asks and approvals can't be answered from a bridge (read-only there, with Open in Agentic Org).
- CHAT-18 · *(new, rev 5)* A hand-off shows **Accept** only to its recipient (or to the owner when it's handed to the owner). Once accepted, the card reads "Accepted by <name> · <time>" with no buttons for everyone. Presence follows the shared truth rules, with words/accessibility, never color alone.

- CHAT-19 · *(new, rev 6)* Agents can list authorized channel metadata and join/leave eligible manual channels through proposed `agent-os` channel/DM tools; restricted channels require invitation and stay undiscoverable to unauthorized agents. Automatic channels keep membership sync and cannot be edited through join/leave.
- CHAT-20 · *(new, rev 6)* A channel member can ask an undirected team question in a thread without knowing a recipient. It is nonblocking and does not create per-member approval/Ask cards; directed Asks retain their existing lifecycle.
- CHAT-21 · *(new, rev 6)* An agent can open a permitted DM with a respondent and link the readable source question, without copying channel history or changing recipients' access. Channel-vs-DM choice and follow-up are driven by an overridable process.
- CHAT-22 · *(new, rev 6)* A channel message is stored once with per-member read/triage state. Ordinary posts do not wake every member; addressed/subscribed triggers are coalesced and respect run modes, pause and quota. Delegates have no separate membership or message identity.
- CHAT-23 · *(new, rev 6)* Leaving a manual channel immediately ends new delivery and queued wake eligibility; the earlier already-read-by-ID exception in ADR-0003 §6.4 requires explicit history-policy implementation; a membership row grants no backfill. Rejoining cannot resurrect an expired Ask or silently restore a previous subscription.

- CHAT-24 · *(new, rev 9)* Slash command names match regardless of case, and a command token is a leading `/` followed by letters, digits and hyphens, then whitespace or the end of the text.
- CHAT-25 · *(new, rev 9)* A standalone token that isn't a command stays in the composer with "No command /<word>." and Send as message, which posts the text as plain text; a near miss (a typed word within edit distance 1 of a command, or the same letters in a different order, as /bgu for /bug) adds " Did you mean /<cmd>?".
- CHAT-26 · *(new, rev 9)* Text with leading whitespace, a path (the token followed by "/", as in "/etc/hosts …"), a token followed by punctuation ("/tmp," or "/tmp.") and a bare "/" are sent normally as plain text.
- CHAT-27 · *(new, rev 9)* Closing a thread keeps its unsent reply and each ask's Reply and Reject text, per thread, while the page stays loaded, including after leaving Chat for another area and coming back. Reopening the thread restores the text with the cursor at the end, sending clears it, and there's no Discard dialog.
- CHAT-28 · *(new, rev 9)* Escape in a text field that has text doesn't close the thread; Escape elsewhere closes it and keeps the draft.
- CHAT-29 · *(new, rev 9)* When the chat content area (right of the app nav) is narrower than 768 px, the conversation list is a 256 px overlay, capped at the available width, opened from a "Conversations" button left of the conversation title. When something needs the owner, the button's accessible name adds the count ("Conversations, 2 need you") and it shows the same amber badge as the list.
- CHAT-30 · *(new, rev 9)* Opening the conversation overlay is modal and moves focus to the selected conversation's item. Choosing a conversation closes it and moves focus to that conversation's heading; Esc, Close or the scrim close it and return focus to the Conversations button.
- CHAT-31 · *(new, rev 9)* Thread and Members overlay the conversation below a 1280 px viewport, and take the full width below 640 px.
- CHAT-32 · *(new, rev 9)* Chat's titles are "#<channel> · Chat · Agentic Org", "<person or agent name> · Chat · Agentic Org", with Mentions, Threads and Search following the same title pattern. Earlier Needs you links open Inbox.
- CHAT-33 · *(new, rev 9)* The URL carries the place, so Back, Forward, reload and deep links open it; Back with a dialog, a sheet or the Conversations overlay open closes it only, as SHELL-28 says.
- CHAT-34 · *(new, rev 9)* With the office not running, every action that writes is disabled: the composer, asks, reactions, pin, mute, channel management (Remove, Add people, Rename, Archive) and Add channel. The disabled controls don't repeat the reason.
- CHAT-35 · *(new, rev 9)* With the office not running, a banner above the conversation reads "Read-only from the last copy. The office isn't running.", and the composer's note reads "The office isn't running. Restart it from Offices."

- CHAT-36 · Protected envelopes/attachments and required receipts survive office-cache loss; SQL is rebuildable and Git alone is insufficient. Private task notes never appear automatically in chat, search or attachments.
- CHAT-37 · Imported IDs and directed visibility remain stable; preserve and verify original v1 source hash/envelope separately from normalized canonical-JSON hashes. Retries retain original delivery identity.
- CHAT-38 · Scoped task links use T- for task/bug/request and qualify project/team scope outside it; legacy B-/REQ- links use explicit mappings with no blind renumbering.
- CHAT-39 · Accepting a handoff does not close a task/delete a worktree. Any assignment mutation commits both assignment fields atomically with expected revision and durable task history.
- CHAT-40 · Concurrent card updates surface revision conflicts; an unknown command/send outcome is reconciled through its retained operation/message identity before retry, without duplicate replies or approvals.

- CHAT-41 · Agent-authored emoji sequences are removed at render only, including joiners/variation selectors/modifiers/keycaps while ordinary digits/# remain; human text and protected stored bodies/hashes remain exact. Message kind markers use Heroicons plus words.
- CHAT-42 · MA-1–9 lifecycle/delivery acceptance in §§2.1–2.2/4.2 applies through authorized owning services; no reply/done/default timer/accept button can infer a task or execution transition.
- CHAT-43 · AC-CP-1–5 in §4.3 limit initial/older pages to 200 authorized messages and validate the 1,000-message scenario with separate CI/manual metrics.

## 9. Open questions

- Core conversation/recipient source shapes and directed import visibility are selected by document 15. Final wire/API adapters and recoverable source/SQL commits remain implementation work; old `chat.*`/`channel.*` sketches are not available command claims.
- Join/backfill/leave-history policy must explicitly resolve the earlier read-by-ID proposal without widening imported directed-thread access.
- Automatic channel sync, wake/default/digest services, announcement folding and task-transition integration require their owning services. Structural data does not select a scheduler/process engine.
- Optional Chat SDK bridges remain unselected; credentials/start lifetime must use the authorized standalone service boundary before shipping. Earlier A42/A44/bridge UX is retained in ADR-0003 for review.
