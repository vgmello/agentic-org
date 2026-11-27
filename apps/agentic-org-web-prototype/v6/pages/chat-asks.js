/* Owner asks inside Chat messages.
   Routing rule: questions and low-risk asks are answered inline on the card; an ask whose data marks it
   `consequential` (binding choice, tradeoff, high or irreversible impact) is decided in Inbox, and the card
   says why. "Ask a follow-up" posts where the owner is (the thread, or the conversation) and on the request,
   and never resolves it; only an explicit answer, option or decision button does. */
import { CH, KIND, M, W, blockedOnOwner } from "../data.js";
import { workHref } from "./work.js";
import { v4BuildButtons, v4ReplyBox } from "./home.js";
import { inboxCommentPrefix } from "./inbox.js";
import { render } from "../shell/router.js";
import { S } from "../state.js";
import { esc, ic, joinSep, pill } from "../ui.js";
import { chatTitle, inboxItem } from "./chat-model.js";
import { followThreadEnd } from "./chat-scroll.js";
import { receiptHtml } from "./ask-receipt.js";
import { decidedInInbox, reviewInInbox, routeReasonHtml } from "./ask-route.js";
import { denyDraftKey, draftText } from "./ask-drafts.js";

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
/* The ask's one response record: the same receipt as Inbox and Home, plus the way to the full record. */
const handledLine = (it) => receiptHtml(it, "handled chat-needs-done", inboxLink(it, "View in Inbox", "alink chat-needs-record"));
const settles = (it) => (it.kind === "question" ? "you answer" : "you decide");
/* The conversation an ask was raised in: its origin, or the conversation whose message carries its card. */
function askHomeId(it) {
  if (it?.origin && CH[it.origin.ch]) return it.origin.ch;
  return Object.keys(CH).find((k) => CH[k].msgs.some((m) => m.needs === it?.id));
}
const placeName = (chId) => (CH[chId].kind === "dm" ? `${M[CH[chId].with].name}'s DM` : chatTitle(CH[chId]));
/* The follow-up names where it will appear before it is sent: this thread, or the conversation. */
function followUpBox(it, where) {
  const who = esc(M[it.from].name), id = esc(it.id), home = askHomeId(it);
  const dest = where.thread ? "this thread" : home ? esc(placeName(home)) : "this conversation";
  const root = where.thread ? ` data-root="${esc(where.thread)}"` : "";
  return `<div class="reply chat-ask-reply"><label class="sr-only" for="ar-${id}">Ask ${who} a follow-up</label>
    <textarea class="field" id="ar-${id}" rows="2" placeholder="Ask ${who} a follow-up…" aria-describedby="ar-help-${id}"${root}>${esc(askDrafts.get(it.id) || "")}</textarea>
    <p class="chat-compose-hint chat-ask-dest" id="ar-help-${id}">${ic(where.thread ? "thread" : "chat", "ic14")}<span>Posts in ${dest} and on the Inbox request. It stays open until ${settles(it)}.</span></p>
    <div class="actions"><button class="btn primary sm" type="button" data-act="chat-ask-send" data-id="${id}">${ic("send", "ic16")}${where.thread ? "Send to thread" : "Send follow-up"}</button><button class="btn ghost sm" type="button" data-act="chat-ask-cancel" data-id="${id}">Cancel</button></div></div>`;
}
function inlineControls(it) {
  const { b, guardedHtml } = v4BuildButtons(it);
  const sep = guardedHtml && S.confirmDeny !== it.id ? '<span class="hb-act-sep" aria-hidden="true"></span>' : "";
  return [...b, sep, guardedHtml];
}
/* The guarded action of an inline ask (Deny, Dismiss…): its label, recorded outcome and icon. */
function guardOf(it) {
  if (it.kind === "approval" && !it.acts) return { label: "Deny", outcome: "Denied", icon: "deny" };
  if (it.kind === "problem" && it.dismiss) return { label: "Dismiss", outcome: "Dismissed", icon: "x" };
  const act = (it.acts || []).find((a) => a[3] === "deny");
  return act ? { label: act[0], outcome: act[1], icon: act[2] || "x" } : null;
}
const CONFIRM_WORD = { Deny: "Confirm denial", Dismiss: "Confirm dismissal" };
/* Armed denial replaces the whole action row: the opposite outcome (Approve) is not on screen while it is armed.
   Focus starts in the optional reason; Keep request open (or Escape) disarms and returns to Deny. */
