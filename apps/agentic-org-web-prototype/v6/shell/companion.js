import { IN, M, P, W } from "../data.js";
import { answerOrgQuestion } from "../pages/home.js";
import { CMDK, closeCmdk, isMacPlatform } from "./cmdk.js";
import { S, TESS_PILL_STORE, qs } from "../state.js";
import { esc, ic } from "../ui.js";

/* ---------------------------------------------------------------- Floating Tess Companion (Bottom-Right, Tess-Only) */

const COMP = {
  mode: "pill", // "hidden" | "pill" | "open" | "expanded"
  dismissedCtxKey: null,
  draft: "",
  returnTo: null,
  currentRoute: ["home"],
};

const TESS_STARTERS = {
  empty: ["How do I launch my first project?", "What's my starting budget & AI capacity?"],
  work: ["What's slowing v1.0?", "What needs me first?", "Budget, capacity & idle agents"],
  default: ["What needs me first?", "What's slowing v1.0?", "What happened in the last 4 hours?"],
};

let getActiveTessJobs = () => [];
let closeNotifHook = () => {};

function registerCompanionHooks({ activeJobs, closeNotif }) {
  getActiveTessJobs = activeJobs;
  closeNotifHook = closeNotif;
}

function restingMode() {
  return S.showTessPill ? "pill" : "hidden";
}

function isOfflineMode() {
  return S.homeState === "error";
}

function shortcutLabel(kind) {
  if (kind === "toggle") return isMacPlatform ? "⌘/" : "Ctrl /";
  if (kind === "expand") return isMacPlatform ? "⌘⇧E" : "Ctrl Shift E";
  return "Esc";
}

function describeRouteContext(r = COMP.currentRoute) {
  const [page, r1] = r;
  const item = qs().get("item");
  if (item) {
    const task = W.find((w) => w.id === item);
    return { key: `item:${item}`, label: task ? `${task.id} · ${task.title}` : `Task · ${item}` };
  }
  if (page === "inbox" && r1) {
    const it = IN.find((i) => i.id === r1);
    return { key: `inbox:${r1}`, label: it ? `Inbox · ${it.what}` : "Inbox" };
  }
  if (page === "projects" && r1 && P[r1]) return { key: `proj:${r1}`, label: `Project · ${P[r1].name}` };
  if (page === "members" && r1 && M[r1]) return { key: `member:${r1}`, label: `Member · ${M[r1].name}` };
  const map = { home: "Home", inbox: "Inbox", work: "Work", chat: "Chat", activity: "Activity", projects: "Projects", teams: "Teams", settings: "Settings" };
  return { key: `page:${page || "home"}`, label: map[page] || "Workspace" };
}

/* ---------------------------------------------------------------- rendering */

function renderContextRow() {
  const ctx = describeRouteContext();
  if (COMP.dismissedCtxKey === ctx.key) return "";
  return `<div class="cmp-ctx-row">
    <span class="cmp-ctx-pill">
      <span class="cmp-ctx-lbl">Context: <strong>${esc(ctx.label)}</strong></span>
      <button type="button" data-act="cmp-dismiss-ctx" data-v="${esc(ctx.key)}" aria-label="Detach page context">${ic("x", "ic14")}</button>
    </span>
  </div>`;
}

function renderActiveBanners() {
  const busy = getActiveTessJobs();
  const busyHtml = busy.length ? `<div class="tess-busy-strip" role="status">
    <span class="notif-job-spin" aria-hidden="true"></span>
    <span>${esc(busy.length > 1 ? `Refining ${busy.length} tasks in background…` : `${busy[0].title} — ${busy[0].step || "Refining…"}`)}</span>
  </div>` : "";
  const offHtml = isOfflineMode()
    ? `<div class="tess-offline-note">${ic("offline", "ic14")}<span>Offline quick summary · Local data only</span></div>` : "";
  return busyHtml + offHtml;
}

function renderTessBody() {
  const turns = [...CMDK.aiHistory].reverse();
  const asked = new Set(CMDK.aiHistory.map((x) => x.q.trim().toLowerCase()));
  const key = S.homeState === "empty" ? "empty" : COMP.currentRoute[0] === "work" ? "work" : "default";
  const chips = (TESS_STARTERS[key] || TESS_STARTERS.default).filter((q) => !asked.has(q.toLowerCase())).slice(0, turns.length ? 2 : 3);
  const threadHtml = turns.length ? `<div class="tess-thread">${turns.map((turn) => `
    <div class="tess-turn">
      <div class="tess-q">${esc(turn.q)}</div>
      <div class="tess-a">
        <div class="tess-a-hd">${ic("sparkle", "ic14")}<span>Tess</span></div>
        <div class="tess-a-body">${turn.html}</div>
      </div>
    </div>`).join("")}</div>` : "";
  const chipsHtml = chips.length ? `<div class="tess-chips" aria-label="Suggested prompts">${chips.map((q) => `
    <button type="button" class="tess-chip" data-act="cmp-ask-chip" data-v="${esc(q)}">
      ${ic("sparkle", "ic14")}<span>${esc(q)}</span>
    </button>`).join("")}</div>` : "";
  return threadHtml + chipsHtml;
}

