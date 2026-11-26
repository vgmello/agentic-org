import { CH, M, P } from "../data.js";
import { answerOrgQuestion } from "../pages/home.js";
import { openSmartTaskComposer } from "../pages/task-composer.js";
import { canonicalWorkRef, workHref, workItems } from "../pages/work.js";
import { closeMenus } from "./actions.js";
import { openShortcutsHelp } from "./dialogs.js";
import { render, route } from "./router.js";
import { openTessPanel, unreadNotificationsCount } from "./topbar-utils.js";
import { S, toggleNavPinned } from "../state.js";
import { av, esc, ic, kindWord, leadTxt, memberIds, toast, unhandled } from "../ui.js";

/* ---------------------------------------------------------------- search & command palette */
function conversationResults(query, channelsOnly = false) {
  return Object.entries(CH)
    .filter(([, channel]) => (!channelsOnly || channel.kind === "channel") && channel.name.toLowerCase().includes(query))
    .map(([id, channel]) => ({
      href: `#/chat/${id}`,
      label: channel.kind === "channel" ? "#" + channel.name : channel.name,
      sub: channel.topic || (channel.kind === "dm" ? "Direct message" : "Between agents"),
      icon: channel.kind === "channel" ? "hash" : "eye",
      av: channel.with,
    }));
}

