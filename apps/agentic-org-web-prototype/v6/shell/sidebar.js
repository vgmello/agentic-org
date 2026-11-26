import { P, TEAMS } from "../data.js";
import { fitTextarea } from "../pages/work.js";
import { CMDK, closeCmdk } from "./cmdk.js";
import { own, render } from "./router.js";
import { getRunningJobs, unreadNotificationsCount } from "./topbar-utils.js";
import { FONT_OPTIONS, S, THEME_OPTIONS, applyNavCollapsed, saveNavWorkspace, sidebarHasOpenMenu } from "../state.js";
import { effectiveHealth, esc, ic, seg, unhandled } from "../ui.js";

/* ---------------------------------------------------------------- sidebar */
const NAV_CAP = 5;
const TEAM_ICON = { leadership: "flag", product: "adjustments", platform: "terminal", app: "bolt", quality: "doc-check" };
function syncNavSearchChrome(homeError) {
  const cmdkTr = document.getElementById("cmdk-trigger");
  if (cmdkTr) { cmdkTr.classList.toggle("is-offline", homeError); cmdkTr.disabled = homeError; }
  if (homeError && CMDK.open) closeCmdk(false);
}

function syncOfficeNavStatus(homeError) {
  const officeLink = document.getElementById("office-link");
  const officeFull = document.getElementById("office-full");
  const officeRailBadge = document.getElementById("office-rail-badge");
  if (!officeLink || !officeFull || !officeRailBadge) return;
  officeLink.classList.toggle("is-offline", homeError);
  officeLink.setAttribute("aria-label", homeError ? "This Mac, HQ office, offline. Open Offices" : "This Mac, HQ office, online. Open Offices");
  officeLink.title = homeError ? "This Mac · HQ office · Offline" : "This Mac · HQ office · Online";
  officeFull.innerHTML = homeError
    ? `${ic("offline", "ic14")}This Mac · HQ office · Offline`
    : `${ic("check-circle", "ic14")}This Mac · HQ office · Online`;
  officeRailBadge.innerHTML = ic(homeError ? "offline" : "check-circle");
}

function projNavIcon(p, homeError) {
  if (homeError) return ["folder", ""];
  const map = { "On track": ["check-circle", "tone-ok"], "At risk": ["warn", "tone-warn"], "Off track": ["x-circle", "tone-bad"] };
  return map[effectiveHealth(p)] || ["folder", ""];
}

function navFavoritesHtml(favs, r0, r1, homeError, favsOpen) {
  const rows = favs.length ? favs.map((p) => {
    const [ico, cls] = projNavIcon(p, homeError);
    const cur = r0 === "projects" && r1 === p;
    return `<li class="nav-fav-row">
            <a class="nav-item" href="#/projects/${p}" title="${esc(P[p].name)} · ${homeError ? "Offline" : esc(effectiveHealth(p) || "")}" ${cur ? 'aria-current="page"' : ""}>${ic(ico, cls)}<span class="lbl">${esc(P[p].name)}</span></a>
            <button class="fav-star-btn" type="button" data-act="fav-proj" data-id="${p}" aria-label="Remove ${esc(P[p].name)} from favorites" title="Unpin from favorites">${ic("star")}</button>
          </li>`;
  }).join("") : `<li class="nav-fav-empty">No favorite projects yet.</li>`;
  return `<li class="nav-fav-list">
      <div class="nav-group-hd">
        <span class="nav-group-title">${ic("star")}<span>Favorite projects</span></span>
        <button class="nav-fold-btn" type="button" data-act="v4-acc" data-id="nav-favs" aria-expanded="${favsOpen}" aria-controls="nav-sub-favs" aria-label="${favsOpen ? "Collapse" : "Expand"} favorite projects">${ic("chev-right")}</button>
      </div>
      ${favsOpen ? `<ul class="nav-list" id="nav-sub-favs" role="list" aria-label="Favorite projects">${rows}</ul>` : ""}
    </li>`;
}

