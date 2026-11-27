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
/* When the reading position cuts into a message, its author line stays visible: the header sticks under the day
   band (.is-cut) until the message scrolls away, or until the reader scrolls back to its start. */
const viewTop = (s) => s.getBoundingClientRect().top + bandOf(s);
function markCut(s) {
  s.style.setProperty("--chat-band", bandOf(s) + "px");
  for (const el of s.querySelectorAll(":scope > .msg.is-cut")) el.classList.remove("is-cut");
  const top = viewTop(s);
  const cut = [...s.querySelectorAll(":scope > .msg:not(.cont)")].find((m) => {
    const r = m.getBoundingClientRect();
    return r.top < top - 1 && r.bottom > top + 32;
  });
  cut?.classList.add("is-cut");
}
function releaseCut(s) {
  const el = s.querySelector(":scope > .msg.is-cut");
  if (el && el.getBoundingClientRect().top >= viewTop(s) - 1) el.classList.remove("is-cut");
}
/* Opening at the end (or a boundary) can leave the message cut at the top showing only a sliver of its attachments
   under the sticky day and author bands. When what shows of it is that thin, the stream opens just past it (a little
   blank space after the newest message, within the boundary cap), so no thumbnail reads as a broken strip. */
const SLIVER = 56;
function clearSliver(s) {
  const cut = s.querySelector(":scope > .msg.is-cut");
  const atts = cut?.querySelector(".chat-atts");
  if (!atts) return;
  const top = viewTop(s), hd = cut.querySelector(".hd")?.offsetHeight || 0;
  const r = atts.getBoundingClientRect(), shown = r.bottom - top - hd;
  if (r.top >= top + hd || shown <= 0 || shown > SLIVER) return;
  const past = cut.getBoundingClientRect().bottom - top + BOUNDARY_GAP;
  if (past > Math.min(120, s.clientHeight / 5) + SLIVER) return;
  scrollToY(s, s.scrollTop + past);
  markStuckDays(s);
  markCut(s);
}
/* An open ask is never the first view's top edge with its buttons showing and its title cut off. When opening at the
   end (or a boundary) cuts one, the stream opens just past that message when it is not the newest open ask and the
   blank space after the newest message stays under 40% of the view; otherwise it opens at that message's start, so
   the ask's title, its context and its responses show together. */
