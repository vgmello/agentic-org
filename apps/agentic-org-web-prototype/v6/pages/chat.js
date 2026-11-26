import { CH, IN, KIND, M, W, blockedOnOwner } from "../data.js";
import { fitTextarea, workHref } from "./work.js";
import { CHAT_SEND_SHORTCUTS, S } from "../state.js";
import {
  av, esc, ic, itemActionButtons, itemReplyComposer, joinSep, kindWord, memberIds, menuItem, mlink,
  moreMenu, pill, plink, plural, presence, projMembers, statusLine, unhandled,
} from "../ui.js";

const DEFAULT_CHAT = "agency-v1";
const GROUPS = [["channel", "Channels"], ["dm", "Direct messages"], ["a2a", "Between agents"]];
const PRESENCE_WORD = { needs: "Needs you", withyou: "With you", working: "Working", paused: "Paused", available: "Available", asleep: "Asleep", offline: "Offline" };
const chatDrafts = new Map();

/* ---------------------------------------------------------------- names and time */
const chatTitle = (ch) => (ch.kind === "channel" ? "#" + ch.name : ch.kind === "dm" ? M[ch.with].name : ch.name);
const chatDocTitle = (id) => `${chatTitle(CH[id && CH[id] ? id : DEFAULT_CHAT])} · Chat`;
const DAY_RE = /^([A-Z][a-z]{2} \d{1,2}|Yesterday),?\s*/;
function splitTime(time) {
  const day = DAY_RE.exec(time || "");
  return day ? [day[1], time.slice(day[0].length)] : ["Today", time || ""];
}
const clock = () => new Date().toTimeString().slice(0, 5);
const authorWord = (who) => (M[who].owner ? "" : kindWord(who));

/* ---------------------------------------------------------------- message text: T- refs and @mentions become links */
function workRef(id) {
  return W.some((w) => w.id === id) ? `<a class="alink chat-ref" href="${esc(workHref({ item: id }))}">${id}</a>` : id;
}
function mentionRef(match, name) {
  const lower = name.toLowerCase();
  const key = Object.keys(M).find((k) => k === lower || M[k].name.split(" ")[0].toLowerCase() === lower);
  return key && !M[key].owner ? `<a class="alink chat-ref" href="#/members/${key}">@${esc(M[key].name)}</a>` : match;
}
const richText = (text) => esc(text).replace(/\bT-A\d+\b/g, workRef).replace(/@([A-Za-z]+)/g, mentionRef);

/* ---------------------------------------------------------------- owner-waiting items inside messages */
const inboxItem = (id) => IN.find((i) => i.id === id);
function waitingFor(it) {
  if (/^\d/.test(it.time)) return "waiting " + it.time;
  return it.time === "Yesterday" ? "waiting since yesterday" : "waiting since " + it.time;
}
function taskPhrase(it) {
  const w = it.task && W.find((x) => x.id === it.task);
  return w ? `<span>${blockedOnOwner(w) ? "Blocks" : "For"} ${workRef(w.id)}</span>` : "";
}
function needsHead(it) {
  const state = it.handled ? pill("Handled", "gray", "check-circle") : pill("Needs you", "amber", "bell");
  return `<p class="chat-needs-head">${state}<span>${esc(it.sub)} from ${esc(M[it.from].name)}${it.handled ? "" : " · " + waitingFor(it)}</span></p>`;
}
function needsActions(it) {
  if (it.handled) return `<p class="handled chat-needs-done" id="handled-${esc(it.id)}" tabindex="-1">${ic("check-circle", "ic16")}${esc(it.outcome)}</p>`;
  if (S.replying === it.id) return itemReplyComposer(it, "outline");
  const reply = it.kind === "question" ? "" : `<button class="btn ghost sm" type="button" data-act="reply" data-id="${esc(it.id)}">${ic("reply", "ic16")}Reply…</button>`;
  const open = `<a class="btn ghost sm" href="#/inbox/${esc(it.id)}">${ic("inbox", "ic16")}Open in Inbox</a>`;
  return `<div class="actions">${[...itemActionButtons(it, "outline"), reply, open].join("")}</div>`;
}
function needsCardHtml(it, msgId) {
  const titleId = `needs-${esc(msgId)}`;
  const meta = joinSep([
    taskPhrase(it),
    it.expires && !it.handled ? `<span class="pin">${ic("clock", "ic14")}${esc(it.expires)}</span>` : "",
  ]);
  return `<section class="chat-needs${it.handled ? " is-done" : ""}" aria-labelledby="${titleId}">
    ${needsHead(it)}
    <h3 class="chat-needs-q" id="${titleId}">${esc(it.what)}</h3>
    ${meta ? `<p class="chat-needs-meta">${meta}</p>` : ""}
    ${it.scope && !it.handled ? `<p class="chat-needs-scope">${ic("lock", "ic14")}<span>Scope: ${esc(it.scope)}</span></p>` : ""}
    ${needsActions(it)}
  </section>`;
}
function needsChipHtml(it) {
  const label = it.handled ? `Handled · ${esc(it.outcome)}` : `Needs you · ${esc(KIND[it.kind].label)} in Inbox`;
  return `<p class="chat-needs-chip${it.handled ? " is-done" : ""}"><a href="#/inbox/${esc(it.id)}">${ic(it.handled ? "check-circle" : "bell", "ic14")}<span>${label}</span>${ic("chev-right", "ic14")}</a></p>`;
}
function messageNeeds(m, cards) {
  const it = inboxItem(m.needs || m.ref);
  if (!it) return "";
  return cards && m.needs ? needsCardHtml(it, m.id) : needsChipHtml(it);
}