function navTeamsGroupHtml(r0, r1, teamsOpen) {
  const teams = Object.keys(TEAMS);
  const inTeamChild = r0 === "teams" && own(TEAMS, r1);
  const inTeams = r0 === "teams" || r0 === "members";
  const subIcon = (href, label, icon, cur) => `<li><a class="nav-item" href="${href}" title="${esc(label)}" ${cur ? 'aria-current="page"' : ""}>${ic(icon, "")}<span class="t">${esc(label)}</span></a></li>`;
  const more = teams.length > NAV_CAP ? `<li><a class="nav-item more" href="#/teams"><span class="t">View all teams</span></a></li>` : "";
  const list = teamsOpen ? `<div class="subwrap" id="nav-sub-teams" data-fold="teams"><ul class="nav-sub" role="list" aria-label="Teams">${teams.slice(0, NAV_CAP).map((t) => subIcon("#/teams/" + t, TEAMS[t].name, TEAM_ICON[t] || "user-group", r0 === "teams" && r1 === t)).join("")}${more}</ul></div>` : "";
  return `<li>
          <div class="nav-row">
            <a class="nav-item${!teamsOpen && inTeamChild ? " cur" : ""}" href="#/teams" title="Teams" ${inTeams && !inTeamChild ? 'aria-current="page"' : ""}>${ic("user-group")}<span class="lbl">Teams</span></a>
            <button class="nav-fold-btn" type="button" data-act="v4-acc" data-id="nav-teams" aria-expanded="${!!teamsOpen}" aria-controls="nav-sub-teams" aria-label="${teamsOpen ? "Collapse" : "Expand"} teams">${ic("chev-right")}</button>
          </div>
          ${list}
        </li>`;
}

/* Workspace group is organizable: pinned items show in the list (registry order), the rest sit under
   "More". Hover an item to pin or unpin it; the choice is saved per browser (S.navPinned). */
const WORKSPACE_ITEMS = [
  ["work", "#/work", "Work", "clipboard"],
  ["chat", "#/chat", "Chat", "chat"],
  ["activity", "#/activity", "Activity", "signal"],
  ["insights", "#/insights", "Insights", "chart", " for the organization"],
  ["projects", "#/projects", "Projects", "folder"],
  ["calendar", "#/calendar", "Calendar", "calendar-check"],
];
function workspaceItemHtml([key, href, label, icon, srSuffix = ""], cur, pinned) {
  const drag = ` draggable="true" data-ws-key="${key}" data-ws-zone="${pinned ? "pinned" : "more"}"`;
  const pin = `<button class="nav-pin" type="button" data-act="nav-pin" data-v="${key}" aria-label="${pinned ? "Move" : "Pin"} ${esc(label)} ${pinned ? "to More" : "to the sidebar"}" title="${pinned ? "Move to More" : "Pin to sidebar"}">${ic(pinned ? "minimize" : "plus", "ic14")}</button>`;
  return `<li class="nav-org-item"${drag}><a class="nav-item" href="${href}" title="${esc(label)} (drag or Alt+Up/Down to reorder)" ${cur ? 'aria-current="page"' : ""}>${ic(icon)}<span class="lbl">${esc(label)}</span>${srSuffix ? `<span class="sr-only">${esc(srSuffix)}</span>` : ""}</a>${pin}</li>`;
}
function workspaceNavHtml(r0, r1) {
  const isCur = (key) => r0 === key && (key !== "projects" || !r1);
  const byKey = Object.fromEntries(WORKSPACE_ITEMS.map((it) => [it[0], it]));
  const pinned = S.navWorkspace.map((k) => byKey[k]).filter(Boolean), extra = WORKSPACE_ITEMS.filter(([k]) => !S.navWorkspace.includes(k));
  if (!extra.length) return pinned.map((it) => workspaceItemHtml(it, isCur(it[0]), true)).join("");
  const open = S.v4Open["nav-more"] ?? extra.some(([k]) => isCur(k));
  const moreBtn = `<li><button class="nav-item nav-more-btn" type="button" data-act="v4-acc" data-id="nav-more" aria-expanded="${!!open}" aria-controls="nav-more-list" title="More">${ic("ellipsis")}<span class="lbl">More</span>${ic("chev-down", "ic14 nav-more-chev")}</button></li>`;
  const list = open ? `<li><ul class="nav-list nav-more-list" id="nav-more-list" role="list" aria-label="More workspace items" data-ws-zone="more">${extra.map((it) => workspaceItemHtml(it, isCur(it[0]), false)).join("")}</ul></li>` : "";
  return pinned.map((it) => workspaceItemHtml(it, isCur(it[0]), true)).join("") + moreBtn + list;
}

