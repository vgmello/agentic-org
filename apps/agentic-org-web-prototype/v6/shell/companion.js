import { IN, M, P, W } from "../data.js";
import { answerOrgQuestion } from "../pages/home.js";
import { CMDK, closeCmdk, isMacPlatform } from "./cmdk.js";
import { S, qs } from "../state.js";
import { esc, ic } from "../ui.js";

/* ---------------------------------------------------------------- Floating Tess Companion (Anchored to Sidebar Start Menu) */

const COMP = {
  mode: "hidden", // "hidden" | "open" | "expanded"
  prevMode: "hidden",
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

function registerCompanionHooks({ activeJobs }) {
  getActiveTessJobs = activeJobs;
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

function placeholderForContext() {
  const ctx = describeRouteContext();
  if (ctx.key === "page:home") return "Ask Tess anything…";
  const short = ctx.label.replace(/^(Project|Member|Inbox|Task)\s*·\s*/, "");
  const clipped = short.length > 26 ? short.slice(0, 25).trimEnd() + "…" : short;
  return `Ask Tess about ${clipped}…`;
}

/* ---------------------------------------------------------------- rendering */

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
      <div class="tess-a"><div class="tess-a-body">${turn.html}</div></div>
    </div>`).join("")}</div>` : "";
  const chipsHtml = chips.length ? `<div class="tess-chips" aria-label="Suggested prompts">${chips.map((q) => `
    <button type="button" class="tess-chip" data-act="cmp-ask-chip" data-v="${esc(q)}">
      <span>${esc(q)}</span><span class="tess-chip-arr" aria-hidden="true">→</span>
    </button>`).join("")}</div>` : "";
  return threadHtml + chipsHtml;
}

function renderTessHeader(expanded) {
  const busy = getActiveTessJobs();
  const offline = isOfflineMode();
  const statusLbl = busy.length ? `Refining (${busy.length})` : offline ? "Offline · Local only" : "";
  const clearBtn = CMDK.aiHistory.length
    ? `<button type="button" class="notif-text-btn" data-act="cmp-clear" title="Clear Tess thread">Clear</button>` : "";
  const expKbd = shortcutLabel("expand");
  const expTip = `${expanded ? "Restore size" : "Maximize"} · ${expKbd}`;
  const closeTip = "Close · Esc";
  return `<div class="cmp-hd">
    <div class="cmp-tess-who">
      ${ic(offline ? "offline" : "sparkle", "ic14")}
      <strong>Tess</strong>
      ${statusLbl ? `<span class="cmp-tess-st${offline ? " is-offline" : ""}">· ${esc(statusLbl)}</span>` : ""}
    </div>
    <div class="cmp-hd-acts">
      ${clearBtn}
      <button type="button" class="btn ghost icon sm cmp-expand-btn" data-act="cmp-expand" data-tip="${esc(expTip)}" aria-label="${esc(expTip)}" aria-keyshortcuts="Meta+Shift+E Control+Shift+E">${ic(expanded ? "minimize" : "maximize", "ic14")}</button>
      <button type="button" class="btn ghost icon sm" data-act="cmp-close" data-tip="${esc(closeTip)}" aria-label="${esc(closeTip)}">${ic("x", "ic14")}</button>
    </div>
  </div>`;
}

function syncSidebarTessTrigger() {
  const btn = document.getElementById("sidebar-tess-trigger");
  if (!btn) return;
  const busy = getActiveTessJobs();
  const open = COMP.mode !== "hidden";
  const kbd = shortcutLabel("toggle");
  const tip = busy.length ? `Tess refining (${busy.length}) · ${kbd}` : `Ask Tess · ${kbd}`;
  btn.classList.toggle("is-active", open);
  btn.classList.toggle("is-busy", busy.length > 0);
  btn.setAttribute("aria-pressed", String(open));
  btn.setAttribute("aria-label", tip);
  btn.title = tip;
  delete btn.dataset.tip;
  const dot = document.getElementById("sidebar-tess-busy");
  if (dot) dot.hidden = busy.length === 0;
}

function syncCompanionClasses(panel, scrim, compact) {
  const visible = COMP.mode !== "hidden";
  const open = COMP.mode === "open";
  const expanded = COMP.mode === "expanded";
  const mobile = window.innerWidth <= 639;
  panel.hidden = !visible;
  panel.classList.toggle("is-compact", open && compact);
  panel.classList.toggle("is-expanded", expanded);
  panel.classList.remove("is-minimized");
  panel.setAttribute("role", expanded || (open && mobile) ? "dialog" : "complementary");
  if (expanded || (open && mobile)) panel.setAttribute("aria-modal", "true");
  else panel.removeAttribute("aria-modal");
  document.body.classList.toggle("companion-open", open);
  document.body.classList.remove("companion-minimized");
  if (scrim) scrim.hidden = !(expanded || (open && mobile));
}

function resolveModeAnim(prev, next) {
  if (prev === next || next === "hidden") return "";
  if (next === "expanded") return "expand";
  if (next === "open") return prev === "expanded" ? "restore" : "open";
  return "";
}

function triggerPanelAnimation(panel, anim) {
  delete panel.dataset.anim;
  if (!anim) return;
  void panel.offsetWidth;
  panel.dataset.anim = anim;
  panel.addEventListener("animationend", () => {
    if (panel.dataset.anim === anim) delete panel.dataset.anim;
  }, { once: true });
}

function renderCompanion() {
  syncSidebarTessTrigger();
  const panel = document.getElementById("tess-panel");
  if (!panel) return;
  const anim = resolveModeAnim(COMP.prevMode, COMP.mode);
  COMP.prevMode = COMP.mode;
  const compact = CMDK.aiHistory.length === 0;
  syncCompanionClasses(panel, document.getElementById("tess-panel-scrim"), compact);
  if (COMP.mode === "hidden") { delete panel.dataset.anim; return; }

  const ph = placeholderForContext();
  panel.innerHTML = `
    ${renderTessHeader(COMP.mode === "expanded")}
    ${renderActiveBanners()}
    <div class="tess-body" id="tess-panel-body">${renderTessBody()}</div>
    <form class="tess-compose" data-form="tess-panel-ask">
      <div class="tess-input-capsule">
        <input id="tess-panel-input" type="text" value="${esc(COMP.draft)}" placeholder="${esc(ph)}" autocomplete="off" aria-label="${esc(ph)}">
        <button type="submit" id="tess-return-btn" class="tess-return-btn${COMP.draft.trim() ? " is-ready" : ""}" aria-label="Send to Tess" title="Send (Enter)">↵</button>
      </div>
    </form>`;

  triggerPanelAnimation(panel, anim);
  const body = document.getElementById("tess-panel-body");
  if (body) body.scrollTop = body.scrollHeight;
}

/* ---------------------------------------------------------------- actions & shortcuts */

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
  if (COMP.mode === "hidden") COMP.returnTo = trigger || document.activeElement;
  COMP.mode = COMP.mode === "expanded" ? "expanded" : "open";
  if (initialQuestion && String(initialQuestion).trim()) sendCompanionMessage(initialQuestion);
  else renderCompanion();
  document.getElementById("tess-panel-input")?.focus({ preventScroll: true });
}

function closeCompanion(restoreFocus = false) {
  if (COMP.mode === "hidden") return;
  COMP.mode = "hidden";
  renderCompanion();
  if (!restoreFocus) return;
  const fallback = COMP.returnTo && document.body.contains(COMP.returnTo)
    ? COMP.returnTo
    : document.getElementById("sidebar-tess-trigger");
  if (fallback && document.body.contains(fallback)) fallback.focus();
  COMP.returnTo = null;
}

function toggleCompanion(trigger) {
  if (COMP.mode === "hidden") { openCompanion("", trigger); return; }
  const panel = document.getElementById("tess-panel");
  if (panel?.contains(document.activeElement) || COMP.mode === "expanded" || trigger?.id === "sidebar-tess-trigger") closeCompanion(true);
  else document.getElementById("tess-panel-input")?.focus({ preventScroll: true });
}

function toggleExpandCompanion() {
  if (COMP.mode === "hidden") {
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
  document.getElementById("tess-return-btn")?.classList.toggle("is-ready", Boolean(COMP.draft.trim()));
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
};
