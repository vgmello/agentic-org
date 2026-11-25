/* ---------------------------------------------------------------- state */
const S = { theme: "ocean", homeState: "filled", inboxKind: "all", inboxProj: "all", inboxTeam: "all", inboxQuery: "", inboxUrgent: false, inboxOrder: "priority", replying: null, replyMode: null, workLayout: "list", workBoardStages: "detailed", focusAfter: null,
  scopedWorkView: "all", scopedWorkLayout: "list", scopedWorkStatus: "all", scopedWorkProject: "all", scopedWorkTeam: "all", scopedWorkGroup: "status", scopedWorkSubgroup: "none", scopedWorkSort: "default", scopedWorkOrder: "asc",
  actView: "assembly", actProject: "all", actFilter: "all", actPanel: null,
  projectActView: "assembly", projectActFilter: "all", openRuns: {}, people: "all", thread: null, showArchived: false, answered: {}, lastRoute: null, consoleAgent: "ada",
  consoleView: "activity", consoleScope: "all", consoleTools: false, agentFocusTools: false, openTraces: {}, openKnow: {}, openIdeation: {}, v4Open: { "nav-favs": true }, favProjs: ["agency-v1", "look-feel"], navCollapsed: false, spotIdx: 0, askQ: "", askAns: null, briefLens: "today", briefExpanded: false, confirmDeny: null, briefCopied: false };
