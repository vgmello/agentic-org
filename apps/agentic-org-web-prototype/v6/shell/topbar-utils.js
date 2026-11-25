import { M, P, W } from "../data.js";
import { answerOrgQuestion } from "../pages/home.js";
import { CMDK, closeCmdk, isMacPlatform } from "./cmdk.js";
import { S, qs } from "../state.js";
import { esc, ic } from "../ui.js";

/* ---------------------------------------------------------------- topbar utilities (Notifications + Persistent Tess Panel) */

const runningJobs = [];
const notifications = [
  {
    id: "n-1",
    title: "Kai finished T-A121",
    summary: "Sync lockfile race test passed on macOS.",
    time: "14m ago",
    href: "#/work?item=T-A121",
    icon: "check-circle",
    tone: "ok",
    read: false,
  },
  {
    id: "n-2",
    title: "412 CI checks green",
    summary: "v1.0 release pipeline passed with zero regressions.",
    time: "38m ago",
    href: "#/activity",
    icon: "signal",
    tone: "ok",
    read: false,
  },
  {
    id: "n-3",
    title: "HQ office synced",
    summary: "This Mac office heartbeats healthy · 7 agents active.",
    time: "1h ago",
    href: "#/settings/offices",
    icon: "office",
    tone: "accent",
    read: true,
  },
  {
    id: "n-4",
    title: "Ada updated v1.0 release notes",
    summary: "Ready for review in Projects · Agentic Org v1.0.",
    time: "2h ago",
    href: "#/projects/agency-v1",
    icon: "doc-check",
    tone: "accent",
    read: true,
  },
];

const UTIL = {
  notifOpen: false,
  tessOpen: false,
  dismissedCtxKey: null,
  notifReturnTo: null,
  tessReturnTo: null,
  currentRoute: ["home"],
};

const TESS_STARTERS = {
  empty: [
    "How do I launch my first project?",
    "What's my starting budget & AI capacity?",
  ],
  work: [
    "What's slowing v1.0?",
    "What needs me first?",
    "Budget, capacity & idle agents",
  ],
  default: [
    "What needs me first?",
    "What's slowing v1.0?",
    "What happened in the last 4 hours?",
  ],
};

function announceNotif(text) {
  const live = document.getElementById("notif-live");
  if (!live) return;
  live.textContent = "";
  requestAnimationFrame(() => { live.textContent = text; });
}

function isOfflineMode() {
  return S.homeState === "error";
}

function unreadNotificationsCount() {
  return notifications.reduce((acc, n) => acc + (n.read ? 0 : 1), 0);
}

function activeTessJobs() {
  return runningJobs.filter((j) => j.source === "tess");
}

function describeRouteContext(r = UTIL.currentRoute) {
  const [page, r1] = r;
  const item = qs().get("item");
  if (item) {
    const task = W.find((w) => w.id === item);
    return { key: `item:${item}`, label: task ? `Work · ${task.id}` : `Task · ${item}` };
  }
  if (page === "projects" && r1 && P[r1]) return { key: `proj:${r1}`, label: `Project · ${P[r1].name}` };
  if (page === "members" && r1 && M[r1]) return { key: `member:${r1}`, label: `Member · ${M[r1].name}` };
  const map = { home: "Home", inbox: "Inbox", work: "Work", chat: "Chat", activity: "Activity", projects: "Projects", teams: "Teams", settings: "Settings" };
  return { key: `page:${page || "home"}`, label: map[page] || "Workspace" };
}

function isTessDocked() {
  const w = window.innerWidth;
  return w >= 1440 || (w >= 1280 && document.body.classList.contains("nav-collapsed"));
}

/* ---------------------------------------------------------------- triggers */

function syncNotifTrigger() {
  const btn = document.getElementById("notif-trigger");
  if (!btn) return;
  const unread = unreadNotificationsCount();
  const running = runningJobs.length;
  btn.classList.toggle("is-busy", running > 0);
  btn.classList.toggle("has-unread", unread > 0);
  btn.setAttribute("aria-expanded", String(UTIL.notifOpen));
  const parts = ["Notifications"];
  if (running > 0) parts.push(`${running} running`);
  if (unread > 0) parts.push(`${unread} unread`);
  const label = parts.join(", ");
  btn.setAttribute("aria-label", label);
  btn.title = running > 0 ? `${running} background operation${running > 1 ? "s" : ""} running` : label;
  const badge = unread > 0 ? `<span class="notif-badge tnum" aria-hidden="true">${unread > 9 ? "9+" : unread}</span>` : "";
  const ring = running > 0 ? `<span class="notif-ring" aria-hidden="true"></span>` : "";
  btn.innerHTML = `${ring}${ic("bell", "ic16")}${badge}`;
}

