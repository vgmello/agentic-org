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
} from "./companion.js";

/* ---------------------------------------------------------------- Inbox Notifications store + Tess coordination */

const runningJobs = [];
const notifications = [
  {
    id: "n-1",
    title: "Kai finished T-A121",
    summary: "Sync lockfile race test passed on macOS.",
    time: "14m ago",
    href: "#/work?item=T-A121",
    hrefLabel: "Open T-A121 in Work",
    icon: "check-circle",
    tone: "ok",
    source: "Work · Agentic Org v1.0",
    read: false,
  },
  {
    id: "n-2",
    title: "412 CI checks green",
    summary: "v1.0 release pipeline passed with zero regressions.",
    time: "38m ago",
    href: "#/activity",
    hrefLabel: "Open Activity",
    icon: "signal",
    tone: "ok",
    source: "CI Pipeline · Agentic Org v1.0",
    read: false,
  },
  {
    id: "n-3",
    title: "HQ office synced",
    summary: "This Mac office heartbeats healthy · 7 agents active.",
    time: "1h ago",
    href: "#/settings/offices",
    hrefLabel: "Open Offices",
    icon: "office",
    tone: "accent",
    source: "Offices · HQ",
    read: true,
  },
  {
    id: "n-4",
    title: "Ada updated v1.0 release notes",
    summary: "Ready for review in Projects · Agentic Org v1.0.",
    time: "2h ago",
    href: "#/projects/agency-v1",
    hrefLabel: "Open Agentic Org v1.0",
    icon: "doc-check",
    tone: "accent",
    source: "Projects · Agentic Org v1.0",
    read: true,
  },
];

let notifChangeListener = () => {};

function registerNotifChangeListener(fn) {
  notifChangeListener = typeof fn === "function" ? fn : () => {};
}

function announceNotif(text) {
  const live = document.getElementById("notif-live");
  if (!live) return;
  live.textContent = "";
  requestAnimationFrame(() => { live.textContent = text; });
}

function unreadNotificationsCount() {
  return notifications.reduce((acc, n) => acc + (n.read ? 0 : 1), 0);
}

function getNotifications() {
  return notifications;
}

function getRunningJobs() {
  return runningJobs;
}

function activeTessJobs() {
  return runningJobs.filter((j) => j.source === "tess");
}

function markNotificationRead(id) {
  const item = notifications.find((n) => n.id === id);
  if (item && !item.read) {
    item.read = true;
    notifChangeListener();
  }
}

function toggleNotificationRead(id) {
  const item = notifications.find((n) => n.id === id);
  if (!item) return;
  item.read = !item.read;
  notifChangeListener();
}

function markAllNotificationsRead() {
  notifications.forEach((n) => { n.read = true; });
  notifChangeListener();
}

function clearReadNotifications() {
  for (let i = notifications.length - 1; i >= 0; i -= 1) {
    if (notifications[i].read) notifications.splice(i, 1);
  }
  notifChangeListener();
}

function startBackgroundJob({ id, title, step = "Tess refining & routing…", source = "tess" }) {
  const idx = runningJobs.findIndex((j) => j.id === id);
  const entry = { id, title, step, source, startedAt: Date.now() };
  if (idx >= 0) runningJobs[idx] = entry;
  else runningJobs.unshift(entry);
  syncTopbarUtils();
  notifChangeListener();
  announceNotif(`Started: ${title}`);
}

function finishBackgroundJob(id, { title, summary, href = "#/work", hrefLabel = "Open item", source = "Tess", icon = "check-circle", tone = "ok" }) {
  const idx = runningJobs.findIndex((j) => j.id === id);
  if (idx >= 0) runningJobs.splice(idx, 1);
  const entry = {
    id: `n-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`,
    title,
    summary,
    time: "Just now",
    href,
    hrefLabel,
    source,
    icon,
    tone,
    read: false,
  };
  notifications.unshift(entry);
  syncTopbarUtils();
  notifChangeListener();
  announceNotif(`Completed: ${title}`);
  return entry;
}

function syncTopbarUtils(r = null) {
  syncCompanionRoute(r);
}

registerCompanionHooks({
  activeJobs: activeTessJobs,
});

window.addEventListener("resize", () => {
  renderCompanion();
});

export {
  COMP,
  clearCompanionHistory,
  clearReadNotifications,
  closeCompanion,
  dismissCompanionContext,
  finishBackgroundJob,
  getNotifications,
  getRunningJobs,
  handleCompanionEscape,
  handleCompanionInput,
  isCompanionModal,
  markAllNotificationsRead,
  markNotificationRead,
  openCompanion as openTessPanel,
  registerNotifChangeListener,
  sendCompanionMessage as askTessInPanel,
  startBackgroundJob,
  syncTopbarUtils,
  toggleCompanion as toggleTessPanel,
  toggleExpandCompanion,
  toggleNotificationRead,
  unreadNotificationsCount,
};