function searchResults(q, max = 5) {
  const t = q.toLowerCase(), workRef = canonicalWorkRef(q);
  const has = (s) => String(s || "").toLowerCase().includes(t);
  return [
    ["Projects", Object.keys(P).filter((p) => has(P[p].name) || has(P[p].goal)).slice(0, max).map((p) => ({ href: "#/projects/" + p, label: P[p].name, sub: P[p].archived ? "Archived" : [P[p].health, leadTxt(p)].filter(Boolean).join(" · "), icon: "folder" }))],
    ["Work", workItems().filter((w) => w.id === workRef || has(w.id) || has(w.title)).slice(0, max).map((w) => ({ href: workHref({ item: w.id }), label: w.id + " " + w.title, sub: P[w.proj].name + " · " + w.status, icon: "clipboard" }))],
    ["Members", memberIds().filter((k) => has(M[k].name) || has(M[k].role)).slice(0, max).map((k) => ({ href: "#/members/" + k, label: M[k].name, sub: M[k].role + " · " + kindWord(k), av: k }))]
  ].filter((g) => g[1].length);
}
const CMDK = { open: false, inline: false, mode: "cmd", q: "", active: 0, flat: [], aiAns: null, aiHistory: [], returnTo: null };
const isMacPlatform = /Mac|iPhone|iPad|iPod/i.test(navigator.platform || navigator.userAgent || "");
{
  document.getElementById("cmdk-kbd").textContent = isMacPlatform ? "⌘K" : "Ctrl K";
  const skbd = document.getElementById("cmdk-switch-kbd");
  if (skbd) skbd.textContent = isMacPlatform ? "⌘/" : "Ctrl /";
}
const CMDK_AI_STARTERS = [
  { q: "What needs me first?", sub: "Urgent approvals & sign-offs" },
  { q: "What's slowing v1.0?", sub: "Blockers & pending decisions" },
  { q: "Next 14 days milestones", sub: "Upcoming deliverables & schedule pace" },
  { q: "Budget, capacity & idle agents", sub: "Monthly spend, AI capacity & idle specialists" },
  { q: "What happened in the last 4 hours?", sub: "Recent work digest & completions" }
];
const CMDK_AI_STARTERS_EMPTY = [
  { q: "How do I launch my first project?", sub: "Day-1 setup guide" },
  { q: "What's my starting budget & AI capacity?", sub: "Monthly spend limit & connected models" }
];
function cmdkCommands() {
  const isEmpty = S.homeState === "empty";
  return [
    { group: "Actions", label: "New task", sub: "Send a task, bug, or request to Tess", icon: "plus", tag: "Command", run: () => openSmartTaskComposer() },
    { group: "Actions", label: "New project", sub: "Start a project wizard to group work", icon: "folder", tag: "Wizard", run: () => toast("New project wizard (prototype action)") },
    { group: "Actions", label: "Hire an agent", sub: "Describe role, review suggestion, hire", icon: "sparkle", tag: "Wizard", run: () => toast("Hire an agent wizard (Describe, Suggestion, Review, Hiring)") },
    { group: "Actions", label: "New message", sub: "Start a conversation in Chat", icon: "chat", tag: "Command", run: () => toast("New message (prototype action)") },
    { group: "Actions", label: "Add a person", sub: "Invite a teammate to this organization", icon: "user-plus", tag: "Command", run: () => toast("Add a person (prototype action)") },
    ...(isEmpty ? [
      { group: "Ask Tess", label: "Ask: How do I launch my first project?", sub: "Walk through setting up projects and hiring agents", icon: "sparkle", tag: "AI", keepOpen: true, run: () => askInCmdk("How do I launch my first project?") }
    ] : [
      { group: "Ask Tess", label: "Ask: What's slowing v1.0?", sub: "Summarize active blockers and pending sign-offs", icon: "sparkle", tag: "AI", keepOpen: true, run: () => askInCmdk("What's slowing v1.0?") },
      { group: "Ask Tess", label: "Ask: Budget, capacity & idle agents", sub: "Check monthly spend, AI capacity & idle specialists", icon: "sparkle", tag: "AI", keepOpen: true, run: () => askInCmdk("Budget, capacity & idle agents") },
      { group: "Ask Tess", label: "Ask: Next 14 days milestones", sub: "Review upcoming deliverables and schedule pace", icon: "sparkle", tag: "AI", keepOpen: true, run: () => askInCmdk("Next 14 days milestones") }
    ]),
    { group: "Navigate", label: "Go to Home", sub: "Executive briefing & decision spotlight", icon: "home", href: "#/home" },
    { group: "Navigate", label: "Go to Inbox", sub: `${isEmpty ? 0 : unhandled().length} waiting on you`, icon: "inbox", href: "#/inbox" },
    { group: "Navigate", label: "Go to Notifications", sub: `${unreadNotificationsCount()} unread in Inbox`, icon: "bell", href: "#/inbox/notifications" },
    { group: "Navigate", label: "Go to Projects", sub: "Portfolio health & progress", icon: "folder", href: "#/projects" },
    { group: "Navigate", label: "Go to Work", sub: "Tasks, bugs, and requests", icon: "clipboard", href: "#/work" },
    { group: "Navigate", label: "Go to Chat", sub: "Team & agent conversations", icon: "chat", href: "#/chat" },
    { group: "Navigate", label: "Go to Activity", sub: "Autonomous run history", icon: "signal", href: "#/activity" },
    { group: "Navigate", label: "Go to Teams", sub: "Teams, members & org chart", icon: "user-group", href: "#/teams" },
    { group: "Navigate", label: "Go to Settings", sub: "Offices, budgets & agent setup", icon: "cog", href: "#/settings" },
    { group: "Workspace", label: "Toggle sidebar", sub: "Pin open or auto-hide navigation rail", icon: "sidebar", tag: "View", run: () => { toggleNavPinned(); } },
    { group: "Workspace", label: "Ask Tess", sub: "Open floating Tess assistant (⌘/)", icon: "sparkle", tag: "⌘/", run: () => { openTessPanel(); } },
    { group: "Workspace", label: "Keyboard shortcuts", sub: "View all keyboard shortcuts (?)", icon: "question", tag: "?", run: () => { openShortcutsHelp(); } }
  ];
}
function askInCmdk(rawQ) {
  const q = String(rawQ || "").trim();
  if (!q) return;
  closeCmdk(false);
  openTessPanel(q);
}
function setCmdkMode(mode) {
  if (mode === "ai") {
    const q = CMDK.q;
    closeCmdk(false);
    openTessPanel(q);
    return;
  }
  if (CMDK.inline) return;
  CMDK.mode = "cmd";
  CMDK.active = 0;
  renderCmdk();
  const inp = document.getElementById("cmdk-input");
  if (inp) inp.focus();
}
function resetStaleCmdkAiState() {
  if (!CMDK.aiAns) return;
  const mismatch = (S.homeState === "empty") !== (CMDK.aiAns._state === "empty");
  if (mismatch) { CMDK.aiAns = null; CMDK.aiHistory = []; }
}

function mountCmdkListHost(list) {
  document.querySelector("#cmdk-dialog .cmdk-foot").before(list);
  document.getElementById("cmdk-root").hidden = false;
  document.getElementById("cmdk-trigger")?.setAttribute("aria-expanded", "true");
}

