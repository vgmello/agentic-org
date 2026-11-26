/* Chat conversation list: sections, "Find a conversation" filtering inside the list (so the narrow
   drawer stays the one overlay), and "New message" with a recipient picker. */
import { CH, M } from "../data.js";
import { S } from "../state.js";
import { av, esc, ic, kindWord, plural, presence } from "../ui.js";
import { chatTitle, findConversations, openAsks, openDirectMessage, recipients } from "./chat-model.js";
import { viewLinks } from "./chat-views.js";

const GROUPS = [["channel", "Channels"], ["dm", "Direct messages"], ["a2a", "Between agents"]];
const PRESENCE_WORD = { needs: "Waiting on you", withyou: "With you", working: "Working", paused: "Paused", available: "Available", asleep: "Asleep", offline: "Offline" };
const list = { q: "", picker: false, to: "", focusAfter: "" };

/* "Needs you" is an amber bell count; unread is a dot plus bold name. Neither relies on the theme's badge style. */
const needsCount = (n, text = "waiting on you") => `<span class="chat-attn">${ic("bell", "ic14")}${n}<span class="sr-only"> ${text}</span></span>`;
const unreadDot = '<span class="chat-unread" aria-hidden="true"></span>';

function rowLead(ch) {
  if (ch.kind === "channel") return ic("hash", "ic16");
  return ch.kind === "dm" ? av(ch.with, 24) : ic("eye", "ic16");
}
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
function recipientButton(k, act = "chat-to") {
  return `<li><button class="chat-person" type="button" data-act="${act}" data-id="${esc(k)}">${av(k, 24)}<span class="chat-row-name">${esc(M[k].name)}</span><span class="chat-row-word">${esc(kindWord(k))}</span></button></li>`;
}
/* With no conversation match, people and agents matching the text can be messaged straight away. */
function noConversation(q) {
  const people = recipients(q).slice(0, 4);
  const start = people.length ? `<h2>Start a conversation</h2><ul>${people.map((k) => recipientButton(k)).join("")}</ul>` : "";
  return `<p class="chat-find-empty">No conversations match “${esc(list.q.trim())}”.</p>${start}`;
}
function navInner(activeId) {
  const q = list.q.trim().toLowerCase();
  const keys = q ? findConversations(q) : Object.keys(CH);
  if (!keys.length) return noConversation(q);
  return (q ? "" : viewLinks(activeId)) + GROUPS.map(([kind, label]) => {
    const rows = keys.filter((k) => CH[k].kind === kind);
    return rows.length ? `<h2>${label}</h2><ul>${rows.map((k) => chatLinkHtml(k, activeId)).join("")}</ul>` : "";
  }).join("");
}
const findStatus = () => (list.q.trim() ? plural(findConversations(list.q.trim().toLowerCase()).length, "conversation", "conversations") : "");

function findHtml() {
  const kbd = S.singleKey ? '<kbd aria-hidden="true">#</kbd>' : "";
  return `<div class="chlist-tools">
      <div class="chat-find">${ic("search", "ic16")}<label class="sr-only" for="chat-find">Find a conversation</label>
        <input class="field" id="chat-find" type="search" placeholder="Find a conversation" value="${esc(list.q)}" autocomplete="off" aria-controls="chat-nav" aria-describedby="chat-find-status"${S.singleKey ? ' aria-keyshortcuts="#"' : ""}>${kbd}</div>
      <p class="sr-only" id="chat-find-status" role="status">${findStatus()}</p>
      <button class="btn ghost sm chat-new" id="chat-new" type="button" data-act="chat-new">${ic("pencil", "ic16")}<span>New message</span></button>
    </div>`;
}
function pickerList() {
  const people = recipients(list.to.trim().toLowerCase());
  return people.length ? people.map((k) => recipientButton(k)).join("") : `<li class="chat-find-empty">No one matches “${esc(list.to.trim())}”.</li>`;
}
function pickerHtml() {
  return `<section class="chat-picker" id="chat-picker" aria-labelledby="chat-picker-title">
      <div class="chat-picker-head"><h2 id="chat-picker-title">New message</h2>
        <button class="btn ghost sm" type="button" data-act="chat-new-cancel">Cancel</button></div>
      <label class="chat-picker-to" for="chat-to">To</label>
      <input class="field" id="chat-to" type="search" placeholder="Name or role" value="${esc(list.to)}" autocomplete="off" aria-controls="chat-to-list">
      <ul id="chat-to-list" aria-label="People and agents">${pickerList()}</ul>
    </section>`;
}
function chatListHtml(id, open) {
  const dialog = open ? ' role="dialog" aria-modal="true" aria-labelledby="chat-list-title"' : "";
  const body = list.picker ? pickerHtml() : `${findHtml()}<nav id="chat-nav" aria-label="Conversations">${navInner(id)}</nav>`;
  return `<div class="chlist" id="chat-list"${dialog}>
      <div class="chlist-head"><h2 id="chat-list-title">Conversations</h2><button class="btn ghost icon" type="button" data-act="chat-list" data-id="${id}" aria-label="Close conversations">${ic("x", "ic20")}</button></div>
      ${body}
    </div>`;
}