function armedDenial(it, g) {
  const who = esc(M[it.from]?.name || "the agent"), id = esc(it.id), confirm = CONFIRM_WORD[g.label] || `Confirm: ${esc(g.label)}`;
  return `<div class="hb-deny-guard chat-deny-guard" role="group" aria-label="${confirm}">
    <label class="sr-only" for="deny-msg-${id}">Optional reason for ${who}</label>
    <input class="field hb-deny-input" id="deny-msg-${id}" data-deny-id="${id}" data-deny-outcome="${esc(g.outcome)}" data-draft="${esc(denyDraftKey(it.id))}" type="text" placeholder="Optional reason for ${who}…" value="${esc(draftText(denyDraftKey(it.id)))}" autocomplete="off">
    <div class="actions"><button class="btn sm hb-deny-confirm" type="button" data-act="deny-confirm" data-id="${id}" data-v="${esc(g.outcome)}">${ic(g.icon, "ic16")}${confirm}</button><button class="btn ghost sm" type="button" data-act="deny-keep" data-id="${id}">Keep request open</button></div>
  </div>`;
}
/* A step that replaces the card's buttons: its receipt, a reply or follow-up being written, or an armed denial. */
function openStep(it, where) {
  if (it.handled) return handledLine(it);
  if (S.replying === it.id) return v4ReplyBox(it);
  if (asks.followUp === it.id) return followUpBox(it, where);
  const guard = S.confirmDeny === it.id && !decidedInInbox(it) && guardOf(it);
  return guard ? armedDenial(it, guard) : "";
}
function needsActions(it, where) {
  const step = openStep(it, where);
  if (step) return step;
  const inbox = decidedInInbox(it);
  const primary = inbox ? [reviewInInbox(it)] : inlineControls(it);
  const followUp = `<button class="btn ghost sm" type="button" data-act="chat-ask-reply" data-id="${esc(it.id)}">${ic("reply", "ic16")}Ask a follow-up</button>`;
  const open = inbox ? "" : inboxLink(it, `${ic("inbox", "ic16")}<span><span class="chat-long">Open in </span>Inbox</span>`);
  const why = inbox ? routeReasonHtml(it, "chat-needs-route") : "";
  const sentTo = where.thread ? " in this thread" : "";
  const note = asks.note === it.id ? `<p class="chat-needs-note" role="status">${ic("check-circle", "ic14")}Follow-up sent to ${esc(M[it.from].name)}${sentTo}. The request stays open until ${settles(it)}.</p>` : "";
  /* The responses (presets and Answer…, or approve and deny) stay together as one group; the follow-up and the
     record link follow as a second group, wrapping as a unit onto their own line when space runs out. */
  return `${note}${why}<div class="actions chat-needs-acts"><span class="chat-needs-answer">${primary.join("")}</span><span class="chat-needs-aside">${followUp}${open}</span></div>`;
}
function tradeoff(it) {
  if (!decidedInInbox(it) || !it.why) return "";
  return `<details class="chat-needs-why"><summary>${ic("scale", "ic14")}Tradeoff${ic("chev-right", "ic14 chat-needs-chev")}</summary><p>${esc(it.why)}</p>${it.rec ? `<p class="chat-needs-rec">${esc(it.rec)}</p>` : ""}</details>`;
}
/* An ask answered here (a one-tap question) shows what the choice weighs, the same context Inbox shows, right above
   its controls. */
const whyHtml = (it) => `<p>${esc(it.why).replace(/\bT-A\d+\b/g, workRef)}</p>`;
function choiceContext(it) {
  if (decidedInInbox(it) || it.handled || !it.why || decisionFacts(it)) return "";
  const rec = it.rec ? `<p class="chat-needs-rec">${esc(it.rec)}</p>` : "";
  return `<div class="chat-needs-context">${whyHtml(it)}${rec}</div>`;
}
/* Where scope and risk already state the decision (an inline approval), the card leads with them and the buttons;
   the recommendation and the longer reasoning, which repeat the message and those facts, follow as one disclosure
   ("Jules recommends: … ›"), so scope, risk and the action are in the first view at 375 px. */
