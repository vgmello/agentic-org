import { M, P, TEAMS } from "../data.js";
import { activityPanelResize, pageActivity, projectActivityId, refreshActivityPanel, syncAssemblySimulationTimer, updateActivityPanelScroll } from "../pages/activity.js";
import { a2aRedirect, mountAgentConversations } from "../pages/agent-conversations.js";
import { mountAgentProfile, stopAgentOrb } from "../pages/agent-profile.js";
import { mountOrgChart } from "../pages/org-chart.js";
import { pageCalendar } from "../pages/calendar-page.js";
import { chatDocTitle, mountChat, pageChat } from "../pages/chat.js";
import { pageHome } from "../pages/home.js";
import { pageInbox } from "../pages/inbox.js";
import { pageInsights } from "../pages/org-insights.js";
import { mountProjectQueue, pageProject, pageProjects } from "../pages/projects.js";
import { processesTitle } from "../pages/processes.js";
import { SET, notFound, pageOrgProcesses, pagePreferences, pageSearch, pageSettings } from "../pages/settings.js";
import { pageMember, pageMembersDir, pageOrgChart, pageTeam, pageTeams } from "../pages/teams.js";
import { boardPlace, fitTextarea, mountBoard, mountWorkToolbar, pageWork, restoreBoardPlace, setWorkPanelExpanded, settleWorkResults, showSidePanel, workPanelOpener } from "../pages/work.js";
import { adoptBoardReturn, takeBoardReturnPlace } from "./board-return.js";
import { isMacPlatform } from "./cmdk.js";
import { endNavigationKeys } from "./keyboard.js";
import { mountHomeProjectHint, renderBanner, renderNav } from "./sidebar.js";
import { registerNotifChangeListener, syncTopbarUtils } from "./topbar-utils.js";
import { S, applyShortcuts, qs } from "../state.js";
import { esc, ic } from "../ui.js";

