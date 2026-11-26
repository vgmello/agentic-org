import { CH, IN, M } from "../data.js";
import { fitTextarea } from "./work.js";
import { openSmartTaskComposer } from "./task-composer.js";
import { applyPos, placeStream, replaceStream, savedPos, trackPos } from "./chat-scroll.js";
import { ASK_CLICKS, messageNeeds, onAskKeydown, recordAskReply, resetAsks, saveAskDraft, workRef } from "./chat-asks.js";
import { chatListHtml, needsCount, onListClick, onListInput, onListKeydown, prepareList, takeListFocus, unreadDot } from "./chat-list.js";
import {
  DEFAULT_CHAT, chatTitle, dmSources, inboxItem, openAsks, openDirectMessage, resolveChatId, sourceHref, sourceLabel,
} from "./chat-model.js";
import { render, route } from "../shell/router.js";
import { CHAT_SEND_SHORTCUTS, S } from "../state.js";
import {
  av, esc, ic, joinSep, kindWord, memberIds, mlink, moreMenu, plink, plural, presence, projMembers, statusLine,
} from "../ui.js";

const chatDrafts = new Map();      /* conversation id -> unsent message */
const threadDrafts = new Map();    /* thread root message id -> unsent thread reply */
const local = { search: null, linked: null, details: false, threadFrom: "" };
const OVERLAY_THREAD = "(max-width: 1279px)";
const NARROW_LIST = "(max-width: 959px)";
const matchesMedia = (q) => Boolean(window.matchMedia?.(q).matches);

/* ---------------------------------------------------------------- names and time */
const chatDocTitle = (id) => `${chatTitle(CH[id && CH[id] ? id : DEFAULT_CHAT])} · Chat`;
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
  return `<div class="msg${cont ? " cont" : ""}${m.pending ? " is-pending" : ""}" data-mid="${esc(m.id)}">${gutter}<div class="body">${head}
    <p class="tx">${richText(m.text)}${status}</p>${tools}
    ${sourceLine(m)}${messageNeeds(m, opts.cards)}${threadButton(m)}</div></div>`;
}
/* Shared preview (project Overview) renders chips, not cards; it is called as Array.map(msgHtml). */
const msgHtml = (m, i, list) => messageHtml(m, list && i ? list[i - 1] : null);