function openCmdk(prefill = "", mode = "cmd", trigger = document.activeElement) {
  if (route()[0] === "home" && S.homeState === "error") {
    toast("HQ office is offline — reconnect to use search and Ask Tess.", "offline");
    return;
  }
  if (CMDK.open) closeCmdk(false);
  Object.assign(CMDK, { inline: false, returnTo: trigger, open: true, mode: mode === "ai" ? "ai" : "cmd", q: prefill, active: 0 });
  resetStaleCmdkAiState();
  closeMenus();
  const list = document.getElementById("cmdk-list");
  const input = document.getElementById("cmdk-input");
  mountCmdkListHost(list);
  if (input.value !== prefill) input.value = prefill;
  input.focus();
  renderCmdk();
}
function closeCmdk(restoreFocus = true) {
  if (!CMDK.open) return;
  CMDK.open = false;
  CMDK.inline = false;
  CMDK.q = "";
  CMDK.flat = [];
  const root = document.getElementById("cmdk-root");
  const inp = document.getElementById("cmdk-input");
  const tr = document.getElementById("cmdk-trigger");
  if (root) root.hidden = true;
  if (inp) inp.value = "";
  if (tr) tr.setAttribute("aria-expanded", "false");
  if (restoreFocus && CMDK.returnTo && document.body.contains(CMDK.returnTo)) CMDK.returnTo.focus();
  CMDK.returnTo = null;
}
function syncCmdkChrome(inp, isAi) {
  const tabCmd = document.getElementById("cmdk-tab-cmd");
  const tabAi = document.getElementById("cmdk-tab-ai");
  const inIco = document.getElementById("cmdk-in-ico");
  const askGo = document.getElementById("cmdk-ask-go");
  const footNav = document.getElementById("cmdk-foot-nav");
  if (tabCmd) tabCmd.setAttribute("aria-selected", String(!isAi));
  if (tabAi) tabAi.setAttribute("aria-selected", String(isAi));
  if (inIco) inIco.innerHTML = `<use href="#i-${isAi ? "sparkle" : "search"}"/>`;
  if (askGo) { askGo.hidden = !isAi; askGo.disabled = !CMDK.q.trim(); }
  if (footNav) footNav.style.display = isAi ? "none" : "";
  if (inp && !CMDK.inline) {
    inp.placeholder = isAi
      ? "Ask Tess — what's blocked, release pace, budget, or idle agents…"
      : "Type a command, search anything, or ask a question…";
  }
}

function renderCmdkAiPane(list, inp) {
  CMDK.flat = [];
  list.setAttribute("role", "tabpanel");
  list.setAttribute("aria-labelledby", "cmdk-tab-ai");
  list.removeAttribute("aria-label");
  const curTurn = CMDK.aiHistory[0] || null;
  const prevTurns = CMDK.aiHistory.slice(1);
  const ansHtml = curTurn ? `<div class="cmdk-ai-card" role="status" aria-live="polite">
      <div class="cmdk-ai-card-hd">
        <span>${ic("sparkle", "ic14")} Tess · “${esc(curTurn.q)}”</span>
        <button type="button" data-act="cmdk-ai-clear">${prevTurns.length ? "Clear history" : "Clear"}</button>
      </div>
      <div>${curTurn.html}</div>
    </div>` : "";
  const prevHtml = prevTurns.length ? `<div class="cmdk-ai-prev-row"><span class="cmdk-ai-prev-lbl">Earlier</span>${prevTurns.map((turn) => `<button type="button" class="cmdk-ai-prev-chip" data-act="cmdk-ai-ask" data-v="${esc(turn.q)}">${ic("clock", "ic14")}<span>${esc(turn.q)}</span></button>`).join("")}</div>` : "";
  const askedSet = new Set(CMDK.aiHistory.map((x) => x.q.trim().toLowerCase()));
  const starters = S.homeState === "empty" ? CMDK_AI_STARTERS_EMPTY : CMDK_AI_STARTERS;
  const chips = starters.filter((s) => !askedSet.has(s.q.trim().toLowerCase()));
  const chipsHtml = chips.length ? `<div class="cmdk-ai-follow"><div class="cmdk-grp">${CMDK.aiHistory.length ? "Ask a follow-up" : "Suggested executive questions"}</div>
      <div class="cmdk-ai-chips">${chips.map((s) => `<button type="button" class="cmdk-ai-chip" data-act="cmdk-ai-ask" data-v="${esc(s.q)}">${ic("sparkle", "ic14")}<span>${esc(s.q)}</span></button>`).join("")}</div></div>` : "";
  list.innerHTML = `<div class="cmdk-ai-pane">${ansHtml}${prevHtml}${chipsHtml}</div>`;
  if (inp) inp.removeAttribute("aria-activedescendant");
}

function appendCmdkOrgResults(groups, q) {
  if (S.homeState === "empty") return;
  const matches = (i) => i.what.toLowerCase().includes(q) || i.sub.toLowerCase().includes(q) || M[i.from].name.toLowerCase().includes(q);
  const inboxHits = unhandled().filter(matches).slice(0, 3)
    .map((i) => ({ href: "#/inbox/" + i.id, label: i.what, sub: `${M[i.from].name} · ${i.sub}`, icon: "inbox", tag: "Inbox" }));
  if (inboxHits.length) groups.push(["Needs you", inboxHits]);
  searchResults(q, 4).forEach(([g, items]) => groups.push([g, items]));
}

