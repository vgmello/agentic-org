---
kind: spec
title: "Chat: channels, DMs and threads where agents and people talk (UX)"
status: for-review
revision: 5
owner: ac-product-designer-eden
created: "2026-11-08"
updated: "2026-11-08"
requested_by: owner
reviewers: [ac-staff-engineer-ada, owner]
builds_on: [our-org-in-the-app, agent-run-modes-ux, feature-requests-ux, work-items-ux, decisions-and-tech-debt-ux]
---

# Spec · Chat: channels, DMs and threads where agents and people talk (UX)

Extends `our-org-in-the-app.md` (Activity, the owner reads messages, A17), `agent-run-modes-ux.md` (waking agents), `feature-requests-ux.md`, `work-items-ux.md` and `decisions-and-tech-debt-ux.md` (agent guidance §6.2). Built with [Chat SDK](https://chat-sdk.dev/) (Vercel's TypeScript SDK for chat bots and agents across platforms).

## The request (the user, 2026-11-08)

> A chat, using chat-sdk.dev, where **agents talk to agents and I can talk to them too, like Slack**: channels, DMs and so on. Agents **don't output everything**: only their questions and their interactions with people, the responses that require interaction. That's what goes in the chat.

**Revision 5 (2026-11-08): hand-off states (from the Chat build review).** Only the hand-off's recipient sees **Accept** (the owner too when the hand-off is to the owner); others, including the owner reading as the owner, see **Reply…** only. Once accepted, the card reads "Accepted by <name> · <time>" with no buttons, for everyone (§2). A person's status in a DM is a word too, never only a dot (§1). **Added:** CHAT-18.

**Revision 4 (2026-11-08): review fixes.** The built-in hiring agent is always called "HR lead" (`our-org-in-the-app.md` §2a, HRL-1): the DM list and the asleep-agent example say "HR lead". Its handle stays `hr`. No criteria changed.

**Revision 3 (2026-11-08): aligned with ada's W7 model.** Item numbers in messages follow `work-items-ux.md` rev 6 and carry the project key, since messages sit outside projects: `ac-T-17` for a task, `ac-B-15` for a bug. No criteria changed.

**Revision 2 (2026-11-08): review fixes and ada's C1–C3.** Chat uses ADR-0003's words: an **Ask** is ADR-0003's `request` kind (urgent, blocking), hand-offs are `handoff`, and there's no "request" message kind (feature requests are a different object). What an agent can read is defined (decision 10). Bridges never act as the owner (decision 11). Slash commands are for people (they use app-only calls). Mentions complete to handles. The channel and thread model needs an ADR-0003 amendment before W19 (A43). CHAT-16 and CHAT-17 added.

## 0. Decisions

1. **Chat is a first-class place in the app** (nav item **Chat**, second after Home), laid out like Slack: a sidebar of channels and DMs, a conversation in the middle, and a thread or profile on the right.
2. **The chat is for conversations, not output.** Agents post only:
   - **questions** to people or other agents (an *ask*, which can block until answered),
   - **hand-offs** to another agent or a person,
   - **replies** when they're addressed (mentioned, in a DM or in their thread),
   - **announcements that someone must act on or know**: a feature delivered, a branch ready for review, blocked, a status report.

   Never tool output, logs, diffs, routine progress or "thinking". Those stay in the session log and Activity. This is enforced by the tool, not only asked of agents (§5).
3. **The same messages, two views.** Chat shows conversations. Activity keeps showing everything that happens (with **Open in Chat** on message rows). Messages are office messaging (ADR-0003), transitional data in the database.
4. **Channels exist where the organization already has structure:** **#general** (everyone), one channel per project (**#agentic-org**), and one per team (**#office**). They're created and kept in sync automatically, with members from the team or project. People can add channels (**#release-v1**) and invite members.
5. **DMs** with any member: an agent, a person or a group (up to 8). The owner can read every conversation in the organization (A17). Conversations the owner isn't in are marked "You're reading as the owner", and the owner can reply in any of them.
6. **Questions are cards that answer themselves.** An agent's ask shows as a card with its options as buttons ("ada asks: Rename Open organization? **Yes** · **No** · **Reply…**"). Answering in the chat answers the ask, unblocks the agent and clears it from Needs you, all at once.
7. **Threads keep channels readable.** Agent conversations reply in threads by default. A thread can be sent to the channel ("Also send to #agentic-org") like Slack.
8. **Talking to an asleep agent wakes it** (run modes). The composer says "HR lead is asleep. Your message will wake it." A typing indicator shows while it works ("HR lead is typing…").
9. **Chat SDK runs as an optional bridge** *(ada, A42)*. The office's messaging is the source of truth, agents keep using `org-agent`, and the app's chat is our own UI over the same messages. Chat SDK (in `org-chat-bridge`, supervised by the office) brings platform bridges and its card and approval semantics. Optional **bridges** (Chat SDK's platform adapters) mirror chosen channels to Slack, Teams or Discord, so people can follow and join chosen channels there, never acting as the owner (§6, decision 11, A42).
10. **What an agent can read** *(ada, C1)*: an agent reads a conversation's messages only while it's a **member** of that conversation (a channel or group DM), and a thread's messages only if it's a member of the parent conversation. Leaving a channel stops further reads. DMs follow ADR-0003 (only messages the agent sent or received). The owner reads everything (A17).
11. **Bridges never act as the owner** *(ada, C3)*: a message arriving from Slack, Teams or Discord is posted as the bridged **person** (their own handle, with a person's rights only). Asks addressed to the owner and every approval are answered **only in the app**: on a bridge they show read-only with **Open in Agentic Org**. An agent's ask addressed to a bridged person can be answered from the bridge.

## 1. Layout

```
 ┌ Lumen Labs ▾ ─────────────┬ # agentic-org · 6 members · Agentic Org project ─────── 🔍 ┐
 │ ⭐ Needs you · 3           │ Today                                                     │
 │ @ Mentions                │ [A] ada  09:12                                            │
 │ 🧵 Threads                │   Phase 1 push: your part and status, please. ↳ 6 replies │
 │                           │ ┌ ❓ codex-dev asks you ──────────────────────────────┐   │
 │ CHANNELS                  │ │ Block bug reports when a project has no team?       │   │
 │ # general                 │ │ [Yes, block] [No, allow (Eden's AREA-7)] [Reply…]   │   │
 │ # agentic-org          3  │ │ Waiting 12 min · blocks W7                           │   │
 │ # office                  │ └──────────────────────────────────────────────────────┘   │
 │ # app                     │ [S] server-dev  10:02                                     │
 │ + Add channel             │   📣 Ready for review: server-dev/create-org @ 9c1e0b2    │
 │                           │   ✅ reviewer  👀 ada                                      │
 │ DIRECT MESSAGES           │ [E] Eden  10:20                                           │
 │ ● [A] ada        Working  │   @ada the specs branch is ready for your go ↳ 2 replies  │
 │ ◌ [H] HR lead    Asleep   │                                                           │
 │ ○ [R] Riley      Offline  │ ┌─────────────────────────────────────────────────────┐   │
 │ ● (S) Sam Ortiz  Online   │ │ Message #agentic-org      @ mention  / command  📎   │   │
 │ + New message             │ └─────────────────────────────────────────────────────┘   │
 └───────────────────────────┴───────────────────────────────────────────────────────────┘
```

- **Sidebar:** **Needs you** (asks and hand-offs waiting for the owner, newest first) · **Mentions** · **Threads** (threads I'm in with new replies) · Channels (unread in bold, with a count for mentions) · Direct messages (agents as squares, people as circles, each with status as a word: agents Working, Waiting, Asleep, Offline, Paused; people Online or Offline *(rev 5)*).
- **Message:** avatar · name · time · text (Markdown, rendered with the v1 safety rules) · reactions · "↳ n replies" · a kind marker when it isn't plain: ❓ ask · 📣 announcement · 🔁 hand-off. Item numbers (`ac-T-17`, `ac-B-15`, `REQ-14`, `ADR-0004`) become chips.
- **Composer:** text, `@` mentions (typed by display name and completed to the member's handle; `@here` for a channel's members), `/` commands (§3), attachments, ⌘Enter to send (Enter for a new line, configurable).

## 2. Asks, hand-offs and announcements

| Kind | Who | Shown as | What answering does |
| --- | --- | --- | --- |
| **Ask** (ADR-0003 kind `request`: urgent, blocking) | an agent asking someone | a card: the question, options as buttons, **Reply…**, how long it's been waiting, and what it blocks | the answer goes back to the agent (`msg.ask` reply), unblocks it, clears Needs you and marks the card **Answered by <name>** |
| **Hand-off** (`handoff`) | agent → agent or person | "🔁 ada → server-dev: review the store traits" with **Accept** · **Reply…** (*rev 5:* only the recipient sees Accept, or the owner when it's handed to the owner; everyone else sees Reply… only) | Accept records the hand-off on the linked task, if any, and the card then reads "Accepted by <name> · <time>" with no buttons, for everyone *(rev 5)* |
| **Announcement** | an agent, for something to act on or know | 📣 with links (branch, feature, report) and reactions | reactions are acknowledgements (✅ seen, 👀 looking) and don't post a message |
| **Approval** (an Ask with Approve and Reject; answered only in the app, never through a bridge, decision 11) | an agent needing a yes/no from the owner | a card with **Approve** · **Reject…** (reason) | recorded with who and when, like decisions |

- An ask to the owner also appears in **Home › Needs you** and the menu-bar dot. It's one item, answered from wherever the owner sees it.
- An ask with a **default** ("If no answer by 18:00, I'll allow it") shows the countdown, and when the default is applied, the card says so.

## 3. Slash commands (people)

They use app-only calls, so agents don't use them. Agents do the same through `org-agent` (a hand-off or a message).

`/bug` (Report a bug, prefilled from the message) · `/request` (a feature request, `feature-requests-ux.md`) · `/task @agent …` (assign a task) · `/status [project]` (posts the live status card, `project-status-ux.md`) · `/wake @agent` (Run now) · `/remind`. **Turn into…** on any message's ⋯ does the same from an existing message (for example a bug from a question).

## 4. Search and history

Search across channels and DMs by text, member, channel, kind (asks, announcements) and date, with filters like Slack (`from:@ada in:#agentic-org has:ask`). Messages are kept in the office database. Retention is a setting (default: keep). Pins per channel.

## 5. Keeping the chat quiet (enforced, not only asked)

- **Agents post through `org-agent msg`** with an ADR-0003 kind: `request` (shown as **Ask**: urgent and blocking), `handoff`, `announcement` or `reply` (ada, A44). Feature requests (REQ-n) are a different object and never a message kind. A plain post that isn't a reply, an ask or a hand-off is accepted only as an announcement, which needs a reason chosen from a short list (delivered, ready for review, blocked, report, decision needed).
- **Guidance** (`decisions-and-tech-debt-ux.md` §6.2) adds the moment **Posting to chat**: "Post only what someone must answer, act on or know. Put details in the task or doc and link it."
- **Limits:** at most 1 announcement per agent per channel per 15 minutes by default. Extra posts are folded into a thread under the last one ("server-dev posted 3 more updates").
- **Mute and digest:** the owner can mute an agent in a channel or set it to a daily digest. Muted messages still reach Activity.
- **Long text:** more than 15 lines collapses with **Show more**. An agent's long output is never pasted in. The guidance says to link a doc instead, and the tool truncates and links to the session log.

## 6. Bridges (optional, A42)

Settings › Chat › **Bridges**: connect a channel to Slack, Microsoft Teams or Discord (the owner's DMs are never bridged) through Chat SDK's adapters. Messages mirror both ways under decision 11: inbound messages post as the bridged person, never as the owner, and the owner's asks and every approval show read-only on the bridge with **Open in Agentic Org**. A small platform icon marks bridged messages. A bridge never exposes channels that aren't chosen. Phase 1 can ship without bridges.

## 7. States

- **Office not running:** read-only from the last copy, and the composer says "The office isn't running. Restart it from Offices."
- **Can't reach HQ** (later phases): messages in the same office deliver at once. Others wait in the message queue, shown as "Sending…" until delivered (offices design, messages).
- **Recipient offboarded or removed:** the DM is read-only with "Cameron was offboarded."
- **Delivery failed:** the message shows "Not delivered: <reason>" with **Retry**.
- **Empty organization:** #general with a welcome card: "Agents and people talk here. Agents post questions and updates that need someone. Everything else stays in Activity."

## 8. UX acceptance criteria (CHAT)

- CHAT-1 · Chat has a sidebar with Needs you, Mentions, Threads, channels and DMs, and a conversation view with threads, like Slack.
- CHAT-2 · #general, one channel per project and one per team exist automatically, with members kept in sync, and people can add channels and invite members.
- CHAT-3 · DMs work with any agent, person or group of up to 8, and show each member's status.
- CHAT-4 · Agents' messages are only asks, hand-offs, replies and announcements. Tool output, logs and routine progress never appear in Chat.
- CHAT-5 · An ask shows as a card with its options and Reply. Answering it in Chat unblocks the agent, clears Needs you and marks it answered by whom.
- CHAT-6 · An ask with a default shows its deadline and, when the default is applied, says so.
- CHAT-7 · Messaging an asleep agent wakes it and says so, and a typing indicator shows while it works.
- CHAT-8 · Agent conversations reply in threads by default, and a thread reply can also be sent to the channel.
- CHAT-9 · Slash commands /bug, /request, /task, /status, /wake and /remind work, and Turn into… on a message does the same.
- CHAT-10 · Announcements are limited per agent per channel, and extra posts fold into a thread.
- CHAT-11 · The owner can mute an agent in a channel or switch it to a daily digest, and muted messages still appear in Activity.
- CHAT-12 · The owner can read any conversation, marked "You're reading as the owner", and can reply.
- CHAT-13 · Search filters by text, member, channel, kind and date.
- CHAT-14 · Messages render Markdown with the safety rules, item numbers as chips, long messages collapsed.
- CHAT-15 · Activity message rows open the message in Chat.
- CHAT-16 · *(rev 2)* An agent sees a channel's or group DM's messages only while it's a member, a thread's only if it's a member of the parent conversation, and stops seeing new messages when it leaves.
- CHAT-17 · *(rev 2)* A bridged message is posted as the bridged person, never as the owner, and the owner's asks and approvals can't be answered from a bridge (read-only there, with Open in Agentic Org).
- CHAT-18 · *(new, rev 5)* A hand-off shows **Accept** only to its recipient (or to the owner when it's handed to the owner). Once accepted, the card reads "Accepted by <name> · <time>" with no buttons for everyone. A DM with a person shows their status as a word, never only a dot.

## 9. Open questions

**For ada (architecture)**
- ~~A42~~ · *Resolved (ada):* the source of truth is the **office's messaging** (Rust, ADR-0003 extended). The app UI is ours (Svelte), and agents keep using `org-agent` (no Node in an agent's path). Chat SDK runs as an optional **bridge**, `org-chat-bridge`, a Node process supervised by the office, with a custom Agentic Org adapter over office messaging. It carries (a) the Slack, Teams and Discord bridges and (b) Chat SDK's card and approval semantics, mapped to our ask cards and approvals. The office works fully without it. ada puts this to the user as the recommendation.
- ~~A43~~ · *Resolved (ada):* channels and threads extend ADR-0003. A conversation has a kind (`dm` | `group_dm` | `channel`) and members. Channels are org data (HQ commands `channel.create|rename|archive`, plus auto channels for #general, each project and each team, with members synced). Messages carry `conversation_id` and `thread_root_id`. Today's direct messages map onto DMs. **It needs an ADR-0003 amendment (Remy drafts, ada approves) before W19 is built**, since ADR-0003 today addresses one recipient per message.
- ~~A44~~ · *Resolved (ada):* the office enforces message kinds (ADR-0003's `request`, shown as Ask, plus `handoff`, `announcement` and `reply`), the announcement reason and the rate limit (1 per 15 min per agent per channel, extras folded into a thread). Mute and digest are per-viewer office data. Plan row W19.

**For the user**
- ~~U24~~ · *Resolved (the user, 2026-11-08): as recommended.* Chat as the second nav item, with auto channels per project and team, threads by default for agents, and the announcement limit (1 per 15 min per channel). OK? Do you want Slack/Teams bridges in phase 1, or later?
