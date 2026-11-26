/* Read state. A conversation's unread messages are its newest `ch.unread`. The read marker advances only as
   those messages come into view (half of one, or half the stream's height of a tall one), so opening an older
   source message no longer clears the conversation's whole unread count. */
import { CH } from "../data.js";

const visit = { ch: null, newFrom: null };

/* The first message that was unread when the conversation was opened; the stream marks it "New" for the visit. */
function startVisit(id) {
  if (visit.ch === id) return;
  const ch = CH[id], n = ch.unread || 0;
  Object.assign(visit, { ch: id, newFrom: n ? ch.msgs[ch.msgs.length - n]?.id ?? null : null });
}
const endVisit = () => Object.assign(visit, { ch: null, newFrom: null });
const newFromId = (id) => (visit.ch === id ? visit.newFrom : null);

function markReadThrough(ch, index) {
  const left = Math.max(0, ch.msgs.length - 1 - index);
  if (left < (ch.unread || 0)) ch.unread = left;
}
const seen = (e, s) => e.isIntersecting && (e.intersectionRatio >= 0.5 || e.intersectionRect.height >= s.clientHeight / 2);
/* Watches the unread messages rendered in stream `s`; call again after the stream is re-rendered. */
function watchUnread(s) {
  const ch = CH[s.dataset.ch];
  if (!ch?.unread) return;
  const index = new Map(ch.msgs.map((m, i) => [m.id, i])), from = ch.msgs.length - ch.unread;
  const io = new IntersectionObserver((entries) => {
    if (!s.isConnected) { io.disconnect(); return; }
    for (const e of entries) if (seen(e, s)) markReadThrough(ch, index.get(e.target.dataset.mid));
    if (!ch.unread) io.disconnect();
  }, { root: s, threshold: [0, 0.5, 1] });
  for (const el of s.querySelectorAll(":scope > .msg[data-mid]")) if (index.get(el.dataset.mid) >= from) io.observe(el);
}
const markAllRead = (id) => { if (CH[id]) CH[id].unread = 0; };

export { endVisit, markAllRead, newFromId, startVisit, watchUnread };