/* ---------------------------------------------------------------- one message */
function messageTools(m) {
  return `<span class="msg-tools" role="group" aria-label="Message actions">
    <button class="btn ghost icon" type="button" data-act="thread" data-id="${esc(m.id)}" aria-label="Reply in thread" title="Reply in thread">${ic("thread", "ic16")}</button>
    <button class="btn ghost icon" type="button" data-act="work-new-task" aria-label="Turn into task" title="Turn into task">${ic("clipboard", "ic16")}</button>
  </span>`;
}
function messageStatus(m, time) {
  if (m.pending) return `<span class="msg-status" role="status">Sending…</span>`;
  return time ? `<time class="msg-time">${esc(time)}</time>` : "";
}
function threadButton(m) {
  if (!m.thread || !m.thread.length) return "";
  return `<button class="thr" type="button" data-act="thread" data-id="${esc(m.id)}" aria-expanded="${S.thread === m.id}">${ic("thread", "ic16")}${plural(m.thread.length, "reply", "replies")}</button>`;
}
const continues = (m, prev) => prev && prev.who === m.who && splitTime(prev.time)[0] === splitTime(m.time)[0] && !prev.needs;
function messageHead(m, time, tools) {
  const word = authorWord(m.who);
  return `<p class="hd"><b>${mlink(m.who)}</b>${word ? `<span class="msg-kind">${word}</span>` : ""}${messageStatus(m, time)}${tools ? messageTools(m) : ""}</p>`;
}
function messageHtml(m, prev, opts = {}) {
  const time = splitTime(m.time)[1], cont = continues(m, prev);
  const head = cont ? `<p class="sr-only">${esc(M[m.who].name)}</p>` : messageHead(m, time, opts.tools);
  const gutter = cont ? `<span class="msg-gutter">${messageStatus(m, time)}</span>` : av(m.who, 32);
  return `<div class="msg${cont ? " cont" : ""}${m.pending ? " is-pending" : ""}" data-mid="${esc(m.id)}">${gutter}<div class="body">${head}
    <p class="tx">${richText(m.text)}</p>${cont && opts.tools ? messageTools(m) : ""}
    ${messageNeeds(m, opts.cards)}${threadButton(m)}</div></div>`;
}
/* Shared preview (project Overview) renders chips, not cards; it is called as Array.map(msgHtml). */
const msgHtml = (m, i, list) => messageHtml(m, list && i ? list[i - 1] : null);

function emptyConversation(ch) {
  const dm = ch.kind === "dm", name = dm ? M[ch.with].name : chatTitle(ch);
  const text = dm ? `Messages here go only to ${name}.${M[ch.with].kind === "agent" ? ` ${name} replies when it next runs.` : ""}` : ch.topic || "";
  return `<div class="chat-empty">${dm ? av(ch.with, 32) : ic("hash", "ic20")}<h2>This is the start of ${dm ? "your conversation with " : ""}${esc(name)}</h2>${text ? `<p>${esc(text)}</p>` : ""}</div>`;
}
function streamHtml(ch) {
  if (!ch.msgs.length) return emptyConversation(ch);
  let lastDay = "";
  return ch.msgs.map((m, i) => {
    const [day] = splitTime(m.time);
    const divider = day !== lastDay ? `<h2 class="chat-day"><span>${esc(day)}</span></h2>` : "";
    lastDay = day;
    return divider + messageHtml(m, i ? ch.msgs[i - 1] : null, { cards: true, tools: ch.kind !== "a2a" });
  }).join("");
}

