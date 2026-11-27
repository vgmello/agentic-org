import { calendarAction } from "../pages/calendar.js";
import { IN, M, OFFICES, OPEN, P, TEAMS, W, blockedOnOwner } from "../data.js";
import { activityFilterKey, activityViewKey, addAssemblySample, assemblySimulation, projectActivityId, refreshAssemblySimulation, setActivityPanel, syncAssemblySimulationTimer, updateAssemblyPlayback } from "../pages/activity.js";
import { closeAgentFocus, isAgentFocusOpen, openAgentFocus } from "../pages/agent-focus.js";
import { consoleSessions } from "../pages/agent-profile.js";
import { chatDrafts, findConversation, openDirectMessage, sendChatMessage, sendThreadReply, toggleChatList } from "../pages/chat.js";
import { receiptLine, responseRecord } from "../pages/ask-receipt.js";
import { answerOrgQuestion } from "../pages/home.js";
import { detailCommentDrafts, inboxCommentPrefix, inboxItems, postDetailComment, refreshInboxSearch, resetInboxSelection } from "../pages/inbox.js";
import { updateProjectInsights } from "../pages/insights.js";
import { removeTeam } from "../pages/projects.js";
import { addIdeationComment, boardPlace, changeWorkCriterion, focusRevealedWork, shownWorkIds, closeWorkPanel, createWorkTask, fitTextarea, jumpToBoardLane, normalizeScopedWorkState, openWorkTask, publishDraftTask, refreshScopedWorkResults, refreshWorkResults, scopedWorkQueries, scopedWorkType, scopedWorkView, selectWorkView, setDoneHistory, setWorkPanelExpanded, workCommentPrefix, workItemHref, workSelection } from "../pages/work.js";
import { openSmartTaskComposer } from "../pages/task-composer.js";
import { CMDK, askInCmdk, closeCmdk, closeNarrowSearch, closeSearch, endSearch, goSearch, narrowSearch, openCmdk, renderCmdk, runCmdkItem, setCmdkMode } from "./cmdk.js";
import { rememberBoardExit, returnToBoard } from "./board-return.js";
import { closePopoverMenu, isPopoverMenu, openPopoverMenu, tabOutOfPopoverMenu } from "./popover-menu.js";
import { closeSheet, openAddTeam, openAssign, openConfirmOffboard, openConfirmRemove, openHealth, openShortcutsHelp, setSheetReturn } from "./dialogs.js";
import { endNavigationKeys, getNavigationKeys, handleNavigationKeys } from "./keyboard.js";
import { pageSearchInput, render, route } from "./router.js";
import { askTessInPanel, clearCompanionHistory, clearReadNotifications, closeCompanion, dismissCompanionContext, handleCompanionEscape, handleCompanionInput, isCompanionModal, markAllNotificationsRead, markNotificationRead, openTessPanel, toggleExpandCompanion, toggleNotificationRead, toggleTessPanel } from "./topbar-utils.js";
import { applyCozyContrast, applyFontStyle, applyShortcuts, applyTheme, ARCHIVE_AFTER_DAYS_STORE, ASSEMBLY_DIVIDERS_STORE, CHAT_SEND_SHORTCUTS, CHAT_SEND_STORE, COZY_CONTRAST_STORE, INBOX_DIVIDERS_STORE, inTextField, ORG_NAME_STORE, overlayOpen, qs, S, saveNavWorkspace, setOrgName, showOrgName, SR, toggleNavPinned, WORK_ARCHIVED_STORE, WORK_DIVIDERS_STORE, WORK_SUBTASKS_STORE, writeSingleKey } from "../state.js";
import { spotQueue, toast } from "../ui.js";

/* ---------------------------------------------------------------- actions */

/* An answer clears only the blocker it settles: the task stops waiting on the owner (To do) once no other
   ask for it is open. In progress is set by the task's start event when the agent next runs, never inferred here.
   A denial keeps the block, says so ("Access denied") and hands the next step to whoever asked. Work the answer
   closes is dated today, so it lands in recent Done history. */
const DENIED = /Denied|Declined|Dismissed/;
const WORK_FIELDS = ["who", "status", "waitsOn", "blockNote", "closedDaysAgo"];
const workSnapshot = (w) => Object.fromEntries(WORK_FIELDS.map((key) => [key, w[key]]));
function restoreWork(w, prev) {
  for (const key of WORK_FIELDS) { if (prev[key] !== undefined) w[key] = prev[key]; else delete w[key]; }
}
function applyDenialToWork(w, it) {
  w.waitsOn = it.from;
  w.blockNote = `${(it.sub || "Request").replace(/\s+request$/i, "")} denied`;
}
/* The answer settles the owner's block when no other ask for the work is open */
function settleOwnerBlock(w, it, outcome) {
  const otherAsks = IN.some((i) => i !== it && !i.handled && i.task === w.id);
  if (!blockedOnOwner(w) || otherAsks) return;
  if (DENIED.test(outcome)) applyDenialToWork(w, it);
  else { delete w.waitsOn; delete w.blockNote; w.status = "To do"; }
}
function nextWorkStatus(w, it, outcome) {
  if (it.sets?.[outcome]) return it.sets[outcome];
  if (w.status === "Scoped") return /Approve/.test(outcome) ? "Approved" : "Declined";
  return w.status;
}
function applyHandleToWork(w, it, outcome, assignee) {
  if (!w) return;
  const assignedTo = assignee || it.assignTo?.[outcome];
  if (assignedTo) w.who = assignedTo;
  const before = w.status, wasOpen = OPEN(w);
  settleOwnerBlock(w, it, outcome);
  w.status = nextWorkStatus(w, it, outcome);
  if (!OPEN(w) && (wasOpen || w.status !== before)) w.closedDaysAgo = 0;
}

function advanceSpotlightAfterHandle() {
  const q = spotQueue();
  const nextIdx = q.findIndex((x, idx) => idx > S.spotIdx && !x.handled);
  const wrapIdx = q.findIndex((x) => !x.handled);
  if (nextIdx >= 0) S.spotIdx = nextIdx;
  else if (wrapIdx >= 0) S.spotIdx = wrapIdx;
}

function makeReopenCallback(it, w, prevW, appliedW, prevSpotIdx) {
  const reopen = () => {
    if (it.reopen !== reopen) return;
    delete it.reopen;
    if (w && Object.keys(appliedW).every((key) => w[key] === appliedW[key])) restoreWork(w, prevW);
    S.spotIdx = prevSpotIdx;
    reopenInboxItem(it);
  };
  return reopen;
}

function focusHandledTarget(id, inInbox) {
  const tgt = document.getElementById("handled-" + id) || (inInbox && document.getElementById("triage-detail-title")) || document.querySelector("#main h1");
  if (!tgt) return;
  if (!tgt.hasAttribute("tabindex")) tgt.setAttribute("tabindex", "-1");
  tgt.focus();
}

function advanceInboxAfterHandle(id, it, next) {
  if (route()[1] !== id && route()[1]) return;
  history.replaceState(history.state, "", "#/inbox/" + (next || it).id);
  S.lastRoute = location.hash;
}

