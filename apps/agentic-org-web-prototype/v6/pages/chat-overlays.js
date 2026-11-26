/* Browser Back closes Chat's open overlay (thread, members, conversation drawer, details) instead of leaving
   the page with it open. Opening any overlay adds one history entry on the same URL; closing the last overlay
   from the page removes it again, so Back never has to be pressed twice. The Chat page binds what is open
   and how to close the topmost overlay (which restores focus to its trigger). */
const hist = { entry: "", hash: "", skip: false, isOpen: () => false, closeTop: () => {} };

function bindOverlays(isOpen, closeTop) {
  Object.assign(hist, { isOpen, closeTop });
}
const ours = () => Boolean(hist.entry) && history.state?.chatOverlay === hist.entry;
/* Call after every overlay opens or closes. */
function syncOverlayHistory() {
  const open = hist.isOpen();
  if (open && !ours()) {
    hist.entry = String(Date.now());
    hist.hash = location.hash;
    history.pushState({ ...(history.state || {}), chatOverlay: hist.entry }, "", location.href);
  } else if (!open && ours()) {
    hist.entry = "";
    hist.skip = true;
    history.back();
  }
}
window.addEventListener("popstate", () => {
  if (hist.skip) { hist.skip = false; return; }
  const mine = hist.entry && location.hash === hist.hash;
  hist.entry = "";
  if (mine && hist.isOpen()) hist.closeTop();
});

export { bindOverlays, syncOverlayHistory };
