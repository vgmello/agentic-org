import { CH, M, P, TEAMS } from "../data.js";
import { activityPanelResize, pageActivity, projectActivityId, refreshActivityPanel, syncAssemblySimulationTimer, updateActivityPanelScroll } from "../pages/activity.js";
import { mountAgentProfile, stopAgentOrb } from "../pages/agent-profile.js";
import { pageChat } from "../pages/chat.js";
import { pageHome } from "../pages/home.js";
import { pageInbox } from "../pages/inbox.js";
import { pageProject, pageProjects } from "../pages/projects.js";
import { SET, notFound, pagePreferences, pageSearch, pageSettings } from "../pages/settings.js";
import { pageMember, pageMembersDir, pageOrgChart, pageTeam, pageTeams } from "../pages/teams.js";
import { fitTextarea, pageWork, scopedWorkQueries, scopedWorkType, setWorkPanelExpanded, showSidePanel } from "../pages/work.js";
import { isMacPlatform } from "./cmdk.js";
import { endNavigationKeys } from "./keyboard.js";
import { mountHomeProjectHint, renderBanner, renderNav } from "./sidebar.js";
import { syncTopbarUtils } from "./topbar-utils.js";
import { S, applyShortcuts, qs } from "../state.js";
import { esc, ic } from "../ui.js";