function handle(id, outcome, assignee = null, text = "") {
  const it = IN.find((i) => i.id === id); if (!it || it.handled) return;
  const w = it.task && W.find((x) => x.id === it.task);
  const prevW = w ? workSnapshot(w) : null;
  const prevSpotIdx = S.spotIdx, inInbox = route()[0] === "inbox";
  const visible = inInbox ? inboxItems() : [], index = visible.indexOf(it);
  const next = index < 0 ? null : visible[index + 1] || visible[index - 1];
  Object.assign(it, { handled: true, handledNow: true, justHandled: true, outcome, response: responseRecord(outcome, text) });
  S.replying = null;
  (it.activity ||= []).push({
    icon: /Denied|Declined|Dismissed/.test(outcome) ? "deny" : "check-circle", text: receiptLine(it) + (text ? `: “${text}”` : ""), at: Date.now(),
    assemblyRun: assemblySimulation.run, assemblyHour: assemblySimulation.hours,
  });
  applyHandleToWork(w, it, outcome, assignee);
  it.reopen = makeReopenCallback(it, w, prevW, w ? workSnapshot(w) : null, prevSpotIdx);
  toast(`${it.receipts?.[outcome] || outcome}. ${M[it.from].name} has been notified.`, "check-circle", it.reopen);
  if (inInbox) advanceInboxAfterHandle(id, it, next);
  if (route()[0] === "home") advanceSpotlightAfterHandle();
  render(false);
  focusHandledTarget(id, inInbox);
}

function reopenInboxItem(item) {
  (item.activity ||= [{ icon: "check-circle", text: item.outcome }]).push({
    icon: "refresh", text: "Returned to your queue", at: Date.now(),
    assemblyRun: assemblySimulation.run, assemblyHour: assemblySimulation.hours,
  });
  item.handled = false; item.handledNow = false; item.justHandled = false;
  delete item.outcome;
  delete item.response;
  if (route()[0] === "inbox") { S.inboxKind = "all"; history.replaceState(history.state, "", "#/inbox/" + item.id); S.lastRoute = location.hash; }
  render(false);
  document.getElementById("triage-detail-title")?.focus();
  toast("Returned to your queue.", "refresh");
}

function closeMenus(except) {
  document.querySelectorAll(".menu").forEach((m) => {
    if (m.id === except) return;
    if (isPopoverMenu(m)) closePopoverMenu(m);
    m.hidden = true;
    document.querySelector(`[aria-controls="${m.id}"]`)?.setAttribute("aria-expanded", "false");
  });
}

/* Zero results: clear filters and search for the current Work view, then return to the Filter control */
function clearWorkFilters() {
  if (document.getElementById("work-results")?.dataset.scope) {
    const r = route();
    Object.assign(S, { scopedWorkStatus: "all", scopedWorkProject: "all", scopedWorkTeam: "all" });
    scopedWorkQueries.delete(scopedWorkType(r) + ":" + r[1]);
  } else {
    Object.assign(S, { workQuery: "", workStatus: "all", workProject: "all", workTeam: "all" });
  }
  render(false);
  document.querySelector('[aria-controls="menu-scoped-work-filter"], [aria-controls="menu-work-filter"]')?.focus();
  toast("Filters cleared.");
}

function toggleStoredFlag(stateKey, attr, storeKey, t, invertAttr = false) {
  S[stateKey] = !S[stateKey];
  document.documentElement.toggleAttribute(attr, invertAttr ? !S[stateKey] : S[stateKey]);
  t.setAttribute("aria-checked", String(S[stateKey]));
  try { localStorage.setItem(storeKey, S[stateKey] ? "1" : "0"); } catch (e) {}
}

function handleSendReply(id) {
  const ta = document.getElementById("r-" + id), txt = ta ? ta.value.trim() : "";
  if (S.replyMode === "deny") { S.replyMode = null; handle(id, txt ? "Denied with note" : "Denied", null, txt); return; }
  if (!txt) {
    if (!ta) return;
    let er = document.getElementById("rerr-" + id);
    if (!er) {
      er = document.createElement("p"); er.id = "rerr-" + id; er.className = "muted";
      er.style.cssText = "color:var(--tone-bad-ink);font-size:13px;margin-top:6px"; er.textContent = "Write a reply before sending.";
      ta.insertAdjacentElement("afterend", er);
    }
    ta.setAttribute("aria-invalid", "true"); ta.setAttribute("aria-describedby", er.id);
    ta.addEventListener("input", () => { ta.removeAttribute("aria-invalid"); ta.removeAttribute("aria-describedby"); er.remove(); }, { once: true });
    ta.focus();
    return;
  }
  const outcome = S.replyMode === "changes" ? "Changes requested" : "Answered";
  S.replyMode = null;
  /* The written answer is kept on the ask's one response record, shown the same in Chat, Inbox and Home. */
  handle(id, outcome, null, txt);
}

function handleSetAction(t, v) {
  const menu = t.closest(".menu"), menuId = menu && menu.id, k = t.dataset.k;
  if (k === "workGroup" || k === "workSubgroup") {
    const selection = workSelection();
    if (k === "workGroup") selection.group = v; else selection.subgroup = v;
    selectWorkView(selection, '[aria-controls="menu-work-display"]');
    return;
  }
  const val = v === "true" ? true : v === "false" ? false : v;
  const scope = (k === "theme" || k === "fontStyle") ? t.closest("#main, #banner-ctl") : document;
  S[k] = val;
  normalizeScopedWorkState(k, val);
  if (k === "theme") applyTheme(true);
  if (k === "fontStyle") applyFontStyle(true);
  if (menuId === "menu-inbox-filter") resetInboxSelection();
  render(false);
  const again = menuId ? document.querySelector(`[aria-controls="${menuId}"]`) : scope?.querySelector(`[data-k="${k}"][data-v="${v}"]`);
  again?.focus();
}

function handleSaveSetup(id) {
  const m = M[id], s = m?.setup; if (!s) return;
  const rt = document.getElementById("s-rt"), mod = document.getElementById("s-model"), how = document.getElementById("s-how"), sel = document.getElementById("s-office");
  if (rt?.value) s.runtime = rt.value;
  if (mod?.value.trim()) s.model = mod.value.trim();
  if (how?.value) s.how = how.value;
  const office = sel?.value, newlyPlaced = !s.office && office && OFFICES[office];
  if (office && OFFICES[office]) { s.office = office; delete s.needs; }
  if (newlyPlaced) {
    const it = IN.find((i) => !i.handled && i.from === id && i.sub === "Unplaced agent");
    if (it) { handle(it.id, "Placed in " + OFFICES[office].name); return; }
  }
  render(false);
  document.querySelector(`[data-act="save-setup"][data-id="${CSS.escape(id)}"]`)?.focus();
  toast(`Saved setup for ${m.name}.`, "check-circle");
}

const BRIEF_MEMOS = {
  week: "Executive Brief (7-Day) — Signed: Tess, Chief of Staff\n• Velocity: 19 tasks delivered (+4 WoW), median turnaround 18m (-14%).\n• Autonomy & Budget: 78% autonomous; $142 of $210 spent (pacing ~$204).\n• Quality: 84% first-pass approval (+6% WoW); 412 automated checks green.",
  board: "Executive Brief (Board) — Signed: Tess, Chief of Staff\n• Summary: Delivery throughput +27% WoW (19 tasks) at 84% first-pass approval, 78% autonomous, tracking $6 under $210 budget.\n• v1.0 Readiness: 412 CI checks green; Kai's sync sign-off is the primary remaining gate for Dec 5.\n• Watchpoints: Build box offline (+5d slip on v0.9, reconnect fix staged); Jules at $210 cap (zero-cost reroute queued).",
  today: "Executive Brief (Today) — Signed: Tess, Chief of Staff\n• Live: 7 agents working across 3 projects; Ada & Maya drafted v1.0 release notes, 412 CI checks green.\n• Problems: Build box offline 2h (+5d slip on v0.9, fix staged); Jules hit $210 cap (3 tasks stalled).\n• Queued: Kai's v1.0 sync sign-off prioritized first; zero-cost reroute for Jules's 3 tasks queued behind it.",
};