function factsFollowUp(it) {
  if (decidedInInbox(it) || it.handled || !it.why || !decisionFacts(it)) return "";
  const label = it.rec ? `<span class="chat-needs-rec">${esc(it.rec)}</span>` : `<span>Why ${esc(M[it.from].name)} asks</span>`;
  return `<details class="chat-needs-why chat-needs-more"><summary>${ic("info", "ic14")}${label}${ic("chev-right", "ic14 chat-needs-chev")}</summary>${whyHtml(it)}</details>`;
}
/* The risk fact an inline approval rests on, from the item's own impact and audit data: its risk level and what it
   cannot touch (data, spend, rollback), e.g. "Low risk · Zero production or customer data". */
function riskFact(it) {
  const level = (it.impact || []).find(([icon]) => icon.startsWith("shield"));
  if (!level) return "";
  const audit = (it.audit || []).find((a) => /\b(data|spend|roll|revers)/i.test(a));
  return [level[2], (audit || level[3]).replace(/ · T-A\d+$/, "")].join(" · ");
}
/* Scope and risk, the facts that justify deciding here, directly above the buttons. A consequential ask keeps its
   scope only; its risk is weighed in Inbox. */
function decisionFacts(it) {
  if (it.handled) return "";
  const risk = decidedInInbox(it) ? "" : riskFact(it);
  const rows = [
    it.scope ? `<li>${ic("lock", "ic14")}<span><span class="chat-needs-fact-k">Scope:</span> ${esc(it.scope)}</span></li>` : "",
    risk ? `<li>${ic("shield-check", "ic14")}<span><span class="chat-needs-fact-k">Risk:</span> ${esc(risk)}</span></li>` : "",
  ].filter(Boolean);
  return rows.length ? `<ul class="chat-needs-facts" role="list" aria-label="Scope and risk">${rows.join("")}</ul>` : "";
}
/* `where.thread`: the card is shown in that thread's panel, so a follow-up from it posts in the thread. */
function needsCardHtml(it, msgId, where = {}) {
  const titleId = `needs-${esc(msgId)}`;
  const meta = joinSep([
    taskPhrase(it),
    it.expires && !it.handled ? `<span class="pin">${ic("clock", "ic14")}${esc(it.expires)}</span>` : "",
  ]);
  return `<section class="chat-needs${it.handled ? " is-done" : ""}" aria-labelledby="${titleId}">
    ${needsHead(it)}
    <h3 class="chat-needs-q" id="${titleId}" tabindex="-1">${esc(it.what)}</h3>
    ${meta ? `<p class="chat-needs-meta">${meta}</p>` : ""}
    ${tradeoff(it)}${decisionFacts(it)}${choiceContext(it)}
    ${needsActions(it, where)}${factsFollowUp(it)}
  </section>`;
}
/* Where a low-risk ask's answerable card is: the message carrying it, outside agents' conversations. */
function cardMessage(it) {
  const home = askHomeId(it), msg = home && CH[home].kind !== "a2a" && CH[home].msgs.find((m) => m.needs === it.id);
  return msg ? { home, msg } : null;
}
/* Above or below the chip's own message, in the same conversation. */
function cardSide(home, cardId, fromId) {
  const ids = CH[home].msgs.map((m) => m.id), from = ids.indexOf(fromId);
  return from >= 0 && from < ids.indexOf(cardId) ? "below" : "above";
}
/* A chip says where the ask is settled: Inbox for a consequential ask; for a low-risk ask, its answerable card,
   in another conversation (e.g. Rowan's DM, from the agents' conversation) or in this one (e.g. after a follow-up).
   The card keeps its own link to the Inbox record. */