/* ---------------------------------------------------------------- composer */
function shortcutHint() {
  const keys = { either: "Ctrl/⌘", control: "Ctrl", command: "⌘" }[S.chatSend] || "Ctrl/⌘";
  return `<span class="sr-only">Enter adds a new line. ${CHAT_SEND_SHORTCUTS[S.chatSend].label} sends.</span><span aria-hidden="true"><kbd>${keys}</kbd> <kbd>Enter</kbd> to send · <kbd>Enter</kbd> new line</span>`;
}
function chatComposer(id, title) {
  const shortcut = CHAT_SEND_SHORTCUTS[S.chatSend];
  return `<form class="compose chat-compose" data-form="send" data-id="${id}">
    <div class="chat-compose-row">
      <label class="sr-only" for="compose">Message ${esc(title)}</label>
      <textarea class="field" id="compose" name="message" rows="1" placeholder="Message ${esc(title)}" aria-describedby="chat-compose-help" autocomplete="off" enterkeyhint="enter">
${esc(chatDrafts.get(id) || "")}</textarea>
      <button class="btn primary" type="submit" title="Send (${shortcut.label})" aria-keyshortcuts="${shortcut.keys}">${ic("send", "ic20")}Send</button>
    </div>
    <p class="chat-compose-hint" id="chat-compose-help" aria-live="polite">${shortcutHint()}</p>
  </form>`;
}
function showEmptySendHint(form, field) {
  const hint = form.querySelector(".chat-compose-hint");
  hint.classList.add("is-error");
  hint.innerHTML = `${ic("info", "ic14")}<span>Write a message first.</span>`;
  field.focus();
  field.addEventListener("input", () => { hint.classList.remove("is-error"); hint.innerHTML = shortcutHint(); }, { once: true });
}
function confirmSent(message) {
  message.pending = false;
  const status = document.querySelector(`.msg[data-mid="${CSS.escape(message.id)}"] .msg-status`);
  if (!status) return;
  status.closest(".msg").classList.remove("is-pending");
  status.outerHTML = `<time class="msg-time">${esc(message.time)}</time>`;
}
function scrollChatToEnd() {
  for (const stream of document.querySelectorAll(".chat .msgs")) stream.scrollTop = stream.scrollHeight;
}
/* After each Chat render: newest messages sit next to the composer, also once web fonts settle. */
function mountChat() {
  scrollChatToEnd();
  requestAnimationFrame(scrollChatToEnd);
  document.fonts?.ready.then(scrollChatToEnd);
}
function sendChatMessage(form) {
  const field = form.elements.message;
  if (!field.value.trim()) { showEmptySendHint(form, field); return; }
  const id = form.dataset.id, ch = CH[id];
  /* Prototype: the office confirms after a short delay; until then the message says "Sending…". */
  const message = { id: "u" + Date.now(), who: "you", time: clock(), text: field.value, pending: true };
  ch.msgs.push(message);
  chatDrafts.delete(id);
  field.value = "";
  fitTextarea(field);
  form.closest(".chmain").querySelector(".msgs").innerHTML = streamHtml(ch);
  scrollChatToEnd();
  field.focus();
  setTimeout(() => confirmSent(message), 700);
}
function sendThreadReply(form) {
  const field = form.elements.message, text = field.value.trim();
  if (!text) { field.focus(); return false; }
  const ch = CH[form.dataset.ch], root = ch.msgs.find((m) => m.id === form.dataset.id);
  const reply = { who: "you", time: clock(), text };
  (root.thread ||= []).push(reply);
  if (form.elements.also?.checked) ch.msgs.push({ id: "u" + Date.now(), ...reply });
  return true;
}
function openDirectMessage(who) {
  const key = "dm-" + who;
  CH[key] ||= { name: M[who].name, kind: "dm", with: who, msgs: [] };
  return "#/chat/" + key;
}

