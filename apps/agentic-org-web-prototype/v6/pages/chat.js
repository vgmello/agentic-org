import { CH, IN, KIND, M, W, blockedOnOwner } from "../data.js";
import { fitTextarea, workHref } from "./work.js";
import { v4BuildButtons, v4ReplyBox } from "./home.js";
import { inboxCommentPrefix } from "./inbox.js";
import { openSmartTaskComposer } from "./task-composer.js";
import { applyPos, placeStream, replaceStream, savedPos, trackPos } from "./chat-scroll.js";
import { render, route } from "../shell/router.js";
import { CHAT_SEND_SHORTCUTS, S } from "../state.js";
import {
  av, esc, ic, joinSep, kindWord, memberIds, mlink, moreMenu, pill, plink, plural, presence, projMembers, statusLine,
} from "../ui.js";

const DEFAULT_CHAT = "agency-v1";
const GROUPS = [["channel", "Channels"], ["dm", "Direct messages"], ["a2a", "Between agents"]];
const PRESENCE_WORD = { needs: "Waiting on you", withyou: "With you", working: "Working", paused: "Paused", available: "Available", asleep: "Asleep", offline: "Offline" };
const chatDrafts = new Map();      /* conversation id -> unsent message */
const threadDrafts = new Map();    /* thread root message id -> unsent thread reply */
const askDrafts = new Map();       /* inbox item id -> unsent reply on that request */
const dmSources = new Map();       /* DM id -> conversation the owner can link in the first message */
const local = { askReply: null, askNote: null, search: null, linked: null };

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
/* Binding choices or a stated tradeoff: decided with the full Inbox context, not from a chat card. */
const consequential = (it) => it.kind === "decision" || (it.impact || []).some(([, tone]) => tone === "warn" || tone === "bad");
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
const inboxLink = (it, label, cls = "btn ghost sm") => `<a class="${cls}" href="#/inbox/${esc(it.id)}">${label}</a>`;
function handledLine(it) {
  return `<p class="handled chat-needs-done" id="handled-${esc(it.id)}" tabindex="-1">${ic("check-circle", "ic16")}<span>${esc(it.outcome)}</span>
    ${inboxLink(it, "View in Inbox", "alink chat-needs-record")}</p>`;
}
function askReplyBox(it) {
  const who = esc(M[it.from].name), id = esc(it.id);
  return `<div class="reply chat-ask-reply"><label class="sr-only" for="ar-${id}">Reply to ${who}</label>
    <textarea class="field" id="ar-${id}" rows="2" placeholder="Reply to ${who}…" aria-describedby="ar-help-${id}">${esc(askDrafts.get(it.id) || "")}</textarea>
    <p class="chat-compose-hint" id="ar-help-${id}">Posts here and on the request. It stays open until you decide.</p>
    <div class="actions"><button class="btn primary sm" type="button" data-act="chat-ask-send" data-id="${id}">${ic("send", "ic16")}Send reply</button><button class="btn ghost sm" type="button" data-act="chat-ask-cancel" data-id="${id}">Cancel</button></div></div>`;
}
function decisionControls(it) {
  if (consequential(it)) return [inboxLink(it, `${ic("inbox", "ic16")}Review in Inbox`, "btn primary sm")];
  const { b, guardedHtml } = v4BuildButtons(it);
  const sep = guardedHtml && S.confirmDeny !== it.id ? '<span class="hb-act-sep" aria-hidden="true"></span>' : "";
  return [...b, sep, guardedHtml];
}
function needsActions(it) {
  if (it.handled) return handledLine(it);
  if (S.replying === it.id) return v4ReplyBox(it);
  if (local.askReply === it.id) return askReplyBox(it);
  const reply = it.kind === "question" ? "" : `<button class="btn ghost sm" type="button" data-act="chat-ask-reply" data-id="${esc(it.id)}">${ic("reply", "ic16")}Reply…</button>`;
  const open = consequential(it) ? "" : inboxLink(it, `${ic("inbox", "ic16")}<span><span class="chat-long">Open in </span>Inbox</span>`);
  const note = local.askNote === it.id ? `<p class="chat-needs-note" role="status">${ic("check-circle", "ic14")}Reply sent to ${esc(M[it.from].name)}. The request stays open until you decide.</p>` : "";
  return `${note}<div class="actions">${[...decisionControls(it), reply, open].join("")}</div>`;
}
function tradeoff(it) {
  if (!consequential(it) || it.handled || !it.why) return "";
  return `<details class="chat-needs-why"><summary>${ic("scale", "ic14")}Tradeoff</summary><p>${esc(it.why)}</p>${it.rec ? `<p class="chat-needs-rec">${esc(it.rec)}</p>` : ""}</details>`;
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
    ${tradeoff(it)}
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
  const id = esc(m.id);
  const menu = `<button role="menuitem" type="button" data-act="thread" data-id="${id}">${ic("thread", "ic20")}Reply in thread</button>`
    + `<button role="menuitem" type="button" data-act="chat-task" data-id="${id}">${ic("clipboard", "ic20")}Turn into task</button>`;
  return `<span class="msg-tools" role="group" aria-label="Message actions">
    <button class="btn ghost icon msg-tool" type="button" data-act="thread" data-id="${id}" aria-label="Reply in thread" title="Reply in thread">${ic("thread", "ic16")}</button>
    <button class="btn ghost icon msg-tool" type="button" data-act="chat-task" data-id="${id}" aria-label="Turn into task" title="Turn into task">${ic("clipboard", "ic16")}</button>
    <span class="msg-menu">${moreMenu("msg-" + m.id, "Actions for message from " + M[m.who].name, menu, "ghost")}</span>
  </span>`;
}
const sendingStatus = () => `<span class="msg-status" role="status">Sending…</span>`;
function threadButton(m) {
  if (!m.thread || !m.thread.length) return "";
  return `<button class="thr" type="button" data-act="thread" data-id="${esc(m.id)}" aria-expanded="${S.thread === m.id}">${ic("thread", "ic16")}${plural(m.thread.length, "reply", "replies")}</button>`;
}
function sourceLine(m) {
  const src = m.src && CH[m.src];
  return src ? `<p class="msg-src">${ic("eye", "ic14")}<span>About <a class="alink chat-ref" href="#/chat/${esc(m.src)}">${esc(chatTitle(src))}</a></span></p>` : "";
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
  return `<div class="msg${cont ? " cont" : ""}${m.pending ? " is-pending" : ""}" data-mid="${esc(m.id)}">${gutter}<div class="body">${head}
    <p class="tx">${richText(m.text)}${status}</p>${opts.tools ? messageTools(m) : ""}
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
  let lastDay = "";
  return list.map((m, i) => {
    const [day] = splitTime(m.time);
    const divider = day !== lastDay ? `<h2 class="chat-day"><span>${esc(day)}</span></h2>` : "";
    lastDay = day;
    return divider + messageHtml(m, i ? list[i - 1] : null, { cards: true, tools: ch.kind !== "a2a" });
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
  if (!src || !CH[src]) return "";
  const title = esc(chatTitle(CH[src]));
  return `<p class="chat-compose-src">${ic("eye", "ic14")}<span>Your message will link to <a class="alink chat-ref" href="#/chat/${esc(src)}">${title}</a></span>
    <button class="btn ghost sm" type="button" data-act="chat-src-remove" data-id="${esc(id)}" aria-label="Don't link ${title}">${ic("x", "ic16")}Remove</button></p>`;
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
function openDirectMessage(who, from) {
  const key = "dm-" + who;
  CH[key] ||= { name: M[who].name, kind: "dm", with: who, msgs: [] };
  if (from && CH[from]) dmSources.set(key, from);
  return "#/chat/" + key;
}

/* ---------------------------------------------------------------- replies on a request (never a decision) */
function saveInboxComment(id, text) {
  try { localStorage.setItem(inboxCommentPrefix(id) + crypto.randomUUID(), JSON.stringify({ text, at: Date.now() })); } catch (e) {}
}
/* Posts the owner's words in the conversation where the request was asked and on the Inbox record. */
function recordAskReply(id, text) {
  const home = Object.values(CH).find((ch) => ch.msgs.some((m) => m.needs === id));
  if (home) home.msgs.push({ id: "u" + Date.now(), who: "you", time: clock(), text, ref: id });
  saveInboxComment(id, text);
}
function sendAskReply(id) {
  const field = document.getElementById("ar-" + id), text = field?.value.trim();
  if (!field) return;
  if (!text) {
    const hint = document.getElementById("ar-help-" + id);
    hint.classList.add("is-error");
    hint.textContent = "Write a reply first.";
    field.focus();
    return;
  }
  recordAskReply(id, text);
  askDrafts.delete(id);
  Object.assign(local, { askReply: null, askNote: id });
  render(false);
  document.querySelector(`[data-act="chat-ask-reply"][data-id="${CSS.escape(id)}"]`)?.focus();
}
function closeAskReply(id) {
  local.askReply = null;
  render(false);
  document.querySelector(`[data-act="chat-ask-reply"][data-id="${CSS.escape(id)}"]`)?.focus();
}

/* ---------------------------------------------------------------- conversation list */
/* Counts what was asked in this conversation; items only mentioned here (m.ref) are counted where they were asked. */
function openAsks(ch) {
  const ids = new Set(ch.msgs.map((m) => m.needs).filter(Boolean));
  return [...ids].filter((id) => { const it = inboxItem(id); return it && !it.handled; });
}
function rowLead(ch) {
  if (ch.kind === "channel") return ic("hash", "ic16");
  return ch.kind === "dm" ? av(ch.with, 24) : ic("eye", "ic16");
}
/* "Needs you" is an amber bell count; unread is a dot plus bold name. Neither relies on the theme's badge style. */
const needsCount = (n, text = "waiting on you") => `<span class="chat-attn">${ic("bell", "ic14")}${n}<span class="sr-only"> ${text}</span></span>`;
const unreadDot = '<span class="chat-unread" aria-hidden="true"></span>';
function rowTrail(ch, unread) {
  const needs = openAsks(ch).length;
  const dot = unread ? unreadDot : "";
  if (needs) return dot + needsCount(needs);
  if (ch.kind !== "dm") return dot;
  const word = M[ch.with].kind === "person" ? "Person" : PRESENCE_WORD[presence(ch.with)];
  return (word ? `<span class="chat-row-word">${word}</span>` : "") + dot;
}
function chatLinkHtml(k, activeId) {
  const ch = CH[k], unread = ch.unread && k !== activeId;
  const attrs = (k === activeId ? ' aria-current="page"' : "") + (unread ? ' class="is-unread"' : "");
  const unreadText = unread ? `<span class="sr-only">, ${plural(ch.unread, "unread message", "unread messages")}</span>` : "";
  return `<li><a href="#/chat/${k}"${attrs}>${rowLead(ch)}<span class="chat-row-name">${esc(ch.kind === "channel" ? ch.name : chatTitle(ch))}</span>${unreadText}${rowTrail(ch, unread)}</a></li>`;
}
function chatListHtml(id, open) {
  const keys = Object.keys(CH);
  const jumpShortcut = S.singleKey ? ' aria-keyshortcuts="#"' : "";
  const jumpKbd = S.singleKey ? '<kbd aria-hidden="true">#</kbd>' : "";
  const dialog = open ? ' role="dialog" aria-modal="true" aria-labelledby="chat-list-title"' : "";
  return `<div class="chlist" id="chat-list"${dialog}>
      <div class="chlist-head"><h2 id="chat-list-title">Conversations</h2><button class="btn ghost icon" type="button" data-act="chat-list" data-id="${id}" aria-label="Close conversations">${ic("x", "ic20")}</button></div>
      <button class="btn ghost sm chat-jump" id="chat-jump" type="button" data-act="chat-jump"
        aria-haspopup="dialog" aria-controls="cmdk-dialog"${jumpShortcut}>${ic("search", "ic16")}<span>Jump to conversation</span>${jumpKbd}</button>
      <nav aria-label="Conversations">
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
  return joinSep([`<span>${esc(m.role)}</span>`, `<span>${kindWord(who)}</span>`, status ? `<span class="chat-status">${status}</span>` : ""]);
}
function headerContext(ch) {
  if (ch.kind === "dm") return dmContext(ch.with);
  /* The read-only rule is stated once, in the footer beside its actions. */
  if (ch.kind === "a2a") return joinSep(["<span>Between agents</span>", `<span>${ch.pair.map((k) => mlink(k)).join(" and ")}</span>`]);
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
function headerActions(id, title) {
  const open = local.search?.id === id;
  return `<div class="ph-actions chat-actions">
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
        <p class="chat-context">${headerContext(ch)}</p>
      </div>
    </div>
    ${headerActions(id, title)}
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
  return `<aside class="thread" aria-labelledby="chat-thread-title"><header class="side-panel-header"><div class="thread-title"><h2 id="chat-thread-title" tabindex="-1">Thread</h2><span class="muted">${esc(title)}</span></div><button class="btn ghost icon" type="button" data-act="thread-close" aria-label="Close thread">${ic("x", "ic20")}</button></header>
      <div class="msgs">${messageHtml(Object.assign({}, root, { thread: null }), null)}<p class="chat-thread-count">${count}</p>${list}</div>
      ${CH[id].kind === "a2a" ? "" : threadComposer(id, root)}</aside>`;
}
function readOnlyFoot(id, ch) {
  const buttons = ch.pair.map((k) => `<button class="btn outline sm" type="button" data-act="chat-dm" data-id="${k}" data-v="${esc(id)}">${ic("chat", "ic16")}Message ${esc(M[k].name)}</button>`).join("");
  return `<div class="readfoot"><p>${ic("eye", "ic16")}You can read this conversation but not reply in it.</p><div class="actions">${buttons}</div></div>`;
}

/* ---------------------------------------------------------------- page */
function pageChat(id) {
  id = id && CH[id] ? id : DEFAULT_CHAT;
  const ch = CH[id];
  ch.unread = 0;
  if (S.chatList && S.chatList !== id) S.chatList = null;
  const root = S.thread && ch.msgs.find((m) => m.id === S.thread);
  const title = chatTitle(ch), listOpen = S.chatList === id;
  const cls = ["chat", root ? "withthread" : "", listOpen ? "list-open" : ""].filter(Boolean).join(" ");
  return `<div class="${cls}" data-ch="${esc(id)}">
    ${chatHeaderHtml(id, ch, title)}
    ${chatListHtml(id, listOpen)}
    <div class="chat-scrim" data-act="chat-list" data-id="${id}" aria-hidden="true"></div>
    <section class="chmain" aria-labelledby="ch-title">
      ${searchRowHtml(id, title)}
      <div class="msgs" aria-label="Messages" data-ch="${esc(id)}">${streamHtml(id)}</div>
      ${ch.kind === "a2a" ? readOnlyFoot(id, ch) : chatComposer(id, title)}</section>
    ${chatThreadHtml(id, root, title)}
  </div>`;
}

/* ---------------------------------------------------------------- after render: position, overlay, local controls */
let inerted = [];
function releaseInert() {
  inerted.forEach((el) => { el.inert = false; });
  inerted = [];
}
/* The narrow conversation list is modal: everything outside it (except its scrim) is inert. */
function inertOutside(el) {
  for (let n = el; n.parentElement && n !== document.body; n = n.parentElement) {
    for (const sib of n.parentElement.children) {
      if (sib === n || sib.inert || sib.classList.contains("chat-scrim") || sib.tagName === "SCRIPT") continue;
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
const CLICKS = {
  "chat-ask-reply": (id) => {
    Object.assign(local, { askReply: id, askNote: null });
    S.replying = null; S.confirmDeny = null;
    render(false);
    document.getElementById("ar-" + id)?.focus();
  },
  "chat-ask-send": (id) => sendAskReply(id),
  "chat-ask-cancel": (id) => closeAskReply(id),
  "chat-search": (_id, _t, chId) => toggleSearch(chId),
  "chat-task": (id, t, chId) => turnIntoTask(t, chId, id),
  "chat-src-remove": (id) => {
    dmSources.delete(id);
    render(false);
    document.getElementById("compose")?.focus();
  },
};
function onChatClick(e) {
  const t = e.target.closest("[data-act]"), fn = t && CLICKS[t.dataset.act];
  if (fn) fn(t.dataset.id, t, e.currentTarget.dataset.ch);
}
function onChatInput(e) {
  const t = e.target;
  if (t.id === "thread-compose") { threadDrafts.set(t.form.dataset.id, t.value); fitTextarea(t); }
  else if (t.id.startsWith("ar-")) askDrafts.set(t.id.slice(3), t.value);
  else if (t.id === "chat-search-input" && local.search) { local.search.q = t.value; refreshSearch(document.querySelector(".chat .chmain .msgs")); }
}
function onChatKeydown(e) {
  const t = e.target;
  if (t.id === "chat-search-input" && e.key === "Escape") { e.preventDefault(); e.stopPropagation(); toggleSearch(local.search.id); return; }
  if (!t.id.startsWith("ar-") || t.tagName !== "TEXTAREA") return;
  const id = t.id.slice(3);
  if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); closeAskReply(id); }
  else if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) { e.preventDefault(); e.stopPropagation(); sendAskReply(id); }
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
  root.addEventListener("click", onChatClick);
  root.addEventListener("input", onChatInput);
  root.addEventListener("keydown", onChatKeydown);
  fitTextarea(document.getElementById("thread-compose"));
  if (root.classList.contains("list-open")) inertOutside(root.querySelector(".chlist"));
}
/* Leaving Chat (or the conversation) always releases the modal list's inert background. */
window.addEventListener("hashchange", () => {
  releaseInert();
  local.askReply = null;
  local.askNote = null;
});

export {
  chatDocTitle,
  chatDrafts,
  msgHtml,
  openDirectMessage,
  mountChat,
  pageChat,
  recordAskReply,
  sendChatMessage,
  sendThreadReply,
};