function handleCopyBrief() {
  const memo = BRIEF_MEMOS[S.briefLens || "today"] || BRIEF_MEMOS.today;
  navigator.clipboard?.writeText?.(memo).catch(() => {});
  S.briefCopied = true;
  render(false);
  document.querySelector('[data-act="ai-copy-brief"]')?.focus();
  clearTimeout(window.__copyT);
  window.__copyT = setTimeout(() => { S.briefCopied = false; render(false); }, 2400);
  toast("Executive brief copied to clipboard.");
}

function handleCopyLink(t, id, v) {
  const taskLink = t.dataset.act === "work-copy-link";
  const value = taskLink ? new URL(`#/work?item=${encodeURIComponent(id)}`, location.href).href : v;
  if (!navigator.clipboard) { toast("Clipboard access is unavailable."); return; }
  navigator.clipboard.writeText(value).then(
    () => toast(taskLink ? "Task link copied." : `Copied ${v}.`, "check-circle"),
    () => toast("Clipboard access is unavailable."));
}

function handlePauseAgent(t, id) {
  const fromMenu = !!t.closest(".menu"); closeMenus(); const m = M[id];
  m.paused = !m.paused;
  if (m.paused && consoleSessions[id] && (consoleSessions[id].busy || consoleSessions[id].current)) consoleSessions[id].stopRun();
  toast(m.paused ? `${m.name} paused. Its work waits until you resume.` : `${m.name} resumed.`, m.paused ? "pause" : "play");
  render(false);
  const backEl = fromMenu ? document.querySelector('[aria-controls="menu-more"]') : document.querySelector(`[data-act="pause"][data-id="${id}"]`);
  backEl?.focus();
}

function handleOffboardConfirm(id) {
  const m = M[id];
  setSheetReturn(null); closeSheet();
  if (!m) return;
  m.paused = true; m.offboarded = true; m.task = "";
  consoleSessions[id]?.stopRun();
  render(false);
  document.querySelector("#main h1")?.focus();
  toast(`${m.name} offboarded. Records and journals remain read-only.`, "check-circle");
}

