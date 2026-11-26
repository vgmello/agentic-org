/* Chat reading position: keep the message the reader is looking at, follow new messages only
   near the end, and open long histories at a message boundary (never on the tail of a sentence).
   Plain DOM helpers; the stream element carries its conversation id in data-ch. */

const NEAR_END = 64;
const BOUNDARY_GAP = 8;
const positions = new Map();

const spacerOf = (s) => Number(s.dataset.spacer) || 0;
const yOf = (s, el) => el.getBoundingClientRect().top - s.getBoundingClientRect().top + s.scrollTop;
/* Day labels are sticky, so their box moves while stuck; the static rule before each label marks the day's
   start for boundary placement. */
const items = (s) => [...s.querySelectorAll(":scope > .msg, :scope > .chat-day-rule")];

/* Height of the sticky day band, which covers the top of whatever message sits at the stream's top edge. */
const bandOf = (s) => s.querySelector(":scope > .chat-day")?.offsetHeight || 0;
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
/* Always opens on a complete message boundary (a message or day divider at the top), never partway
   through a message or card: the boundary that wastes least, where hiding the newest lines below the
   fold costs twice as much as leaving blank space after them. Blank space is capped (a fifth of the
   view, at most 120 px); when a tall last message leaves no boundary near the end, the stream opens
   at that message's start. */
