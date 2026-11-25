import { CMDK, closeCmdk } from "./cmdk.js";
import {
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
  syncCompanionRoute,
  toggleCompanion,
  toggleExpandCompanion,
  toggleTessPillVisible,
} from "./companion.js";
import { S } from "../state.js";
import { esc, ic } from "../ui.js";

/* ---------------------------------------------------------------- topbar Notifications + Floating Tess coordination */

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
  notifReturnTo: null,
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

/* ---------------------------------------------------------------- notifications trigger & popover */

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
  if (!notifications.length) return `<p class="notif-empty">All caught up — no recent system notifications.</p>`;
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
  if (window.innerWidth < 900 && COMP.mode === "open") closeCompanion(false);
  UTIL.notifOpen = true;
  UTIL.notifReturnTo = trigger || document.getElementById("notif-trigger");
  syncNotifTrigger();
  renderNotifPopover();
  document.querySelector("#notif-popover .notif-x")?.focus({ preventScroll: true });
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
  syncCompanionRoute(r);
  syncNotifTrigger();
  if (UTIL.notifOpen) renderNotifPopover();
}

registerCompanionHooks({
  activeJobs: activeTessJobs,
  closeNotif: closeNotificationsPopover,
});

window.addEventListener("resize", () => {
  renderCompanion();
  if (UTIL.notifOpen) positionNotifPopover();
});

export {
  COMP,
  UTIL,
  clearCompanionHistory,
  clearReadNotifications,
  closeCompanion,
  closeNotificationsPopover,
  dismissCompanionContext,
  finishBackgroundJob,
  handleCompanionEscape,
  handleCompanionInput,
  isCompanionModal,
  markAllNotificationsRead,
  markNotificationRead,
  openCompanion as openTessPanel,
  sendCompanionMessage as askTessInPanel,
  startBackgroundJob,
  syncTopbarUtils,
  toggleCompanion as toggleTessPanel,
  toggleExpandCompanion,
  toggleNotificationsPopover,
  toggleTessPillVisible,
};