function syncTessTrigger() {
  const btn = document.getElementById("tess-trigger");
  if (!btn) return;
  const busy = activeTessJobs();
  const offline = isOfflineMode();
  const kbd = isMacPlatform ? "⌘/" : "Ctrl /";
  btn.classList.toggle("is-busy", busy.length > 0);
  btn.classList.toggle("is-offline", offline);
  btn.classList.toggle("is-active", UTIL.tessOpen);
  btn.setAttribute("aria-expanded", String(UTIL.tessOpen));
  const statusText = busy.length
    ? `Tess · ${busy.length > 1 ? `Refining ${busy.length} tasks` : busy[0].step || "Working"}`
    : offline ? `Tess · Offline local summary (${kbd})` : `Ask Tess (${kbd})`;
  btn.title = statusText;
  btn.setAttribute("aria-label", statusText);
  const dot = busy.length > 0 ? `<span class="tess-live-dot" aria-hidden="true"></span>` : "";
  btn.innerHTML = `${ic(offline ? "offline" : "sparkle", "ic14")}<span class="util-lbl">Tess</span>${dot}`;
}

/* ---------------------------------------------------------------- notifications popover */

function renderNotifRunningSection() {
  if (!runningJobs.length) return "";
  const items = runningJobs.map((job) => `
    <li class="notif-job-row">
      <span class="notif-job-spin" aria-hidden="true"></span>
      <span class="notif-row-copy">
        <span class="notif-row-title">${esc(job.title)}</span>
        <span class="notif-row-sub">${esc(job.step || "Processing in background…")}</span>
      </span>
      <span class="notif-job-pill">Active</span>
    </li>`).join("");
  return `<div class="notif-sec">
    <div class="notif-sec-hd">In progress</div>
    <ul class="notif-list" role="list">${items}</ul>
  </div>`;
}

function renderNotifRecentSection() {
  if (!notifications.length) {
    return `<p class="notif-empty">All caught up — no recent system notifications.</p>`;
  }
  const rows = notifications.map((n) => `
    <li>
      <a class="notif-row${n.read ? "" : " is-unread"}" href="${esc(n.href)}" data-act="notif-open" data-id="${esc(n.id)}">
        <span class="notif-row-ic tone-${esc(n.tone || "accent")}" aria-hidden="true">${ic(n.icon || "check-circle", "ic14")}</span>
        <span class="notif-row-copy">
          <span class="notif-row-top">
            <span class="notif-row-title">${esc(n.title)}</span>
            <span class="notif-row-time">${esc(n.time)}</span>
          </span>
          <span class="notif-row-sub">${esc(n.summary)}</span>
        </span>
        ${n.read ? "" : '<span class="notif-unread-dot" aria-hidden="true"></span>'}
      </a>
    </li>`).join("");
  return `<div class="notif-sec">
    <div class="notif-sec-hd">Recent</div>
    <ul class="notif-list" role="list">${rows}</ul>
  </div>`;
}

function positionNotifPopover() {
  const pop = document.getElementById("notif-popover");
  const trigger = document.getElementById("notif-trigger");
  if (!pop || !trigger || pop.hidden) return;
  const rect = trigger.getBoundingClientRect();
  pop.style.top = `${Math.round(rect.bottom + 8)}px`;
  pop.style.right = `${Math.max(12, Math.round(window.innerWidth - rect.right))}px`;
}