const CLICK_ACTIONS = {
  "insight-clear-drill": () => { const params = qs(); params.delete("group"); updateProjectInsights(params, "insights-usage-heading"); },
  skip: (e) => { e.preventDefault(); document.getElementById("main").focus(); },
  menu: (_e, t, id) => {
    const m = document.getElementById("menu-" + id), open = m.hidden;
    closeMenus(m.id); m.hidden = !open; t.setAttribute("aria-expanded", String(open));
    if (isPopoverMenu(m)) { if (open) openPopoverMenu(m, t); else closePopoverMenu(m); }
    if (open) (m.querySelector('[role^="menuitem"][aria-checked="true"]') || m.querySelector('[role^="menuitem"], button'))?.focus();
  },
  proto: (_e, t, _id, v) => {
    const mm = t.closest(".menu"); closeMenus();
    if (mm) document.querySelector(`[aria-controls="${mm.id}"]`)?.focus();
    toast("Prototype: not built · " + v);
  },
  handle: (_e, _t, id, v) => { S.confirmDeny = null; handle(id, v); },
  "deny-arm": (_e, _t, id) => {
    S.confirmDeny = id; render(false);
    (document.getElementById("deny-msg-" + id) || document.querySelector(`[data-act="deny-confirm"][data-id="${CSS.escape(id || "")}"]`))?.focus();
  },
  "deny-confirm": (_e, _t, id, v) => {
    const msg = document.getElementById("deny-msg-" + id)?.value.trim() || "";
    S.confirmDeny = null;
    const base = v || "Denied";
    handle(id, msg ? `${base}: ${msg}` : base);
  },
  "deny-keep": (_e, _t, id) => { S.confirmDeny = null; render(false); document.querySelector(`[data-act="deny-arm"][data-id="${CSS.escape(id || "")}"]`)?.focus(); },
  reply: (_e, _t, id, v) => { S.replying = id; S.replyMode = v === "changes" ? "changes" : v === "deny" ? "deny" : null; render(false); document.getElementById("r-" + id)?.focus(); },
  "cancel-reply": (_e, _t, id) => {
    const mode = S.replyMode; S.replying = null; S.replyMode = null; render(false);
    const backEl = document.querySelector(`[data-act="reply"][data-id="${CSS.escape(id || "")}"]${mode ? `[data-v="${mode}"]` : ""}`)
      || document.querySelector(`[data-act="reply"][data-id="${CSS.escape(id || "")}"]`) || document.querySelector("#main h1");
    if (backEl) { if (backEl.tagName === "H1" && !backEl.hasAttribute("tabindex")) backEl.setAttribute("tabindex", "-1"); backEl.focus(); }
  },
  "send-reply": (_e, _t, id) => handleSendReply(id),
  "assembly-play": () => { assemblySimulation.playing = !assemblySimulation.playing; updateAssemblyPlayback(); },
  "assembly-speed": (_e, t, _id, v) => { assemblySimulation.speed = Number(v); for (const o of t.parentElement.querySelectorAll("button")) o.setAttribute("aria-pressed", String(o === t)); },
  "assembly-restart": () => { Object.assign(assemblySimulation, { hours: 0, playing: false, samples: [], events: [] }); assemblySimulation.run += 1; assemblySimulation.progress.clear(); refreshAssemblySimulation(); updateAssemblyPlayback(); },
  "assembly-add-sample": () => addAssemblySample(),
  "activity-panel": (_e, _t, _id, v) => setActivityPanel(S.actPanel === v ? null : v),
  "activity-panel-close": () => setActivityPanel(null),
  "activity-panel-more": (_e, t) => { const scroll = document.getElementById("activity-panel-scroll"), hh = scroll.querySelector(".side-panel-header").offsetHeight; scroll.scrollBy(0, Math.max(t.offsetHeight, scroll.clientHeight - hh - t.offsetHeight)); },
  "assembly-dividers": (_e, t) => { toggleStoredFlag("assemblyDividers", "data-assembly-dividers", ASSEMBLY_DIVIDERS_STORE, t); document.querySelector('[aria-controls="menu-assembly-display"]').toggleAttribute("data-active", S.assemblyDividers); },
  "activity-review": (_e, _t, id) => {
    const pid = projectActivityId(); S[activityViewKey(pid)] = "feed"; S[activityFilterKey(pid)] = "needs"; render(false);
    const panel = document.getElementById("activity-triage-" + id); panel?.focus(); panel?.scrollIntoView({ block: "nearest" });
  },
  "inbox-category": (_e, _t, _id, v) => {
    S.inboxKind = v;
    if (v === "notifications") {
      history.replaceState(null, "", "#/inbox/notifications");
      S.lastRoute = location.hash;
    } else {
      resetInboxSelection();
    }
    render(false);
    (document.querySelector(`.queue-tabs > [data-act="inbox-category"][data-v="${v}"]`) || document.querySelector('[aria-controls="menu-inbox-kinds"]'))?.focus();
  },
  "work-preset": (_e, _t, _id, v) => selectWorkView({ ...workSelection(), view: v }, `[data-act="work-preset"][data-v="${v}"]`),
  "work-copy-link": (_e, t, id, v) => handleCopyLink(t, id, v),
  "inbox-copy": (_e, t, id, v) => handleCopyLink(t, id, v),
  "work-comment-focus": () => document.getElementById("work-comment")?.focus(),
  "inbox-reopen": (_e, _t, id) => { const item = IN.find((c) => c.id === id); if (item?.handled) { if (item.reopen) item.reopen(); else reopenInboxItem(item); } },
  set: (_e, t, _id, v) => handleSetAction(t, v),
  /* The toolbar's history chip resets Done history; focus goes to Display, where the history lives */
  "board-done": (_e, _t, _id, v) => {
    setDoneHistory(v);
    closeMenus();
    render(false);
    document.querySelector('[aria-controls="menu-work-display"], [aria-controls="menu-scoped-work-display"]')?.focus();
  },
  /* "Show N older/archived matches" widens history, then lands on the first item it added and says how many */
  "work-reveal": (_e, _t, _id, v) => {
    const before = shownWorkIds();
    setDoneHistory(v);
    render(false);
    focusRevealedWork(before);
  },
  "work-clear-filters": () => clearWorkFilters(),
  "board-lane": (_e, _t, _id, v) => jumpToBoardLane(v),
  "cozy-contrast": (_e, t) => { S.cozyContrast = !S.cozyContrast; try { localStorage.setItem(COZY_CONTRAST_STORE, S.cozyContrast ? "1" : "0"); } catch (e) {} applyCozyContrast(); t.setAttribute("aria-checked", String(S.cozyContrast)); },
  "work-expand-detail": (_e, t) => { const panel = t.closest("dialog"); setWorkPanelExpanded(panel, !panel.hasAttribute("data-expanded")); },
  "agent-focus-open": (_e, t, id) => { if (id) openAgentFocus(id, t); },
  "work-close-detail": () => closeWorkPanel(),
  "work-new-task": (_e, t, id) => openSmartTaskComposer(t, { agentId: id || undefined }),
  "work-publish-draft": (_e, _t, id) => { publishDraftTask(id); render(false); },
  "work-toggle-ideation": (_e, _t, id) => { S.openIdeation[id] = !S.openIdeation[id]; render(false); },
  "work-cancel-task": (_e, t) => t.closest("dialog").close(),
  run: (_e, _t, id) => { S.openRuns[id] = !S.openRuns[id]; render(false); document.querySelector(`[data-act="run"][data-id="${id}"]`)?.focus(); },
  "v4-acc": (_e, _t, id) => { S.v4Open[id] = !S.v4Open[id]; render(false); document.querySelector(`[data-act="v4-acc"][data-id="${id}"]`)?.focus(); },
  "spot-step": (_e, _t, _id, v) => { const n = spotQueue().length || 1; S.spotIdx = (S.spotIdx + Number(v || 1) + n) % n; S.replying = null; S.confirmDeny = null; render(false); },
  "spot-go": (_e, _t, _id, v) => { S.spotIdx = Number(v || 0); S.replying = null; S.confirmDeny = null; render(false); },
  "hb-ask-chip": (_e, _t, _id, v) => { S.askQ = ""; S.askAns = answerOrgQuestion(v); render(false); document.getElementById("hb-ai-reply")?.scrollIntoView({ block: "nearest" }); },
  "hb-ask-clear": () => { S.askAns = null; S.askQ = ""; render(false); document.getElementById("hb-ask-input")?.focus(); },
  "nav-collapse": () => toggleNavPinned(),
  "work-subtasks": (_e, t) => toggleStoredFlag("workSubtasks", "data-hide-work-subtasks", WORK_SUBTASKS_STORE, t, true),
  "inbox-dividers": (_e, t) => toggleStoredFlag("inboxDividers", "data-inbox-dividers", INBOX_DIVIDERS_STORE, t),
  "work-dividers": (_e, t) => toggleStoredFlag("workDividers", "data-work-dividers", WORK_DIVIDERS_STORE, t),
  "work-archived": (_e, t) => {
    const menuId = t.closest(".menu")?.id;
    S.workShowArchived = !S.workShowArchived;
    try { sessionStorage.setItem(WORK_ARCHIVED_STORE, S.workShowArchived ? "1" : "0"); } catch (e) {}
    render(false);
    const trigger = menuId ? document.querySelector(`[aria-controls="${menuId}"]`)
      : document.querySelector('[aria-controls="menu-scoped-work-display"], [aria-controls="menu-work-display"]');
    trigger?.focus();
  },
  "fav-proj": (_e, _t, id) => { const idx = S.favProjs.indexOf(id); if (idx >= 0) { S.favProjs.splice(idx, 1); toast(`Removed ${P[id].name} from favorites.`); } else { S.favProjs.push(id); toast(`Added ${P[id].name} to favorites.`); } render(false); },
  answer: (_e, t, id, v) => {
    if (/^Ask why/i.test(v)) { toast("Prototype: your question goes to the agent; the ask stays open."); return; }
    S.answered[id] = v;
    const it = IN.find((i) => i.id === t.dataset.inbox);
    if (it && !it.handled) { handle(it.id, (/^(Not now|No|Deny|Decline)/i.test(v) ? "Denied: " : "Answered: ") + v); return; }
    render(false);
  },
  pause: (_e, t, id) => handlePauseAgent(t, id),
  offboard: (_e, t, id) => { const fromMenu = !!t.closest(".menu"); closeMenus(); openConfirmOffboard(id, fromMenu ? document.querySelector('[aria-controls="menu-more"]') : t); },
  "offboard-confirm": (_e, _t, id) => handleOffboardConfirm(id),
  "toggle-know": (_e, _t, id, v) => { const key = id + ":" + v; S.openKnow[key] = !S.openKnow[key]; render(false); document.querySelector(`[data-act="toggle-know"][data-id="${CSS.escape(id)}"][data-v="${CSS.escape(v)}"]`)?.focus(); },
  "save-setup": (_e, _t, id) => handleSaveSetup(id),
  go: (_e, _t, _id, v) => { closeMenus(); location.hash = v; },
  "single-key": () => { S.singleKey = !S.singleKey; writeSingleKey(S.singleKey); render(false); document.querySelector('[data-act="single-key"]')?.focus(); toast(S.singleKey ? "Single-key shortcuts are on." : "Single-key shortcuts are off.", "check-circle"); },
  health: (_e, _t, id) => { closeMenus(); openHealth(id, document.querySelector('[aria-controls="menu-pmore"]')); },
  "health-save": (_e, _t, id) => {
    const c = document.querySelector('input[name="health"]:checked'), pr = P[id];
    if (c) { if (c.value === "none") delete pr.health; else pr.health = c.value; }
    closeSheet(); render(false);
    document.querySelector('[aria-controls="menu-pmore"]')?.focus();
    toast(pr.health ? `${pr.name} health set to ${pr.health}.` : `${pr.name} health cleared.`, "check-circle");
  },
  "team-rm": (_e, t, id, v) => openConfirmRemove(id, v, t),
  "team-rm-confirm": (_e, _t, id, v) => { setSheetReturn(null); closeSheet(); removeTeam(id, v); },
  "team-add": (_e, t, id) => openAddTeam(id, t),
  assign: (_e, t, id) => openAssign(id, t),
  "assign-save": (_e, _t, id) => { const who = document.getElementById("assign-to")?.value, it = IN.find((i) => i.id === id); setSheetReturn(null); closeSheet(); if (it && who && M[who]) handle(id, "Assigned to " + M[who].name, who); },
  "team-add-save": (_e, _t, id) => {
    const tk = document.getElementById("add-team")?.value; setSheetReturn(null); closeSheet();
    if (tk && TEAMS[tk] && !P[id].teams.includes(tk)) { P[id].teams.push(tk); render(false); toast(`${TEAMS[tk].name} added to ${P[id].name}.`, "check-circle"); }
    (document.getElementById("team-add") || document.querySelector(".tchip:last-of-type .rm"))?.focus();
  },
  "search-close": () => closeCmdk(true),
  sgo: (_e, _t, _id, v) => goSearch(v),
  "chat-list": (_e, _t, id) => toggleChatList(id),
  "chat-dm": (_e, _t, id, v) => { location.hash = openDirectMessage(id, v); },
  "search-toggle": (_e, _t, _id, v) => toggleInlineSearch(v),
  "search-open": (_e, t) => openCmdk(pageSearchInput()?.value || "", "cmd", t),
  "cmdk-open": (_e, t) => openCmdk(pageSearchInput()?.value || "", "cmd", t),
  "cmdk-open-ai": (_e, t) => openTessPanel("", t),
  "cmdk-close": () => closeCmdk(true),
  "cmdk-mode": (_e, _t, _id, v) => setCmdkMode(v),
  "cmdk-ai-ask": (_e, _t, _id, v) => askInCmdk(v),
  "brief-ai-ask": (_e, _t, _id, v) => openTessPanel(v),
  "tess-toggle": (_e, t) => toggleTessPanel(t),
  "tess-close": () => { if (!handleCompanionEscape()) closeCompanion(true); },
  "cmp-close": () => closeCompanion(true),
  "cmp-restore": (_e, t) => openTessPanel("", t),
  "cmp-expand": () => toggleExpandCompanion(),
  "cmp-clear": () => clearCompanionHistory(),
  "cmp-ask-chip": (_e, _t, _id, v) => askTessInPanel(v),
  "cmp-dismiss-ctx": (_e, _t, _id, v) => dismissCompanionContext(v),
  "shortcuts-help": (_e, t) => { closeMenus(); openShortcutsHelp(t); },
  "notif-open": (_e, _t, id) => markNotificationRead(id),
  "notif-toggle-read": (_e, _t, id) => toggleNotificationRead(id),
  "notif-mark-all": () => markAllNotificationsRead(),
  "notif-clear-read": () => clearReadNotifications(),
  "brief-lens": (_e, _t, _id, v) => { S.briefLens = v; S.briefCopied = false; render(false); document.querySelector(`[data-act="brief-lens"][data-v="${CSS.escape(v || "")}"]`)?.focus(); },
  "nav-pin": (_e, _t, _id, v) => {
    S.navWorkspace = S.navWorkspace.includes(v) ? S.navWorkspace.filter((k) => k !== v) : [...S.navWorkspace, v];
    saveNavWorkspace(); render(false);
    document.querySelector(`.nav [data-act="nav-pin"][data-v="${CSS.escape(v)}"]`)?.focus();
  },
  cal: (_e, _t, id, v) => calendarAction(id, v),
  "brief-view": (_e, _t, _id, v) => { S.briefView = v === "memo" ? "memo" : "highlights"; render(false); document.querySelector(`[data-act="brief-view"][data-v="${CSS.escape(S.briefView)}"]`)?.focus(); },
  "brief-expand": () => { S.briefExpanded = !S.briefExpanded; render(false); },
  "ai-copy-brief": () => handleCopyBrief(),
  "ai-copy-board": () => handleCopyBrief(),
  "cmdk-ai-submit": () => askInCmdk(CMDK.q),
  "cmdk-ai-clear": () => { CMDK.aiAns = null; CMDK.aiHistory = []; renderCmdk(); document.getElementById("cmdk-input")?.focus(); },
  "cmdk-run": (_e, _t, _id, v) => runCmdkItem(Number(v)),
  "banner-close": () => { document.getElementById("banner")?.remove(); document.getElementById("banner-hotzone")?.remove(); document.getElementById("cmdk-trigger")?.focus(); },
  "close-sheet": () => closeSheet(),
  drawer: () => { document.body.classList.add("drawer"); document.querySelector("#nav a").focus(); },
  "drawer-close": () => document.body.classList.remove("drawer"),
};

