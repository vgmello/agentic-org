/* Owner asks inside Chat messages.
   Routing rule: questions and low-risk asks are answered inline on the card; an ask whose data marks it
   `consequential` (binding choice, tradeoff, high or irreversible impact) is decided in Inbox, and the card
   says why. "Ask a follow-up" posts in the conversation and on the request and never resolves it; only an
   explicit answer, option or decision button does. */
import { CH, KIND, M, W, blockedOnOwner } from "../data.js";
import { workHref } from "./work.js";
import { v4BuildButtons, v4ReplyBox } from "./home.js";
import { inboxCommentPrefix } from "./inbox.js";
import { render } from "../shell/router.js";
import { S } from "../state.js";
import { esc, ic, joinSep, pill } from "../ui.js";
import { inboxItem } from "./chat-model.js";

const askDrafts = new Map(); /* inbox item id -> unsent follow-up on that request */
const asks = { followUp: null, note: null };
const clock = () => new Date().toTimeString().slice(0, 5);

function workRef(id) {
  return W.some((w) => w.id === id) ? `<a class="alink chat-ref" href="${esc(workHref({ item: id }))}">${id}</a>` : id;
}
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
const settles = (it) => (it.kind === "question" ? "you answer" : "you decide");
function followUpBox(it) {
  const who = esc(M[it.from].name), id = esc(it.id);
  return `<div class="reply chat-ask-reply"><label class="sr-only" for="ar-${id}">Ask ${who} a follow-up</label>
    <textarea class="field" id="ar-${id}" rows="2" placeholder="Ask ${who} a follow-up…" aria-describedby="ar-help-${id}">${esc(askDrafts.get(it.id) || "")}</textarea>
    <p class="chat-compose-hint" id="ar-help-${id}">Posts here and on the request. It stays open until ${settles(it)}.</p>
    <div class="actions"><button class="btn primary sm" type="button" data-act="chat-ask-send" data-id="${id}">${ic("send", "ic16")}Send follow-up</button><button class="btn ghost sm" type="button" data-act="chat-ask-cancel" data-id="${id}">Cancel</button></div></div>`;
}
function inlineControls(it) {
  const { b, guardedHtml } = v4BuildButtons(it);
  const sep = guardedHtml && S.confirmDeny !== it.id ? '<span class="hb-act-sep" aria-hidden="true"></span>' : "";
  return [...b, sep, guardedHtml];
}
function needsActions(it) {
  if (it.handled) return handledLine(it);
  if (S.replying === it.id) return v4ReplyBox(it);
  if (asks.followUp === it.id) return followUpBox(it);
  const inbox = Boolean(it.consequential);
  const primary = inbox ? [inboxLink(it, `${ic("inbox", "ic16")}Review in Inbox`, "btn primary sm")] : inlineControls(it);
  const followUp = `<button class="btn ghost sm" type="button" data-act="chat-ask-reply" data-id="${esc(it.id)}">${ic("reply", "ic16")}Ask a follow-up</button>`;
  const open = inbox ? "" : inboxLink(it, `${ic("inbox", "ic16")}<span><span class="chat-long">Open in </span>Inbox</span>`);
  const why = inbox ? `<p class="chat-needs-route">${ic("inbox", "ic14")}<span>Decide in Inbox · ${esc(it.consequential)}</span></p>` : "";
  const note = asks.note === it.id ? `<p class="chat-needs-note" role="status">${ic("check-circle", "ic14")}Follow-up sent to ${esc(M[it.from].name)}. The request stays open until ${settles(it)}.</p>` : "";
  return `${note}${why}<div class="actions">${[...primary, followUp, open].join("")}</div>`;
}
function tradeoff(it) {
  if (!it.consequential || it.handled || !it.why) return "";
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
    <h3 class="chat-needs-q" id="${titleId}" tabindex="-1">${esc(it.what)}</h3>
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
/* A card where the ask was raised (m.needs); a chip where it is only mentioned (m.ref) or previewed elsewhere. */
function messageNeeds(m, cards) {
  const it = inboxItem(m.needs || m.ref);
  if (!it) return "";
  return cards && m.needs ? needsCardHtml(it, m.id) : needsChipHtml(it);
}

/* ---------------------------------------------------------------- follow-ups and answers posted from Chat */
function saveInboxComment(id, text) {
  try { localStorage.setItem(inboxCommentPrefix(id) + crypto.randomUUID(), JSON.stringify({ text, at: Date.now() })); } catch (e) {}
}
/* Posts the owner's words in the conversation where the request was raised and on the Inbox record. */
function recordAskReply(id, text) {
  const it = inboxItem(id);
  const home = (it?.origin && CH[it.origin.ch]) || Object.values(CH).find((ch) => ch.msgs.some((m) => m.needs === id));
  if (home) home.msgs.push({ id: "u" + Date.now(), who: "you", time: clock(), text, ref: id });
  saveInboxComment(id, text);
}
const focusFollowUp = (id) => document.querySelector(`[data-act="chat-ask-reply"][data-id="${CSS.escape(id)}"]`)?.focus();
function sendFollowUp(id) {
  const field = document.getElementById("ar-" + id), text = field?.value.trim();
  if (!field) return;
  if (!text) {
    const hint = document.getElementById("ar-help-" + id);
    hint.classList.add("is-error");
    hint.textContent = "Write a follow-up first.";
    field.focus();
    return;
  }
  recordAskReply(id, text);
  askDrafts.delete(id);
  Object.assign(asks, { followUp: null, note: id });
  render(false);
  focusFollowUp(id);
}
function closeFollowUp(id) {
  asks.followUp = null;
  render(false);
  focusFollowUp(id);
}
function openFollowUp(id) {
  Object.assign(asks, { followUp: id, note: null });
  S.replying = null; S.confirmDeny = null;
  render(false);
  document.getElementById("ar-" + id)?.focus();
}
const ASK_CLICKS = { "chat-ask-reply": openFollowUp, "chat-ask-send": sendFollowUp, "chat-ask-cancel": closeFollowUp };
function onAskKeydown(e) {
  const t = e.target;
  if (!t.id.startsWith("ar-") || t.tagName !== "TEXTAREA") return;
  const id = t.id.slice(3);
  if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); closeFollowUp(id); }
  else if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) { e.preventDefault(); e.stopPropagation(); sendFollowUp(id); }
}
const saveAskDraft = (t) => askDrafts.set(t.id.slice(3), t.value);
const resetAsks = () => Object.assign(asks, { followUp: null, note: null });

export { ASK_CLICKS, messageNeeds, onAskKeydown, recordAskReply, resetAsks, saveAskDraft, workRef };