function renderNotifPopover() {
  const pop = document.getElementById("notif-popover");
  if (!pop) return;
  pop.hidden = !UTIL.notifOpen;
  if (!UTIL.notifOpen) return;
  positionNotifPopover();
  const unread = unreadNotificationsCount();
  const hasRead = notifications.some((n) => n.read);
  const runningPill = runningJobs.length
    ? `<span class="notif-running-pill tnum">${runningJobs.length} running</span>` : "";
  const markAllBtn = unread > 0
    ? `<button type="button" class="notif-text-btn" data-act="notif-mark-all">Mark all read</button>` : "";
  const offlineRow = isOfflineMode()
    ? `<div class="notif-offline-row">${ic("offline", "ic14")}<span>HQ office offline · Live updates paused</span></div>` : "";
  const clearReadBtn = hasRead
    ? `<button type="button" class="notif-text-btn" data-act="notif-clear-read">Clear read</button>` : "";
  pop.innerHTML = `
    <div class="notif-hd">
      <div class="notif-hd-title">
        <strong>Notifications</strong>
        ${runningPill}
      </div>
      <div class="notif-hd-acts">
        ${markAllBtn}
        <button type="button" class="btn ghost icon sm notif-x" data-act="notif-close" aria-label="Close notifications">${ic("x", "ic14")}</button>
      </div>
    </div>
    ${offlineRow}
    <div class="notif-body">
      ${renderNotifRunningSection()}
      ${renderNotifRecentSection()}
    </div>
    <div class="notif-foot">
      <a href="#/activity" data-act="notif-close">View all activity</a>
      ${clearReadBtn}
    </div>`;
}

/* ---------------------------------------------------------------- persistent Tess side panel */

function renderTessActiveStrip() {
  const busy = activeTessJobs();
  if (!busy.length) return "";
  const label = busy.length > 1
    ? `Refining ${busy.length} tasks in background…`
    : `${busy[0].title} — ${busy[0].step || "Refining…"}`;
  return `<div class="tess-busy-strip" role="status">
    <span class="notif-job-spin" aria-hidden="true"></span>
    <span>${esc(label)}</span>
  </div>`;
}

function renderTessContextBar() {
  const ctx = describeRouteContext();
  if (UTIL.dismissedCtxKey === ctx.key) return "";
  return `<div class="tess-ctx-bar">
    <span class="tess-ctx-pill">
      <span>Viewing: <strong>${esc(ctx.label)}</strong></span>
      <button type="button" data-act="tess-dismiss-ctx" data-v="${esc(ctx.key)}" aria-label="Dismiss page context">${ic("x", "ic14")}</button>
    </span>
  </div>`;
}

function renderTessThread() {
  const turns = [...CMDK.aiHistory].reverse();
  if (!turns.length) {
    return `<div class="tess-welcome">
      <p class="tess-welcome-lead">Ask Tess about blockers, release readiness, agent workload, or recent updates.</p>
      <p class="tess-welcome-sub">Answers draw from live workspace state and stay with you as you navigate.</p>
    </div>`;
  }
  return `<div class="tess-thread">${turns.map((turn) => `
    <div class="tess-turn">
      <div class="tess-q">${esc(turn.q)}</div>
      <div class="tess-a">
        <div class="tess-a-hd">${ic("sparkle", "ic14")}<span>Tess</span></div>
        <div class="tess-a-body">${turn.html}</div>
      </div>
    </div>`).join("")}</div>`;
}

function renderTessStarters() {
  const asked = new Set(CMDK.aiHistory.map((x) => x.q.trim().toLowerCase()));
  const key = S.homeState === "empty" ? "empty" : UTIL.currentRoute[0] === "work" ? "work" : "default";
  const chips = (TESS_STARTERS[key] || TESS_STARTERS.default).filter((q) => !asked.has(q.toLowerCase())).slice(0, 3);
  if (!chips.length) return "";
  return `<div class="tess-chips" aria-label="Suggested prompts">${chips.map((q) => `
    <button type="button" class="tess-chip" data-act="tess-ask-chip" data-v="${esc(q)}">
      ${ic("sparkle", "ic14")}<span>${esc(q)}</span>
    </button>`).join("")}</div>`;
}

function syncTessPanelChrome(panel, scrim, open, docked) {
  panel.hidden = !open;
  panel.classList.toggle("is-docked", docked);
  panel.classList.toggle("is-modal", open && !docked);
  panel.setAttribute("role", docked ? "complementary" : "dialog");
  if (open && !docked) panel.setAttribute("aria-modal", "true");
  else panel.removeAttribute("aria-modal");
  document.body.classList.toggle("tess-open", docked);
  if (scrim) scrim.hidden = !(open && !docked);
}

