/* Unsent written answers and denial notes, kept per ask (and per reply mode) while the page re-renders, a thread
   closes or the owner moves between pages. A draft goes when it is sent or the owner cancels or keeps the ask.
   Fields opt in with data-draft="<key>"; Home, Inbox previews and Chat cards share the same keys. */
const drafts = new Map();

const replyDraftKey = (id, mode) => `${id}:${mode || "answer"}`;
const denyDraftKey = (id) => `${id}:deny-note`;
const draftText = (key) => drafts.get(key) || "";

function forgetDrafts(id) {
  for (const key of [...drafts.keys()]) if (key.startsWith(id + ":")) drafts.delete(key);
}

document.addEventListener("input", (e) => {
  const key = e.target.dataset?.draft;
  if (key) drafts.set(key, e.target.value);
});
/* Capture phase: runs before the shared action handlers re-render and drop the field. */
const SETTLES = '[data-act="send-reply"], [data-act="deny-confirm"], [data-act="handle"], [data-act="cancel-reply"], [data-act="deny-keep"]';
document.addEventListener("click", (e) => {
  const t = e.target.closest?.(SETTLES);
  if (!t?.dataset.id) return;
  /* An empty written answer is refused with an error, so nothing is settled and the (empty) draft can stay. */
  if (t.dataset.act === "send-reply" && !document.getElementById("r-" + t.dataset.id)?.value.trim()) return;
  forgetDrafts(t.dataset.id);
}, true);
document.addEventListener("keydown", (e) => {
  if (e.key === "Enter" && e.target.classList?.contains("hb-deny-input")) forgetDrafts(e.target.dataset.denyId);
}, true);

export { denyDraftKey, draftText, replyDraftKey };