function chipRoute(it, here, fromId) {
  const inbox = `#/inbox/${it.id}`;
  if (it.handled) return [inbox, `Handled · ${it.outcome}`];
  if (decidedInInbox(it)) return [inbox, "Needs you · Decide in Inbox"];
  const card = cardMessage(it), href = card && `#/chat/${card.home}/${card.msg.id}`;
  if (card && card.home !== here) return [href, `Needs you · Answer in ${placeName(card.home)}`];
  if (card) return [href, `Needs you · ${KIND[it.kind].label} on the card ${cardSide(card.home, card.msg.id, fromId)}`];
  return [inbox, `Needs you · ${KIND[it.kind].label} in Inbox`];
}
/* In a thread whose root carries the ask, the chip moves focus to that card at the top of the panel. */
function threadCardChip(it, msgId) {
  return `<p class="chat-needs-chip"><button type="button" data-act="chat-thread-card" data-id="${esc(msgId)}">${ic("bell", "ic14")}<span>Needs you · ${esc(KIND[it.kind].label)} on the card above</span>${ic("chev-right", "ic14")}</button></p>`;
}
function needsChipHtml(it, here, fromId) {
  const [href, label] = chipRoute(it, here, fromId);
  return `<p class="chat-needs-chip${it.handled ? " is-done" : ""}"><a href="${esc(href)}">${ic(it.handled ? "check-circle" : "bell", "ic14")}<span>${esc(label)}</span>${ic("chev-right", "ic14")}</a></p>`;
}
/* While the ask's message is open in the thread panel, its card lives there; the stream points to it.
   A consequential ask is still decided in Inbox, so its chip says so and goes there. */
function inThreadChip(it, msgId) {
  if (decidedInInbox(it)) return needsChipHtml(it);
  const label = it.handled ? `Handled · ${esc(it.outcome)}` : "Needs you · answer in the thread";
  return `<p class="chat-needs-chip${it.handled ? " is-done" : ""}"><button type="button" data-act="chat-thread-card" data-id="${esc(msgId)}">${ic(it.handled ? "check-circle" : "bell", "ic14")}<span>${label}</span>${ic("chev-right", "ic14")}</button></p>`;
}
/* A card where the ask was raised (m.needs); a chip where it is only mentioned (m.ref) or previewed elsewhere.
   `mode`: "card" (writable stream), "thread-card" (card in the thread panel), "in-thread" (the stream's pointer
   to a card shown in the open thread), or a chip. `here` is the conversation being shown. */
function messageNeeds(m, mode, here, threadRoot) {
  const it = inboxItem(m.needs || m.ref);
  if (!it) return "";
  const onRoot = threadRoot && threadRoot.needs === it.id && !it.handled && !decidedInInbox(it);
  if (!m.needs && onRoot) return threadCardChip(it, threadRoot.id);
  if (!m.needs) return needsChipHtml(it, here, m.id);
  if (mode === "in-thread") return inThreadChip(it, m.id);
  if (mode === "thread-card") return needsCardHtml(it, m.id, { thread: m.id });
  return mode === "card" ? needsCardHtml(it, m.id) : needsChipHtml(it, here);
}

/* ---------------------------------------------------------------- follow-ups and answers posted from Chat */
function saveInboxComment(id, text) {
  try { localStorage.setItem(inboxCommentPrefix(id) + crypto.randomUUID(), JSON.stringify({ text, at: Date.now() })); } catch (e) {}
}
/* Posts the owner's words on the Inbox record and where the owner wrote them: the thread on the ask's
   message (`root`), or the conversation where the request was raised. */
function recordAskReply(id, text, root) {
  const home = CH[askHomeId(inboxItem(id))];
  const rootMsg = root && home?.msgs.find((m) => m.id === root);
  const reply = { who: "you", time: clock(), text, ref: id };
  if (rootMsg) { (rootMsg.thread ||= []).push(reply); followThreadEnd(); }
  else if (home) home.msgs.push({ id: "u" + Date.now(), ...reply });
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
  recordAskReply(id, text, field.dataset.root);
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
/* A message whose ask is answered on its card in Chat (low-risk and open): arriving at it focuses the card. */
function answerableHere(m) {
  const it = m?.needs && inboxItem(m.needs);
  return Boolean(it && !it.handled && !decidedInInbox(it));
}

export { ASK_CLICKS, answerableHere, messageNeeds, onAskKeydown, recordAskReply, resetAsks, saveAskDraft, workRef };