function renderTessHeader(busy, offline) {
  const statusLbl = busy.length ? `Refining (${busy.length})` : offline ? "Offline · Local only" : "Chief of Staff";
  const clearBtn = CMDK.aiHistory.length
    ? `<button type="button" class="notif-text-btn" data-act="tess-clear">Clear</button>` : "";
  return `<div class="tess-hd">
    <div class="tess-hd-who">
      <span class="tess-hd-mark" aria-hidden="true">${ic("sparkle", "ic14")}</span>
      <div class="tess-hd-txt">
        <strong>Tess</strong>
        <span class="tess-hd-status${offline ? " is-offline" : ""}">${esc(statusLbl)}</span>
      </div>
    </div>
    <div class="tess-hd-acts">
      ${clearBtn}
      <a class="tess-dm-link" href="#/chat/tess" data-act="tess-open-dm" title="Open full Tess DM">DM ${ic("open", "ic14")}</a>
      <button type="button" class="btn ghost icon sm" data-act="tess-close" aria-label="Close Tess panel">${ic("x", "ic16")}</button>
    </div>
  </div>`;
}

function renderTessPanel() {
  const panel = document.getElementById("tess-panel");
  if (!panel) return;
  const open = UTIL.tessOpen;
  syncTessPanelChrome(panel, document.getElementById("tess-panel-scrim"), open, open && isTessDocked());
  if (!open) return;

  const offline = isOfflineMode();
  const offlineNotice = offline
    ? `<div class="tess-offline-note">${ic("offline", "ic14")}<span>Offline quick summary · Local data only</span></div>` : "";
  const draftVal = document.getElementById("tess-panel-input")?.value || "";

  panel.innerHTML = `
    ${renderTessHeader(activeTessJobs(), offline)}
    ${renderTessContextBar()}
    ${renderTessActiveStrip()}
    ${offlineNotice}
    <div class="tess-body" id="tess-panel-body">
      ${renderTessThread()}
      ${renderTessStarters()}
    </div>
    <form class="tess-compose" data-form="tess-panel-ask">
      <input id="tess-panel-input" type="text" value="${esc(draftVal)}" placeholder="Ask Tess anything…" autocomplete="off" aria-label="Ask Tess">
      <button type="submit" class="btn primary sm icon tess-send-btn" aria-label="Send to Tess">${ic("send", "ic14")}</button>
    </form>`;

  const body = document.getElementById("tess-panel-body");
  if (body) body.scrollTop = body.scrollHeight;
}

/* ---------------------------------------------------------------- public controls */

function closeNotificationsPopover(restoreFocus = false) {
  if (!UTIL.notifOpen) return;
  UTIL.notifOpen = false;
  syncNotifTrigger();
  renderNotifPopover();
  if (restoreFocus && UTIL.notifReturnTo && document.body.contains(UTIL.notifReturnTo)) UTIL.notifReturnTo.focus();
  UTIL.notifReturnTo = null;
}

function toggleNotificationsPopover(trigger) {
  if (UTIL.notifOpen) { closeNotificationsPopover(true); return; }
  if (CMDK.open) closeCmdk(false);
  if (UTIL.tessOpen && !isTessDocked()) closeTessPanel(false);
  UTIL.notifOpen = true;
  UTIL.notifReturnTo = trigger || document.getElementById("notif-trigger");
  syncNotifTrigger();
  renderNotifPopover();
  document.querySelector("#notif-popover .notif-x")?.focus({ preventScroll: true });
}

function closeTessPanel(restoreFocus = false) {
  if (!UTIL.tessOpen) return;
  UTIL.tessOpen = false;
  syncTessTrigger();
  renderTessPanel();
  if (restoreFocus && UTIL.tessReturnTo && document.body.contains(UTIL.tessReturnTo)) UTIL.tessReturnTo.focus();
  UTIL.tessReturnTo = null;
}

function askTessInPanel(rawQ) {
  const q = String(rawQ || "").trim();
  if (!q) return;
  const ans = answerOrgQuestion(q);
  if (ans) {
    ans._state = S.homeState;
    CMDK.aiAns = ans;
    CMDK.aiHistory = [ans, ...CMDK.aiHistory.filter((x) => x.q.toLowerCase() !== ans.q.toLowerCase())];
  }
  const inp = document.getElementById("tess-panel-input");
  if (inp) inp.value = "";
  renderTessPanel();
  document.getElementById("tess-panel-input")?.focus({ preventScroll: true });
}