/* ---------------------------------------------------------------- conversation list */
/* Counts what was asked in this conversation; items only mentioned here (m.ref) are counted where they were asked. */
function conversationNeeds(ch) {
  const ids = new Set(ch.msgs.map((m) => m.needs).filter(Boolean));
  return [...ids].filter((id) => { const it = inboxItem(id); return it && !it.handled; }).length;
}
function rowLead(ch) {
  if (ch.kind === "channel") return ic("hash", "ic16");
  return ch.kind === "dm" ? av(ch.with, 24) : ic("eye", "ic16");
}
const countBadge = (n) => `<span class="badge chat-row-count">${n}<span class="sr-only"> waiting on you</span></span>`;
function rowTrail(ch) {
  const needs = conversationNeeds(ch);
  if (needs) return countBadge(needs);
  if (ch.kind !== "dm") return "";
  const word = M[ch.with].kind === "person" ? "Person" : PRESENCE_WORD[presence(ch.with)];
  return word ? `<span class="chat-row-word">${word}</span>` : "";
}
function chatLinkHtml(k, activeId) {
  const ch = CH[k], unread = ch.unread && k !== activeId;
  const attrs = (k === activeId ? ' aria-current="page"' : "") + (unread ? ' class="is-unread"' : "");
  const unreadText = unread ? `<span class="sr-only">, ${plural(ch.unread, "unread message", "unread messages")}</span>` : "";
  return `<li><a href="#/chat/${k}"${attrs}>${rowLead(ch)}<span class="chat-row-name">${esc(ch.kind === "channel" ? ch.name : chatTitle(ch))}</span>${unreadText}${rowTrail(ch)}</a></li>`;
}
function chatListHtml(id) {
  const keys = Object.keys(CH), open = unhandled().length;
  const jumpShortcut = S.singleKey ? ' aria-keyshortcuts="#"' : "";
  const jumpKbd = S.singleKey ? '<kbd aria-hidden="true">#</kbd>' : "";
  return `<div class="chlist" id="chat-list">
      <button class="btn ghost sm chat-jump" id="chat-jump" type="button" data-act="chat-jump"
        aria-haspopup="dialog" aria-controls="cmdk-dialog"${jumpShortcut}>${ic("search", "ic16")}<span>Jump to conversation</span>${jumpKbd}</button>
      <nav aria-label="Conversations">
        <ul class="chat-inbox-row"><li><a href="#/inbox">${ic("inbox", "ic16")}<span class="chat-row-name">Inbox</span>${open ? countBadge(open) : ""}</a></li></ul>
        ${GROUPS.map(([kind, label]) => `<h2>${label}</h2><ul>${keys.filter((k) => CH[k].kind === kind).map((k) => chatLinkHtml(k, id)).join("")}</ul>`).join("")}
      </nav>
    </div>`;
}

/* ---------------------------------------------------------------- header */
function membersLink(ch) {
  if (ch.proj) return `<a class="alink" href="#/projects/${ch.proj}/team">${plural(projMembers(ch.proj).length, "member", "members")}</a>`;
  return `<a class="alink" href="#/teams/members">${plural(memberIds().length, "member", "members")}</a>`;
}
function dmContext(who) {
  const m = M[who], status = m.kind === "person" ? esc(m.last || "") : statusLine(who);
  return joinSep([`<span>${esc(m.role)}</span>`, `<span>${kindWord(who)}</span>`, status ? `<span>${status}</span>` : ""]);
}
function headerContext(ch) {
  if (ch.kind === "dm") return dmContext(ch.with);
  if (ch.kind === "a2a") return joinSep(["<span>Between agents</span>", "<span>You can read, not reply</span>", `<span>${ch.pair.map((k) => mlink(k)).join(" and ")}</span>`]);
  return joinSep([
    ch.topic ? `<span class="chat-topic">${esc(ch.topic)}</span>` : "",
    ch.proj ? `<span class="chat-proj">${ic("folder", "ic14")}${plink(ch.proj)}</span>` : "",
    membersLink(ch),
  ]);
}
function headerLead(ch) {
  if (ch.kind === "dm") return av(ch.with, 32);
  return ch.kind === "a2a" ? `<span class="chat-pair" aria-hidden="true">${ch.pair.map((k) => av(k, 24)).join("")}</span>` : "";
}
function headerActions(title) {
  const items = [
    menuItem("proto", "Search in " + title, "search", "Search in conversation", ' class="chat-search-item"'),
    menuItem("proto", "Mute " + title, "bell", "Mute conversation"),
    menuItem("proto", "Details for " + title, "info", "Conversation details"),
  ].join("");
  return `<div class="ph-actions chat-actions">
      <button class="btn ghost icon chat-search" type="button" data-act="proto" data-v="Search in ${esc(title)}" aria-label="Search in ${esc(title)}" title="Search in ${esc(title)}">${ic("search", "ic20")}</button>
      ${moreMenu("chat-more", "More actions for " + title, items, "ghost")}
    </div>`;
}
function listButton(id) {
  const elsewhere = Object.keys(CH).filter((k) => k !== id).reduce((n, k) => n + conversationNeeds(CH[k]), 0);
  return `<button class="btn ghost chat-list-btn" type="button" data-act="chat-list" data-id="${id}" aria-expanded="${S.chatList === id}" aria-controls="chat-list">${ic("chat", "ic20")}<span>Conversations</span>${elsewhere ? countBadge(elsewhere) : ""}</button>`;
}
function chatHeaderHtml(id, ch, title) {
  return `<header class="ph chhead">
    ${listButton(id)}
    <div class="ph-t chat-title">
      ${headerLead(ch)}
      <div class="chat-heading"><h1 id="ch-title" tabindex="-1">${esc(title)}</h1>
        <p class="chat-context">${headerContext(ch)}</p>
      </div>
    </div>
    ${ch.kind === "a2a" ? "" : headerActions(title)}
  </header>`;
}

