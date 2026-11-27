/* Tooltips for the board's small icons and compact controls ([data-icon-tip]: card icons, the kind icon, the team dot, the Needs you bell, the
   priority pill, a Done lane's ⋯). Hover (after a short pause) or keyboard focus shows the full text above the icon; a
   tap focuses the icon, so touch shows it too. The same text is the icon's accessible name, so the tooltip is hidden
   from assistive technology. It lives in the top layer, so the board's scroller never clips it. */
const SHOW_DELAY = 150;
const SELECTOR = ".board [data-icon-tip]";
let tip = null, owner = null, timer = 0, bound = false;

function tipElement() {
  if (tip?.isConnected) return tip;
  tip = document.createElement("div");
  tip.className = "board-tip board-icon-tip";
  tip.setAttribute("popover", "manual");
  tip.setAttribute("aria-hidden", "true");
  document.body.append(tip);
  return tip;
}

function place(el, target) {
  const r = target.getBoundingClientRect(), gap = 6, edge = 8;
  const { width, height } = el.getBoundingClientRect();
  const above = r.top - height - gap;
  const top = above >= edge ? above : Math.min(r.bottom + gap, innerHeight - height - edge);
  const left = Math.max(edge, Math.min(r.left + r.width / 2 - width / 2, innerWidth - width - edge));
  el.style.top = `${Math.round(top)}px`;
  el.style.left = `${Math.round(left)}px`;
}


function show(target) {
  clearTimeout(timer);
  if (!target.isConnected) return;
  const el = tipElement();
  owner = target;
  el.textContent = target.dataset.iconTip;
  if (!el.matches(":popover-open")) el.showPopover();
  place(el, target);
}

function hide() {
  clearTimeout(timer);
  owner = null;
  if (tip?.matches(":popover-open")) tip.hidePopover();
}

const iconOf = (node) => (node instanceof Element ? node.closest(SELECTOR) : null);
const isIconTarget = (node) => Boolean(iconOf(node));

function onPointerOver(e) {
  const target = iconOf(e.target);
  if (!target || target === owner || e.pointerType === "touch") return;
  clearTimeout(timer);
  timer = setTimeout(() => show(target), SHOW_DELAY);
}

function onPointerOut(e) {
  const from = iconOf(e.target);
  if (!from || from.contains(e.relatedTarget)) return;
  if (from === owner) hide(); else clearTimeout(timer);
}

function onFocusIn(e) {
  const target = iconOf(e.target);
  if (target && target === e.target) show(target);
  else if (owner) hide();
}

function bindBoardIconTips() {
  if (bound) return;
  bound = true;
  document.addEventListener("pointerover", onPointerOver);
  document.addEventListener("pointerout", onPointerOut);
  document.addEventListener("focusin", onFocusIn);
  document.addEventListener("focusout", (e) => { if (owner && e.target === owner) hide(); });
  document.addEventListener("keydown", (e) => { if (e.key === "Escape" && owner) hide(); });
  document.addEventListener("scroll", () => { if (owner) hide(); }, { capture: true, passive: true });
  addEventListener("hashchange", hide);
}

export { bindBoardIconTips, isIconTarget };
