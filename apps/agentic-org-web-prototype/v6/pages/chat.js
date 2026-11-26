import { CH, IN, M, OFFICES } from "../data.js";
import { fitTextarea } from "./work.js";
import { openSmartTaskComposer } from "./task-composer.js";
import { applyPos, jumpToAsk, placeStream, replaceStream, savedPos, trackPos, watchAsks } from "./chat-scroll.js";
import { ASK_CLICKS, messageNeeds, onAskKeydown, resetAsks, saveAskDraft, workRef } from "./chat-asks.js";
import { CHAT_VIEWS, chatViewHtml, isChatView } from "./chat-views.js";
import { bindOverlays, syncOverlayHistory } from "./chat-overlays.js";
import { chatListHtml, needsCount, onListClick, onListInput, onListKeydown, prepareList, takeListFocus, unreadDot } from "./chat-list.js";
import {
  DEFAULT_CHAT, chatTitle, dmSources, inboxItem, openAsks, openDirectMessage, resolveChatId, sourceHref, sourceLabel,
} from "./chat-model.js";
import { render, route } from "../shell/router.js";
import { CHAT_SEND_SHORTCUTS, S } from "../state.js";
import {
  av, esc, ic, joinSep, kindWord, mlink, moreMenu, plink, plural, presence, statusLine,
} from "../ui.js";

const chatDrafts = new Map();      /* conversation id -> unsent message */
const threadDrafts = new Map();    /* thread root message id -> unsent thread reply */
const local = { search: null, linked: null, pendingLink: null, details: false, members: false, threadFrom: "", openThread: null };
const OVERLAY_THREAD = "(max-width: 1279px)";
const NARROW_LIST = "(max-width: 959px)";
const matchesMedia = (q) => Boolean(window.matchMedia?.(q).matches);

/* ---------------------------------------------------------------- names and time */
function chatDocTitle(id) {
  if (isChatView(id)) return `${CHAT_VIEWS[id].title} · Chat`;
  return `${chatTitle(CH[id && Object.hasOwn(CH, id) ? id : DEFAULT_CHAT])} · Chat`;
}
const DAY_RE = /^([A-Z][a-z]{2} \d{1,2}|Yesterday),?\s*/;
function splitTime(time) {
  const day = DAY_RE.exec(time || "");
  return day ? [day[1], time.slice(day[0].length)] : ["Today", time || ""];
}
const clock = () => new Date().toTimeString().slice(0, 5);
const authorWord = (who) => (M[who].owner ? "" : kindWord(who));
const sentAt = (m) => splitTime(m.time).join(" ");

/* ---------------------------------------------------------------- message text: T- refs and @mentions become links */
function mentionRef(match, name) {
  const lower = name.toLowerCase();
  const key = Object.keys(M).find((k) => k === lower || M[k].name.split(" ")[0].toLowerCase() === lower);
  return key && !M[key].owner ? `<a class="alink chat-ref" href="#/members/${key}">@${esc(M[key].name)}</a>` : match;
}
const richText = (text) => esc(text).replace(/\bT-A\d+\b/g, workRef).replace(/@([A-Za-z]+)/g, mentionRef);