/* ---------------------------------------------------------------- router */
function route() { const h = location.hash.replace(/^#\/?/, "").split("?")[0] || "home"; return h.split("/").filter(Boolean).map((s) => { try { return decodeURIComponent(s); } catch (e) { return s; } }); }
const own = (o, k) => k != null && Object.prototype.hasOwnProperty.call(o, k);
const TITLES = { home: "Home", inbox: "Inbox", projects: "Projects", work: "Work", chat: "Chat", activity: "Activity", teams: "Teams", members: "Member", settings: "Organization settings", preferences: "Your preferences", search: "Search" };
const MEMBER_ALIASES = ["members", "people", "everyone"];
const homeToolbar = document.getElementById("home-toolbar");
const drawerButton = homeToolbar.querySelector(".menu-btn");
const commandBar = document.getElementById("cmdk-shell");
const topbarUtils = document.getElementById("topbar-utils");
const pageSearchTools = document.createElement("div");
pageSearchTools.id = "page-search-tools";
const localSearchHost = document.getElementById("cmdk-local");
const homeSearchField = document.getElementById("cmdk-home-field");
const homeSearchInput = document.getElementById("cmdk-home-input");
const homeSearchResults = document.getElementById("cmdk-home-results");
const PAGE_SEARCH = {
  inbox: { id: "triage-search", value: () => S.inboxQuery, label: "Search inbox", placeholder: "Search inbox…" },
  work: { id: "work-search", value: () => S.workQuery, label: "Search this work view", placeholder: "Search work…" }
};
function resolvePageSearchConfig(currentRoute) {
  const [page, id] = currentRoute;
  const scope = scopedWorkType(currentRoute);
  if (scope) {
    return {
      id: "scoped-work-search",
      value: () => scopedWorkQueries.get(scope + ":" + id) ?? "",
      label: "Search work in this view",
      placeholder: scope === "agent" ? "Search tasks…" : "Search work…",
    };
  }
  return own(PAGE_SEARCH, page) ? PAGE_SEARCH[page] : null;
}

function attachToolbarHost(main, heading) {
  if (heading) {
    const backLink = main.firstElementChild;
    if (backLink?.matches("a.back")) heading.prepend(backLink);
    heading.classList.add("has-page-toolbar");
    heading.prepend(drawerButton);
    heading.append(pageSearchTools);
    pageSearchTools.append(commandBar, topbarUtils);
  } else if (commandBar.parentElement !== homeToolbar || topbarUtils.parentElement !== homeToolbar) {
    homeToolbar.append(drawerButton, commandBar, topbarUtils);
  }
  homeToolbar.hidden = Boolean(heading);
}

function syncHomeSearchField(isHome, workspaceSearch) {
  homeSearchField.hidden = !isHome && !workspaceSearch;
  if (location.hash !== S.lastRoute) homeSearchInput.value = "";
  homeSearchInput.setAttribute("aria-label", workspaceSearch ? "Search across the workspace" : "Search, run a command, or ask Tess");
  const fieldShortcut = document.getElementById("cmdk-home-kbd");
  fieldShortcut.textContent = isHome ? (isMacPlatform ? "⌘K" : "Ctrl K") : "/";
  fieldShortcut.hidden = workspaceSearch && !S.singleKey;
  if (workspaceSearch && S.singleKey) homeSearchInput.setAttribute("aria-keyshortcuts", "/");
  else homeSearchInput.removeAttribute("aria-keyshortcuts");
}

function placeSearchToolbar(main, currentRoute) {
  const [page] = currentRoute;
  const heading = page === "home" ? null : main.querySelector("h1")?.closest(".ph, .prof, .chhead");
  const search = resolvePageSearchConfig(currentRoute);
  attachToolbarHost(main, heading);
  const isHome = page === "home", workspaceSearch = Boolean(heading && !search);
  syncHomeSearchField(isHome, workspaceSearch);
  document.getElementById("cmdk-trigger").hidden = isHome;
  localSearchHost.hidden = !search;
  const showShortcut = Boolean(search) && S.singleKey;
  localSearchHost.innerHTML = search
    ? `<label>${ic("search", "ic16")}<span class="sr-only">${search.label}</span><input id="${search.id}" type="search" value="${esc(search.value())}" placeholder="${search.placeholder}" autocomplete="off"${showShortcut ? ' aria-keyshortcuts="/"' : ""}>${showShortcut ? '<kbd aria-hidden="true">/</kbd>' : ""}</label>`
    : "";
}
const pageSearchInput = () => localSearchHost.querySelector("input")
  || (pageSearchTools.contains(homeSearchField) && !homeSearchField.hidden ? homeSearchInput : null);

function normalizeRouteRedirects() {
  if (/^#\/?members\/you(\/|$)/.test(location.hash)) {
    try { history.replaceState(null, "", "#/home"); } catch (e) { location.hash = "#/home"; return null; }
  }
  if (/^#\/?console(\/|$)/.test(location.hash)) {
    const cr = route();
    const aid = cr[1] && own(M, cr[1]) && M[cr[1]].kind === "agent" ? cr[1] : S.consoleAgent;
    const target = `#/members/${aid}/sessions`;
    try { history.replaceState(null, "", target); } catch (e) { location.hash = target; return null; }
  }
  let r = route();
  if (r[0] === "teams" && own(TEAMS, r[1]) && !r[2] && qs().has("item")) {
    const target = `#/teams/${encodeURIComponent(r[1])}/work?${qs()}`;
    try { history.replaceState(null, "", target); } catch (e) { location.hash = target; return null; }
    r = route();
  }
  return r;
}

const ROUTE_RENDERERS = {
  home: () => pageHome(),
  inbox: (r1) => pageInbox(r1),
  projects: (r1, r2) => (!r1 ? pageProjects() : own(P, r1) ? pageProject(r1, r2) : notFound()),
  work: () => pageWork(),
  chat: (r1) => pageChat(own(CH, r1) ? r1 : null),
  activity: () => pageActivity(),
  teams: (r1, r2) => (MEMBER_ALIASES.includes(r1) ? pageMembersDir() : r1 === "org-chart" ? pageOrgChart() : !r1 ? pageTeams() : own(TEAMS, r1) ? pageTeam(r1, r2) : notFound()),
  members: (r1, r2) => (own(M, r1) ? pageMember(r1, r2) : notFound()),
  settings: (r1, r2) => (!r1 || SET.some(([k]) => k === r1) ? pageSettings(r1, own(M, r2) ? r2 : null) : notFound()),
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
  const workLink = sameWorkPage && priorPanel ? main.querySelector(`#work-results .work-item-link[data-item="${CSS.escape(priorPanel.dataset.item)}"]`) : null;
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
    if (keep?.matches(".work-item-link") && !sameWorkPage) keep.scrollIntoView({ block: "nearest", inline: "nearest" });
    return;
  }
  if (!toolbarFocus) return;
  const control = document.getElementById(toolbarFocus);
  control?.focus({ preventScroll: true });
  if (toolbarCaret) control?.setSelectionRange(...toolbarCaret);
}

function captureBeforeRender(main, content) {
  const priorBoard = main.querySelector("#work-results .board");
  const toolbarFocus = pageSearchTools.contains(document.activeElement) ? document.activeElement.id : "";
  const toolbarCaret = toolbarFocus && document.activeElement.type === "search" ? [document.activeElement.selectionStart, document.activeElement.selectionEnd] : null;
  return {
    priorPanel: main.querySelector("#work-panel"),
    priorActivity: main.querySelector(".activity-main"),
    panelScroll: document.getElementById("activity-panel-scroll")?.scrollTop ?? 0,
    pageScroll: content.scrollTop,
    workBoardScroll: priorBoard && { left: priorBoard.scrollLeft, top: priorBoard.scrollTop },
    toolbarFocus,
    toolbarCaret,
  };
}

function syncActivityScopeState(r0, projectId) {
  const activityScope = r0 === "activity" ? "portfolio" : projectId;
  const sameActivity = activityScope && document.querySelector(".activity-workspace")?.dataset.activityScope === activityScope;
  if (activityScope && !sameActivity) {
    S.actPanel = "release";
    if (projectId) { S.projectActView = "assembly"; S.projectActFilter = "all"; }
  }
  return sameActivity;
}

function render(focus = true) {
  endNavigationKeys();
  const r = normalizeRouteRedirects();
  if (!r) return;
  stopAgentOrb();
  const [r0, r1, r2] = r, projectId = projectActivityId(r);
  const sameActivity = syncActivityScopeState(r0, projectId);
  document.documentElement.toggleAttribute("data-interior-page", r0 !== "home");
  const main = document.getElementById("main"), content = document.getElementById("content"), key = location.hash;
  const snap = captureBeforeRender(main, content);
  main.innerHTML = (ROUTE_RENDERERS[r0] || notFound)(r1, r2);
  const workPanel = main.querySelector("#work-panel");
  const sameWorkPage = (snap.priorPanel || workPanel) && S.lastRoute?.split("?")[0] === key.split("?")[0];
  const shell = document.getElementById("agent-shell");
  if (shell) mountAgentProfile(shell.dataset.agent, shell.dataset.tab);
  placeSearchToolbar(main, r);
  if (S.singleKey) main.querySelector("[data-page-menu]")?.setAttribute("aria-keyshortcuts", "M");
  scrollCurrentTabsIntoView(main);
  renderNav(r);
  renderBanner(r0);
  mountActivityAfterRender(main, sameActivity, snap.priorActivity, snap.panelScroll, projectId);
  document.title = (r0 === "members" && own(M, r1) ? M[r1].name : (TITLES[r0] || "Not found")) + " · Agentic Org";
  if (sameWorkPage) {
    content.scrollTop = snap.pageScroll;
    if (snap.workBoardScroll) main.querySelector("#work-results .board")?.scrollTo(snap.workBoardScroll);
  }
  restoreFocusAfterRender({ main, content, r0, r1, key, focus, workPanel, priorPanel: snap.priorPanel, sameWorkPage, toolbarFocus: snap.toolbarFocus, toolbarCaret: snap.toolbarCaret });
  S.lastRoute = key; S.focusAfter = null;
  mountHomeProjectHint();
  fitTextarea(document.getElementById("work-comment"));
  fitTextarea(document.getElementById("compose"));
  applyShortcuts();
  syncTopbarUtils(r);
  syncAssemblySimulationTimer();
}

export {
  homeSearchField,
  homeSearchInput,
  homeSearchResults,
  own,
  pageSearchInput,
  pageSearchTools,
  render,
  route,
};