function renderTessHeader(expanded) {
  const busy = getActiveTessJobs();
  const offline = isOfflineMode();
  const statusLbl = busy.length ? `Refining (${busy.length})` : offline ? "Offline · Local only" : "Chief of Staff";
  const clearBtn = CMDK.aiHistory.length
    ? `<button type="button" class="notif-text-btn" data-act="cmp-clear" title="Clear Tess thread">Clear</button>` : "";
  const expKbd = shortcutLabel("expand");
  const expTip = `${expanded ? "Restore size" : "Maximize"} · ${expKbd}`;
  const closeTip = `${S.showTessPill ? "Minimize to bottom" : "Close"} · Esc`;
  return `<div class="cmp-hd">
    <div class="cmp-tess-who">
      <span class="cmp-tess-mark" aria-hidden="true">${ic(offline ? "offline" : "sparkle", "ic14")}</span>
      <div class="cmp-tess-txt">
        <strong>Tess</strong>
        <span class="cmp-tess-st${offline ? " is-offline" : ""}">${esc(statusLbl)}</span>
      </div>
    </div>
    <div class="cmp-hd-acts">
      ${clearBtn}
      <button type="button" class="btn ghost icon sm cmp-expand-btn" data-act="cmp-expand" data-tip="${esc(expTip)}" aria-label="${esc(expTip)}" aria-keyshortcuts="Meta+Shift+E Control+Shift+E">${ic(expanded ? "minimize" : "maximize", "ic14")}</button>
      <button type="button" class="btn ghost icon sm" data-act="cmp-close" data-tip="${esc(closeTip)}" aria-label="${esc(closeTip)}">${ic("x", "ic14")}</button>
    </div>
  </div>`;
}

function renderRestingPill(panel) {
  const busy = getActiveTessJobs();
  const offline = isOfflineMode();
  const kbd = shortcutLabel("toggle");
  const tip = busy.length ? `Tess refining (${busy.length}) · ${kbd}` : `Ask Tess · ${kbd}`;
  const sub = busy.length ? `Refining (${busy.length})` : offline ? "Offline" : "";
  const dot = busy.length ? `<span class="tess-live-dot" aria-hidden="true"></span>` : "";
  panel.innerHTML = `<button type="button" class="cmp-dock-btn" id="tess-dock-pill" data-act="cmp-restore" data-tip="${esc(tip)}" aria-label="${esc(tip)}" aria-keyshortcuts="Meta+/ Control+/">
    ${ic(offline ? "offline" : "sparkle", "ic14")}
    <strong>Tess</strong>
    ${sub ? `<span class="cmp-pill-st">${esc(sub)}</span>` : `<kbd class="cmp-pill-kbd" aria-hidden="true">${esc(kbd)}</kbd>`}
    ${dot}
  </button>`;
}

function syncCompanionClasses(panel, scrim, compact) {
  const visible = COMP.mode !== "hidden";
  const pill = COMP.mode === "pill";
  const open = COMP.mode === "open";
  const expanded = COMP.mode === "expanded";
  const mobile = window.innerWidth <= 639;
  panel.hidden = !visible;
  panel.classList.toggle("is-compact", open && compact);
  panel.classList.toggle("is-expanded", expanded);
  panel.classList.toggle("is-minimized", pill);
  panel.setAttribute("role", expanded || (open && mobile) ? "dialog" : "complementary");
  if (expanded || (open && mobile)) panel.setAttribute("aria-modal", "true");
  else panel.removeAttribute("aria-modal");
  document.body.classList.toggle("companion-open", open);
  document.body.classList.toggle("companion-minimized", pill);
  if (scrim) scrim.hidden = !(expanded || (open && mobile));
}

function renderCompanion() {
  const panel = document.getElementById("tess-panel");
  if (!panel) return;
  if (COMP.mode === "pill" && !S.showTessPill) COMP.mode = "hidden";
  if (COMP.mode === "hidden" && S.showTessPill) COMP.mode = "pill";
  const compact = CMDK.aiHistory.length === 0;
  syncCompanionClasses(panel, document.getElementById("tess-panel-scrim"), compact);
  if (COMP.mode === "hidden") return;
  if (COMP.mode === "pill") { renderRestingPill(panel); return; }

  panel.innerHTML = `
    ${renderTessHeader(COMP.mode === "expanded")}
    ${renderContextRow()}
    ${renderActiveBanners()}
    <div class="tess-body" id="tess-panel-body">${renderTessBody()}</div>
    <form class="tess-compose" data-form="tess-panel-ask">
      <input id="tess-panel-input" type="text" value="${esc(COMP.draft)}" placeholder="Ask Tess anything…" autocomplete="off" aria-label="Ask Tess anything">
      <button type="submit" class="btn primary sm icon tess-send-btn" aria-label="Send to Tess">${ic("send", "ic14")}</button>
    </form>`;

  const body = document.getElementById("tess-panel-body");
  if (body) body.scrollTop = body.scrollHeight;
}