/* ---------------------------------------------------------------- one message */
/* Hover tools on wide pointers; one touch-sized menu (with the message's time) on narrow or touch screens. */
function toolButton(act, id, icon, label) {
  return `<button class="btn ghost icon msg-tool" type="button" data-act="${act}" data-id="${id}" aria-label="${esc(label)}" title="${esc(label)}">${ic(icon, "ic16")}</button>`;
}
const menuEntry = (act, id, icon, label) => `<button role="menuitem" type="button" data-act="${act}" data-id="${id}">${ic(icon, "ic20")}${esc(label)}</button>`;
function messageTools(m, readOnly) {
  const id = esc(m.id), name = M[m.who].name;
  const tools = readOnly
    ? (M[m.who].owner ? [] : [["chat-dm-msg", "chat", `Message ${name} about this`]])
    : [["thread", "thread", "Reply in thread"], ["chat-task", "clipboard", "Turn into task"]];
  if (!tools.length) return "";
  const time = `<p class="msg-menu-time" role="none">${ic("clock", "ic14")}Sent ${esc(sentAt(m))}</p>`;
  const menu = time + tools.map(([act, icon, label]) => menuEntry(act, id, icon, label)).join("");
  return `<span class="msg-tools" role="group" aria-label="Message actions">
    ${tools.map(([act, icon, label]) => toolButton(act, id, icon, label)).join("")}
    <span class="msg-menu">${moreMenu("msg-" + m.id, `Actions for message from ${name}, ${sentAt(m)}`, menu, "ghost")}</span>
  </span>`;
}
const sendingStatus = () => `<span class="msg-status" role="status">Sending…</span>`;
function threadButton(m) {
  if (!m.thread || !m.thread.length) return "";
  return `<button class="thr" type="button" data-act="thread" data-id="${esc(m.id)}" aria-expanded="${S.thread === m.id}">${ic("thread", "ic16")}${plural(m.thread.length, "reply", "replies")}</button>`;
}
/* A DM's first message can link to the conversation or the exact message it is about. */
function sourceLine(m) {
  const src = typeof m.src === "string" ? { ch: m.src } : m.src;
  if (!src || !CH[src.ch]) return "";
  return `<p class="msg-src">${ic("eye", "ic14")}<span>About <a class="alink chat-ref" href="${esc(sourceHref(src))}">${esc(sourceLabel(src))}</a></span></p>`;
}
/* A thread reply also sent to the channel links back to its thread. */
function threadCopyLine(m) {
  if (!m.inThread) return "";
  const { id, who } = m.inThread;
  return `<p class="msg-src">${ic("thread", "ic14")}<span>Sent from <button class="alink chat-ref" type="button" data-act="thread" data-id="${esc(id)}">the thread on ${esc(M[who].name)}'s message</button></span></p>`;
}
const continues = (m, prev) => prev && prev.who === m.who && splitTime(prev.time)[0] === splitTime(m.time)[0] && !prev.needs;
function messageHead(m, time) {
  const word = authorWord(m.who);
  const status = m.pending ? sendingStatus() : time ? `<time class="msg-time">${esc(time)}</time>` : "";
  return `<p class="hd"><b>${mlink(m.who)}</b>${word ? `<span class="msg-kind">${word}</span>` : ""}${status}</p>`;
}
function messageHtml(m, prev, opts = {}) {
  const time = splitTime(m.time)[1], cont = continues(m, prev);
  const head = cont ? `<p class="sr-only">${esc(M[m.who].name)}</p>` : messageHead(m, time);
  const gutter = cont ? `<span class="msg-gutter">${m.pending ? "" : `<time class="msg-time">${esc(time)}</time>`}</span>` : av(m.who, 32);
  /* A grouped message has no header, so its send status sits after the text, never in the narrow gutter. */
  const status = cont && m.pending ? sendingStatus() : "";
  const tools = opts.tools ? messageTools(m, opts.readOnly) : "";
  const needs = messageNeeds(m, opts.tools && S.thread === m.id ? "in-thread" : opts.cards);
  return `<div class="msg${cont ? " cont" : ""}${m.pending ? " is-pending" : ""}" data-mid="${esc(m.id)}">${gutter}<div class="body">${head}
    <p class="tx">${richText(m.text)}${status}</p>${tools}
    ${sourceLine(m)}${threadCopyLine(m)}${needs}${threadButton(m)}</div></div>`;
}
/* Shared preview (project Overview) renders chips, not cards; it is called as Array.map(msgHtml). */
const msgHtml = (m, i, list) => messageHtml(m, list && i ? list[i - 1] : null);

function emptyConversation(ch) {
  const dm = ch.kind === "dm", name = dm ? M[ch.with].name : chatTitle(ch);
  const text = dm ? `Messages here go only to ${name}.` : ch.topic || "";
  return `<div class="chat-empty">${dm ? av(ch.with, 32) : ic("hash", "ic20")}<h2>This is the start of ${dm ? "your conversation with " : ""}${esc(name)}</h2>${text ? `<p>${esc(text)}</p>` : ""}</div>`;
}
const searchQuery = (id) => (local.search && local.search.id === id ? local.search.q.trim().toLowerCase() : "");
const matches = (m, q) => m.text.toLowerCase().includes(q) || M[m.who].name.toLowerCase().includes(q);
function noMatches(ch, q) {
  return `<div class="chat-empty"><h2>No messages match “${esc(q)}”</h2><p>Search looks at the messages in ${esc(chatTitle(ch))}.</p></div>`;
}
function streamHtml(id) {
  const ch = CH[id], q = searchQuery(id);
  if (!ch.msgs.length) return emptyConversation(ch);
  const list = q ? ch.msgs.filter((m) => matches(m, q)) : ch.msgs;
  if (!list.length) return noMatches(ch, q);
  const readOnly = ch.kind === "a2a";
  const opts = { cards: readOnly ? "chip" : "card", tools: true, readOnly };
  let lastDay = "";
  return list.map((m, i) => {
    const [day] = splitTime(m.time);
    const divider = day !== lastDay ? `<h2 class="chat-day"><span>${esc(day)}</span></h2>` : "";
    lastDay = day;
    return divider + messageHtml(m, i ? list[i - 1] : null, opts);
  }).join("");
}

