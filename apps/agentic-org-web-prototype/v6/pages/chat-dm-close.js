/* Close a DM, as Slack's "Close conversation": a × on the list row (shown on hover and keyboard focus) and an item in
   the open DM's header menu. Closing only hides the row; the history stays, and any open request stays in Inbox.
   A toast offers Undo. Focus placement after the re-render is the caller's (`queueFocus`), so this module needs
   nothing from the list. */
import { CH, M } from "../data.js";
import { render, route } from "../shell/router.js";
import { esc, ic, moreMenu, toast } from "../ui.js";
import { DEFAULT_CHAT, closeDmState, reopenDm } from "./chat-model.js";

/* Where Chat goes when the DM being closed is the one open. */
const AFTER_CLOSE = "general";
const nameOf = (key) => M[CH[key].with].name;
const rowLink = (key) => `#chat-nav a[href="#/chat/${key}"]`;

function closeButton(key) {
  const label = `Close conversation with ${nameOf(key)}`;
  return `<button class="btn ghost icon chat-dm-close" type="button" data-act="chat-dm-close" data-id="${esc(key)}" aria-label="${esc(label)}" title="Close conversation">${ic("x", "ic16")}</button>`;
}
function dmHeaderMenu(key) {
  const item = `<button role="menuitem" type="button" data-act="chat-dm-close" data-id="${esc(key)}">${ic("x", "ic20")}Close conversation</button>`;
  return `<span class="chat-head-menu">${moreMenu("chat-conv", `More actions for the conversation with ${nameOf(key)}`, item, "ghost")}</span>`;
}

const isOpen = (key) => route()[0] === "chat" && route()[1] === key;
function undoClose(key, wasOpen, queueFocus) {
  reopenDm(key);
  if (route()[0] !== "chat") return;
  queueFocus(wasOpen ? "ch-title" : rowLink(key));
  if (wasOpen) location.hash = "#/chat/" + key;
  else render(false);
}
/* `focus`: what takes focus after the re-render (the neighbouring row, or the conversation's heading). */
function closeConversation(key, focus, queueFocus) {
  if (CH[key]?.kind !== "dm") return;
  const wasOpen = isOpen(key);
  closeDmState(key);
  queueFocus(focus);
  if (wasOpen) location.hash = "#/chat/" + (CH[AFTER_CLOSE] ? AFTER_CLOSE : DEFAULT_CHAT);
  else render(false);
  toast(`Closed DM with ${nameOf(key)}`, "check-circle", () => undoClose(key, wasOpen, queueFocus));
}
/* Where focus goes: from a row, the row that takes its place (the next, else the one before, else Find); from the
   header menu, the heading of the conversation Chat moves to. */
function neighbourFocus(button) {
  if (!button.closest("#chat-nav")) return "ch-title";
  const li = button.closest("li"), next = (li?.nextElementSibling || li?.previousElementSibling)?.querySelector("a");
  return next ? `#chat-nav a[href="${next.getAttribute("href")}"]` : "chat-find";
}

export { closeButton, closeConversation, dmHeaderMenu, neighbourFocus };