function handleBackdropOrJumpClick(e) {
  if (e.target.matches("dialog.work-panel")) {
    const { top, right, bottom, left } = e.target.getBoundingClientRect();
    if (e.clientX < left || e.clientX > right || e.clientY < top || e.clientY > bottom) { closeWorkPanel(); return true; }
  }
  const spotJump = e.target.closest('[data-act="spot-jump"]');
  if (spotJump && route()[0] === "home") {
    const idx = spotQueue().findIndex((x) => x.id === spotJump.dataset.id);
    if (idx >= 0) {
      e.preventDefault();
      if (CMDK.open) closeCmdk(false);
      S.spotIdx = idx; S.replying = null; render(false);
      const sp = document.getElementById("hb-spotlight");
      if (sp) { sp.scrollIntoView({ behavior: "smooth", block: "center" }); sp.classList.add("is-pulse"); setTimeout(() => sp.classList.remove("is-pulse"), 950); }
      return true;
    }
  }
  return false;
}

function dismissTopbarOverlaysForLink(link) {
  const act = link.dataset.act;
  if (act === "notif-open" && link.dataset.id) { markNotificationRead(link.dataset.id); return; }
  if (link.closest("#tess-panel") && isCompanionModal()) closeCompanion(false);
  if (act === "close-sheet") closeSheet();
}

/* Leaving a work board by one of its links (an Inbox request, a project, a person) keeps the board's place */
function rememberBoardLinkExit(link, href) {
  const results = document.getElementById("work-results");
  if (!results?.querySelector(".board") || !results.contains(link) || link.matches(".work-item-link") || href === location.hash) return;
  rememberBoardExit(boardPlace(results, link), href);
}