/* ---------------------------------------------------------------- composers */
function shortcutHint() {
  const keys = { either: "Ctrl/⌘", control: "Ctrl", command: "⌘" }[S.chatSend] || "Ctrl/⌘";
  return `<span class="sr-only">Enter adds a new line. ${CHAT_SEND_SHORTCUTS[S.chatSend].label} sends.</span><span aria-hidden="true"><kbd>${keys}</kbd> <kbd>Enter</kbd> to send · <kbd>Enter</kbd> new line</span>`;
}
/* Main and thread composers share markup, the send hint and empty-send validation. */
function composerHtml(o) {
  const shortcut = CHAT_SEND_SHORTCUTS[S.chatSend];
  return `<form class="compose chat-compose${o.cls || ""}" data-form="${o.form}" data-id="${esc(o.id)}"${o.ch ? ` data-ch="${esc(o.ch)}"` : ""}>
    ${o.before || ""}${o.note ? `<p class="chat-compose-note" id="${o.field}-note">${o.note}</p>` : ""}
    <div class="chat-compose-row">
      <label class="sr-only" for="${o.field}">${esc(o.label)}</label>
      <textarea class="field" id="${o.field}" name="message" rows="1" placeholder="${esc(o.label)}" aria-describedby="${o.note ? o.field + "-note " : ""}${o.field}-help" autocomplete="off" enterkeyhint="enter">
${esc(o.draft || "")}</textarea>
      <button class="btn ${o.button}" type="submit" title="${o.submit} (${shortcut.label})" aria-keyshortcuts="${shortcut.keys}">${ic("send", "ic20")}${o.submit}</button>
    </div>
    <p class="chat-compose-hint" id="${o.field}-help" aria-live="polite">${shortcutHint()}</p>
    ${o.after || ""}
  </form>`;
}
function composeSource(id) {
  const src = dmSources.get(id);
  if (!src || !CH[src.ch]) return "";
  const label = esc(sourceLabel(src));
  return `<p class="chat-compose-src">${ic("eye", "ic14")}<span>Your message will link to <a class="alink chat-ref" href="${esc(sourceHref(src))}">${label}</a></span>
    <button class="btn ghost sm" type="button" data-act="chat-src-remove" data-id="${esc(id)}" aria-label="Don't link ${label}">${ic("x", "ic16")}Remove</button></p>`;
}
/* Why a DM's agent can't run right now (doc-40 §0 item 8): the composer says so beside the field. */
function cannotRun(who) {
  const m = M[who], setup = m.setup, office = setup?.office && OFFICES[setup.office];
  if (m.kind !== "agent" || !setup) return "";
  if (!setup.office) return [`${m.name} can't run right now: ${setup.needs || "not placed in an office"}. Your message waits until ${m.name.split(" ")[0]} is placed.`, `#/settings/agents/${who}`];
  if (m.paused) return [`${m.name} is paused, so it won't answer until it's resumed.`];
  if (office && office.status === "Offline") return [`${m.name} can't run right now: its office, ${office.name}, is offline. Your message waits until it reconnects.`];
  return "";
}
function availabilityNote(ch) {
  const why = ch.kind === "dm" && cannotRun(ch.with);
  if (!why) return "";
  const [text, href] = why;
  return `${ic("info", "ic14")}<span>${esc(text)}${href ? ` <a class="alink chat-ref" href="${esc(href)}">Open Agent setup</a>` : ""}</span>`;
}
const chatComposer = (id, title) => composerHtml({
  form: "send", id, field: "compose", label: "Message " + title, draft: chatDrafts.get(id), button: "primary", submit: "Send",
  before: composeSource(id), note: availabilityNote(CH[id]),
});
/* Cross-posting exists only where a thread hangs off a channel; in a DM it would duplicate the same reader's stream. */
const threadComposer = (id, root) => composerHtml({
  form: "thread-send", id: root.id, ch: id, cls: " thread-compose", field: "thread-compose", label: "Reply in thread",
  draft: threadDrafts.get(root.id), button: "outline", submit: "Reply",
  after: CH[id].kind === "channel" ? `<label class="chat-also"><input type="checkbox" name="also"> Also send to ${esc(chatTitle(CH[id]))}</label>` : "",
});
function showEmptySendHint(form, field) {
  const hint = form.querySelector(".chat-compose-hint");
  hint.classList.add("is-error");
  hint.innerHTML = `${ic("info", "ic14")}<span>Write a message first.</span>`;
  field.focus();
  field.addEventListener("input", () => { hint.classList.remove("is-error"); hint.innerHTML = shortcutHint(); }, { once: true });
}
const streamFor = (id) => document.querySelector(`.chat .chmain .msgs[data-ch="${CSS.escape(id)}"]`);
function confirmSent(id, message) {
  message.pending = false;
  const s = streamFor(id);
  if (s) replaceStream(s, streamHtml(id));
}
function sendChatMessage(form) {
  const field = form.elements.message;
  if (!field.value.trim()) { showEmptySendHint(form, field); return; }
  const id = form.dataset.id, ch = CH[id];
  /* Prototype: the office confirms after a short delay; until then the message says "Sending…". */
  const message = { id: "u" + Date.now(), who: "you", time: clock(), text: field.value, pending: true };
  if (dmSources.has(id)) { message.src = dmSources.get(id); dmSources.delete(id); form.querySelector(".chat-compose-src")?.remove(); }
  ch.msgs.push(message);
  chatDrafts.delete(id);
  local.search = null;
  document.querySelector(".chat-search-row")?.remove();
  field.value = "";
  fitTextarea(field);
  replaceStream(streamFor(id), streamHtml(id), true);
  field.focus();
  setTimeout(() => confirmSent(id, message), 700);
}
function sendThreadReply(form) {
  const field = form.elements.message, text = field.value.trim();
  if (!text) { showEmptySendHint(form, field); return false; }
  const ch = CH[form.dataset.ch], root = ch.msgs.find((m) => m.id === form.dataset.id);
  const reply = { who: "you", time: clock(), text };
  (root.thread ||= []).push(reply);
  threadDrafts.delete(root.id);
  if (form.elements.also?.checked && ch.kind === "channel") ch.msgs.push({ id: "u" + Date.now(), ...reply, inThread: { id: root.id, who: root.who } });
  return true;
}

