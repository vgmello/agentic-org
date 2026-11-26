/* The conversation list is inline when the chat content area (right of the app nav) is 768 px or wider, and a
   modal overlay opened from the header's Conversations button below that (doc-40 §1, CHAT-29). It follows the
   content width, not the viewport: the app nav's own width changes at its breakpoints. */
const LIST_INLINE_MIN = 768;
const contentWidth = () => document.getElementById("main")?.clientWidth || window.innerWidth;
const listIsOverlay = () => contentWidth() < LIST_INLINE_MIN;

let observer = null;
/* Calls `cross(root, overlay)` whenever the content width crosses the threshold while `root` is shown. */
function watchListMode(root, cross) {
  observer?.disconnect();
  const main = document.getElementById("main");
  if (!main || !window.ResizeObserver) return;
  const ro = new ResizeObserver(() => {
    if (!root.isConnected) { ro.disconnect(); return; }
    const overlay = listIsOverlay();
    if (overlay !== root.classList.contains("list-overlay")) cross(root, overlay);
  });
  ro.observe(main);
  observer = ro;
}

export { listIsOverlay, watchListMode };