const openAskCard = (m) => m.querySelector(".chat-needs:not(.is-done)");
function cutAskMessage(s) {
  const top = viewTop(s);
  return [...s.querySelectorAll(":scope > .msg")].find((m) => {
    const card = openAskCard(m), title = card?.querySelector(".chat-needs-q");
    return title && title.getBoundingClientRect().top < top + (m.querySelector(".hd")?.offsetHeight || 0) && card.getBoundingClientRect().bottom > top;
  });
}
function snapToAsk(s) {
  const msg = cutAskMessage(s);
  if (!msg) return;
  const newest = [...s.querySelectorAll(":scope > .msg")].filter(openAskCard).pop();
  const past = msg.getBoundingClientRect().bottom - viewTop(s) + BOUNDARY_GAP;
  const blank = past - (s.scrollHeight - spacerOf(s) - s.clientHeight - s.scrollTop);
  if (msg !== newest && blank <= s.clientHeight * 0.4) scrollToY(s, s.scrollTop + past);
  else scrollToY(s, Math.max(0, yOf(s, msg) - BOUNDARY_GAP - bandOf(s)));
  markStuckDays(s);
  markCut(s);
}
function settleOpening(s) {
  clearSliver(s);
  snapToAsk(s);
}
function applyPos(s, pos) {
  placeAt(s, pos);
  markStuckDays(s);
  markCut(s);
  if (!pos.end && !pos.boundary) return;
  settleOpening(s);
  /* The earlier-asks row above the stream may appear for this opening and take its height: settle once more with
     the stream's final size, so the opening keeps a whole ask in view. */
  if (syncAskJump(s)) settleOpening(s);
}
const savedPos = (id) => positions.get(id);
const savePos = (id, pos) => positions.set(id, pos);
function trackPos(s) {
  s.addEventListener("scroll", () => { positions.set(s.dataset.ch, readPos(s)); markStuckDays(s); releaseCut(s); }, { passive: true });
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
   A compact row of its own above the stream (never over a message or a day label) names the nearest earlier ask:
   "1 earlier request · Approve publishing Kai's sync…". It jumps there, nearest first. The row appearing or going
   scrolls the stream by its height, so the text being read doesn't move. */
function asksAbove(s) {
  const top = s.getBoundingClientRect().top;
  return [...s.querySelectorAll(":scope > .msg .chat-needs:not(.is-done)")].filter((c) => c.getBoundingClientRect().bottom < top + 8);
}
function askJumpLabel(btn, cards) {
  const n = cards.length, title = cards.at(-1)?.querySelector(".chat-needs-q")?.textContent.trim() || "";
  const count = `${n} earlier ${n === 1 ? "request" : "requests"}`;
  btn.setAttribute("aria-label", `${count} ${n === 1 ? "needs" : "need"} you. Nearest: ${title}`);
  btn.querySelector(".chat-jump-count").textContent = count;
  btn.querySelector(".chat-jump-what").textContent = title;
}
function syncAskJump(s) {
  const btn = document.querySelector('[data-act="chat-asks-jump"]'), row = btn?.parentElement;
  if (!btn) return 0;
  const cards = asksAbove(s), before = row.offsetHeight;
  btn.hidden = !cards.length;
  if (cards.length) askJumpLabel(btn, cards);
  const delta = row.offsetHeight - before;
  if (delta && row.getBoundingClientRect().top <= s.getBoundingClientRect().top) s.scrollTop += delta;
  return delta;
}
function jumpToAsk() {
  const s = document.querySelector(".chat .chmain .msgs[data-ch]"), card = s && asksAbove(s).pop();
  if (!card) return;
  s.scrollTop += card.closest(".msg").getBoundingClientRect().top - s.getBoundingClientRect().top - 8 - bandOf(s);
  card.querySelector(".chat-needs-q").focus({ preventScroll: true });
  syncAskJump(s);
}
/* ---------------------------------------------------------------- newer messages below the reading position
   When the opening position (or the reader) leaves messages below the fold, a row between the stream and the
   composer says what is there ("1 newer message"); it goes once the newest message is fully in view. */
const lastMessage = (s) => [...s.querySelectorAll(":scope > .msg")].pop();
function newerBelow(s) {
  const last = lastMessage(s);
  return Boolean(last) && last.getBoundingClientRect().bottom > s.getBoundingClientRect().bottom + 1;
}
/* Messages wholly below the fold; the one being read (cut by the fold) is not "newer". */
function unseenBelow(s) {
  const bottom = s.getBoundingClientRect().bottom - 1;
  return [...s.querySelectorAll(":scope > .msg")].filter((m) => m.getBoundingClientRect().top >= bottom).length;
}
/* "1 newer message" says what is below; "Jump to latest" when only the rest of the newest message is. */
function syncLatestJump(s) {
  const btn = document.querySelector('[data-act="chat-latest-jump"]');
  if (!btn) return;
  btn.hidden = !newerBelow(s);
  if (btn.hidden) return;
  const n = unseenBelow(s), label = n ? `${n} newer ${n === 1 ? "message" : "messages"}` : "Jump to latest";
  btn.setAttribute("aria-label", n ? `${label}, jump to the latest` : label);
  btn.querySelector(".chat-jump-n").textContent = label;
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
  const ro = new ResizeObserver(() => (s.isConnected ? syncJumps(s) : ro.disconnect()));
  ro.observe(s.parentElement);
  s.addEventListener("scroll", () => { cancelAnimationFrame(frame); frame = requestAnimationFrame(() => syncJumps(s)); }, { passive: true });
  setTimeout(() => s.isConnected && syncJumps(s), 400);
  setTimeout(() => s.isConnected && syncJumps(s), 1600);
  syncJumps(s);
}

export { jumpToAsk, jumpToLatest, syncLatestJump, watchAsks };