/* ---------------------------------------------------------------- router */
function route() { const h = location.hash.replace(/^#\/?/, "").split("?")[0] || "home"; return h.split("/").filter(Boolean).map((s) => { try { return decodeURIComponent(s); } catch (e) { return s; } }); }
const own = (o, k) => k != null && Object.prototype.hasOwnProperty.call(o, k);
const TITLES = { home: "Home", inbox: "Inbox", projects: "Projects", work: "Work", chat: "Chat", activity: "Activity", insights: "Insights", calendar: "Calendar", teams: "Teams", members: "Member", settings: "Organization settings", preferences: "Your preferences", search: "Search" };
const MEMBER_ALIASES = ["members", "people", "everyone"];
const homeToolbar = document.getElementById("home-toolbar");
const drawerButton = homeToolbar.querySelector(".menu-btn");
const commandBar = document.getElementById("cmdk-shell");
const pageSearchTools = document.createElement("div");
pageSearchTools.id = "page-search-tools";
const localSearchHost = document.getElementById("cmdk-local");
/* Header search box per page. Inbox and Work now use an inline search row (ui.js searchToggle/searchRow). */
const PAGE_SEARCH = {};
function resolvePageSearchConfig(currentRoute) {
  const [page] = currentRoute;
  return own(PAGE_SEARCH, page) ? PAGE_SEARCH[page] : null;
}

function attachToolbarHost(main, heading, hasLocalSearch) {
  if (heading) {
    const backLink = main.querySelector(":scope > .back, :scope > .settings > .back");
    if (backLink) heading.prepend(backLink);
    heading.classList.toggle("has-page-toolbar", hasLocalSearch);
    heading.classList.toggle("has-page-nav", !hasLocalSearch);
    heading.prepend(drawerButton);
    if (hasLocalSearch) {
      heading.append(pageSearchTools);
      pageSearchTools.append(commandBar);
    } else if (commandBar.parentElement !== homeToolbar) {
      homeToolbar.append(commandBar);
    }
  } else {
    if (drawerButton.parentElement !== homeToolbar) homeToolbar.prepend(drawerButton);
    if (commandBar.parentElement !== homeToolbar) homeToolbar.append(commandBar);
  }
  homeToolbar.hidden = Boolean(heading);
}

function placeSearchToolbar(main, currentRoute) {
  const [page] = currentRoute;
  const heading = page === "home" ? null : main.querySelector("h1")?.closest(".ph, .prof, .chhead");
  const search = resolvePageSearchConfig(currentRoute);
  const hasLocalSearch = Boolean(heading && search);
  attachToolbarHost(main, heading, hasLocalSearch);
  commandBar.hidden = !hasLocalSearch;
  localSearchHost.hidden = !hasLocalSearch;
  const showShortcut = hasLocalSearch && S.singleKey;
  localSearchHost.innerHTML = hasLocalSearch
    ? `<label>${ic("search", "ic16")}<span class="sr-only">${search.label}</span><input id="${search.id}" type="search" value="${esc(search.value())}" placeholder="${search.placeholder}" autocomplete="off"${showShortcut ? ' aria-keyshortcuts="/"' : ""}>${showShortcut ? '<kbd aria-hidden="true">/</kbd>' : ""}</label>`
    : "";
}
const pageSearchInput = () => (localSearchHost.hidden ? null : localSearchHost.querySelector("input"));

/* Old or shorthand addresses, each replaced in place (in order) by the address that now shows it. */
function consoleRedirect() {
  if (!/^#\/?console(\/|$)/.test(location.hash)) return null;
  const cr = route();
  const aid = cr[1] && own(M, cr[1]) && M[cr[1]].kind === "agent" ? cr[1] : S.consoleAgent;
  return `#/members/${aid}/sessions`;
}
function teamItemRedirect(r) {
  return r[0] === "teams" && own(TEAMS, r[1]) && !r[2] && qs().has("item") ? `#/teams/${encodeURIComponent(r[1])}/work?${qs()}` : null;
}
const REDIRECTS = [
  () => (/^#\/?members\/you(\/|$)/.test(location.hash) ? "#/home" : null),
  consoleRedirect,
  /* Agents' conversations are read in an agent's page; old Chat links to them land there. */
  () => a2aRedirect(route(), S.lastRoute),
  () => teamItemRedirect(route()),
];
function replaceHash(target) {
  try { history.replaceState(null, "", target); return true; } catch (e) { location.hash = target; return false; }
}
function normalizeRouteRedirects() {
  for (const rule of REDIRECTS) {
    const target = rule();
    if (target && !replaceHash(target)) return null;
  }
  return route();
}

const ROUTE_RENDERERS = {
  home: () => pageHome(),
  inbox: (r1, r2) => pageInbox(r1, r2),
  projects: (r1, r2, r3) => (!r1 ? pageProjects() : own(P, r1) ? pageProject(r1, r2, r3) : notFound()),
  work: () => pageWork(),
  chat: (r1) => pageChat(r1), /* Chat resolves unknown ids; dm-<member> opens or creates that DM */
  activity: () => pageActivity(),
  insights: () => pageInsights(),
  calendar: () => pageCalendar(),
  teams: (r1, r2) => (MEMBER_ALIASES.includes(r1) ? pageMembersDir() : r1 === "org-chart" || !r1 ? pageOrgChart() : r1 === "all" ? pageTeams() : own(TEAMS, r1) ? pageTeam(r1, r2) : notFound()),
  members: (r1, r2, r3) => (own(M, r1) ? pageMember(r1, r2, r3) : notFound()),
  settings: (r1, r2) => (r1 === "processes" || r1 === "dev-processes" ? pageOrgProcesses(r1, r2) : !r1 || SET.some(([k]) => k === r1) ? pageSettings(r1, own(M, r2) ? r2 : null) : notFound()),
  processes: (r1) => pageOrgProcesses("processes", r1),
  "dev-processes": () => pageOrgProcesses("dev-processes"),
  preferences: () => pagePreferences(),
  search: () => pageSearch(qs().get("q") || ""),
};

function scrollCurrentTabsIntoView(main) {
  const sel = ".tabs > [aria-current=page], .primary-tab-row > .queue-tabs > :is([aria-pressed=true], [aria-selected=true])";
  for (const current of main.querySelectorAll(sel)) {
    const tabs = current.parentElement;
    const { left, right } = current.getBoundingClientRect();
    const bounds = tabs.getBoundingClientRect();
    if (left < bounds.left) tabs.scrollLeft += left - bounds.left;
    if (right > bounds.right) tabs.scrollLeft += right - bounds.right;
  }
}

function mountActivityAfterRender(main, sameActivity, priorActivity, panelScroll, projectId) {
  const activity = main.querySelector(".activity-main");
  if (sameActivity && activity && priorActivity?.dataset.view === activity.dataset.view) activity.scrollTop = priorActivity.scrollTop;
  activityPanelResize.disconnect();
  if (activity && S.actPanel) {
    refreshActivityPanel();
    if (sameActivity) document.getElementById("activity-panel-scroll").scrollTop = panelScroll;
  }
  if (!projectId) return;
  const scroll = document.getElementById("activity-panel-scroll");
  scroll.addEventListener("scroll", updateActivityPanelScroll, { passive: true });
  activityPanelResize.observe(scroll);
  activityPanelResize.observe(scroll.querySelector(".activity-panel-body"));
}

function resolveRouteFocus(main, r0, r1, priorPanel, sameWorkPage) {
  const requestedFocus = S.focusAfter ? document.querySelector(S.focusAfter) : null;
  const workLink = sameWorkPage && priorPanel ? workPanelOpener(main, priorPanel.dataset.item) : null;
  const keep = [requestedFocus, workLink].find((control) => control?.getClientRects().length);
  const target = keep || (r0 === "inbox" && r1 && main.querySelector("#triage-detail-title")) || main.querySelector("h1");
  return { keep, target };
}

function restoreFocusAfterRender({ main, content, r0, r1, key, focus, workPanel, priorPanel, sameWorkPage, toolbarFocus, toolbarCaret }) {
  if (workPanel) {
    if (sameWorkPage && priorPanel?.hasAttribute("data-expanded")) setWorkPanelExpanded(workPanel, true);
    showSidePanel(workPanel, !priorPanel);
    return;
  }
  if (focus && key !== S.lastRoute) {
    const { keep, target } = resolveRouteFocus(main, r0, r1, priorPanel, sameWorkPage);
    if (!sameWorkPage) content.scrollTop = 0;
    target?.focus({ preventScroll: true });
    if (keep?.matches("[data-item]") && !sameWorkPage) keep.scrollIntoView({ block: "nearest", inline: "nearest" });
    return;
  }
  if (!toolbarFocus) return;
  const control = document.getElementById(toolbarFocus);
  control?.focus({ preventScroll: true });
  if (toolbarCaret) control?.setSelectionRange(...toolbarCaret);
}

function captureBeforeRender(main, content) {
  const priorResults = main.querySelector("#work-results");
  const active = document.activeElement;
  const toolbarFocus = pageSearchTools.contains(active) || active?.classList?.contains("inline-search-input") ? active.id : "";
  const toolbarCaret = toolbarFocus && document.activeElement.type === "search" ? [document.activeElement.selectionStart, document.activeElement.selectionEnd] : null;
  return {
    priorPanel: main.querySelector("#work-panel"),
    priorActivity: main.querySelector(".activity-main"),
    panelScroll: document.getElementById("activity-panel-scroll")?.scrollTop ?? 0,
    pageScroll: content.scrollTop,
    workBoardPlace: priorResults && boardPlace(priorResults),
    toolbarFocus,
    toolbarCaret,
  };
}

function syncActivityScopeState(r0, projectId) {
  const activityScope = r0 === "activity" ? "portfolio" : projectId;
  const sameActivity = activityScope && document.querySelector(".activity-workspace")?.dataset.activityScope === activityScope;
  if (activityScope && !sameActivity) {
    /* The viewer's last choice (open panel, or none) is remembered locally; first visit opens Release gates */
    let saved = null;
    try { saved = localStorage.getItem("ao.activityPanel"); } catch (e) {}
    S.actPanel = saved === null ? "release" : saved || null;
    if (projectId) { S.projectActView = "assembly"; S.projectActFilter = "all"; }
  }
  return sameActivity;
}

function pageTitle(r0, r1) {
  if (r0 === "members" && own(M, r1)) return M[r1].name;
  if (r0 === "processes" || (r0 === "settings" && r1 === "processes")) return processesTitle(); /* "<place> · Processes" (PROC-32) */
  if (r0 === "chat") return chatDocTitle(r1); /* Chat resolves conversations and its Mentions and Threads views */
  return TITLES[r0] || "Not found";
}

/* An in-place re-render (search opened, Display changed, panel opened) keeps the board's lane and scroll */
function restoreBoardInPlace(results, place) {
  if (results && place) restoreBoardPlace(results, place, "");
  settleWorkResults(results);
}

/* Coming back to a board you left (Back, or "Back to work board" in Inbox) returns to its lanes, scroll and control */
function restoreReturnedBoard(results) {
  const back = takeBoardReturnPlace();
  if (back && results) restoreBoardPlace(results, back);
}

function render(focus = true) {
  endNavigationKeys();
  const r = normalizeRouteRedirects();
  if (!r) return;
  stopAgentOrb();
  const [r0, r1, r2] = r, projectId = projectActivityId(r);
  adoptBoardReturn(r0);
  const sameActivity = syncActivityScopeState(r0, projectId);
  document.documentElement.toggleAttribute("data-interior-page", r0 !== "home");
  const main = document.getElementById("main"), content = document.getElementById("content"), key = location.hash;
  const snap = captureBeforeRender(main, content);
  main.innerHTML = (ROUTE_RENDERERS[r0] || notFound)(r1, r2, r[3]);
  const workPanel = main.querySelector("#work-panel");
  const sameWorkPage = (snap.priorPanel || workPanel) && S.lastRoute?.split("?")[0] === key.split("?")[0];
  const shell = document.getElementById("agent-shell");
  if (shell) mountAgentProfile(shell.dataset.agent, shell.dataset.tab);
  mountAgentConversations(main, r);
  mountOrgChart(main);
  mountProjectQueue(main);
  mountBoard(main);
  placeSearchToolbar(main, r);
  mountWorkToolbar();
  if (S.singleKey) main.querySelector("[data-page-menu]")?.setAttribute("aria-keyshortcuts", "M");
  scrollCurrentTabsIntoView(main);
  renderNav(r);
  renderBanner(r0);
  mountActivityAfterRender(main, sameActivity, snap.priorActivity, snap.panelScroll, projectId);
  document.title = pageTitle(r0, r1) + " · Agentic Org";
  if (sameWorkPage) content.scrollTop = snap.pageScroll;
  const results = main.querySelector("#work-results");
  if (sameWorkPage || key === S.lastRoute) restoreBoardInPlace(results, snap.workBoardPlace);
  restoreFocusAfterRender({ main, content, r0, r1, key, focus, workPanel, priorPanel: snap.priorPanel, sameWorkPage, toolbarFocus: snap.toolbarFocus, toolbarCaret: snap.toolbarCaret });
  if (focus && key !== S.lastRoute && !workPanel) restoreReturnedBoard(results);
  S.lastRoute = key; S.focusAfter = null;
  mountHomeProjectHint();
  fitTextarea(document.getElementById("work-comment"));
  fitTextarea(document.getElementById("compose"));
  if (r0 === "chat") mountChat();
  applyShortcuts();
  syncTopbarUtils(r);
  syncAssemblySimulationTimer();
}

registerNotifChangeListener(() => {
  const r = route();
  renderNav(r);
  if (r[0] === "inbox") render(false);
});

export {
  own,
  pageSearchInput,
  pageSearchTools,
  render,
  route,
};