/* ---------------------------------------------------------------- header */
/* The channel's own admitted members (doc-40 §0 item 4), not its project's or team's roster. */
const channelMembers = (ch) => (ch.members || []).filter((k) => M[k]);
function membersLink(ch) {
  const n = channelMembers(ch).length;
  return n ? `<button class="alink chat-members-btn" type="button" data-act="chat-members" aria-expanded="${local.members}" aria-controls="chat-members">${plural(n, "member", "members")}</button>` : "";
}
/* "Waiting on you" in a DM names requests raised elsewhere as "Also waiting in Inbox"; the DM's own ask is on its card. */
function dmWaiting(who, chId) {
  const here = new Set(openAsks(CH[chId]));
  const elsewhere = IN.filter((i) => !i.handled && i.from === who && !here.has(i.id));
  if (!elsewhere.length) {
    const it = inboxItem([...here][0]), m = it && CH[chId].msgs.find((x) => x.needs === it.id);
    return it ? `Waiting on you: <a class="alink" href="#/chat/${esc(chId)}/${esc(m.id)}">${esc(it.what)}</a>` : "";
  }
  const more = elsewhere.length > 1 ? ` and ${elsewhere.length - 1} more` : "";
  return `Also waiting in Inbox: <a class="alink" href="#/inbox/${esc(elsewhere[0].id)}">${esc(elsewhere[0].what)}</a>${more}`;
}
function dmStatus(who, chId) {
  const m = M[who];
  if (m.kind === "person") return esc(m.last || "");
  return presence(who) === "needs" ? dmWaiting(who, chId) : statusLine(who);
}
/* The parts of a conversation's context: a line under the title when wide, the Details panel when narrow. */
function contextParts(ch, chId) {
  if (ch.kind === "dm") {
    const status = dmStatus(ch.with, chId);
    return [["Role", esc(M[ch.with].role)], ["Kind", kindWord(ch.with)], ["Status", status ? `<span class="chat-status">${status}</span>` : ""]];
  }
  /* The owner's role leads the context; the footer offers the ways to weigh in. */
  if (ch.kind === "a2a") return [["Access", `<span class="chat-readas">${ic("eye", "ic14")}Reading as the owner</span>`], ["Kind", "Between agents"], ["Agents", ch.pair.map((k) => mlink(k)).join(" and ")]];
  return [
    ["Topic", ch.topic ? `<span class="chat-topic">${esc(ch.topic)}</span>` : ""],
    ["Project", ch.proj ? `<span class="chat-proj">${ic("folder", "ic14")}${plink(ch.proj)}</span>` : ""],
    ["Members", membersLink(ch)],
  ];
}
const headerContext = (ch, chId) => joinSep(contextParts(ch, chId).map(([, v]) => (v ? `<span>${v}</span>` : "")));
function detailsPanel(ch, chId) {
  if (!local.details) return "";
  const rows = contextParts(ch, chId).filter(([, v]) => v).map(([k, v]) => `<div><dt>${k}</dt><dd>${v}</dd></div>`).join("");
  return `<dl class="chat-details" id="chat-details" aria-label="Conversation details">${rows}</dl>`;
}
function headerLead(ch) {
  if (ch.kind === "dm") return av(ch.with, 32);
  return ch.kind === "a2a" ? `<span class="chat-pair" aria-hidden="true">${ch.pair.map((k) => av(k, 24)).join("")}</span>` : "";
}
function headerActions(id, title) {
  const open = local.search?.id === id;
  return `<div class="ph-actions chat-actions">
      <button class="btn ghost sm chat-details-btn" type="button" data-act="chat-details" aria-expanded="${local.details}" aria-controls="chat-details">${ic("info", "ic16")}Details</button>
      <button class="btn ghost icon chat-search" type="button" data-act="chat-search" aria-expanded="${open}" aria-controls="chat-search-row" aria-label="Search in ${esc(title)}" title="Search in ${esc(title)}">${ic("search", "ic20")}</button>
    </div>`;
}
/* Asks waiting in other conversations, each counted once; unread elsewhere shows as a dot. */
function listButton(id) {
  const here = new Set(CH[id] ? openAsks(CH[id]) : []);
  const elsewhere = new Set(Object.keys(CH).filter((k) => k !== id).flatMap((k) => openAsks(CH[k])).filter((x) => !here.has(x))).size;
  const unread = Object.keys(CH).some((k) => k !== id && CH[k].unread);
  /* The name always begins with the visible word, whatever the width hides; the badge and dot are described in it. */
  const name = ["Conversations", elsewhere ? plural(elsewhere, "request", "requests") + " waiting on you in other conversations" : "", unread ? "unread messages" : ""].filter(Boolean).join(", ");
  const badge = elsewhere ? `<span aria-hidden="true">${needsCount(elsewhere)}</span>` : "";
  return `<button class="btn ghost chat-list-btn" type="button" data-act="chat-list" data-id="${id}" aria-expanded="${S.chatList === id}" aria-controls="chat-list" aria-label="${esc(name)}" title="Conversations">${ic("chat", "ic20")}<span class="chat-list-label">Conversations</span>${badge}${unread ? unreadDot : ""}</button>`;
}
function chatHeaderHtml(id, ch, title) {
  return `<header class="ph chhead">
    ${listButton(id)}
    <div class="ph-t chat-title">
      ${headerLead(ch)}
      <div class="chat-heading"><h1 id="ch-title" tabindex="-1">${esc(title)}</h1>
        <p class="chat-context">${headerContext(ch, id)}</p>
      </div>
    </div>
    ${headerActions(id, title)}
    ${detailsPanel(ch, id)}
  </header>`;
}
function searchRowHtml(id, title) {
  if (local.search?.id !== id) return "";
  const q = local.search.q.trim(), n = q ? CH[id].msgs.filter((m) => matches(m, q.toLowerCase())).length : null;
  return `<div class="chat-search-row" id="chat-search-row" role="search">
    ${ic("search", "ic16")}<label class="sr-only" for="chat-search-input">Search messages in ${esc(title)}</label>
    <input class="field" id="chat-search-input" type="search" placeholder="Search messages in ${esc(title)}" value="${esc(local.search.q)}" autocomplete="off" aria-describedby="chat-search-status">
    <span class="chat-search-status" id="chat-search-status" role="status">${n === null ? "" : plural(n, "message", "messages")}</span>
    <button class="btn ghost icon" type="button" data-act="chat-search" aria-label="Close search">${ic("x", "ic20")}</button>
  </div>`;
}