function boundaryY(s) {
  const max = s.scrollHeight - spacerOf(s) - s.clientHeight;
  if (max <= 0) return 0;
  const cap = Math.min(120, s.clientHeight / 5);
  let best = max, bestCost = Infinity;
  const band = bandOf(s);
  for (const el of items(s)) {
    /* A message opens just below the day band; a day's own start shows its label in place. */
    const y = yOf(s, el) - BOUNDARY_GAP - (el.classList.contains("msg") ? band : 0);
    if (y > max + cap) continue;
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
/* A linked message sits a third of the way down, or just below the sticky day band when it is taller than the view,
   so the band never covers its author line. */
const linkedOffset = (s, el) => Math.max(BOUNDARY_GAP + bandOf(s), (s.clientHeight - el.offsetHeight) / 3);
function placeAt(s, pos) {
  const anchor = messageEl(s, pos.mid || pos.target);
  if (anchor && pos.target) return scrollToY(s, Math.max(0, yOf(s, anchor) - linkedOffset(s, anchor)));
  if (anchor) return scrollToY(s, yOf(s, anchor) - pos.offset);
  if (pos.top) return scrollToY(s, 0);
  return pos.end ? scrollToEnd(s) : scrollToY(s, boundaryY(s));
}
/* A day label stuck at the stream's top edge becomes a solid band, so the lines scrolling under it never show
   through; in place it is the plain divider. */
function markStuckDays(s) {
  const top = s.getBoundingClientRect().top;
  for (const d of s.querySelectorAll(":scope > .chat-day")) d.classList.toggle("is-stuck", s.scrollTop > 0 && d.getBoundingClientRect().top - top < 1);
}
function applyPos(s, pos) {
  placeAt(s, pos);
  markStuckDays(s);
}
const savedPos = (id) => positions.get(id);
const savePos = (id, pos) => positions.set(id, pos);
function trackPos(s) {
  s.addEventListener("scroll", () => { positions.set(s.dataset.ch, readPos(s)); markStuckDays(s); }, { passive: true });
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
  syncLatestJump(s);
}

export { applyPos, placeStream, readPos, replaceStream, savePos, savedPos, scrollToEnd, trackPos };

/* ---------------------------------------------------------------- thread panel position
   Each thread keeps its own scroll position across close, reopen and re-render; after the owner sends in a
   thread, the panel opens at its end so the new reply is in view. */
const threadPositions = new Map();
const thread = { toEnd: false };
const followThreadEnd = () => { thread.toEnd = true; };
/* Opened from its reply count: when the root card fills a short screen, the panel opens at the replies; the ask
   stays one scroll above. */
function showReplies(s) {
  const count = s.querySelector(":scope > .chat-thread-count"), first = count?.nextElementSibling;
  if (first && yOf(s, first) + Math.min(first.offsetHeight, 48) > s.clientHeight) s.scrollTop = yOf(s, count) - BOUNDARY_GAP;
}
/* Opened from a search match: that reply sits a third of the way down, marked. */
function showReply(s, rootId, i) {
  const el = s.querySelector(`:scope > [data-mid="${CSS.escape(rootId + "-r" + i)}"]`);
  if (!el) return;
  el.classList.add("is-linked");
  s.scrollTop = yOf(s, el) - Math.max(BOUNDARY_GAP, (s.clientHeight - el.offsetHeight) / 3);
}
/* `land`: { replies: true } from the reply count, { reply: i } from a search match; otherwise the saved position. */
function placeThread(panel, rootId, land = null) {
  const s = panel?.querySelector(":scope > .msgs");
  if (!s) return;
  if (thread.toEnd) { thread.toEnd = false; s.scrollTop = s.scrollHeight; threadPositions.set(rootId, s.scrollTop); }
  else if (land && Number.isInteger(land.reply)) showReply(s, rootId, land.reply);
  else if (threadPositions.has(rootId)) s.scrollTop = threadPositions.get(rootId);
  else if (land?.replies) showReplies(s);
  s.addEventListener("scroll", () => threadPositions.set(rootId, s.scrollTop), { passive: true });
}

export { followThreadEnd, placeThread };

/* ---------------------------------------------------------------- earlier asks above the reading position
   One button jumps to the nearest, oldest last. On phones it is its own row above the stream (never over a
   message); when that row appears or goes, the stream scrolls by its height so the text doesn't move. */
function asksAbove(s) {
  const top = s.getBoundingClientRect().top;
  return [...s.querySelectorAll(":scope > .msg .chat-needs:not(.is-done)")].filter((c) => c.getBoundingClientRect().bottom < top + 8);
}
function syncAskJump(s) {
  const btn = document.querySelector('[data-act="chat-asks-jump"]'), row = btn?.parentElement;
  if (!btn) return;
  const n = asksAbove(s).length, before = row.offsetHeight;
  btn.hidden = !n;
  btn.querySelector("span").textContent = n === 1 ? "1 earlier request needs you" : `${n} earlier requests need you`;
  const delta = row.offsetHeight - before;
  if (delta) s.scrollTop += delta;
}
function jumpToAsk() {
  const s = document.querySelector(".chat .chmain .msgs[data-ch]"), card = s && asksAbove(s).pop();
  if (!card) return;
  s.scrollTop += card.closest(".msg").getBoundingClientRect().top - s.getBoundingClientRect().top - 8 - bandOf(s);
  card.querySelector(".chat-needs-q").focus({ preventScroll: true });
  syncAskJump(s);
}
/* ---------------------------------------------------------------- newer messages below the reading position
   When the opening position (or the reader) leaves the newest message below the fold, "Jump to latest" floats at
   the stream's foot; it goes once the newest message is fully in view. */
const lastMessage = (s) => [...s.querySelectorAll(":scope > .msg")].pop();
function newerBelow(s) {
  const last = lastMessage(s);
  return Boolean(last) && last.getBoundingClientRect().bottom > s.getBoundingClientRect().bottom + 1;
}
function syncLatestJump(s) {
  const btn = document.querySelector('[data-act="chat-latest-jump"]');
  if (btn) btn.hidden = !newerBelow(s);
}
/* Scrolls to the end and moves focus to the newest message (the button that had focus goes away). */
function jumpToLatest() {
  const s = document.querySelector(".chat .chmain .msgs[data-ch]"), last = s && lastMessage(s);
  if (!last) return;
  scrollToEnd(s);
  markStuckDays(s);
  positions.set(s.dataset.ch, { end: true });
  last.tabIndex = -1;
  last.setAttribute("aria-label", `Latest message from ${last.querySelector(".hd b")?.textContent || last.querySelector(".sr-only")?.textContent || "this conversation"}`);
  last.focus({ preventScroll: true });
  syncLatestJump(s);
}
function syncJumps(s) {
  syncAskJump(s);
  syncLatestJump(s);
}
function watchAsks(s) {
  let frame = 0;
  s.addEventListener("scroll", () => { cancelAnimationFrame(frame); frame = requestAnimationFrame(() => syncJumps(s)); }, { passive: true });
  setTimeout(() => s.isConnected && syncJumps(s), 400);
  setTimeout(() => s.isConnected && syncJumps(s), 1600);
  syncJumps(s);
}

export { jumpToAsk, jumpToLatest, syncLatestJump, watchAsks };