function handleHashLinkClick(e) {
  const link = e.target.closest('a[href^="#/"]');
  if (!link || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return false;
  const href = link.getAttribute("href");
  e.preventDefault();
  if (link.matches(".triage-board-return") && returnToBoard()) return true;
  rememberBoardLinkExit(link, href);
  document.body.classList.remove("drawer");
  closeMenus();
  dismissTopbarOverlaysForLink(link);
  if (link.dataset.act === "insight-drill") { updateProjectInsights(new URLSearchParams(href.split("?")[1]), "insights-usage-heading"); return true; }
  if (location.hash !== href) location.hash = href;
  else if (link.matches(".work-item-link")) document.getElementById("work-detail-title")?.focus({ preventScroll: true });
  else render(true);
  return true;
}

document.addEventListener("cancel", (e) => {
  if (e.target.id !== "work-panel") return;
  e.preventDefault();
  closeWorkPanel();
}, true);

document.addEventListener("click", (e) => {
  if (handleBackdropOrJumpClick(e) || handleHashLinkClick(e)) return;
  const t = e.target.closest("[data-act]");
  if (!e.target.closest(".menu-wrap")) closeMenus();
  if (!e.target.closest(".search")) {
    if (SR.open) closeSearch();
    if (document.body.classList.contains("searching")) { const q = document.getElementById("q"); if (q) q.value = ""; SR.q = ""; document.body.classList.remove("searching"); }
  }
  if (!t) {
    const card = e.target.closest("[data-card-href]");
    if (card && !e.target.closest("a, button")) location.hash = card.dataset.cardHref;
    return;
  }
  const fn = CLICK_ACTIONS[t.dataset.act];
  if (fn) fn(e, t, t.dataset.id, t.dataset.v);
});

document.addEventListener("click", (e) => {
  if (e.target?.id === "cmdk-root") { closeCmdk(true); return; }
  if (CMDK.open && e.target.closest?.('.cmdk-ai-card a[href^="#/"]')) closeCmdk(false);
});

function handleDetailCommentInput(target) {
  target.setCustomValidity("");
  const id = target.form.dataset.id, work = target.id === "work-comment";
  detailCommentDrafts.set(work ? workCommentPrefix(id) : inboxCommentPrefix(id), target.value);
  if (work) fitTextarea(target);
}

function handleWorkSearchInput(target) {
  S.workQuery = target.value;
  if (document.querySelector(".work-reading")) { history.replaceState(null, "", workItemHref()); render(false); }
  else refreshWorkResults();
}

/* Inline page search (Inbox, Work): toolbar icon or "/" opens the row and focuses it; Esc leaves it. */
function focusInlineSearch(inputId) {
  const input = document.getElementById(inputId);
  if (!input) return;
  input.focus({ preventScroll: true });
  const end = input.value.length;
  input.setSelectionRange(end, end);
}
function openInlineSearch(inputId) {
  if (!document.getElementById(inputId)) { S.searchOpen[inputId] = true; render(false); }
  focusInlineSearch(inputId);
}
function closeInlineSearch(input) {
  const inputId = input.id;
  if (!input.value.trim()) { S.searchOpen[inputId] = false; render(false); }
  else input.blur();
  document.querySelector(`[data-act="search-toggle"][data-v="${CSS.escape(inputId)}"]`)?.focus({ preventScroll: true });
}
function toggleInlineSearch(inputId) {
  const input = document.getElementById(inputId);
  if (input && !input.value.trim()) closeInlineSearch(input);
  else openInlineSearch(inputId);
}

const INPUT_BY_ID = {
  compose: (target) => { chatDrafts.set(target.form.dataset.id, target.value); fitTextarea(target); },
  "projects-search": (target) => { S.projectQuery = target.value; render(false); },
  "triage-search": (target) => { S.inboxQuery = target.value; refreshInboxSearch(); },
  "triage-comment": handleDetailCommentInput,
  "work-comment": handleDetailCommentInput,
  "scoped-work-search": (target) => {
    const currentRoute = route(), view = scopedWorkView(currentRoute, scopedWorkType(currentRoute));
    scopedWorkQueries.set(view.key, target.value);
    refreshScopedWorkResults(view);
  },
  "work-search": handleWorkSearchInput,
  "o-name": (target) => target.setCustomValidity(""),
};

document.addEventListener("input", (e) => {
  const target = e.target;
  if (target.form?.dataset.form === "insight-range") target.form.elements.to.setCustomValidity("");
  if (target.name === "title" && target.form?.dataset.form === "work-create") target.setCustomValidity("");
  INPUT_BY_ID[target.id]?.(target);
  if (handleCompanionInput(target)) return;
  if (target.id === "cmdk-input") { CMDK.q = target.value; CMDK.active = 0; renderCmdk(); }
});

document.addEventListener("change", (e) => {
  if (e.target.matches("[data-insight-control]")) {
    const { id, value } = e.target, params = qs();
    const key = { "insight-period": "period", "insight-display": "display", "insight-breakdown": "by", "insight-investment": "investmentBy" }[id];
    params.set(key, value);
    if (key === "period" || key === "by") params.delete("group");
    if (key === "period") { params.delete("from"); params.delete("to"); }
    updateProjectInsights(params, id);
  }
  if (e.target.matches("[data-work-criterion]")) changeWorkCriterion(e.target);
  if (e.target.id === "chat-send-shortcut" && Object.hasOwn(CHAT_SEND_SHORTCUTS, e.target.value)) {
    S.chatSend = e.target.value;
    try { localStorage.setItem(CHAT_SEND_STORE, S.chatSend); } catch (e) {}
  }
  if (e.target.matches("[data-font-select]")) {
    S.fontStyle = e.target.value;
    applyFontStyle(true);
    render(false);
    document.getElementById(e.target.id)?.focus();
  }
  if (e.target.id === "settings-section") { S.focusAfter = "#settings-section"; location.hash = e.target.value; }
});

document.addEventListener("mousedown", (e) => {
  if (e.target.closest?.(".cmdk-home-results .cmdk-opt, :is(.queue-header, .work-subbar-header, .section-toolbar) .menu button")) e.preventDefault();
});

function resetInboxSenderPreview(e) {
  const sender = e.target.closest?.(".triage-from-member[data-preview-dismissed]");
  if (sender && !sender.contains(e.relatedTarget)) delete sender.dataset.previewDismissed;
}
document.addEventListener("pointerover", resetInboxSenderPreview);
document.addEventListener("pointerdown", () => endNavigationKeys());
document.addEventListener("focusin", (e) => {
  const activeNavKeys = getNavigationKeys();
  if (activeNavKeys && !activeNavKeys.scope.contains(e.target)) endNavigationKeys();
  resetInboxSenderPreview(e);
  if (e.target.closest && !e.target.closest(".menu-wrap")) closeMenus();
});

document.addEventListener("submit", (e) => {
  if (e.target.id === "c-prompt-form" || e.target.id === "af-prompt-form" || e.target.id === "hb-ask-form") return;
  e.preventDefault();
  if (["inbox-comment", "work-comment"].includes(e.target.dataset.form)) { postDetailComment(e.target); return; }
  if (e.target.dataset.form === "work-ideation-comment") { addIdeationComment(e.target); render(false); return; }
  if (e.target.dataset.form === "insight-range") {
    const { from, to } = e.target.elements;
    if (from.value > to.value) { to.setCustomValidity("Choose an end date on or after the start date."); to.reportValidity(); return; }
    const params = qs();
    params.set("period", "custom"); params.set("from", from.value); params.set("to", to.value); params.delete("group");
    updateProjectInsights(params, "insight-apply-period");
    return;
  }
  if (e.target.dataset.form === "org-settings") {
    const field = e.target.elements.name, name = field.value.trim();
    if (!name) { field.setCustomValidity("Enter an organization name."); field.reportValidity(); return; }
    setOrgName(field.value = name); showOrgName();
    const days = Number(e.target.elements.archiveDays?.value);
    if (Number.isFinite(days) && days >= 0) S.archiveAfterDays = days;
    try {
      localStorage.setItem(ORG_NAME_STORE, name);
      localStorage.setItem(ARCHIVE_AFTER_DAYS_STORE, String(S.archiveAfterDays));
      toast("Organization settings saved.", "check-circle");
    } catch (err) { toast("Organization settings updated for this session."); }
    return;
  }
  if (e.target.dataset.form === "work-create") { createWorkTask(e.target); return; }
  if (e.target.dataset.form === "tess-panel-ask") {
    askTessInPanel(document.getElementById("tess-panel-input")?.value || "");
    return;
  }
  if (e.target.dataset.form === "send") sendChatMessage(e.target);
  if (e.target.dataset.form === "thread-send" && sendThreadReply(e.target)) { render(false); document.getElementById("thread-compose")?.focus(); }
});

const CHAT_COMPOSERS = new Set(["compose", "thread-compose"]);
function isChatSendKey(e) {
  if (!CHAT_COMPOSERS.has(e.target.id) || e.key !== "Enter" || e.altKey || e.shiftKey) return false;
  return (e.ctrlKey && S.chatSend !== "command") || (e.metaKey && S.chatSend !== "control");
}

function handleComposerKeydown(e) {
  if (isChatSendKey(e)) {
    e.preventDefault(); if (!e.repeat) e.target.form.requestSubmit(); return true;
  }
  if (e.target.id === "work-comment" && (e.metaKey || e.ctrlKey) && e.key === "Enter") {
    e.preventDefault(); e.target.form.requestSubmit(); return true;
  }
  return false;
}

function trapCmdkTab(e) {
  if (CMDK.inline) { closeCmdk(false); return; }
  const dlg = document.getElementById("cmdk-dialog");
  if (!dlg) return;
  const focusable = [...dlg.querySelectorAll('button:not([hidden]):not([disabled]), input:not([disabled]), a[href]')];
  if (!focusable.length) return;
  const i = focusable.indexOf(document.activeElement);
  if (e.shiftKey && i <= 0) { e.preventDefault(); focusable[focusable.length - 1].focus(); }
  else if (!e.shiftKey && i === focusable.length - 1) { e.preventDefault(); focusable[0].focus(); }
}

function handleCmdkListKey(e) {
  if (e.key === "ArrowDown" || e.key === "ArrowUp") {
    e.preventDefault();
    if (CMDK.flat.length) CMDK.active = (CMDK.active + (e.key === "ArrowDown" ? 1 : -1) + CMDK.flat.length) % CMDK.flat.length;
    renderCmdk();
    return;
  }
  if (e.key === "Enter" && e.target.id === "cmdk-input") {
    e.preventDefault(); runCmdkItem(CMDK.active);
  }
}

function handleCmdkKeydown(e) {
  if ((e.metaKey || e.ctrlKey) && !e.altKey && e.key === "/") { e.preventDefault(); setCmdkMode(CMDK.mode === "ai" ? "cmd" : "ai"); return; }
  if (e.key === "Escape") { e.preventDefault(); closeCmdk(true); return; }
  if (e.key === "Tab") { trapCmdkTab(e); return; }
  if (CMDK.mode === "ai") { if (e.key === "Enter" && e.target.id === "cmdk-input") { e.preventDefault(); askInCmdk(CMDK.q); } return; }
  handleCmdkListKey(e);
}

function handleOpenMenuKeydown(e, openMenu) {
  /* Items a narrow layout hides (an overflow link shown only on phones) are skipped */
  const items = [...openMenu.querySelectorAll('[role^="menuitem"]')].filter((el) => el.getClientRects().length), i = items.indexOf(e.target);
  const go = (n) => { e.preventDefault(); items[(n + items.length) % items.length].focus(); return true; };
  if (e.key === "ArrowDown") return go(i + 1);
  if (e.key === "ArrowUp") return go(i - 1);
  if (e.key === "Home") return go(0);
  if (e.key === "End") return go(items.length - 1);
  const applyDisplay = e.key === "Enter" && openMenu.matches("#menu-work-display, #menu-scoped-work-display, #menu-inbox-display, #menu-assembly-display") && e.target.getAttribute("role") === "menuitemcheckbox";
  if (e.key === "Escape" || applyDisplay) {
    e.preventDefault(); if (applyDisplay) e.target.click();
    const trigger = document.querySelector(`[aria-controls="${openMenu.id}"]`);
    closeMenus(); trigger?.focus(); return true;
  }
  if (e.key === "Tab" && isPopoverMenu(openMenu)) {
    e.preventDefault();
    closeMenus();
    tabOutOfPopoverMenu(openMenu, e.shiftKey);
    return true;
  }
  if (e.key === "Tab") {
    closeMenus();
    if (openMenu.closest(".queue-controls")) document.querySelector(`[aria-controls="${openMenu.id}"]`)?.focus();
  }
  return false;
}

function dismissLocalEscapeTarget(e) {
  if (S.chatList && e.target.closest?.(".chat.list-open .chlist")) { e.preventDefault(); toggleChatList(S.chatList); return true; }
  if (e.target.tagName === "TEXTAREA" && /^r-/.test(e.target.id || "")) {
    const c = document.querySelector(`[data-act="cancel-reply"][data-id="${CSS.escape(e.target.id.slice(2))}"]`);
    if (c) { e.preventDefault(); c.click(); return true; }
  }
  if (e.target.matches("#triage-comment")) { e.preventDefault(); document.querySelector(".triage-header h1").focus({ preventScroll: true }); return true; }
  if (S.actPanel && e.target.closest("#activity-panel, .activity-tools")) { e.preventDefault(); setActivityPanel(null); return true; }
  const sender = document.querySelector(".triage-from-member:is(:hover, :focus-within):not([data-preview-dismissed])");
  if (sender) { sender.dataset.previewDismissed = ""; e.preventDefault(); return true; }
  return false;
}

function handleEscapeKeydown(e) {
  if (dismissLocalEscapeTarget(e)) return;
  if (handleCompanionEscape()) { e.preventDefault(); return; }
  if (isAgentFocusOpen()) { e.preventDefault(); closeAgentFocus(true); return; }
  const closeThread = e.target.closest(".thread")?.querySelector('[data-act="thread-close"]');
  if (closeThread) { e.preventDefault(); closeThread.click(); return; }
  if (S.confirmDeny) {
    const cid = S.confirmDeny; S.confirmDeny = null; render(false);
    document.querySelector(`[data-act="deny-arm"][data-id="${CSS.escape(cid)}"]`)?.focus();
    e.preventDefault(); return;
  }
  if (e.target.closest?.(".search") && document.body.classList.contains("searching") && narrowSearch()) { e.preventDefault(); closeNarrowSearch(); return; }
  if (document.querySelector(".sheet")) closeSheet();
  closeMenus(); endSearch(); document.body.classList.remove("drawer");
}

function moveHorizontalIndex(e, list, i) {
  if (e.key === "Home") return 0;
  if (e.key === "End") return list.length - 1;
  return (i + (e.key === "ArrowRight" ? 1 : -1) + list.length) % list.length;
}

function handleHomeArrowKeydown(e) {
  if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return false;
  const lensGrp = e.target.closest?.(".hb-brief-lenses");
  if (lensGrp) {
    const btns = [...lensGrp.querySelectorAll("button")], i = btns.indexOf(e.target);
    if (i >= 0) { e.preventDefault(); btns[moveHorizontalIndex(e, btns, i)].click(); return true; }
  }
  if (!e.target.closest?.(".hb-spot-nav")) return false;
  e.preventDefault();
  const n = spotQueue().length || 1;
  S.spotIdx = (S.spotIdx + (e.key === "ArrowRight" ? 1 : -1) + n) % n;
  S.replying = null; S.confirmDeny = null; render(false);
  document.querySelector(".hb-dot-btn.is-cur")?.focus();
  return true;
}

function handleWidgetArrowKeydown(e) {
  if (handleHomeArrowKeydown(e)) return true;
  if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(e.key)) return false;
  const tabEl = e.target.closest?.('[role="tab"]');
  if (tabEl) {
    const list = [...tabEl.parentElement.querySelectorAll('[role="tab"]')], n = moveHorizontalIndex(e, list, list.indexOf(tabEl));
    e.preventDefault(); list[n].click(); if (document.body.contains(list[n])) list[n].focus(); return true;
  }
  const pageOption = e.target.closest?.("#triage-categories .primary-tab-row .queue-tab, [data-page-menu] [data-menu-label]");
  if (!pageOption) return false;
  const options = [...pageOption.closest(".primary-tab-row").querySelectorAll("[data-menu-label], .queue-tab")];
  e.preventDefault(); options[moveHorizontalIndex(e, options, options.indexOf(pageOption))].focus();
  return true;
}