function appendCmdkAskGroups(groups, raw) {
  const ask = { label: `Ask Tess: “${raw}”`, sub: "Get an instant answer right here from live org state", icon: "sparkle", tag: "AI", keepOpen: true, run: () => askInCmdk(raw) };
  const fullSearch = S.homeState === "empty" ? [] : [{ label: `See all search results for “${raw}”`, sub: "Open full search page", icon: "search", href: "#/search?q=" + encodeURIComponent(raw) }];
  if (CMDK.inline && fullSearch.length) groups.push(["Search", fullSearch]);
  groups.push(["Ask Tess", CMDK.inline ? [ask] : [ask, ...fullSearch]]);
}

function buildCmdkSearchGroups(raw, q, cmdOnly, cmds) {
  const groups = [];
  if (!cmdOnly && route()[0] === "chat") {
    const chats = conversationResults(q);
    if (chats.length) groups.push(["Conversations", chats]);
  }
  const matchedCmds = cmds.filter((c) => c.label.toLowerCase().includes(q) || c.sub.toLowerCase().includes(q));
  if (matchedCmds.length) groups.push(["Commands", matchedCmds]);
  if (cmdOnly) return groups;
  appendCmdkOrgResults(groups, q);
  appendCmdkAskGroups(groups, raw);
  return groups;
}

function buildCmdkGroups(raw) {
  const cmdOnly = raw.startsWith(">");
  const q = (cmdOnly ? raw.slice(1) : raw).trim().toLowerCase();
  const cmds = cmdkCommands();
  if (raw.startsWith("#")) {
    const channels = conversationResults(raw.slice(1).trim().toLowerCase(), true);
    return channels.length ? [["Channels", channels]] : [];
  }
  if (!q) {
    const byGrp = {};
    cmds.forEach((c) => { (byGrp[c.group] = byGrp[c.group] || []).push(c); });
    return ["Actions", "Ask Tess", "Navigate", "Workspace"].filter((g) => byGrp[g]).map((g) => [g, byGrp[g]]);
  }
  return buildCmdkSearchGroups(raw, q, cmdOnly, cmds);
}

function renderCmdk() {
  const list = document.getElementById("cmdk-list");
  if (!list || !CMDK.open) return;
  const inp = document.getElementById("cmdk-input");
  const isAi = CMDK.mode === "ai";
  syncCmdkChrome(inp, isAi);
  if (isAi) { renderCmdkAiPane(list, inp); return; }
  list.setAttribute("role", "listbox");
  list.setAttribute("aria-label", "Commands and search results");
  list.removeAttribute("aria-labelledby");
  const raw = CMDK.q.trim();
  const groups = buildCmdkGroups(raw);
  CMDK.flat = groups.flatMap((g) => g[1]);
  if (CMDK.active >= CMDK.flat.length) CMDK.active = 0;
  let n = 0;
  list.innerHTML = groups.length ? groups.map(([g, items]) => `<div class="cmdk-grp">${esc(g)}</div>` + items.map((it) => {
    const idx = n++, sel = idx === CMDK.active;
    return `<div role="option" id="cmdk-o-${idx}" class="cmdk-opt" aria-selected="${sel}" data-act="cmdk-run" data-v="${idx}">
      ${it.av ? av(it.av, 24) : ic(it.icon || "search", "ic16")}
      <span class="cmdk-opt-txt"><span class="cmdk-opt-lbl">${esc(it.label)}</span>${it.sub ? `<span class="cmdk-opt-sub">${esc(it.sub)}</span>` : ""}</span>
      ${it.tag ? `<span class="cmdk-opt-tag">${esc(it.tag)}</span>` : ""}
    </div>`;
  }).join("")).join("") : `<p class="cmdk-empty">No commands or results match “${esc(raw)}”.</p>`;
  if (inp) {
    if (CMDK.flat.length) inp.setAttribute("aria-activedescendant", "cmdk-o-" + CMDK.active);
    else inp.removeAttribute("aria-activedescendant");
  }
  document.getElementById("cmdk-o-" + CMDK.active)?.scrollIntoView?.({ block: "nearest" });
}
function runCmdkItem(idx) {
  const it = CMDK.flat[idx];
  if (!it) return;
  if (!it.keepOpen) closeCmdk(false);
  if (it.run) it.run();
  else if (it.href) {
    if (location.hash === it.href) render(true);
    else location.hash = it.href;
  }
}
function renderSearch() {}
function closeSearch() { closeCmdk(false); }
function endSearch() { closeCmdk(false); }
const narrowSearch = () => !!(window.matchMedia && window.matchMedia("(max-width: 639px)").matches);
function closeNarrowSearch() { closeCmdk(true); }
function goSearch(href) { closeCmdk(false); if (location.hash === href) render(true); else location.hash = href; }

export {
  CMDK,
  askInCmdk,
  closeCmdk,
  closeNarrowSearch,
  closeSearch,
  endSearch,
  goSearch,
  isMacPlatform,
  narrowSearch,
  openCmdk,
  renderCmdk,
  runCmdkItem,
  searchResults,
  setCmdkMode,
};