/* ---------------------------------------------------------------- side panels (thread, members) and read-only footer */
/* Where a side panel covers the conversation it is a modal dialog with a scrim (made inert-outside on mount). */
const panelModal = () => (matchesMedia(OVERLAY_THREAD) ? ' role="dialog" aria-modal="true"' : "");
function chatThreadHtml(id, root, title) {
  if (!root) return "";
  const replies = root.thread || [], readOnly = CH[id].kind === "a2a";
  const count = replies.length ? plural(replies.length, "reply", "replies") : "No replies yet";
  const list = replies.map((r, i) => messageHtml(Object.assign({ id: root.id + "-r" + i }, r), i ? replies[i - 1] : null)).join("");
  /* A writable thread carries the ask's own card, answerable here; the stream points to it while the thread is open. */
  const head = messageHtml(Object.assign({}, root, { thread: null }), null, { cards: readOnly ? "chip" : "card" });
  return `<div class="thread-scrim" data-act="thread-close" data-scrim aria-hidden="true"></div>
    <aside class="thread"${panelModal()} aria-labelledby="chat-thread-title"><header class="side-panel-header"><div class="thread-title"><h2 id="chat-thread-title" tabindex="-1">Thread</h2><span class="muted">${esc(title)}</span></div><button class="btn ghost icon" type="button" data-act="thread-close" aria-label="Close thread">${ic("x", "ic20")}</button></header>
      <div class="msgs">${head}<p class="chat-thread-count">${count}</p>${list}</div>
      ${readOnly ? "" : threadComposer(id, root)}</aside>`;
}
function memberRow(k) {
  const m = M[k], name = m.owner ? "You" : mlink(k);
  const dm = m.owner ? "" : `<a class="btn ghost sm" href="#/chat/dm-${esc(k)}" aria-label="Message ${esc(m.name)}">${ic("chat", "ic16")}Message</a>`;
  return `<li class="chat-member">${av(k, 32)}<span class="chat-member-who"><b>${name}</b><span>${m.owner ? "Owner" : `${esc(m.role)} · ${kindWord(k)}`}</span></span>${dm}</li>`;
}
function membersPanelHtml(ch, title) {
  if (!local.members || ch.kind !== "channel") return "";
  const ids = channelMembers(ch);
  return `<div class="thread-scrim" data-act="chat-members" data-scrim aria-hidden="true"></div>
    <aside class="thread chat-members-panel" id="chat-members"${panelModal()} aria-labelledby="chat-members-title"><header class="side-panel-header"><div class="thread-title"><h2 id="chat-members-title" tabindex="-1">Members</h2><span class="muted">${esc(title)} · ${ids.length}</span></div><button class="btn ghost icon" type="button" data-act="chat-members" aria-label="Close members">${ic("x", "ic20")}</button></header>
      <ul class="chat-member-list">${ids.map(memberRow).join("")}</ul></aside>`;
}
function readOnlyFoot(id, ch) {
  const buttons = ch.pair.map((k) => `<button class="btn outline sm" type="button" data-act="chat-dm" data-id="${k}" data-v="${esc(id)}">${ic("chat", "ic16")}Message ${esc(M[k].name)}</button>`).join("");
  return `<div class="readfoot"><p>${ic("eye", "ic16")}You can't reply here. To weigh in, message ${ch.pair.map((k) => esc(M[k].name)).join(" or ")}.</p><div class="actions">${buttons}</div></div>`;
}
const askJump = () => `<div class="chat-asks-jump"><button class="btn sm" type="button" data-act="chat-asks-jump" hidden>${ic("bell", "ic16")}<span></span>${ic("chev-down", "ic16 chat-up")}</button></div>`;