function handleListArrowKeydown(e) {
  if (e.target.matches?.(".triage-item") && ["ArrowDown", "ArrowUp"].includes(e.key)) {
    const rows = [...document.querySelectorAll(".triage-item")], next = rows[rows.indexOf(e.target) + (e.key === "ArrowDown" ? 1 : -1)];
    if (next) { e.preventDefault(); next.focus(); }
    return true;
  }
  if (e.target.matches?.(".chlist nav a") && !e.altKey && !e.ctrlKey && !e.metaKey && ["ArrowUp", "ArrowDown", "Home", "End"].includes(e.key)) {
    const links = [...e.target.closest("nav").querySelectorAll("a")], index = links.indexOf(e.target);
    const next = e.key === "Home" ? links[0] : e.key === "End" ? links.at(-1) : links[index + (e.key === "ArrowDown" ? 1 : -1)];
    if (next) { e.preventDefault(); next.focus(); }
    return true;
  }
  return false;
}

function isPlainAltGrOrUnmodified(e) {
  if (e.metaKey) return false;
  return (e.ctrlKey && e.altKey) || (!e.ctrlKey && !e.altKey);
}

function tryInboxApprovalShortcut(e) {
  if (e.key.toLowerCase() !== "a" || route()[0] !== "inbox" || e.target.closest("#triage-categories, #sidebar")) return false;
  const approval = document.querySelector('.triage-document [data-act="handle"].primary');
  if (!approval || !IN.some((item) => item.id === approval.dataset.id && item.kind === "approval")) return false;
  e.preventDefault(); approval.click(); return true;
}

