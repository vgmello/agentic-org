/* The thread side panel's width (also Members and Files). Inline from a 1280 px viewport: about a third of the chat
   area by default, at least 380 px, resizable by dragging its left edge or with the arrow keys on that edge (a focusable
   separator). The chosen width is remembered on this device. Below 1280 px the panel is a modal overlay. */
const KEY = "agentic-org.v6.chat.panelWidth";
const MIN = 380, STEP = 16, MAIN_MIN = 420;
const INLINE = "(min-width: 1280px)";

function storedWidth() {
  try { const n = Number(localStorage.getItem(KEY)); return n >= MIN ? n : null; } catch (e) { return null; }
}
function storeWidth(px) {
  try { localStorage.setItem(KEY, String(px)); } catch (e) { /* the width is only kept for this visit */ }
}
/* The frame's inline style carries a remembered width; CSS clamps it to the room there is. */
const panelWidthStyle = () => { const w = storedWidth(); return w ? ` style="--chat-panel-w: ${w}px"` : ""; };
function addSeparator(panel) {
  const sep = document.createElement("div");
  Object.assign(sep, { className: "chat-panel-resize", tabIndex: 0, title: "Drag, or use the arrow keys, to resize" });
  for (const [k, v] of [["role", "separator"], ["aria-orientation", "vertical"], ["aria-label", "Resize side panel"], ["aria-valuemin", String(MIN)]]) sep.setAttribute(k, v);
  panel.prepend(sep);
  return sep;
}

/* The widest the panel may be: the conversation keeps room to read (and the inline list its place). */
function maxWidth(root) {
  const list = root.querySelector(":scope > .chlist");
  return Math.max(MIN, root.clientWidth - (list?.offsetWidth || 0) - MAIN_MIN);
}
function apply(root, sep, px) {
  const w = Math.round(Math.min(maxWidth(root), Math.max(MIN, px)));
  root.style.setProperty("--chat-panel-w", w + "px");
  sep.setAttribute("aria-valuenow", String(w));
  sep.setAttribute("aria-valuemax", String(maxWidth(root)));
  return w;
}
const currentWidth = (panel) => Math.round(panel.getBoundingClientRect().width);
function onKey(e, root, sep, panel) {
  const now = currentWidth(panel);
  const next = { ArrowLeft: now + STEP, ArrowRight: now - STEP, Home: MIN, End: maxWidth(root) }[e.key];
  if (next === undefined) return;
  e.preventDefault();
  storeWidth(apply(root, sep, next));
}
function onDrag(e, root, sep) {
  e.preventDefault();
  sep.setPointerCapture?.(e.pointerId);
  const right = root.getBoundingClientRect().right;
  let w = null;
  const move = (ev) => { w = apply(root, sep, right - ev.clientX); };
  const up = () => {
    sep.removeEventListener("pointermove", move);
    root.classList.remove("is-resizing");
    if (w) storeWidth(w);
  };
  root.classList.add("is-resizing");
  sep.addEventListener("pointermove", move);
  sep.addEventListener("pointerup", up, { once: true });
  sep.addEventListener("pointercancel", up, { once: true });
}
/* Gives the open inline panel its separator after each render. */
function bindPanelResize(root) {
  const panel = root.querySelector(":scope > aside.thread");
  if (!panel || !window.matchMedia?.(INLINE).matches) return;
  const sep = addSeparator(panel);
  sep.setAttribute("aria-valuenow", String(currentWidth(panel)));
  sep.setAttribute("aria-valuemax", String(maxWidth(root)));
  sep.addEventListener("keydown", (e) => onKey(e, root, sep, panel));
  sep.addEventListener("pointerdown", (e) => onDrag(e, root, sep));
}

export { bindPanelResize, panelWidthStyle };