/* Drag to reorder pinned items; drag between the list and More to pin or unpin. Alt+Up/Down on a
   focused item moves it. Bound once on #nav, since the nav's contents re-render. */
function moveWorkspaceItem(key, beforeKey, zone) {
  const list = S.navWorkspace.filter((k) => k !== key);
  if (zone !== "more") list.splice(beforeKey ? Math.max(0, list.indexOf(beforeKey)) : list.length, 0, key);
  if (!list.length) return;
  S.navWorkspace = list; saveNavWorkspace(); render(false);
  document.querySelector(`.nav-org-item[data-ws-key="${CSS.escape(key)}"] > .nav-item`)?.focus({ preventScroll: true });
}
let wsDrag = null;
function dropTarget(e) {
  const li = e.target.closest?.(".nav-org-item"), zone = e.target.closest?.("[data-ws-zone]")?.dataset.wsZone;
  if (!li) return { zone: zone || "pinned", before: null };
  const r = li.getBoundingClientRect(), after = e.clientY > r.top + r.height / 2;
  if (li.dataset.wsZone === "more") return { zone: "more", before: null, li, after };
  const next = after ? li.nextElementSibling?.dataset.wsKey ?? null : li.dataset.wsKey;
  return { zone: "pinned", before: next, li, after };
}
function clearDropMarks(nav) { nav.querySelectorAll(".drop-before, .drop-after").forEach((el) => el.classList.remove("drop-before", "drop-after")); }
function onWsDragStart(e) {
  const li = e.target.closest?.(".nav-org-item");
  if (!li) return;
  wsDrag = li.dataset.wsKey;
  li.classList.add("is-dragging");
  e.dataTransfer.effectAllowed = "move";
  e.dataTransfer.setData("text/plain", wsDrag);
}
function onWsDragOver(nav, e) {
  if (!wsDrag) return;
  e.preventDefault();
  clearDropMarks(nav);
  const t = dropTarget(e);
  t.li?.classList.add(t.after ? "drop-after" : "drop-before");
}
function onWsDrop(nav, e) {
  if (!wsDrag) return;
  e.preventDefault();
  const t = dropTarget(e), key = wsDrag;
  wsDrag = null;
  clearDropMarks(nav);
  moveWorkspaceItem(key, t.before === key ? null : t.before, t.zone);
}
function onWsDragEnd(nav) {
  wsDrag = null;
  clearDropMarks(nav);
  nav.querySelectorAll(".is-dragging").forEach((el) => el.classList.remove("is-dragging"));
}
function onWsKeydown(e) {
  const li = e.target.closest?.('.nav-org-item[data-ws-zone="pinned"]');
  const dir = { ArrowUp: -1, ArrowDown: 1 }[e.key];
  if (!li || !e.altKey || !dir) return;
  e.preventDefault();
  const list = S.navWorkspace, key = li.dataset.wsKey, j = list.indexOf(key) + dir;
  if (j < 0 || j >= list.length) return;
  moveWorkspaceItem(key, dir < 0 ? list[j] : list[j + 1] ?? null, "pinned");
}
function bindWorkspaceDrag(nav) {
  if (nav.dataset.wsBound) return;
  nav.dataset.wsBound = "1";
  nav.addEventListener("dragstart", onWsDragStart);
  nav.addEventListener("dragover", (e) => onWsDragOver(nav, e));
  nav.addEventListener("drop", (e) => onWsDrop(nav, e));
  nav.addEventListener("dragend", () => onWsDragEnd(nav));
  nav.addEventListener("keydown", onWsKeydown);
}

