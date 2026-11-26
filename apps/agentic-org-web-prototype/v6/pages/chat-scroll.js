/* Chat reading position: keep the message the reader is looking at, follow new messages only
   near the end, and open long histories at a message boundary (never on the tail of a sentence).
   Plain DOM helpers; the stream element carries its conversation id in data-ch. */

const NEAR_END = 64;
const BOUNDARY_GAP = 8;
const positions = new Map();

const spacerOf = (s) => Number(s.dataset.spacer) || 0;
const yOf = (s, el) => el.getBoundingClientRect().top - s.getBoundingClientRect().top + s.scrollTop;
const items = (s) => [...s.querySelectorAll(":scope > .msg, :scope > .chat-day")];

function setSpacer(s, px) {
  s.dataset.spacer = String(px);
  s.style.setProperty("--chat-spacer", px + "px");
}
/* Scrolls to y; when y is past the natural end, a bottom spacer makes it reachable. */
function scrollToY(s, y) {
  const max = s.scrollHeight - spacerOf(s) - s.clientHeight;
  setSpacer(s, Math.max(0, Math.round(y - max)));
  s.scrollTop = y;
}
function scrollToEnd(s) {
  setSpacer(s, 0);
  s.scrollTop = s.scrollHeight;
}
/* Opens on a message boundary: the boundary that wastes least, where hiding the newest lines
   below the fold costs twice as much as leaving blank space after them. Blank space is capped
   (a fifth of the view, at most 120 px) and so is hidden text (half that); with no boundary in
   that window the stream opens at its end, like any chat. */
function boundaryY(s) {
  const max = s.scrollHeight - spacerOf(s) - s.clientHeight;
  if (max <= 0) return 0;
  const cap = Math.min(120, s.clientHeight / 5);
  let best = max, bestCost = Infinity;
  for (const el of items(s)) {
    const y = yOf(s, el) - BOUNDARY_GAP;
    if (y > max + cap || y < max - cap / 2) continue;
    const cost = y >= max ? y - max : (max - y) * 2;
    if (cost < bestCost) { best = y; bestCost = cost; }
  }
  return Math.max(0, best);
}
function readPos(s) {
  const end = s.scrollHeight - spacerOf(s) - s.clientHeight;
  /* Near the end follows new messages; inside the boundary spacer it re-opens on a boundary. */
  if (end - s.scrollTop < NEAR_END) return spacerOf(s) ? { boundary: true } : { end: true };
  const top = s.scrollTop;
  const el = [...s.querySelectorAll(":scope > [data-mid]")].find((m) => yOf(s, m) + m.offsetHeight > top + 1);
  return el ? { mid: el.dataset.mid, offset: yOf(s, el) - top } : { end: true };
}
function messageEl(s, mid) {
  return mid ? s.querySelector(`:scope > [data-mid="${CSS.escape(mid)}"]`) : null;
}
function applyPos(s, pos) {
  const anchor = messageEl(s, pos.mid || pos.target);
  if (anchor && pos.target) return scrollToY(s, Math.max(0, yOf(s, anchor) - (s.clientHeight - anchor.offsetHeight) / 3));
  if (anchor) return scrollToY(s, yOf(s, anchor) - pos.offset);
  if (pos.top) return scrollToY(s, 0);
  return pos.end ? scrollToEnd(s) : scrollToY(s, boundaryY(s));
}
const savedPos = (id) => positions.get(id);
const savePos = (id, pos) => positions.set(id, pos);
function trackPos(s) {
  s.addEventListener("scroll", () => positions.set(s.dataset.ch, readPos(s)), { passive: true });
}
/* Places the stream and keeps that placement while fonts, styles and avatars settle,
   until the reader scrolls or the page has been still for a moment. */
function placeStream(s, pos) {
  applyPos(s, pos);
  const ro = new ResizeObserver(() => { if (s.isConnected) applyPos(s, pos); });
  const stop = () => {
    ro.disconnect();
    for (const type of ["wheel", "touchstart", "keydown", "pointerdown"]) s.removeEventListener(type, stop);
  };
  for (const child of s.children) ro.observe(child);
  for (const type of ["wheel", "touchstart", "keydown", "pointerdown"]) s.addEventListener(type, stop, { passive: true });
  setTimeout(stop, 1500);
}
/* Re-renders the stream content in place, keeping (or following) the reading position. */
function replaceStream(s, html, follow = false) {
  const pos = follow ? { end: true } : readPos(s);
  s.innerHTML = html;
  applyPos(s, pos);
  positions.set(s.dataset.ch, readPos(s));
}

export { applyPos, placeStream, readPos, replaceStream, savePos, savedPos, scrollToEnd, trackPos };