/* ---------------------------------------------------------------- actions & shortcuts */

function syncTessVisibilityMenu() {
  const menuItem = document.getElementById("menu-tess-toggle");
  if (menuItem) menuItem.setAttribute("aria-checked", String(S.showTessPill));
}

function toggleTessPillVisible() {
  S.showTessPill = !S.showTessPill;
  try { localStorage.setItem(TESS_PILL_STORE, S.showTessPill ? "1" : "0"); } catch (e) {}
  if (!S.showTessPill && COMP.mode === "pill") COMP.mode = "hidden";
  if (S.showTessPill && COMP.mode === "hidden") COMP.mode = "pill";
  syncTessVisibilityMenu();
  renderCompanion();
}

function sendCompanionMessage(rawText) {
  const q = String(rawText || "").trim();
  if (!q) return;
  COMP.draft = "";
  const ans = answerOrgQuestion(q);
  if (ans) {
    ans._state = S.homeState;
    CMDK.aiAns = ans;
    CMDK.aiHistory = [ans, ...CMDK.aiHistory.filter((x) => x.q.toLowerCase() !== ans.q.toLowerCase())];
  }
  renderCompanion();
  document.getElementById("tess-panel-input")?.focus({ preventScroll: true });
}

function openCompanion(initialQuestion = "", trigger = null) {
  if (CMDK.open) closeCmdk(false);
  if (window.innerWidth < 900) closeNotifHook(false);
  if (COMP.mode === "pill" || COMP.mode === "hidden") COMP.returnTo = trigger || document.activeElement;
  COMP.mode = COMP.mode === "expanded" ? "expanded" : "open";
  if (initialQuestion && String(initialQuestion).trim()) sendCompanionMessage(initialQuestion);
  else renderCompanion();
  document.getElementById("tess-panel-input")?.focus({ preventScroll: true });
}

function closeCompanion(restoreFocus = false) {
  const target = restingMode();
  if (COMP.mode === target) return;
  COMP.mode = target;
  renderCompanion();
  if (!restoreFocus) return;
  const fallback = S.showTessPill ? document.getElementById("tess-dock-pill") : COMP.returnTo;
  if (fallback && document.body.contains(fallback)) fallback.focus();
  COMP.returnTo = null;
}

function toggleCompanion(trigger) {
  if (COMP.mode === "pill" || COMP.mode === "hidden") { openCompanion("", trigger); return; }
  const panel = document.getElementById("tess-panel");
  if (panel?.contains(document.activeElement) || COMP.mode === "expanded") closeCompanion(true);
  else document.getElementById("tess-panel-input")?.focus({ preventScroll: true });
}

function toggleExpandCompanion() {
  if (COMP.mode === "pill" || COMP.mode === "hidden") {
    COMP.mode = "expanded";
    renderCompanion();
    document.getElementById("tess-panel-input")?.focus({ preventScroll: true });
    return;
  }
  COMP.mode = COMP.mode === "expanded" ? "open" : "expanded";
  renderCompanion();
  document.getElementById("tess-panel-input")?.focus({ preventScroll: true });
}

function handleCompanionInput(target) {
  if (target.id !== "tess-panel-input") return false;
  COMP.draft = target.value;
  return true;
}

function handleCompanionEscape() {
  if (COMP.mode === "expanded") { COMP.mode = "open"; renderCompanion(); document.getElementById("tess-panel-input")?.focus(); return true; }
  if (COMP.mode === "open") { closeCompanion(true); return true; }
  return false;
}

function clearCompanionHistory() {
  CMDK.aiAns = null;
  CMDK.aiHistory = [];
  renderCompanion();
  document.getElementById("tess-panel-input")?.focus({ preventScroll: true });
}

function dismissCompanionContext(key) {
  COMP.dismissedCtxKey = key;
  renderCompanion();
}

function syncCompanionRoute(r) {
  if (Array.isArray(r)) {
    const prevKey = describeRouteContext(COMP.currentRoute).key;
    COMP.currentRoute = r;
    if (prevKey !== describeRouteContext(r).key) COMP.dismissedCtxKey = null;
  }
  syncTessVisibilityMenu();
  renderCompanion();
}

function isCompanionModal() {
  return COMP.mode === "expanded" || (COMP.mode === "open" && window.innerWidth <= 639);
}

export {
  COMP,
  clearCompanionHistory,
  closeCompanion,
  dismissCompanionContext,
  handleCompanionEscape,
  handleCompanionInput,
  isCompanionModal,
  openCompanion,
  registerCompanionHooks,
  renderCompanion,
  sendCompanionMessage,
  shortcutLabel,
  syncCompanionRoute,
  toggleCompanion,
  toggleExpandCompanion,
  toggleTessPillVisible,
};
