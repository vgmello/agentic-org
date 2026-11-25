import { inboxTabLabel, narrow, resetInboxSelection } from "../pages/inbox.js";
import { narrowSearch } from "./cmdk.js";
import { render } from "./router.js";
import { S, inTextField, overlayOpen } from "../state.js";

/* ---------------------------------------------------------------- keyboard navigation */
const NAVIGATION_KEY_DELAY = 650;
let navigationKeys = null;
function getNavigationKeys() { return navigationKeys; }

function endNavigationKeys(restoreFocus = false) {
  if (!navigationKeys) return;
  const { origin, openedDrawer, hint, timer } = navigationKeys;
  navigationKeys = null;
  clearTimeout(timer);
  hint.remove();
  if (restoreFocus) {
    if (openedDrawer) document.body.classList.remove("drawer");
    const focus = origin.isConnected && origin !== document.body ? origin : document.querySelector("#main h1");
    focus?.focus({ preventScroll: true });
  }
}

function collectNavigationEntries(scope, inbox, pageMenu) {
  const selector = inbox ? ".queue-tab, #menu-inbox-kinds [data-act=inbox-category]"
    : pageMenu ? "[data-menu-label]" : "a.nav-item";
  return [...scope.querySelectorAll(selector)]
    .filter((node) => inbox || node.getClientRects().length)
    .map((target) => {
      const label = inbox ? (target.dataset.v ? inboxTabLabel(target.dataset.v) : "More")
        : pageMenu ? target.dataset.menuLabel : target.querySelector(".lbl, .t").textContent.trim();
      return { target, label };
    });
}

function createNavigationHint(inbox, pageMenu) {
  const hint = document.createElement("div");
  hint.className = "toast";
  const instruction = document.createElement("p");
  const title = inbox ? "Inbox menu" : pageMenu ? "Page menu" : "Sidebar";
  instruction.textContent = title + " · Type an item's first letter · Esc to cancel";
  hint.append(instruction);
  document.getElementById("toasts").append(hint);
  return { hint, instruction };
}

function startNavigationKeys(key) {
  const pageMenu = key === "m";
  const inbox = pageMenu && document.getElementById("triage-categories");
  const origin = document.activeElement;
  let scope = pageMenu ? inbox || document.querySelector("#main [data-page-menu]") : document.getElementById("nav");
  if (!scope) return false;
  if (inbox && narrow() && scope.closest(".triage-has-detail")) {
    resetInboxSelection();
    render(false);
    scope = document.getElementById("triage-categories");
  }
  const openedDrawer = !pageMenu && narrowSearch();
  if (openedDrawer) document.body.classList.add("drawer");
  const current = scope.querySelector('[aria-current="page"], .queue-tab[aria-pressed="true"], .queue-tab[aria-selected="true"], .queue-tab[data-current-kind]');
  const first = scope.querySelector(pageMenu ? ".queue-tab, .tab" : "a.nav-item");
  (current?.getClientRects().length ? current : first)?.focus();
  const entries = collectNavigationEntries(scope, inbox, pageMenu);
  const { hint, instruction } = createNavigationHint(inbox, pageMenu);
  navigationKeys = { scope, origin, openedDrawer, entries, hint, instruction, letter: "", index: 0, target: null, timer: null };
  return true;
}

function selectNavigationKeyTarget() {
  const target = navigationKeys?.target;
  endNavigationKeys();
  if (target?.isConnected) target.click();
}

function advanceNavigationLetter(key) {
  const selection = navigationKeys;
  const index = key === selection.letter ? selection.index + 1 : 0;
  const entry = selection.entries.filter(({ label }) => label.toLowerCase().startsWith(key))[index];
  if (entry) {
    Object.assign(selection, { letter: key, index, target: entry.target });
    const visible = entry.target.getClientRects().length ? entry.target : selection.scope.querySelector('[aria-controls="menu-inbox-kinds"]');
    visible?.focus();
    selection.instruction.textContent = entry.label + " · Enter to select · Esc to cancel";
  }
  if (selection.target) {
    clearTimeout(selection.timer);
    selection.timer = setTimeout(selectNavigationKeyTarget, NAVIGATION_KEY_DELAY);
  }
}

function isNavShortcutBlocked(e) {
  return !S.singleKey || e.altKey || e.ctrlKey || e.metaKey || inTextField(e.target);
}

function tryBeginNavigationKey(e, key) {
  if (e.repeat || (key !== "m" && key !== "s") || overlayOpen() || !startNavigationKeys(key)) return false;
  e.preventDefault();
  return true;
}

function handleNavigationKeys(e) {
  if (navigationKeys && e.key === "Escape") { e.preventDefault(); endNavigationKeys(true); return true; }
  if (e.key === "Shift") return false;
  if (isNavShortcutBlocked(e)) { endNavigationKeys(); return false; }
  const key = e.key.toLowerCase();
  if (!navigationKeys) return tryBeginNavigationKey(e, key);
  if (key === "enter" && navigationKeys.target) { e.preventDefault(); selectNavigationKeyTarget(); return true; }
  if (!/^[a-z]$/.test(key)) { endNavigationKeys(); return false; }
  e.preventDefault();
  if (!e.repeat) advanceNavigationLetter(key);
  return true;
}

export {
  endNavigationKeys,
  getNavigationKeys,
  handleNavigationKeys,
};