/* ---------------------------------------------------------------- page */
/* A source link (#/chat/<ch>/<msg>) opens the unfiltered stream on that exact message, once per link. */
function takeLink(id) {
  const target = route()[2];
  if (!target || local.linked === location.hash || !CH[id].msgs.some((m) => m.id === target)) return;
  Object.assign(local, { linked: location.hash, pendingLink: target, search: null });
}
/* "Open thread" from the Threads view opens the conversation with that thread. */
function takeThread(id) {
  if (local.openThread?.ch !== id) return;
  S.thread = local.openThread.id;
  local.threadFrom = "";
}
function chatFrame(id, cls, header, main, side = "") {
  const listOpen = S.chatList === id;
  return `<div class="${["chat", ...cls, listOpen ? "list-open" : ""].filter(Boolean).join(" ")}" data-ch="${esc(id)}">
    ${header}
    ${chatListHtml(id, listOpen)}
    <div class="chat-scrim" data-act="chat-list" data-id="${id}" data-scrim aria-hidden="true"></div>
    <section class="chmain" aria-labelledby="ch-title">${main}</section>
    ${side}
  </div>`;
}
function pageChatView(kind) {
  if (S.chatList && S.chatList !== kind) S.chatList = null;
  const view = CHAT_VIEWS[kind];
  const header = `<header class="ph chhead">${listButton(kind)}
    <div class="ph-t chat-title"><div class="chat-heading"><h1 id="ch-title" tabindex="-1">${esc(view.title)}</h1><p class="chat-context">${esc(view.about)}</p></div></div></header>`;
  return chatFrame(kind, [], header, chatViewHtml(kind));
}
function pageChat(id) {
  if (isChatView(id)) return pageChatView(id);
  id = resolveChatId(id);
  const ch = CH[id];
  ch.unread = 0;
  if (S.chatList && S.chatList !== id) S.chatList = null;
  takeLink(id);
  takeThread(id);
  if (S.thread) local.members = false;
  const root = S.thread && ch.msgs.find((m) => m.id === S.thread);
  const title = chatTitle(ch), members = local.members && ch.kind === "channel";
  const cls = [root || members ? "withthread" : "", local.details ? "details-open" : ""];
  const main = `${searchRowHtml(id, title)}${askJump()}
      <div class="msgs" aria-label="Messages" data-ch="${esc(id)}">${streamHtml(id)}</div>
      ${ch.kind === "a2a" ? readOnlyFoot(id, ch) : chatComposer(id, title)}`;
  return chatFrame(id, cls, chatHeaderHtml(id, ch, title), main, chatThreadHtml(id, root, title) + membersPanelHtml(ch, title));
}

