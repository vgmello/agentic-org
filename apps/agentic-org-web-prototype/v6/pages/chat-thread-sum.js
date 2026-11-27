/* Narrow threads (below 640 px): the thread's first message collapses to a compact summary (who, when, its first
   lines and, for an ask, its title), with the reply count and a jump to the replies, so replies are in the first view.
   "Show full message" opens the whole message (and its ask card) in place; the choice is kept per thread. */
import { M } from "../data.js";
import { av, esc, ic, plural } from "../ui.js";
import { inboxItem, splitTime } from "./chat-model.js";
import { decidedInInbox, reviewInInbox, routeReasonHtml } from "./ask-route.js";

const openRoots = new Set();

const rootOpen = (rootId) => openRoots.has(rootId);
function askLine(root) {
  const it = root.needs && inboxItem(root.needs);
  if (!it) return "";
  const state = it.handled ? `${ic("check-circle", "ic14")}<span>Handled · ` : `${ic("bell", "ic14")}<span>Needs you · `;
  return `<p class="chat-thread-sum-ask${it.handled ? " is-done" : ""}">${state}<b>${esc(it.what)}</b></span></p>${inboxRoute(it)}`;
}
/* A consequential ask is decided in Inbox: the collapsed summary keeps its short reason and Review in Inbox in view,
   as the full card states them. */
function inboxRoute(it) {
  if (!decidedInInbox(it)) return "";
  return `<div class="chat-thread-sum-route">${routeReasonHtml(it, "chat-thread-sum-why")}${reviewInInbox(it)}</div>`;
}
function repliesButton(n) {
  if (!n) return `<span class="chat-thread-sum-n">No replies yet</span>`;
  return `<button class="btn ghost sm" type="button" data-act="chat-thread-replies">${ic("chev-down", "ic16")}<span>${plural(n, "reply", "replies")}</span></button>`;
}
function threadSummaryHtml(root) {
  const open = rootOpen(root.id), n = (root.thread || []).length;
  const toggle = `<button class="btn ghost sm" type="button" data-act="chat-thread-root" data-id="${esc(root.id)}" aria-expanded="${open}" aria-controls="chat-thread-root">${open ? "Hide full message" : "Show full message"}</button>`;
  return `<div class="chat-thread-sum">
      <p class="chat-thread-sum-who">${av(root.who, 24)}<b>${esc(M[root.who].name)}</b><time>${esc(splitTime(root.time)[1])}</time></p>
      ${root.text ? `<p class="chat-thread-sum-tx">${esc(root.text)}</p>` : ""}${askLine(root)}
      <div class="chat-thread-sum-acts">${toggle}${repliesButton(n)}</div>
    </div>`;
}
/* Marks the thread's first message so narrow CSS can collapse it behind the summary. */
const markRoot = (html) => html.replace('<div class="msg', '<div id="chat-thread-root" class="chat-thread-root msg');

function setRootOpen(rootId, open) {
  if (open) openRoots.add(rootId);
  else openRoots.delete(rootId);
  const aside = document.querySelector("aside.thread:not(.chat-members-panel, .chat-files-panel)");
  aside?.classList.toggle("root-open", open);
  const btn = aside?.querySelector('[data-act="chat-thread-root"]');
  if (!btn) return;
  btn.setAttribute("aria-expanded", String(open));
  btn.textContent = open ? "Hide full message" : "Show full message";
}
function jumpToReplies() {
  const s = document.querySelector("aside.thread .msgs"), count = s?.querySelector(":scope > .chat-thread-count");
  if (!count) return;
  s.scrollTop += count.getBoundingClientRect().top - s.getBoundingClientRect().top - 8;
  const first = count.nextElementSibling;
  if (!first) return;
  first.tabIndex = -1;
  first.focus({ preventScroll: true });
}
const THREAD_SUM_CLICKS = {
  "chat-thread-root": (id) => setRootOpen(id, !rootOpen(id)),
  "chat-thread-replies": () => jumpToReplies(),
};

export { THREAD_SUM_CLICKS, markRoot, rootOpen, setRootOpen, threadSummaryHtml };