function openTessPanel(initialQuestion = "", trigger = null) {
  if (CMDK.open) closeCmdk(false);
  closeNotificationsPopover(false);
  const wasOpen = UTIL.tessOpen;
  UTIL.tessOpen = true;
  if (!wasOpen) UTIL.tessReturnTo = trigger || document.getElementById("tess-trigger");
  if (initialQuestion && String(initialQuestion).trim()) {
    askTessInPanel(initialQuestion);
  } else {
    renderTessPanel();
  }
  syncTessTrigger();
  document.getElementById("tess-panel-input")?.focus({ preventScroll: true });
}

function toggleTessPanel(trigger) {
  if (UTIL.currentRoute[0] === "chat" && UTIL.currentRoute[1] === "tess") {
    document.getElementById("compose")?.focus();
    return;
  }
  if (UTIL.tessOpen) closeTessPanel(true);
  else openTessPanel("", trigger);
}

function markNotificationRead(id) {
  const item = notifications.find((n) => n.id === id);
  if (item) item.read = true;
  closeNotificationsPopover(false);
}

function markAllNotificationsRead() {
  notifications.forEach((n) => { n.read = true; });
  syncNotifTrigger();
  renderNotifPopover();
}

function clearReadNotifications() {
  for (let i = notifications.length - 1; i >= 0; i -= 1) {
    if (notifications[i].read) notifications.splice(i, 1);
  }
  syncNotifTrigger();
  renderNotifPopover();
}

function clearTessHistory() {
  CMDK.aiAns = null;
  CMDK.aiHistory = [];
  renderTessPanel();
  document.getElementById("tess-panel-input")?.focus({ preventScroll: true });
}

function dismissTessContext(key) {
  UTIL.dismissedCtxKey = key;
  renderTessPanel();
}

function startBackgroundJob({ id, title, step = "Tess refining & routing…", source = "tess" }) {
  const idx = runningJobs.findIndex((j) => j.id === id);
  const entry = { id, title, step, source, startedAt: Date.now() };
  if (idx >= 0) runningJobs[idx] = entry;
  else runningJobs.unshift(entry);
  syncTopbarUtils();
  announceNotif(`Started: ${title}`);
}

function finishBackgroundJob(id, { title, summary, href = "#/work", icon = "check-circle", tone = "ok" }) {
  const idx = runningJobs.findIndex((j) => j.id === id);
  if (idx >= 0) runningJobs.splice(idx, 1);
  notifications.unshift({
    id: `n-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`,
    title,
    summary,
    time: "Just now",
    href,
    icon,
    tone,
    read: false,
  });
  syncTopbarUtils();
  announceNotif(`Completed: ${title}`);
}

function syncTopbarUtils(r = null) {
  if (Array.isArray(r)) {
    const prevKey = describeRouteContext(UTIL.currentRoute).key;
    UTIL.currentRoute = r;
    const nextKey = describeRouteContext(r).key;
    if (prevKey !== nextKey) UTIL.dismissedCtxKey = null;
  }
  syncNotifTrigger();
  syncTessTrigger();
  if (UTIL.notifOpen) renderNotifPopover();
  if (UTIL.tessOpen) renderTessPanel();
}

function isTopbarPopoverOpen() {
  return UTIL.notifOpen || (UTIL.tessOpen && !isTessDocked());
}

window.addEventListener("resize", () => {
  if (UTIL.tessOpen) renderTessPanel();
  if (UTIL.notifOpen) positionNotifPopover();
});

export {
  UTIL,
  askTessInPanel,
  clearReadNotifications,
  clearTessHistory,
  closeNotificationsPopover,
  closeTessPanel,
  dismissTessContext,
  finishBackgroundJob,
  isTessDocked,
  isTopbarPopoverOpen,
  markAllNotificationsRead,
  markNotificationRead,
  openTessPanel,
  startBackgroundJob,
  syncTopbarUtils,
  toggleNotificationsPopover,
  toggleTessPanel,
};