/* ---------------------------------------------------------------- after render: position, overlays, local controls */
let inerted = [];
function releaseInert() {
  inerted.forEach((el) => { el.inert = false; });
  inerted = [];
}
/* A modal overlay (narrow conversation list, overlay side panel): everything outside it but scrims is inert. */
function inertOutside(el) {
  for (let n = el; n.parentElement && n !== document.body; n = n.parentElement) {
    for (const sib of n.parentElement.children) {
      if (sib === n || sib.inert || sib.hasAttribute("data-scrim") || sib.tagName === "SCRIPT") continue;
      sib.inert = true;
      inerted.push(sib);
    }
  }
}
const initialPos = (id) => (local.pendingLink ? { target: local.pendingLink } : savedPos(id) || { boundary: true });
function markLinked(s) {
  const target = route()[2], el = target && s.querySelector(`:scope > [data-mid="${CSS.escape(target)}"]`);
  el?.classList.add("is-linked");
}
/* The linked message takes focus (after the router focused the heading), so keyboard and screen-reader users land on it. */
function focusLinked(s) {
  const el = local.pendingLink && s.querySelector(`:scope > [data-mid="${CSS.escape(local.pendingLink)}"]`);
  local.pendingLink = null;
  if (!el) return;
  el.tabIndex = -1;
  el.setAttribute("aria-label", `Linked message from ${el.querySelector(".hd b")?.textContent || "this conversation"}`);
  el.focus({ preventScroll: true });
}
function highlightMatches(s, q) {
  if (!q) return;
  const walker = document.createTreeWalker(s, NodeFilter.SHOW_TEXT, { acceptNode: (n) => (n.parentElement.closest(".tx, .hd b") ? NodeFilter.FILTER_ACCEPT : NodeFilter.FILTER_REJECT) });
  const nodes = [];
  while (walker.nextNode()) nodes.push(walker.currentNode);
  for (const node of nodes) {
    const i = node.data.toLowerCase().indexOf(q);
    if (i < 0) continue;
    const hit = node.splitText(i);
    hit.splitText(q.length);
    const mark = document.createElement("mark");
    hit.replaceWith(mark);
    mark.append(hit);
  }
}
function refreshSearch(s) {
  const id = s.dataset.ch, q = searchQuery(id);
  s.innerHTML = streamHtml(id);
  highlightMatches(s, q);
  applyPos(s, q ? { top: true } : { end: true });
  const status = document.getElementById("chat-search-status");
  if (status) status.textContent = q ? plural(CH[id].msgs.filter((m) => matches(m, q)).length, "message", "messages") : "";
}
function toggleSearch(id) {
  local.search = local.search?.id === id ? null : { id, q: "" };
  render(false);
  (local.search ? document.getElementById("chat-search-input") : document.querySelector(".chat-search"))?.focus();
}
function turnIntoTask(t, id, mid) {
  const ch = CH[id], m = ch.msgs.find((x) => x.id === mid);
  if (!m) return;
  const prompt = `${m.text}\n\nFrom ${M[m.who].name} in ${chatTitle(ch)} (${m.time}): #/chat/${id}/${m.id}`;
  openSmartTaskComposer(t, { prompt });
}

/* ---------------------------------------------------------------- overlays: open, close, focus return, Back */
const visible = (el) => el && el.getClientRects().length > 0;
const focusFirst = (selectors) => selectors.filter(Boolean).map((sel) => document.querySelector(sel)).find(visible)?.focus();
/* Threads: focus returns to the control that opened it, or the visible one for that message. */
function threadOpener(t, id) {
  const menu = t.closest(".menu");
  if (menu) return `[aria-controls="${menu.id}"]`;
  return t.classList.contains("thr") ? `.chmain .thr[data-id="${CSS.escape(id)}"]` : `.chmain .msg-tool[data-act="thread"][data-id="${CSS.escape(id)}"]`;
}
function focusThreadOpener(id) {
  const e = CSS.escape(id);
  focusFirst([local.threadFrom, `.chmain .thr[data-id="${e}"]`, `[aria-controls="menu-msg-${e}"]`, `.chmain .msg-tool[data-act="thread"][data-id="${e}"]`, `.chmain [data-mid="${e}"] .chat-needs-chip button`]);
}
function toggleThread(id, t) {
  const closing = S.thread === id;
  if (!closing) local.threadFrom = threadOpener(t, id);
  S.thread = closing ? null : id;
  render(false);
  if (closing) focusThreadOpener(id);
  else document.getElementById("chat-thread-title")?.focus();
  syncOverlayHistory();
}
function closeThread() {
  const id = S.thread;
  if (!id) return;
  S.thread = null;
  render(false);
  focusThreadOpener(id);
  syncOverlayHistory();
}
function toggleMembers() {
  local.members = !local.members;
  if (local.members) S.thread = null;
  render(false);
  (local.members ? document.getElementById("chat-members-title") : null)?.focus();
  if (!local.members) focusFirst([".chat-context .chat-members-btn", ".chat-details .chat-members-btn", ".chat-details-btn"]);
  syncOverlayHistory();
}
function toggleDetails() {
  local.details = !local.details;
  render(false);
  document.querySelector(".chat-details-btn")?.focus();
  syncOverlayHistory();
}
/* Conversation list below 960px: an overlay opened from the header (also the "chat-list" app action). */
function toggleChatList(id) {
  const open = S.chatList !== id;
  S.chatList = open ? id : null;
  render(false);
  (open ? document.querySelector('.chlist [aria-current="page"]') : document.querySelector(".chat-list-btn"))?.focus();
  syncOverlayHistory();
}
const anyOverlay = () => route()[0] === "chat" && Boolean(S.thread || local.members || S.chatList || local.details);
/* Back closes the topmost overlay first: a side panel, then the drawer, then Details. */
function closeTopOverlay() {
  if (S.thread) closeThread();
  else if (local.members) toggleMembers();
  else if (S.chatList) toggleChatList(S.chatList);
  else if (local.details) toggleDetails();
}
bindOverlays(anyOverlay, closeTopOverlay);
/* The stream's "answer in the thread" chip moves focus to the card in the open thread. */
function focusThreadCard() {
  document.querySelector(".thread .chat-needs-q, .thread .chat-needs-done")?.focus();
}
function openViewThread(t) {
  local.openThread = { ch: t.dataset.ch, id: t.dataset.id };
  location.hash = `#/chat/${t.dataset.ch}`;
}

