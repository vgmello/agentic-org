/* One durable answer record per ask (doc-40 §2.1): actor, time, and the option chosen or the text written.
   Chat (card and thread), Inbox and Home all render the same receipt from it:
   "Answered by you · 10:14: One tree", with a written answer quoted underneath. */
import { M } from "../data.js";
import { esc, ic } from "../ui.js";

const clock = () => new Date().toTimeString().slice(0, 5);
/* Outcome words that read badly before "by you"; surface suffixes ("in chat") are never part of the record. */
const VERBS = { Chose: "Decided", "Denied with note": "Denied", "Answered in chat": "Answered", "Denied in chat": "Denied" };

/* `outcome` is the handled outcome ("Approved", "Answered: One tree", "Denied: reason"); `text` a written answer. */
function responseRecord(outcome, text = "") {
  const split = /^([^:]+):\s*(.+)$/.exec(outcome);
  const verb = split ? split[1] : outcome;
  const written = text.trim();
  return {
    verb: VERBS[verb] || verb,
    answer: written ? "" : split ? split[2] : "",
    text: written,
    by: "you",
    time: clock(),
    at: Date.now(),
  };
}
const actor = (by) => (by === "you" || !M[by] ? "you" : M[by].name);
/* The receipt line, as words (also used for accessible names and toasts). */
function receiptLine(it) {
  const r = it.response;
  if (!r) return it.outcome || "Handled";
  return `${r.verb} by ${actor(r.by)}${r.time ? " · " + r.time : ""}${r.answer ? ": " + r.answer : ""}`;
}
/* `cls` styles the line for its surface; `after` adds a surface link (Chat's "View in Inbox"). */
function receiptHtml(it, cls, after = "") {
  const text = it.response?.text;
  const quote = text ? `<blockquote class="ask-receipt-text">${esc(text)}</blockquote>` : "";
  return `<div class="ask-receipt"><p class="${cls}" id="handled-${esc(it.id)}" tabindex="-1">${ic("check-circle", "ic16")}<span>${esc(receiptLine(it))}</span>${after}</p>${quote}</div>`;
}

export { receiptHtml, receiptLine, responseRecord };