/* ---------------------------------------------------------------- thread panel and read-only footer */
function threadComposer(id, root) {
  return `<form class="compose chat-compose thread-compose" data-form="thread-send" data-id="${esc(root.id)}" data-ch="${esc(id)}">
    <div class="chat-compose-row">
      <label class="sr-only" for="thread-compose">Reply in thread</label>
      <textarea class="field" id="thread-compose" name="message" rows="1" placeholder="Reply…" autocomplete="off"></textarea>
      <button class="btn outline" type="submit">${ic("send", "ic20")}Reply</button>
    </div>
    <label class="chat-also"><input type="checkbox" name="also"> Also send to ${esc(chatTitle(CH[id]))}</label>
  </form>`;
}
function chatThreadHtml(id, root, title) {
  if (!root) return "";
  const replies = root.thread || [];
  const count = replies.length ? plural(replies.length, "reply", "replies") : "No replies yet";
  const list = replies.map((r, i) => messageHtml(Object.assign({ id: root.id + "-r" + i }, r), i ? replies[i - 1] : null)).join("");
  return `<aside class="thread" aria-labelledby="chat-thread-title"><header class="side-panel-header"><div class="thread-title"><h2 id="chat-thread-title" tabindex="-1">Thread</h2><span class="muted">${esc(title)}</span></div><button class="btn ghost icon" type="button" data-act="thread-close" aria-label="Close thread">${ic("x", "ic20")}</button></header>
      <div class="msgs">${messageHtml(Object.assign({}, root, { thread: null }), null)}<p class="chat-thread-count">${count}</p>${list}</div>
      ${CH[id].kind === "a2a" ? "" : threadComposer(id, root)}</aside>`;
}
function readOnlyFoot(ch) {
  const buttons = ch.pair.map((k) => `<button class="btn outline sm" type="button" data-act="chat-dm" data-id="${k}">${ic("chat", "ic16")}Message ${esc(M[k].name)}</button>`).join("");
  return `<div class="readfoot"><p>${ic("eye", "ic16")}You can read this conversation but not reply in it.</p><div class="actions">${buttons}</div></div>`;
}

/* ---------------------------------------------------------------- page */
function pageChat(id) {
  id = id && CH[id] ? id : DEFAULT_CHAT;
  const ch = CH[id];
  ch.unread = 0;
  if (S.chatList && S.chatList !== id) S.chatList = null;
  const root = S.thread && ch.msgs.find((m) => m.id === S.thread);
  const title = chatTitle(ch);
  const cls = ["chat", root ? "withthread" : "", S.chatList === id ? "list-open" : ""].filter(Boolean).join(" ");
  return `<div class="${cls}">
    ${chatHeaderHtml(id, ch, title)}
    ${chatListHtml(id)}
    <div class="chat-scrim" data-act="chat-list" data-id="${id}" aria-hidden="true"></div>
    <section class="chmain" aria-labelledby="ch-title">
      <div class="msgs" aria-label="Messages">${streamHtml(ch)}</div>
      ${ch.kind === "a2a" ? readOnlyFoot(ch) : chatComposer(id, title)}</section>
    ${chatThreadHtml(id, root, title)}
  </div>`;
}

export {
  chatDocTitle,
  chatDrafts,
  msgHtml,
  openDirectMessage,
  mountChat,
  pageChat,
  sendChatMessage,
  sendThreadReply,
};