function isSingleKeyBlocked(e) {
  return !S.singleKey || e.isComposing || e.repeat || inTextField(e.target) || overlayOpen();
}

function tryPunctuationShortcut(e) {
  if (!isPlainAltGrOrUnmodified(e)) return false;
  if (e.key === "#" && route()[0] === "chat") { e.preventDefault(); findConversation(); return true; }
  if (e.key === "/") {
    e.preventDefault();
    const inline = document.querySelector('#main [data-act="search-toggle"]');
    const input = pageSearchInput();
    if (inline) openInlineSearch(inline.dataset.v); else if (input) input.focus(); else openCmdk();
    return true;
  }
  return false;
}

function isNewTaskKey(e) {
  return e.key === "n" || e.key === "N" || (!/^[a-z]$/i.test(e.key) && e.code === "KeyN");
}

function tryShortcutsHelpKey(e) {
  if (e.key !== "?" || e.isComposing || e.repeat || inTextField(e.target) || overlayOpen()) return false;
  if (e.metaKey || e.ctrlKey || e.altKey) return false;
  e.preventDefault();
  openShortcutsHelp(document.activeElement);
  return true;
}

function handleSingleKeyShortcut(e) {
  if (tryShortcutsHelpKey(e) || isSingleKeyBlocked(e) || tryPunctuationShortcut(e)) return;
  if (e.metaKey || e.ctrlKey || e.altKey || tryInboxApprovalShortcut(e)) return;
  if (isNewTaskKey(e)) { e.preventDefault(); openSmartTaskComposer(document.activeElement); }
}

function isCmdKCombo(e) {
  return (e.metaKey || e.ctrlKey) && !e.altKey && (e.key === "k" || e.key === "K");
}

function isCmdSlashCombo(e) {
  return (e.metaKey || e.ctrlKey) && !e.altKey && e.key === "/";
}

function isTessExpandCombo(e) {
  return (e.metaKey || e.ctrlKey) && e.shiftKey && !e.altKey && (e.key === "e" || e.key === "E" || e.key === "m" || e.key === "M");
}

function tryTessGlobalShortcut(e) {
  if (isCmdSlashCombo(e)) {
    e.preventDefault();
    if (CMDK.open) { const q = CMDK.q; closeCmdk(false); openTessPanel(q); }
    else toggleTessPanel(document.activeElement);
    return true;
  }
  if (isTessExpandCombo(e)) {
    e.preventDefault();
    if (CMDK.open) closeCmdk(false);
    toggleExpandCompanion();
    return true;
  }
  return false;
}

function handleSearchGlobalKeydown(e) {
  if (isCmdKCombo(e)) {
    e.preventDefault();
    if (CMDK.open) closeCmdk(true);
    else openCmdk(pageSearchInput()?.value || "");
    return true;
  }
  if (tryTessGlobalShortcut(e)) return true;
  if (e.key === "Escape" && e.target.classList?.contains("inline-search-input")) {
    e.preventDefault(); closeInlineSearch(e.target); return true;
  }
  if (e.key === "Escape" && e.target === pageSearchInput()) {
    e.preventDefault(); closeCmdk(false); document.querySelector("#main h1")?.focus({ preventScroll: true }); return true;
  }
  return false;
}

function handleDenyInputEnter(e) {
  if (e.key !== "Enter" || !e.target.classList?.contains("hb-deny-input")) return false;
  e.preventDefault();
  const msg = e.target.value.trim(), cout = e.target.dataset.denyOutcome || "Denied";
  S.confirmDeny = null;
  handle(e.target.dataset.denyId, msg ? `${cout}: ${msg}` : cout);
  return true;
}

function handleDocumentKeydown(e) {
  if (e.isComposing || e.target.closest?.(".work-task-dialog")) return;
  if (handleComposerKeydown(e) || e.target.closest?.(".side-panel") || handleSearchGlobalKeydown(e)) return;
  if (CMDK.open) { handleCmdkKeydown(e); return; }
  if (handleNavigationKeys(e)) return;
  const openMenu = e.target.closest?.(".menu:not([hidden])");
  if (openMenu && handleOpenMenuKeydown(e, openMenu)) return;
  if (handleDenyInputEnter(e)) return;
  if (e.key === "Escape") { handleEscapeKeydown(e); return; }
  if (handleWidgetArrowKeydown(e) || handleListArrowKeydown(e)) return;
  handleSingleKeyShortcut(e);
}

document.addEventListener("keydown", handleDocumentKeydown);

if (window.matchMedia) {
  const mq = window.matchMedia("(max-width: 959px)"), onMq = () => render(false);
  if (mq.addEventListener) mq.addEventListener("change", onMq); else if (mq.addListener) mq.addListener(onMq);
  const mqp = window.matchMedia("(max-width: 639px)");
  if (mqp.addEventListener) mqp.addEventListener("change", applyShortcuts); else if (mqp.addListener) mqp.addListener(applyShortcuts);
}
window.addEventListener("hashchange", () => {
  closeCmdk(false);
  if (isCompanionModal()) closeCompanion(false);
  document.querySelector(".work-task-dialog[open]")?.close(); document.body.classList.remove("drawer");
  S.thread = null; S.replying = null; S.replyMode = null; S.confirmDeny = null;
  IN.forEach((i) => { if (i.handled) i.handledNow = false; });
  render(true);
});
document.addEventListener("visibilitychange", () => syncAssemblySimulationTimer());

document.addEventListener("submit", (e) => {
  if (e.target?.id !== "hb-ask-form") return;
  e.preventDefault();
  const val = (document.getElementById("hb-ask-input")?.value || "").trim();
  if (!val) return;
  S.askQ = ""; S.askAns = answerOrgQuestion(val); render(false);
  document.getElementById("hb-ask-input")?.focus();
});

export {
  closeMenus,
};
