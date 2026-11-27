/* Chat conversation list: sections, "Find a conversation" filtering inside the list (so the narrow
   drawer stays the one overlay), and "New message" with a recipient picker. */
import { CH, M } from "../data.js";
import { S } from "../state.js";
import { av, esc, ic, kindWord, plural, presence } from "../ui.js";
import { chatTitle, findConversations, matchReason, openAsks, openDirectMessage, recipients } from "./chat-model.js";
import { viewLinks } from "./chat-views.js";
import { A2A_CLICKS, a2aSectionHtml, lastPicked, onA2AInput, onA2AKeydown } from "./chat-a2a.js";

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
function rowTrail(ch, unread, when = "") {
  const needs = openAsks(ch).length;
  const dot = (when ? `<time class="chat-row-when">${esc(when)}</time>` : "") + (unread ? unreadDot : "");
  if (needs) return dot + needsCount(needs);
  if (ch.kind !== "dm") return dot;
  const word = M[ch.with].kind === "person" ? "Person" : PRESENCE_WORD[presence(ch.with)];
  return (word ? `<span class="chat-row-word">${word}</span>` : "") + dot;
}
/* `why`: while finding, what matched when the name doesn't show it (a project, a task id, a message's words). */
function chatLinkHtml(k, activeId, why = "", when = "") {
  const ch = CH[k], unread = ch.unread && k !== activeId;
  const attrs = (k === activeId ? ' aria-current="page"' : "") + (unread ? ' class="is-unread"' : "");
  const unreadText = unread ? `<span class="sr-only">, ${plural(ch.unread, "unread message", "unread messages")}</span>` : "";
  const reason = why ? `<span class="chat-row-why"><span class="sr-only">, </span>${esc(why)}</span>` : "";
  return `<li><a href="#/chat/${k}"${attrs}>${rowLead(ch)}<span class="chat-row-name">${esc(ch.kind === "channel" ? ch.name : chatTitle(ch))}${reason}</span>${unreadText}${rowTrail(ch, unread, when)}</a></li>`;
}
function groupHtml(kind, label, rows, activeId, q) {
  if (kind === "a2a" && !q) return rows.length ? a2aSectionHtml(rows, activeId, (k, when) => chatLinkHtml(k, activeId, "", when)) : "";
  return rows.length ? `<h2>${label}</h2><ul>${rows.map((k) => chatLinkHtml(k, activeId, q ? matchReason(CH[k], q) : "")).join("")}</ul>` : "";
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
  return (q ? "" : viewLinks(activeId)) + GROUPS.map(([kind, label]) => groupHtml(kind, label, keys.filter((k) => CH[k].kind === kind), activeId, q)).join("");
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
/* While the recipient picker is open, the message has no destination yet: the conversation behind it steps back
   (inert, its composer suspended) and the main pane says "Choose a recipient" until a choice or Cancel. */
function syncPicking(root) {
  const main = root.querySelector(".chmain");
  if (!main) return;
  const on = list.picker && Boolean(CH[root.dataset.ch]);
  root.classList.toggle("is-picking", on);
  main.querySelector(":scope > .chat-picking")?.remove();
  for (const el of main.children) el.inert = on;
  if (!on) return;
  const from = chatTitle(CH[root.dataset.ch]);
  main.insertAdjacentHTML("afterbegin", `<div class="chat-picking" role="status">
      <span class="chat-picking-ic">${ic("pencil", "ic20")}</span>
      <h2 class="chat-picking-t">Choose a recipient</h2>
      <p class="chat-picking-p">Pick a person or agent under <b>To</b> to start a direct message. Messages to ${esc(from)} are paused until you choose or cancel.</p>
      <button class="btn ghost sm" type="button" data-act="chat-new-cancel">Cancel new message</button>
    </div>`);
}
function setPicker(root, on) {
  Object.assign(list, { picker: on, to: "" });
  rerenderList(root, on ? "chat-to" : "chat-new");
  syncPicking(root);
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
/* Choosing any conversation closes the drawer. Choosing the one already open changes no URL, so the caller
   closes the drawer in place and focuses the conversation's heading ("same-conversation"). */
function chooseConversation(e, link) {
  list.q = "";
  S.chatList = null;
  if (link.getAttribute("href") !== location.hash) return "";
  e.preventDefault();
  e.stopPropagation();
  return "same-conversation";
}
/* "Between agents" is the list's last section: while its picker is open the list scrolls just enough to show the
   section whole (or from its heading down), so the picked names and their conversations are in view together. */
function revealA2A(nav) {
  const head = nav.querySelector("#chat-a2a-head");
  if (!head || !nav.querySelector("#chat-a2a-filter")) return;
  const headTop = head.getBoundingClientRect().top - nav.getBoundingClientRect().top + nav.scrollTop;
  nav.scrollTop = Math.max(nav.scrollTop, Math.min(headTop - 4, nav.scrollHeight - nav.clientHeight));
}
/* The agents' section's controls re-render the list in place and keep focus on the control used. */
function repaintA2A(root, focusId) {
  const nav = root.querySelector("#chat-nav"), top = nav.scrollTop;
  nav.innerHTML = navInner(root.dataset.ch);
  nav.scrollTop = top;
  revealA2A(nav);
  document.getElementById(focusId)?.focus({ preventScroll: true });
}
function onA2AClick(root, t) {
  repaintA2A(root, A2A_CLICKS[t.dataset.act](t.dataset.id));
  return "handled";
}
/* Enter picks the highlighted agent, Backspace in the empty field removes the last one, Escape closes the picker. */
function onPickerKeydown(e, root) {
  const r = onA2AKeydown(e);
  if (r === "close") repaintA2A(root, A2A_CLICKS["chat-a2a-filter"]());
  else if (r === "remove") repaintA2A(root, A2A_CLICKS["chat-a2a-agent"](lastPicked()));
  else if (r) repaintA2A(root, A2A_CLICKS["chat-a2a-agent"](r));
}
function onListClick(e, root) {
  const link = e.target.closest("#chat-nav a");
  if (link) return chooseConversation(e, link);
  const t = e.target.closest("[data-act]");
  if (t && A2A_CLICKS[t.dataset.act]) return onA2AClick(root, t);
  const fn = t && LIST_CLICKS[t.dataset.act];
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
  } else if (t.id === "chat-a2a-q") onA2AInput(t);
}
/* Escape clears the filter, then closes the picker; only an empty list field lets Escape close the drawer. */
function onListKeydown(e, root) {
  const t = e.target;
  if (t.id === "chat-a2a-q") { onPickerKeydown(e, root); return; }
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
  PRESENCE_WORD,
  chatListHtml,
  needsCount,
  onListClick,
  onListInput,
  onListKeydown,
  prepareList,
  syncPicking,
  takeListFocus,
  unreadDot,
};
