/* Where an owner ask is settled, shared by every surface outside Inbox (Chat cards and chips, Home's Spotlight
   and cards, Activity triage). Questions and low-risk asks are answered in place; an ask whose data marks it
   `consequential` (binding choice, tradeoff, high or irreversible impact) is decided in Inbox, and the surface
   says why with the same short reason. Inbox itself always decides in place. */
import { esc, ic } from "../ui.js";

const decidedInInbox = (it) => Boolean(it?.consequential) && !it.handled;
const routeReason = (it) => `Decide in Inbox · ${it.consequential}`;
/* The one primary control: "Review in Inbox". A surface may add its own button chrome (`cls`, `attrs`, `inner`). */
function reviewInInbox(it, { cls = "btn primary sm", attrs = "", inner = (label) => label } = {}) {
  return `<a class="${cls}"${attrs} href="#/inbox/${esc(it.id)}">${inner(`${ic("inbox", "ic16")}<span>Review in Inbox</span>`)}</a>`;
}
/* The short reason, stated quietly beside or above the control. */
function routeReasonHtml(it, cls = "ask-route") {
  return `<p class="${cls}">${ic("inbox", "ic14")}<span>${esc(routeReason(it))}</span></p>`;
}

export { decidedInInbox, reviewInInbox, routeReason, routeReasonHtml };