const ORG_NAME_STORE = "agentic-org.v6.organizationName";
let orgName = "Agentic Org";
try { orgName = localStorage.getItem(ORG_NAME_STORE)?.trim().slice(0, 80) || orgName; } catch (e) {}
function getOrgName() { return orgName; }
function setOrgName(next) { orgName = next; }
function showOrgName() {
  const logo = document.querySelector(".sidebar .logo"), name = logo.querySelector(".name");
  name.textContent = orgName;
  name.title = orgName;
  logo.title = `${orgName} home`;
  logo.setAttribute("aria-label", logo.title);
}
showOrgName();
const THEME_STORE = "agentic-org.v6.theme";
const COZY_CONTRAST_STORE = "agentic-org.v6.cozyContrast";
S.cozyContrast = false;
try { S.cozyContrast = localStorage.getItem(COZY_CONTRAST_STORE) === "1"; } catch (e) {}
const THEME_OPTIONS = [
  ["ocean", "Ocean"], ["classic", "Classic"], ["report", "Report"],
  ["report-dark", "Report Night"], ["graphite", "Graphite"], ["cozy", "Cozy Glass"]
];
const VALID_THEMES = new Set(THEME_OPTIONS.map(([value]) => value));
const normalizeTheme = (t) => (t === "light" ? "report" : t);
const urlTheme = normalizeTheme(new URLSearchParams(location.search).get("theme"));
if (VALID_THEMES.has(urlTheme)) {
  S.theme = urlTheme;
} else {
  try {
    const savedTheme = normalizeTheme(localStorage.getItem(THEME_STORE));
    if (VALID_THEMES.has(savedTheme)) S.theme = savedTheme;
  } catch (e) {}
}
function applyCozyContrast() {
  document.body.classList.toggle("a11y-mode", S.theme === "cozy" && S.cozyContrast);
}
function applyTheme(remember = false) {
  const t = VALID_THEMES.has(normalizeTheme(S.theme)) ? normalizeTheme(S.theme) : "ocean";
  S.theme = t;
  const isLight = t === "report";
  document.documentElement.dataset.theme = t;
  document.documentElement.classList.toggle("dark", !isLight);
  document.documentElement.classList.toggle("light", isLight);
  applyCozyContrast();
  if (!remember) return;
  try { localStorage.setItem(THEME_STORE, t); } catch (e) {}
  const url = new URL(location.href);
  if (url.searchParams.has("theme")) {
    url.searchParams.delete("theme");
    history.replaceState(history.state, "", url);
  }
}
applyTheme();
const INBOX_DIVIDERS_STORE = "agentic-org.v6.inboxDividers";
S.inboxDividers = false;
try { S.inboxDividers = localStorage.getItem(INBOX_DIVIDERS_STORE) === "1"; } catch (e) {}
document.documentElement.toggleAttribute("data-inbox-dividers", S.inboxDividers);
const WORK_DIVIDERS_STORE = "agentic-org.v6.workDividers";
const WORK_SUBTASKS_STORE = "agentic-org.v6.workSubtasks";
const WORK_ARCHIVED_STORE = "agentic-org.v6.workShowArchived";
const ARCHIVE_AFTER_DAYS_STORE = "agentic-org.v6.archiveAfterDays";
const ARCHIVE_DAY_OPTIONS = [
  [0, "Off (never auto-archive)"],
  [3, "3 days after closed"],
  [7, "7 days after closed"],
  [10, "10 days after closed"],
  [14, "14 days after closed"],
  [30, "30 days after closed"],
];
S.workDividers = false;
S.workSubtasks = true;
S.workShowArchived = false;
S.archiveAfterDays = 10;
try { S.workDividers = localStorage.getItem(WORK_DIVIDERS_STORE) === "1"; } catch (e) {}
try { S.workSubtasks = localStorage.getItem(WORK_SUBTASKS_STORE) !== "0"; } catch (e) {}
try { S.workShowArchived = sessionStorage.getItem(WORK_ARCHIVED_STORE) === "1"; } catch (e) {}
try {
  const rawDays = localStorage.getItem(ARCHIVE_AFTER_DAYS_STORE);
  if (rawDays !== null) {
    const savedDays = Number(rawDays);
    if (ARCHIVE_DAY_OPTIONS.some(([d]) => d === savedDays)) S.archiveAfterDays = savedDays;
  }
} catch (e) {}
document.documentElement.toggleAttribute("data-work-dividers", S.workDividers);
const ASSEMBLY_DIVIDERS_STORE = "agentic-org.v6.assemblyDividers";
S.assemblyDividers = false;
try { S.assemblyDividers = localStorage.getItem(ASSEMBLY_DIVIDERS_STORE) === "1"; } catch (e) {}
document.documentElement.toggleAttribute("data-assembly-dividers", S.assemblyDividers);
document.documentElement.toggleAttribute("data-hide-work-subtasks", !S.workSubtasks);
const NAV_PIN_STORE = "agentic-org.v6.navPinned";
S.navPinned = false;
S.navHover = false;
try { if (localStorage.getItem(NAV_PIN_STORE) === "1") S.navPinned = true; } catch (e) {}
function sidebarHasOpenMenu() {
  const m = document.getElementById("menu-acct");
  return !!(m && !m.hidden);
}
function applyNavCollapsed() {
  const sidebar = document.getElementById("sidebar");
  const kbdInside = !!(sidebar && document.activeElement && sidebar.contains(document.activeElement) && document.activeElement.matches(":focus-visible"));
  const collapsed = !S.navPinned && !S.navHover && !sidebarHasOpenMenu() && !kbdInside;
  S.navCollapsed = collapsed;
  document.body.classList.toggle("nav-collapsed", collapsed);
  const b = document.getElementById("nav-collapse-btn");
  if (b) {
    b.setAttribute("aria-pressed", String(!!S.navPinned));
    b.setAttribute("aria-label", S.navPinned ? "Auto-hide sidebar" : "Pin sidebar open");
    b.title = S.navPinned ? "Auto-hide sidebar" : "Pin sidebar open";
  }
}
function toggleNavPinned() {
  S.navPinned = !S.navPinned;
  if (!S.navPinned) S.navHover = false;
  try { localStorage.setItem(NAV_PIN_STORE, S.navPinned ? "1" : "0"); } catch (e) {}
  applyNavCollapsed();
}
const SR = { q: "", open: false, active: 0, flat: [] };
const KEYS_STORE = "agentic-org.singleKeyShortcuts";
function readSingleKey() { try { return localStorage.getItem(KEYS_STORE) !== "0"; } catch (e) { return true; } }
function writeSingleKey(on) { try { localStorage.setItem(KEYS_STORE, on ? "1" : "0"); } catch (e) {} }
S.singleKey = readSingleKey();
const CHAT_SEND_STORE = "agentic-org.v6.chatSendShortcut";
const CHAT_SEND_SHORTCUTS = {
  either: { label: "Ctrl or Command + Enter", keys: "Control+Enter Meta+Enter" },
  control: { label: "Ctrl + Enter", keys: "Control+Enter" },
  command: { label: "Command + Enter", keys: "Meta+Enter" },
};
S.chatSend = "either";
try {
  const shortcut = localStorage.getItem(CHAT_SEND_STORE);
  if (Object.hasOwn(CHAT_SEND_SHORTCUTS, shortcut)) S.chatSend = shortcut;
} catch (e) {}
function applyShortcuts() {
  const on = S.singleKey, phone = !!(window.matchMedia && window.matchMedia("(max-width: 639px)").matches);
  document.body.classList.toggle("nokeys", !on);
  const q = document.getElementById("q"), nb = document.getElementById("new-btn");
  const nav = document.getElementById("nav");
  if (q) { if (on && !phone) q.setAttribute("aria-keyshortcuts", "/"); else q.removeAttribute("aria-keyshortcuts"); }
  if (nb) { if (on) { nb.setAttribute("aria-keyshortcuts", "N"); nb.title = "New · N"; } else { nb.removeAttribute("aria-keyshortcuts"); nb.title = "New"; } }
  if (on) nav.setAttribute("aria-keyshortcuts", "S"); else nav.removeAttribute("aria-keyshortcuts");
}
const inTextField = (el) => !!el && (["input", "textarea", "select"].includes((el.tagName || "").toLowerCase()) || !!el.isContentEditable);
const overlayOpen = () => !!(document.querySelector(".sheet, dialog[open], .menu:not([hidden]), .af-shell") || document.body.classList.contains("drawer"));
const qs = () => new URLSearchParams((location.hash.split("?")[1]) || "");

export {
  ARCHIVE_AFTER_DAYS_STORE,
  ARCHIVE_DAY_OPTIONS,
  ASSEMBLY_DIVIDERS_STORE,
  CHAT_SEND_SHORTCUTS,
  CHAT_SEND_STORE,
  COZY_CONTRAST_STORE,
  INBOX_DIVIDERS_STORE,
  ORG_NAME_STORE,
  S,
  SR,
  THEME_OPTIONS,
  WORK_ARCHIVED_STORE,
  WORK_DIVIDERS_STORE,
  WORK_SUBTASKS_STORE,
  applyCozyContrast,
  applyNavCollapsed,
  applyShortcuts,
  applyTheme,
  getOrgName,
  inTextField,
  overlayOpen,
  qs,
  setOrgName,
  showOrgName,
  sidebarHasOpenMenu,
  toggleNavPinned,
  writeSingleKey,
};