function renderNav(route) {
  const [r0, r1] = route;
  const homeEmpty = r0 === "home" && S.homeState === "empty";
  const homeError = r0 === "home" && S.homeState === "error";
  const badge = (homeEmpty || homeError) ? 0 : unhandled().length + unreadNotificationsCount() + getRunningJobs().length;
  syncNavSearchChrome(homeError);
  syncOfficeNavStatus(homeError);
  const item = (href, label, icon, cur, extra = "") => `<li><a class="nav-item" href="${href}" title="${esc(label)}" ${cur ? 'aria-current="page"' : ""}>${ic(icon)}<span class="lbl">${esc(label)}</span>${extra}</a></li>`;
  const favs = homeEmpty ? [] : S.favProjs.filter((p) => P[p] && !P[p].archived);
  const inTeamChild = r0 === "teams" && own(TEAMS, r1);
  const favsOpen = S.v4Open["nav-favs"] !== false;
  const teamsOpen = S.v4Open["nav-teams"] ?? inTeamChild;
  const nav = document.getElementById("nav"); if (!nav) return;
  bindWorkspaceDrag(nav);
  nav.innerHTML = `<ul class="nav-scroll" role="list">
    <li>
      <div class="nav-label">Focus</div>
      <ul class="nav-list" role="list">
        ${item("#/home", "Home", "home", r0 === "home")}
        ${item("#/inbox", "Inbox", "inbox", r0 === "inbox", badge ? `<span class="nav-dot" aria-hidden="true"></span><span class="sr-only">, ${badge} waiting on you</span>` : "")}
      </ul>
    </li>
    <li>
      <div class="nav-label">Workspace</div>
      <ul class="nav-list" role="list">
        ${workspaceNavHtml(r0, r1)}
      </ul>
    </li>
    ${navFavoritesHtml(favs, r0, r1, homeError, favsOpen)}
    <li>
      <div class="nav-label">Organization</div>
      <ul class="nav-list" role="list">
        ${navTeamsGroupHtml(r0, r1, teamsOpen)}
        ${item("#/settings", "Settings", "cog", r0 === "settings" || r0 === "preferences")}
      </ul>
    </li>
  </ul>`;
  fitNav();
}
function fitNav() {
  const nav = document.getElementById("nav"), list = nav && nav.querySelector(".nav-scroll"); if (!list) return;
  const side = document.getElementById("sidebar");
  nav.classList.remove("fold-teams", "fold-projects"); if (side) side.classList.remove("tight");
  nav.querySelectorAll(".nav-item.cur").forEach((a) => a.classList.remove("cur"));
  const over = () => list.scrollHeight > list.clientHeight + 1;
  for (const k of ["teams", "projects"]) {
    if (!over()) break;
    nav.classList.add("fold-" + k);
    const w = nav.querySelector(`.subwrap[data-fold="${k}"]`), parent = w && w.previousElementSibling && w.previousElementSibling.querySelector(".nav-item");
    if (parent && w.querySelector('[aria-current="page"]')) parent.classList.add("cur");
  }
  if (over() && side) side.classList.add("tight");
}
let fitRaf = 0;
window.addEventListener("resize", () => { cancelAnimationFrame(fitRaf); fitRaf = requestAnimationFrame(fitNav); });
window.addEventListener("resize", () => fitTextarea(document.getElementById("compose")));
function renderBanner(r0) {
  const el = document.getElementById("banner-ctl"); if (!el) return;
  const themeCtl = seg("theme", THEME_OPTIONS, "Theme");
  const fontOpts = FONT_OPTIONS.map(([v, label]) => `<option value="${esc(v)}"${S.fontStyle === v ? " selected" : ""}>${esc(label)}</option>`).join("");
  const fontCtl = `<label class="banner-font-wrap"><span>Font</span><select class="banner-select" id="banner-font-select" data-font-select aria-label="Typography">${fontOpts}</select></label>`;
  const homeCtl = r0 === "home" ? seg("homeState", [["filled", "Filled"], ["empty", "Empty"], ["loading", "Loading"], ["error", "Error"]], "Home sections") : "";
  el.innerHTML = themeCtl + fontCtl + homeCtl;
}