function emptyConversation(ch) {
  const dm = ch.kind === "dm", name = dm ? M[ch.with].name : chatTitle(ch);
  const text = dm ? `Messages here go only to ${name}.${M[ch.with].kind === "agent" ? ` ${name} replies when it next runs.` : ""}` : ch.topic || "";
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
  const opts = { cards: true, tools: true, readOnly: ch.kind === "a2a" };
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
    ${o.before || ""}
    <div class="chat-compose-row">
      <label class="sr-only" for="${o.field}">${esc(o.label)}</label>
      <textarea class="field" id="${o.field}" name="message" rows="1" placeholder="${esc(o.label)}" aria-describedby="${o.field}-help" autocomplete="off" enterkeyhint="enter">
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
const chatComposer = (id, title) => composerHtml({
  form: "send", id, field: "compose", label: "Message " + title, draft: chatDrafts.get(id), button: "primary", submit: "Send", before: composeSource(id),
});
const threadComposer = (id, root) => composerHtml({
  form: "thread-send", id: root.id, ch: id, cls: " thread-compose", field: "thread-compose", label: "Reply in thread",
  draft: threadDrafts.get(root.id), button: "outline", submit: "Reply",
  after: `<label class="chat-also"><input type="checkbox" name="also"> Also send to ${esc(chatTitle(CH[id]))}</label>`,
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
  if (form.elements.also?.checked) ch.msgs.push({ id: "u" + Date.now(), ...reply });
  return true;
}

/* ---------------------------------------------------------------- header */
function membersLink(ch) {
  if (ch.proj) return `<a class="alink" href="#/projects/${ch.proj}/team">${plural(projMembers(ch.proj).length, "member", "members")}</a>`;
  return `<a class="alink" href="#/teams/members">${plural(memberIds().length, "member", "members")}</a>`;
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
  /* The read-only rule is stated once, in the footer beside its actions. */
  if (ch.kind === "a2a") return [["Kind", "Between agents"], ["Agents", ch.pair.map((k) => mlink(k)).join(" and ")]];
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
  const here = new Set(openAsks(CH[id]));
  const elsewhere = new Set(Object.keys(CH).filter((k) => k !== id).flatMap((k) => openAsks(CH[k])).filter((x) => !here.has(x))).size;
  const unread = Object.keys(CH).some((k) => k !== id && CH[k].unread);
  return `<button class="btn ghost chat-list-btn" type="button" data-act="chat-list" data-id="${id}" aria-expanded="${S.chatList === id}" aria-controls="chat-list" title="Conversations">${ic("chat", "ic20")}<span class="chat-list-label">Conversations</span>${elsewhere ? needsCount(elsewhere, elsewhere === 1 ? "needs you in another conversation" : "need you in other conversations") : ""}${unread ? `${unreadDot}<span class="sr-only">, unread messages</span>` : ""}</button>`;
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

/* ---------------------------------------------------------------- thread panel and read-only footer */
function chatThreadHtml(id, root, title) {
  if (!root) return "";
  const replies = root.thread || [];
  const count = replies.length ? plural(replies.length, "reply", "replies") : "No replies yet";
  const list = replies.map((r, i) => messageHtml(Object.assign({ id: root.id + "-r" + i }, r), i ? replies[i - 1] : null)).join("");
  /* Where the thread covers the conversation it is a modal dialog with a scrim (made inert-outside on mount). */
  const modal = matchesMedia(OVERLAY_THREAD) ? ' role="dialog" aria-modal="true"' : "";
  return `<div class="thread-scrim" data-act="thread-close" data-scrim aria-hidden="true"></div>
    <aside class="thread"${modal} aria-labelledby="chat-thread-title"><header class="side-panel-header"><div class="thread-title"><h2 id="chat-thread-title" tabindex="-1">Thread</h2><span class="muted">${esc(title)}</span></div><button class="btn ghost icon" type="button" data-act="thread-close" aria-label="Close thread">${ic("x", "ic20")}</button></header>
      <div class="msgs">${messageHtml(Object.assign({}, root, { thread: null }), null)}<p class="chat-thread-count">${count}</p>${list}</div>
      ${CH[id].kind === "a2a" ? "" : threadComposer(id, root)}</aside>`;
}
function readOnlyFoot(id, ch) {
  const buttons = ch.pair.map((k) => `<button class="btn outline sm" type="button" data-act="chat-dm" data-id="${k}" data-v="${esc(id)}">${ic("chat", "ic16")}Message ${esc(M[k].name)}</button>`).join("");
  return `<div class="readfoot"><p>${ic("eye", "ic16")}You can read this conversation but not reply in it.</p><div class="actions">${buttons}</div></div>`;
}
const askJump = () => `<div class="chat-asks-jump"><button class="btn sm" type="button" data-act="chat-asks-jump" hidden>${ic("bell", "ic16")}<span></span>${ic("chev-down", "ic16 chat-up")}</button></div>`;

/* ---------------------------------------------------------------- page */
function pageChat(id) {
  id = resolveChatId(id);
  const ch = CH[id];
  ch.unread = 0;
  if (S.chatList && S.chatList !== id) S.chatList = null;
  const root = S.thread && ch.msgs.find((m) => m.id === S.thread);
  const title = chatTitle(ch), listOpen = S.chatList === id;
  const cls = ["chat", root ? "withthread" : "", listOpen ? "list-open" : "", local.details ? "details-open" : ""].filter(Boolean).join(" ");
  return `<div class="${cls}" data-ch="${esc(id)}">
    ${chatHeaderHtml(id, ch, title)}
    ${chatListHtml(id, listOpen)}
    <div class="chat-scrim" data-act="chat-list" data-id="${id}" data-scrim aria-hidden="true"></div>
    <section class="chmain" aria-labelledby="ch-title">
      ${searchRowHtml(id, title)}
      ${askJump()}
      <div class="msgs" aria-label="Messages" data-ch="${esc(id)}">${streamHtml(id)}</div>
      ${ch.kind === "a2a" ? readOnlyFoot(id, ch) : chatComposer(id, title)}</section>
    ${chatThreadHtml(id, root, title)}
  </div>`;
}

/* ---------------------------------------------------------------- after render: position, overlays, local controls */
let inerted = [];
function releaseInert() {
  inerted.forEach((el) => { el.inert = false; });
  inerted = [];
}
/* A modal overlay (narrow conversation list, overlay thread): everything outside it but scrims is inert. */
function inertOutside(el) {
  for (let n = el; n.parentElement && n !== document.body; n = n.parentElement) {
    for (const sib of n.parentElement.children) {
      if (sib === n || sib.inert || sib.hasAttribute("data-scrim") || sib.tagName === "SCRIPT") continue;
      sib.inert = true;
      inerted.push(sib);
    }
  }
}
function initialPos(id) {
  const target = route()[2];
  if (target && local.linked !== location.hash && CH[id].msgs.some((m) => m.id === target)) {
    local.linked = location.hash;
    return { target };
  }
  return savedPos(id) || { boundary: true };
}
function markLinked(s) {
  const target = route()[2], el = target && s.querySelector(`:scope > [data-mid="${CSS.escape(target)}"]`);
  el?.classList.add("is-linked");
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

/* Earlier asks above the reading position: one button jumps to the nearest, oldest last. */
function asksAbove(s) {
  const top = s.getBoundingClientRect().top;
  return [...s.querySelectorAll(":scope > .msg .chat-needs:not(.is-done)")].filter((c) => c.getBoundingClientRect().bottom < top + 8);
}
function syncAskJump(s) {
  const btn = document.querySelector('[data-act="chat-asks-jump"]');
  if (!btn) return;
  const n = asksAbove(s).length;
  btn.hidden = !n;
  btn.querySelector("span").textContent = n === 1 ? "1 earlier request needs you" : `${n} earlier requests need you`;
}
function jumpToAsk() {
  const s = document.querySelector(".chat .chmain .msgs"), card = s && asksAbove(s).pop();
  if (!card) return;
  s.scrollTop += card.closest(".msg").getBoundingClientRect().top - s.getBoundingClientRect().top - 8;
  card.querySelector(".chat-needs-q").focus({ preventScroll: true });
  syncAskJump(s);
}
function watchAsks(s) {
  let frame = 0;
  s.addEventListener("scroll", () => { cancelAnimationFrame(frame); frame = requestAnimationFrame(() => syncAskJump(s)); }, { passive: true });
  setTimeout(() => s.isConnected && syncAskJump(s), 400);
  syncAskJump(s);
}

/* Threads: focus returns to the control that opened it, or the visible one for that message. */
function threadOpener(t, id) {
  const menu = t.closest(".menu");
  if (menu) return `[aria-controls="${menu.id}"]`;
  return t.classList.contains("thr") ? `.chmain .thr[data-id="${CSS.escape(id)}"]` : `.chmain .msg-tool[data-act="thread"][data-id="${CSS.escape(id)}"]`;
}
const visible = (el) => el && el.getClientRects().length > 0;
function focusThreadOpener(id) {
  const options = [local.threadFrom, `.chmain .thr[data-id="${CSS.escape(id)}"]`, `[aria-controls="menu-msg-${CSS.escape(id)}"]`, `.chmain .msg-tool[data-act="thread"][data-id="${CSS.escape(id)}"]`];
  const el = options.filter(Boolean).map((sel) => document.querySelector(sel)).find(visible);
  el?.focus();
}
function toggleThread(id, t) {
  const closing = S.thread === id;
  if (!closing) local.threadFrom = threadOpener(t, id);
  S.thread = closing ? null : id;
  render(false);
  if (closing) focusThreadOpener(id);
  else document.getElementById("chat-thread-title")?.focus();
}
function closeThread() {
  const id = S.thread;
  if (!id) return;
  S.thread = null;
  render(false);
  focusThreadOpener(id);
}
function toggleDetails() {
  local.details = !local.details;
  render(false);
  document.querySelector(".chat-details-btn")?.focus();
}
const CLICKS = {
  ...ASK_CLICKS,
  thread: (id, t) => toggleThread(id, t),
  "thread-close": () => closeThread(),
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
  else if (t.id === "chat-search-input" && local.search) { local.search.q = t.value; refreshSearch(document.querySelector(".chat .chmain .msgs")); }
  else onListInput(e, e.currentTarget);
}
function onChatKeydown(e) {
  const t = e.target;
  if (t.id === "chat-search-input" && e.key === "Escape") { e.preventDefault(); e.stopPropagation(); toggleSearch(local.search.id); return; }
  onListKeydown(e, e.currentTarget);
  onAskKeydown(e);
}
function mountOverlays(root) {
  if (root.classList.contains("list-open")) inertOutside(root.querySelector(".chlist"));
  const thread = root.querySelector('.thread[aria-modal="true"]');
  if (thread) inertOutside(thread);
}
function mountChat() {
  releaseInert();
  const root = document.querySelector("#main .chat"), s = root?.querySelector(".chmain .msgs");
  if (!s) return;
  const id = s.dataset.ch, q = searchQuery(id);
  highlightMatches(s, q);
  markLinked(s);
  placeStream(s, q ? { top: true } : initialPos(id));
  trackPos(s);
  watchAsks(s);
  root.addEventListener("click", onChatClick);
  root.addEventListener("input", onChatInput);
  root.addEventListener("keydown", onChatKeydown);
  fitTextarea(document.getElementById("thread-compose"));
  mountOverlays(root);
  takeListFocus();
}

/* "#" and the New message command open the list (as the drawer below 960 px) on its search or recipient picker. */
function openChatList(mode) {
  const here = route()[0] === "chat", id = here ? resolveChatId(route()[1]) : DEFAULT_CHAT;
  prepareList(mode);
  if (matchesMedia(NARROW_LIST)) S.chatList = id;
  if (here) render(false);
  else location.hash = "#/chat/" + id;
}
const findConversation = () => openChatList("find");
const startNewMessage = () => openChatList("new");

/* Leaving Chat (or the conversation) always releases modal overlays' inert background. */
window.addEventListener("hashchange", () => {
  releaseInert();
  resetAsks();
  local.details = false;
});
/* Crossing the overlay-thread width re-renders so the thread is a dialog only where it covers the conversation. */
window.matchMedia?.(OVERLAY_THREAD).addEventListener?.("change", () => { if (route()[0] === "chat" && S.thread) render(false); });

export {
  chatDocTitle,
  chatDrafts,
  findConversation,
  msgHtml,
  openDirectMessage,
  mountChat,
  pageChat,
  recordAskReply,
  sendChatMessage,
  sendThreadReply,
  startNewMessage,
};
