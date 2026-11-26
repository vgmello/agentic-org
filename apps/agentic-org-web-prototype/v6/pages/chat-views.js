/* Chat's Mentions and Threads views (doc-40 §1: above the conversation sections).
   Mentions: messages from others that ask the owner something or name the owner.
   Threads: messages continued in a thread, in conversations the owner can read. */
import { CH, M } from "../data.js";
import { av, esc, ic, plural } from "../ui.js";
import { chatTitle } from "./chat-model.js";

/* Mentions come from explicit data only: an ask raised to the owner (`needs`), or the owner among a message's
   `mentions`, in a post or a thread reply. Prose such as "I asked the owner" is not a mention. */
const addressesOwner = (m) => m.who !== "you" && (Boolean(m.needs) || Boolean(m.mentions?.includes("you")));
function mentionItems(ch) {
  return CH[ch].msgs.flatMap((m) => [
    ...(addressesOwner(m) ? [{ ch, m }] : []),
    ...(m.thread || []).flatMap((r, i) => (addressesOwner(r) ? [{ ch, m: r, root: m, reply: i }] : [])),
  ]);
}
function viewItems(kind) {
  if (kind === "mentions") return Object.keys(CH).flatMap(mentionItems);
  return Object.keys(CH).flatMap((ch) => CH[ch].msgs.filter((m) => m.thread && m.thread.length).map((m) => ({ ch, m })));
}
const CHAT_VIEWS = {
  mentions: { title: "Mentions", icon: "at", about: "Messages that ask you something or name you", empty: "Nothing mentions you yet." },
  threads: { title: "Threads", icon: "thread", about: "Messages continued in a thread", empty: "No threads yet." },
};
const isChatView = (id) => Object.hasOwn(CHAT_VIEWS, id || "");

function itemHead(ch, m, root) {
  const where = root ? `in a thread on ${M[root.who].name}'s message in ${chatTitle(CH[ch])}` : `in ${chatTitle(CH[ch])}`;
  return `<p class="chat-view-head">${av(m.who, 24)}<b>${esc(M[m.who].name)}</b><span>${esc(where)}</span><time>${esc(m.time)}</time></p>`;
}
/* A thread reply opens its thread on that reply; a post opens the message. */
function mentionItem({ ch, m, root, reply }) {
  const open = root
    ? `<button class="btn ghost sm" type="button" data-act="chat-open-thread" data-ch="${esc(ch)}" data-id="${esc(root.id)}" data-hit="${reply}">${ic("thread", "ic16")}Open in thread</button>`
    : `<a class="btn ghost sm" href="#/chat/${esc(ch)}/${esc(m.id)}">${ic("chev-right", "ic16")}Open message</a>`;
  return `<li class="chat-view-item">${itemHead(ch, m, root)}<p class="chat-view-text">${esc(m.text)}</p>${open}</li>`;
}
function threadItem({ ch, m }) {
  const last = m.thread[m.thread.length - 1];
  return `<li class="chat-view-item">${itemHead(ch, m)}<p class="chat-view-text">${esc(m.text)}</p>
    <p class="chat-view-last">${ic("thread", "ic14")}<span>${plural(m.thread.length, "reply", "replies")} · last from ${esc(M[last.who].name)} at ${esc(last.time)}</span></p>
    <button class="btn ghost sm" type="button" data-act="chat-open-thread" data-ch="${esc(ch)}" data-id="${esc(m.id)}">${ic("thread", "ic16")}Open thread</button></li>`;
}
function chatViewHtml(kind) {
  const view = CHAT_VIEWS[kind], items = viewItems(kind);
  const body = items.length
    ? `<ul class="chat-view-list" aria-label="${esc(view.title)}">${items.map(kind === "mentions" ? mentionItem : threadItem).join("")}</ul>`
    : `<div class="chat-empty"><h2>${esc(view.empty)}</h2></div>`;
  return `<div class="msgs chat-view">${body}</div>`;
}
/* The icon set has no "@", so Mentions uses the character itself. */
const viewIcon = (v) => (v.icon === "at" ? '<span class="chat-at ic16" aria-hidden="true">@</span>' : ic(v.icon, "ic16"));
/* The two entries above the list's sections. */
function viewLinks(activeId) {
  return `<ul class="chat-views">${Object.entries(CHAT_VIEWS).map(([k, v]) => `<li><a href="#/chat/${k}"${k === activeId ? ' aria-current="page"' : ""}>${viewIcon(v)}<span class="chat-row-name">${v.title}</span></a></li>`).join("")}</ul>`;
}

export { CHAT_VIEWS, chatViewHtml, isChatView, viewLinks };