let stopHomeProjectHint;
function mountHomeProjectHint() {
  stopHomeProjectHint?.();
  stopHomeProjectHint = null;
  const hint = document.getElementById("home-scroll-hint");
  hint.hidden = true;
  if (S.homeState !== "filled") return;

  const projects = document.getElementById("hb-projects");
  if (!projects) return;

  const content = document.getElementById("content");
  const wrapper = projects.parentElement;
  const stage = wrapper.parentElement.querySelector(".hb-stage");
  const stretchStage = !S.theme.startsWith("report");
  const sideBySide = window.matchMedia("(min-width: 1121px)");
  let projectsBelowFold = false;
  const updateHint = () => {
    hint.hidden = !projectsBelowFold || content.scrollTop > 0;
  };
  const projectsTop = () =>
    wrapper.getBoundingClientRect().top - content.getBoundingClientRect().top + content.scrollTop;
  const fitProjects = () => {
    if (stretchStage) stage.style.minHeight = "";
    const height = document.body.clientHeight;
    const naturalTop = projectsTop();
    projectsBelowFold = naturalTop + projects.getBoundingClientRect().height > height;
    if (projectsBelowFold && stretchStage && sideBySide.matches) {
      hint.hidden = false;
      const availableSpace = content.clientHeight - naturalTop;
      if (availableSpace > 0) {
        stage.style.minHeight = `${stage.getBoundingClientRect().height + availableSpace}px`;
      }
    }
    const padding = projectsBelowFold ? Math.max(0, Math.ceil(height - projectsTop())) : 0;
    wrapper.style.paddingBlockStart = padding ? `${padding}px` : "";
    updateHint();
  };
  fitProjects();
  let resizeFrame = 0;
  const observer = new ResizeObserver(() => {
    cancelAnimationFrame(resizeFrame);
    resizeFrame = requestAnimationFrame(fitProjects);
  });
  [document.body, projects, ...wrapper.parentElement.querySelectorAll(".hb-hero, .hb-pillars, .hb-stage")]
    .forEach((element) => observer.observe(element));
  content.addEventListener("scroll", updateHint, { passive: true });
  stopHomeProjectHint = () => {
    cancelAnimationFrame(resizeFrame);
    observer.disconnect();
    content.removeEventListener("scroll", updateHint);
  };
}