const CLICKS = {
  ...ASK_CLICKS,
  thread: (id, t) => toggleThread(id, t),
  "thread-close": () => closeThread(),
  "chat-members": () => toggleMembers(),
  "chat-thread-card": () => focusThreadCard(),
  "chat-open-thread": (_id, t) => openViewThread(t),
  "chat-search": (_id, _t, chId) => toggleSearch(chId),
  "chat-details": () => toggleDetails(),
  "chat-task": (id, t, chId) => turnIntoTask(t, chId, id),
  "chat-asks-jump": () => jumpToAsk(),
  "chat-dm-msg": (id, _t, chId) => {
    const m = CH[chId].msgs.find((x) => x.id === id);
    if (m) location.hash = openDirectMessage(m.who, { ch: chId, msg: id });
  },
  "chat-src-remove": (id) => {
    dmSources.delete(id);
    render(false);
    document.getElementById("compose")?.focus();
  },
};
function onChatClick(e) {
  onListClick(e, e.currentTarget);
  const t = e.target.closest("[data-act]"), fn = t && CLICKS[t.dataset.act];
  if (fn) fn(t.dataset.id, t, e.currentTarget.dataset.ch);
}
function onChatInput(e) {
  const t = e.target;
  if (t.id === "thread-compose") { threadDrafts.set(t.form.dataset.id, t.value); fitTextarea(t); }
  else if (t.id.startsWith("ar-")) saveAskDraft(t);
  else if (t.id === "chat-search-input" && local.search) { local.search.q = t.value; refreshSearch(document.querySelector(".chat .chmain .msgs[data-ch]")); }
  else onListInput(e, e.currentTarget);
}
function onChatKeydown(e) {
  const t = e.target;
  if (t.id === "chat-search-input" && e.key === "Escape") { e.preventDefault(); e.stopPropagation(); toggleSearch(local.search.id); return; }
  if (e.key === "Escape" && t.closest(".chat-members-panel")) { e.preventDefault(); e.stopPropagation(); toggleMembers(); return; }
  onListKeydown(e, e.currentTarget);
  onAskKeydown(e);
}
function mountOverlays(root) {
  if (root.classList.contains("list-open")) inertOutside(root.querySelector(".chlist"));
  const panel = root.querySelector('.thread[aria-modal="true"]');
  if (panel) inertOutside(panel);
}
/* Details is offered whenever the context line under the title is cut off (always below 640 px, by CSS). */
function watchContext(root) {
  const ctx = root.querySelector(".chhead .chat-context");
  if (!ctx || !root.querySelector(".chat-details-btn")) return;
  const measure = () => {
    if (!ctx.isConnected) { ro.disconnect(); return; }
    root.classList.toggle("ctx-overflow", local.details || ctx.scrollWidth > ctx.clientWidth + 1);
  };
  const ro = new ResizeObserver(measure);
  ro.observe(ctx);
  measure();
}
function mountStream(root) {
  const s = root.querySelector(".chmain .msgs[data-ch]");
  if (!s) return;
  const id = s.dataset.ch, q = searchQuery(id);
  highlightMatches(s, q);
  markLinked(s);
  placeStream(s, q ? { top: true } : initialPos(id));
  trackPos(s);
  watchAsks(s);
  focusLinked(s);
}
function mountChat() {
  releaseInert();
  const root = document.querySelector("#main .chat");
  if (!root) return;
  root.addEventListener("click", onChatClick);
  root.addEventListener("input", onChatInput);
  root.addEventListener("keydown", onChatKeydown);
  mountStream(root);
  fitTextarea(document.getElementById("thread-compose"));
  mountOverlays(root);
  watchContext(root);
  takeListFocus();
  if (local.openThread) {
    local.openThread = null;
    document.getElementById("chat-thread-title")?.focus();
    syncOverlayHistory();
  }
}

/* "#" and the New message command open the list (as the drawer below 960 px) on its search or recipient picker. */
function openChatList(mode) {
  const here = route()[0] === "chat", r1 = route()[1], id = here ? (isChatView(r1) ? r1 : resolveChatId(r1)) : DEFAULT_CHAT;
  prepareList(mode);
  if (matchesMedia(NARROW_LIST)) S.chatList = id;
  if (here) { render(false); syncOverlayHistory(); }
  else location.hash = "#/chat/" + id;
}
const findConversation = () => openChatList("find");
const startNewMessage = () => openChatList("new");

/* Leaving Chat (or the conversation) always releases modal overlays' inert background. */
window.addEventListener("hashchange", () => {
  releaseInert();
  resetAsks();
  local.details = false;
  local.members = false;
});
/* Crossing the overlay width re-renders so a side panel is a dialog only where it covers the conversation. */
window.matchMedia?.(OVERLAY_THREAD).addEventListener?.("change", () => { if (route()[0] === "chat" && (S.thread || local.members)) render(false); });

export {
  chatDocTitle,
  chatDrafts,
  findConversation,
  msgHtml,
  openDirectMessage,
  mountChat,
  pageChat,
  sendChatMessage,
  sendThreadReply,
  startNewMessage,
  toggleChatList,
};