/* ---------------------------------------------------------------- behaviour (delegated from the Chat root) */
function rerenderList(root, focusId) {
  const el = root.querySelector(".chlist");
  const open = root.classList.contains("list-open");
  el.outerHTML = chatListHtml(root.dataset.ch, open);
  document.getElementById(focusId)?.focus();
}
function setPicker(root, on) {
  Object.assign(list, { picker: on, to: "" });
  rerenderList(root, on ? "chat-to" : "chat-new");
}
/* Opens the DM with the composer focused. When that DM is already open the hash would not change, so the
   caller closes the picker and drawer in place ("same-conversation"). */
function startMessage(root, who) {
  Object.assign(list, { picker: false, to: "", q: "", focusAfter: "compose" });
  const hash = openDirectMessage(who);
  if (hash === "#/chat/" + root.dataset.ch) return "same-conversation";
  S.chatList = null;
  location.hash = hash;
  return "";
}
const LIST_CLICKS = {
  "chat-new": (root) => setPicker(root, true),
  "chat-new-cancel": (root) => setPicker(root, false),
  "chat-to": (root, id) => startMessage(root, id),
};
function onListClick(e, root) {
  if (e.target.closest("#chat-nav a")) list.q = "";
  const t = e.target.closest("[data-act]"), fn = t && LIST_CLICKS[t.dataset.act];
  return fn ? fn(root, t.dataset.id) : "";
}
function onListInput(e, root) {
  const t = e.target;
  if (t.id === "chat-find") {
    list.q = t.value;
    root.querySelector("#chat-nav").innerHTML = navInner(root.dataset.ch);
    root.querySelector("#chat-find-status").textContent = findStatus();
  } else if (t.id === "chat-to") {
    list.to = t.value;
    root.querySelector("#chat-to-list").innerHTML = pickerList();
  }
}
/* Escape clears the filter, then closes the picker; only an empty list field lets Escape close the drawer. */
function onListKeydown(e, root) {
  const t = e.target;
  if (e.key === "Escape" && t.id === "chat-to") { e.preventDefault(); e.stopPropagation(); setPicker(root, false); return; }
  if (e.key === "Escape" && t.id === "chat-find" && list.q) { e.preventDefault(); e.stopPropagation(); list.q = ""; rerenderList(root, "chat-find"); return; }
  if (e.key === "Enter" && (t.id === "chat-find" || t.id === "chat-to")) {
    e.preventDefault();
    root.querySelector(t.id === "chat-find" ? "#chat-nav a, #chat-nav button" : "#chat-to-list button")?.click();
  }
}
/* After a navigation started here (picker, "#", New message command), the intended field takes focus. */
function takeListFocus() {
  const el = list.focusAfter ? document.getElementById(list.focusAfter) : null;
  list.focusAfter = "";
  el?.focus();
  return Boolean(el);
}
/* Prepares the list for the next render: the recipient picker, or the conversation filter. */
function prepareList(mode) {
  Object.assign(list, mode === "new" ? { picker: true, to: "", focusAfter: "chat-to" } : { picker: false, focusAfter: "chat-find" });
}

export {
  chatListHtml,
  needsCount,
  onListClick,
  onListInput,
  onListKeydown,
  prepareList,
  takeListFocus,
  unreadDot,
};