/* Auto-hide prototype top banner unless mouse hovers near top edge or banner */
function bannerFocusLocksOpen(banner) {
  const active = document.activeElement;
  return !!(active && banner.contains(active) && (active.tagName === "SELECT" || active.matches(":focus-visible")));
}
function blurBannerButton(banner) {
  const active = document.activeElement;
  if (active && banner.contains(active) && active.tagName === "BUTTON") active.blur();
}
function handleBannerMouseMove(e, banner, ctl) {
  if (!document.body.contains(banner)) return;
  if (e.clientY <= 14 && e.clientX >= banner.getBoundingClientRect().left) { ctl.overZone = true; ctl.show(); }
  else if (ctl.overZone && e.clientY > 18) { ctl.overZone = false; ctl.scheduleHide(); }
}
function handleBannerEscape(e, banner, ctl) {
  if (e.key !== "Escape" || !banner.classList.contains("is-open")) return;
  ctl.overBanner = false; ctl.overZone = false;
  if (document.activeElement && banner.contains(document.activeElement)) document.activeElement.blur();
  ctl.setOpen(false);
}
function initBannerAutoHide() {
  const banner = document.getElementById("banner"), hotzone = document.getElementById("banner-hotzone");
  if (!banner || !hotzone) return;
  const ctl = { hideTimer: null, overBanner: false, overZone: false };
  ctl.setOpen = (open) => { banner.classList.toggle("is-open", open); hotzone.classList.toggle("is-hidden", open); };
  const keepOpen = () => !document.body.contains(banner) || ctl.overBanner || ctl.overZone || bannerFocusLocksOpen(banner);
  ctl.show = () => { clearTimeout(ctl.hideTimer); if (document.body.contains(banner)) ctl.setOpen(true); };
  ctl.scheduleHide = (delay = 240) => { clearTimeout(ctl.hideTimer); ctl.hideTimer = setTimeout(() => { if (!keepOpen()) ctl.setOpen(false); }, delay); };
  hotzone.addEventListener("pointerenter", () => { ctl.overZone = true; ctl.show(); });
  hotzone.addEventListener("pointerleave", () => { ctl.overZone = false; ctl.scheduleHide(); });
  hotzone.addEventListener("click", ctl.show);
  banner.addEventListener("pointerenter", () => { ctl.overBanner = true; ctl.show(); });
  banner.addEventListener("pointerleave", () => { ctl.overBanner = false; blurBannerButton(banner); ctl.scheduleHide(); });
  banner.addEventListener("focusin", (e) => { if (e.target?.tagName === "SELECT" || e.target?.matches(":focus-visible")) ctl.show(); });
  banner.addEventListener("focusout", () => ctl.scheduleHide(160));
  document.addEventListener("mousemove", (e) => handleBannerMouseMove(e, banner, ctl), { passive: true });
  document.addEventListener("keydown", (e) => handleBannerEscape(e, banner, ctl));
}
initBannerAutoHide();

/* Hover-expanding desktop icon-rail sidebar (pushes page smoothly) */
function isDesktopSidebar() {
  return !window.matchMedia || window.matchMedia("(min-width: 960px)").matches;
}
function blurSidebarLink(sidebar) {
  const active = document.activeElement;
  if (!active || !sidebar.contains(active) || sidebarHasOpenMenu()) return;
  if (active.tagName === "A" || active.tagName === "BUTTON") active.blur();
}
function initSidebarHoverExpand() {
  const sidebar = document.getElementById("sidebar");
  if (!sidebar) return;
  let leaveTimer = null;
  const scheduleCollapse = (delay = 240) => { clearTimeout(leaveTimer); leaveTimer = setTimeout(() => { if (isDesktopSidebar()) applyNavCollapsed(); }, delay); };
  sidebar.addEventListener("pointerenter", () => { if (!isDesktopSidebar()) return; clearTimeout(leaveTimer); S.navHover = true; applyNavCollapsed(); });
  sidebar.addEventListener("pointerleave", () => { if (!isDesktopSidebar()) return; S.navHover = false; blurSidebarLink(sidebar); scheduleCollapse(240); });
  sidebar.addEventListener("focusin", () => { if (!isDesktopSidebar()) return; clearTimeout(leaveTimer); applyNavCollapsed(); });
  sidebar.addEventListener("focusout", () => { if (isDesktopSidebar()) scheduleCollapse(160); });
  document.addEventListener("click", (e) => { if (isDesktopSidebar() && !sidebar.contains(e.target) && !S.navHover) scheduleCollapse(80); });
}
initSidebarHoverExpand();

export {
  mountHomeProjectHint,
  renderBanner,
  renderNav,
};
