/* Board card tooltip: hovering a card (after a short pause) or focusing its title shows a small dark label above the
   card with the title, then the status and the ETA or closing date, but only when the card's title is clamped. The card says the same to assistive technology, so the
   tooltip is hidden from it. It lives in the top layer, so the board's scroller never clips it;
   it stays while the pointer moves onto it, and Escape, scrolling or a click dismisses it. */
import { bindBoardIconTips, isIconTarget } from "./board-icon-tip.js";

const SHOW_DELAY = 400;
const HIDE_DELAY = 120;
let tip = null, card = null, showTimer = 0, hideTimer = 0, bound = false;

function tipElement() {
  if (tip?.isConnected) return tip;
  tip = document.createElement("div");
  tip.className = "board-tip";
  tip.setAttribute("popover", "manual");
  tip.setAttribute("aria-hidden", "true");
  tip.innerHTML = '<strong class="board-tip-title"></strong><span class="board-tip-meta"></span>';
  tip.addEventListener("pointerleave", (e) => { if (!card?.contains(e.relatedTarget)) scheduleHide(); });
  tip.addEventListener("pointerenter", () => clearTimeout(hideTimer));
  document.body.append(tip);
  return tip;
}

function place(el, target) {
  const r = target.getBoundingClientRect(), gap = 8, edge = 8;
  const { width, height } = el.getBoundingClientRect();
  const above = r.top - height - gap;
  const top = above >= edge ? above : Math.min(r.bottom + gap, innerHeight - height - edge);
  const left = Math.max(edge, Math.min(r.left + 12, innerWidth - width - edge));
  el.style.top = `${Math.round(top)}px`;
  el.style.left = `${Math.round(left)}px`;
}

/* The card tooltip earns its place only when it reveals the rest of a clamped title */
function revealsHidden(target) {
  const title = target.querySelector(":scope > .work-item-link");
  return title ? title.scrollHeight > title.clientHeight + 1 : false;
}

function show(target) {
  clearTimeout(showTimer);
  clearTimeout(hideTimer);
  if (!target.isConnected || !target.dataset.boardTip || !revealsHidden(target)) return;
  const el = tipElement();
  card = target;
  el.querySelector(".board-tip-title").textContent = target.querySelector(":scope > .work-item-link")?.textContent ?? "";
  el.querySelector(".board-tip-meta").textContent = target.dataset.boardTip;
  if (!el.matches(":popover-open")) el.showPopover();
  place(el, target);
}

function hide() {
  clearTimeout(showTimer);
  clearTimeout(hideTimer);
  card = null;
  if (tip?.matches(":popover-open")) tip.hidePopover();
}

function scheduleHide() {
  clearTimeout(showTimer);
  clearTimeout(hideTimer);
  hideTimer = setTimeout(hide, HIDE_DELAY);
}

const boardCard = (node) => (node instanceof Element ? node.closest(".board .bcard") : null);

function onPointerOver(e) {
  if (e.pointerType === "touch") return;
  const target = boardCard(e.target);
  if (!target) return;
  /* An icon has its own tooltip; the card's steps aside */
  if (isIconTarget(e.target)) { if (card) hide(); return; }
  clearTimeout(hideTimer);
  if (target === card) return;
  clearTimeout(showTimer);
  /* Moving from one card to the next swaps the label at once; a first hover waits for a short pause */
  if (card && tip?.matches(":popover-open")) show(target);
  else showTimer = setTimeout(() => show(target), SHOW_DELAY);
}

function onPointerOut(e) {
  const from = boardCard(e.target);
  if (!from || from.contains(e.relatedTarget) || tip?.contains(e.relatedTarget)) return;
  if (from === card) scheduleHide();
  else clearTimeout(showTimer);
}

function onFocusIn(e) {
  const link = e.target;
  if (link instanceof Element && link.matches(".board .bcard > .work-item-link") && link.matches(":focus-visible")) show(link.closest(".bcard"));
  else if (card) hide();
}

export function bindBoardTips() {
  if (bound) return;
  bound = true;
  bindBoardIconTips();
  document.addEventListener("pointerover", onPointerOver);
  document.addEventListener("pointerout", onPointerOut);
  document.addEventListener("focusin", onFocusIn);
  document.addEventListener("focusout", (e) => { if (card && !card.contains(e.relatedTarget)) scheduleHide(); });
  document.addEventListener("keydown", (e) => { if (e.key === "Escape" && card) hide(); });
  document.addEventListener("pointerdown", () => { if (card) hide(); });
  document.addEventListener("scroll", () => { if (card) hide(); }, { capture: true, passive: true });
  addEventListener("hashchange", hide);
}
