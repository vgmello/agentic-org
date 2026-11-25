import { CH, IN, M, P, W } from "../data.js";
import { answerOrgQuestion } from "../pages/home.js";
import { CMDK, closeCmdk } from "./cmdk.js";
import { S, qs } from "../state.js";
import { av, esc, ic, memberIds } from "../ui.js";

/* ---------------------------------------------------------------- Floating Contextual Companion */

const COMP = {
  mode: "closed", // "closed" | "open" | "minimized" | "expanded"
  target: { type: "tess" },
  lastNonTess: null,
  pickerOpen: false,
  pickerQuery: "",
  dismissedCtxKey: null,
  unreadReply: false,
  drafts: new Map(),
  returnTo: null,
  currentRoute: ["home"],
};

const TESS_STARTERS = {
  empty: ["How do I launch my first project?", "What's my starting budget & AI capacity?"],
  work: ["What's slowing v1.0?", "What needs me first?", "Budget, capacity & idle agents"],
  default: ["What needs me first?", "What's slowing v1.0?", "What happened in the last 4 hours?"],
};

const PRESENCE_LABEL = {
  working: "Working",
  withyou: "With you",
  available: "Available",
  needsyou: "Needs you",
  asleep: "Idle",
  offline: "Offline",
};

let onStateChange = null;
let getActiveTessJobs = () => [];
let closeNotifHook = () => {};

function registerCompanionHooks({ onChange, activeJobs, closeNotif }) {
  onStateChange = onChange;
  getActiveTessJobs = activeJobs;
  closeNotifHook = closeNotif;
}

function targetKey(t = COMP.target) {
  return t.type === "tess" ? "tess" : `${t.type}:${t.id}`;
}

function isSameTarget(a, b) {
  return !!a && !!b && a.type === b.type && a.id === b.id;
}

function isOfflineMode() {
  return S.homeState === "error";
}

function ensureDmChannel(memberId) {
  const existing = Object.keys(CH).find((k) => CH[k].kind === "dm" && CH[k].with === memberId);
  if (existing) return existing;
  const id = `dm-${memberId}`;
  const m = M[memberId];
  CH[id] = { name: m ? m.name : memberId, kind: "dm", with: memberId, msgs: [] };
  return id;
}

function resolveChannelForTarget(t = COMP.target) {
  if (t.type === "channel" && CH[t.id]) return { id: t.id, ch: CH[t.id] };
  if (t.type === "member" && M[t.id]) {
    const id = ensureDmChannel(t.id);
    return { id, ch: CH[id] };
  }
  return null;
}

function describeRouteContext(r = COMP.currentRoute) {
  const [page, r1] = r;
  const item = qs().get("item");
  if (item) {
    const task = W.find((w) => w.id === item);
    return { key: `item:${item}`, label: task ? `${task.id} · ${task.title}` : `Task · ${item}`, short: task ? task.id : item };
  }
  if (page === "inbox" && r1) {
    const it = IN.find((i) => i.id === r1);
    return { key: `inbox:${r1}`, label: it ? `Inbox · ${it.what}` : "Inbox", short: "Inbox" };
  }
  if (page === "projects" && r1 && P[r1]) return { key: `proj:${r1}`, label: `Project · ${P[r1].name}`, short: P[r1].name };
  if (page === "members" && r1 && M[r1]) return { key: `member:${r1}`, label: `Member · ${M[r1].name}`, short: M[r1].name };
  const map = { home: "Home", inbox: "Inbox", work: "Work", chat: "Chat", activity: "Activity", projects: "Projects", teams: "Teams", settings: "Settings" };
  const name = map[page] || "Workspace";
  return { key: `page:${page || "home"}`, label: name, short: name };
}

function detectContextTarget(r = COMP.currentRoute) {
  const [page, r1] = r;
  const item = qs().get("item");
  if (item) {
    const task = W.find((w) => w.id === item);
    if (task?.who && task.who !== "you" && M[task.who]) return { type: "member", id: task.who, hint: task.id };
  }
  if (page === "inbox" && r1) {
    const it = IN.find((i) => i.id === r1);
    if (it?.from && it.from !== "you" && M[it.from]) return { type: "member", id: it.from, hint: "Inbox" };
  }
  if (page === "members" && r1 && r1 !== "you" && M[r1]) return { type: "member", id: r1, hint: null };
  if (page === "projects" && r1 && P[r1]?.lead && M[P[r1].lead]) return { type: "member", id: P[r1].lead, hint: "Lead" };
  return COMP.lastNonTess;
}

function targetDisplay(t = COMP.target) {
  if (t.type === "tess") return { name: "Tess", sub: "Chief of Staff", chatHref: "#/chat", icon: "sparkle" };
  if (t.type === "channel" && CH[t.id]) {
    return { name: `#${CH[t.id].name}`, sub: CH[t.id].topic || "Channel", chatHref: `#/chat/${t.id}`, icon: "hash" };
  }
  const m = M[t.id];
  const chId = ensureDmChannel(t.id);
  const pres = m?.paused ? "Paused" : (PRESENCE_LABEL[m?.live] || "Available");
  return { name: m ? m.name.split(" ")[0] : t.id, fullName: m?.name || t.id, sub: m ? `${m.role} · ${pres}` : "Member", chatHref: `#/chat/${chId}`, memberId: t.id };
}

function hasMessagesForTarget(t = COMP.target) {
  if (t.type === "tess") return CMDK.aiHistory.length > 0;
  const resolved = resolveChannelForTarget(t);
  return !!(resolved && resolved.ch.msgs.length > 0);
}

/* ---------------------------------------------------------------- rendering */

function renderRecipientPill(t, active) {
  const info = targetDisplay(t);
  const key = targetKey(t);
  const leading = t.type === "tess"
    ? ic("sparkle", "ic14")
    : t.type === "channel" ? ic("hash", "ic14") : av(t.id, 18);
  const hint = t.hint ? `<span class="cmp-pill-hint">${esc(t.hint)}</span>` : "";
  return `<button type="button" class="cmp-rec-pill${active ? " is-active" : ""}" data-act="cmp-target" data-v="${esc(key)}" aria-pressed="${active}">
    ${leading}<span>${esc(info.name)}</span>${hint}
  </button>`;
}

function renderRecipientStrip() {
  const ctxTarget = detectContextTarget();
  const secondary = COMP.target.type !== "tess"
    ? (isSameTarget(COMP.target, ctxTarget) ? ctxTarget : COMP.target)
    : ctxTarget;
  const tessPill = renderRecipientPill({ type: "tess" }, COMP.target.type === "tess");
  const secondPill = secondary ? renderRecipientPill(secondary, isSameTarget(COMP.target, secondary)) : "";
  return `<div class="cmp-rec-strip" role="group" aria-label="Conversation recipient">
    ${tessPill}
    ${secondPill}
    <button type="button" class="cmp-at-btn${COMP.pickerOpen ? " is-active" : ""}" data-act="cmp-picker-toggle" aria-expanded="${COMP.pickerOpen}" aria-label="Switch agent or channel" title="Talk to another agent or channel (@)">
      <span>@</span>${ic("chev-down", "ic14")}
    </button>
  </div>`;
}

function renderPickerAgentRows(q) {
  const matches = (id) => !q || M[id].name.toLowerCase().includes(q) || M[id].role.toLowerCase().includes(q);
  const agents = memberIds().filter((id) => id !== "you" && !M[id].builtin && !M[id].offboarded && matches(id));
  return agents.slice(0, 6).map((id) => {
    const m = M[id], pres = m.paused ? "Paused" : (PRESENCE_LABEL[m.live] || "Available");
    return `<button type="button" class="cmp-pick-row" data-act="cmp-pick" data-v="member:${esc(id)}">
      ${av(id, 22)}<span class="cmp-pick-copy"><strong>${esc(m.name)}</strong><span>${esc(m.role)}</span></span>
      <span class="cmp-pick-meta">${esc(pres)}</span>
    </button>`;
  }).join("");
}

function renderPickerChannelRows(q) {
  const matches = (k) => !q || CH[k].name.toLowerCase().includes(q) || (CH[k].topic || "").toLowerCase().includes(q);
  const channels = Object.keys(CH).filter((k) => CH[k].kind === "channel" && matches(k));
  return channels.slice(0, 4).map((k) => `<button type="button" class="cmp-pick-row" data-act="cmp-pick" data-v="channel:${esc(k)}">
    <span class="cmp-pick-ch">${ic("hash", "ic14")}</span>
    <span class="cmp-pick-copy"><strong>#${esc(CH[k].name)}</strong><span>${esc(CH[k].topic || "Channel")}</span></span>
  </button>`).join("");
}

function renderPickerPopover() {
  if (!COMP.pickerOpen) return "";
  const q = COMP.pickerQuery.trim().toLowerCase();
  const agentRows = renderPickerAgentRows(q);
  const chRows = renderPickerChannelRows(q);
  const empty = !agentRows && !chRows ? `<p class="cmp-pick-empty">No matching agent or channel.</p>` : "";
  return `<div class="cmp-picker" role="listbox" aria-label="Choose agent or channel">
    <input id="cmp-picker-input" class="cmp-picker-in" type="search" value="${esc(COMP.pickerQuery)}" placeholder="Filter agents or #channels…" autocomplete="off" aria-label="Filter agents or channels">
    <div class="cmp-pick-list">
      ${agentRows ? `<div class="cmp-pick-sec">Agents &amp; People</div>${agentRows}` : ""}
      ${chRows ? `<div class="cmp-pick-sec">Channels</div>${chRows}` : ""}
      ${empty}
    </div>
  </div>`;
}

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
  const offline = isOfflineMode();
  const busy = COMP.target.type === "tess" ? getActiveTessJobs() : [];
  const busyHtml = busy.length ? `<div class="tess-busy-strip" role="status">
    <span class="notif-job-spin" aria-hidden="true"></span>
    <span>${esc(busy.length > 1 ? `Refining ${busy.length} tasks in background…` : `${busy[0].title} — ${busy[0].step || "Refining…"}`)}</span>
  </div>` : "";
  const offMsg = COMP.target.type === "tess"
    ? "Offline quick summary · Local data only"
    : "HQ office offline · Messages will send on reconnect";
  const offHtml = offline ? `<div class="tess-offline-note">${ic("offline", "ic14")}<span>${offMsg}</span></div>` : "";
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

function renderChannelBody() {
  const resolved = resolveChannelForTarget();
  const msgs = resolved ? resolved.ch.msgs.slice(-12) : [];
  const ctx = describeRouteContext();
  const info = targetDisplay();
  if (!msgs.length) {
    const prompts = [
      `Quick status check on ${ctx.short}?`,
      `Can you take a look at ${ctx.short} when you have a moment?`,
    ];
    return `<div class="tess-chips" aria-label="Quick openers">${prompts.map((q) => `
      <button type="button" class="tess-chip" data-act="cmp-ask-chip" data-v="${esc(q)}">
        ${ic("chat", "ic14")}<span>${esc(q)}</span>
      </button>`).join("")}</div>`;
  }
  return `<div class="tess-thread">${msgs.map((msg) => {
    const mine = msg.who === "you";
    if (mine) return `<div class="tess-turn"><div class="tess-q">${esc(msg.text)}</div></div>`;
    const who = M[msg.who];
    return `<div class="tess-turn">
      <div class="tess-a">
        <div class="tess-a-hd">${av(msg.who, 16)}<span>${esc(who ? who.name : info.name)}</span><span class="cmp-msg-time">${esc(msg.time)}</span></div>
        <div class="tess-a-body">${esc(msg.text)}</div>
      </div>
    </div>`;
  }).join("")}</div>`;
}

function renderMinimizedPill(panel) {
  const info = targetDisplay();
  const busy = getActiveTessJobs();
  const status = busy.length ? `Refining (${busy.length})` : COMP.unreadReply ? "New reply" : info.sub.split(" · ").pop();
  const leading = COMP.target.type === "tess"
    ? ic("sparkle", "ic14")
    : COMP.target.type === "channel" ? ic("hash", "ic14") : av(COMP.target.id, 20);
  const dot = (COMP.unreadReply || busy.length > 0) ? `<span class="tess-live-dot" aria-hidden="true"></span>` : "";
  panel.innerHTML = `<div class="cmp-pill-bar">
    <button type="button" class="cmp-pill-restore" data-act="cmp-restore" aria-label="Restore ${esc(info.name)} companion">
      ${leading}<strong>${esc(info.name)}</strong><span class="cmp-pill-st">${esc(status)}</span>${dot}
    </button>
    <button type="button" class="btn ghost icon sm cmp-pill-x" data-act="cmp-close" aria-label="Close companion">${ic("x", "ic14")}</button>
  </div>`;
}

function syncCompanionClasses(panel, scrim, compact) {
  const open = COMP.mode !== "closed";
  const minimized = COMP.mode === "minimized";
  const expanded = COMP.mode === "expanded";
  const mobile = window.innerWidth <= 639;
  panel.hidden = !open;
  panel.classList.toggle("is-compact", COMP.mode === "open" && compact);
  panel.classList.toggle("is-expanded", expanded);
  panel.classList.toggle("is-minimized", minimized);
  panel.setAttribute("role", expanded || mobile ? "dialog" : "complementary");
  if (expanded || (open && !minimized && mobile)) panel.setAttribute("aria-modal", "true");
  else panel.removeAttribute("aria-modal");
  document.body.classList.toggle("companion-open", open && !minimized && !expanded);
  document.body.classList.toggle("companion-minimized", minimized);
  if (scrim) scrim.hidden = !(expanded || (open && !minimized && mobile));
}

function companionPlaceholder(info, ctx) {
  if (COMP.target.type === "tess") return "Ask Tess or type @agent…";
  const skipCtx = COMP.dismissedCtxKey === ctx.key || (COMP.target.type === "member" && ctx.key === `member:${COMP.target.id}`);
  return skipCtx ? `Message ${info.name}…` : `Message ${info.name} about ${ctx.short}…`;
}

function renderCompanionHeader(info, expanded) {
  const clearBtn = COMP.target.type === "tess" && CMDK.aiHistory.length
    ? `<button type="button" class="notif-text-btn" data-act="cmp-clear" title="Clear Tess thread">Clear</button>` : "";
  const expLbl = expanded ? "Restore floating size" : "Expand to studio view";
  const expTitle = expanded ? "Float in corner" : "Expand";
  return `<div class="cmp-hd">
    ${renderRecipientStrip()}
    <div class="cmp-hd-acts">
      ${clearBtn}
      <button type="button" class="btn ghost icon sm" data-act="cmp-minimize" aria-label="Minimize to pill" title="Minimize">${ic("chev-down", "ic14")}</button>
      <button type="button" class="btn ghost icon sm cmp-expand-btn" data-act="cmp-expand" aria-label="${expLbl}" title="${expTitle}">${ic(expanded ? "minimize" : "maximize", "ic14")}</button>
      <a class="btn ghost icon sm" href="${esc(info.chatHref)}" data-act="cmp-open-chat" aria-label="Open in Chat" title="Open in Chat">${ic("open", "ic14")}</a>
      <button type="button" class="btn ghost icon sm" data-act="cmp-close" aria-label="Close companion" title="Close (Esc)">${ic("x", "ic14")}</button>
    </div>
  </div>`;
}

function renderCompanion() {
  const panel = document.getElementById("tess-panel");
  if (!panel) return;
  const compact = !hasMessagesForTarget() && !COMP.pickerOpen;
  syncCompanionClasses(panel, document.getElementById("tess-panel-scrim"), compact);
  if (COMP.mode === "closed") return;
  if (COMP.mode === "minimized") { renderMinimizedPill(panel); return; }

  const info = targetDisplay();
  const draft = COMP.drafts.get(targetKey()) || "";
  const placeholder = companionPlaceholder(info, describeRouteContext());
  const threadView = COMP.target.type === "tess" ? renderTessBody() : renderChannelBody();

  panel.innerHTML = `
    ${renderCompanionHeader(info, COMP.mode === "expanded")}
    ${renderPickerPopover()}
    ${renderContextRow()}
    ${renderActiveBanners()}
    <div class="tess-body" id="tess-panel-body">${threadView}</div>
    <form class="tess-compose" data-form="tess-panel-ask">
      <input id="tess-panel-input" type="text" value="${esc(draft)}" placeholder="${esc(placeholder)}" autocomplete="off" aria-label="${esc(placeholder)}">
      <button type="submit" class="btn primary sm icon tess-send-btn" aria-label="Send message">${ic("send", "ic14")}</button>
    </form>`;

  const body = document.getElementById("tess-panel-body");
  if (body) body.scrollTop = body.scrollHeight;
}

/* ---------------------------------------------------------------- messaging & state transitions */

function replyFromAgent(memberId, text, chId) {
  const m = M[memberId];
  if (!m || m.kind !== "agent") return;
  const ctx = describeRouteContext();
  const useGeneric = COMP.dismissedCtxKey === ctx.key || ctx.key === `member:${memberId}`;
  const reply = `Got it — checking ${useGeneric ? "that" : ctx.short} now. I'll follow up here and post any update on the task.`;
  setTimeout(() => {
    CH[chId].msgs.push({ id: `r-${Date.now()}`, who: memberId, time: "now", text: reply });
    if (COMP.mode === "minimized") COMP.unreadReply = true;
    renderCompanion();
    onStateChange?.();
  }, 650);
}

function sendCompanionMessage(rawText) {
  const q = String(rawText || "").trim();
  if (!q) return;
  COMP.drafts.delete(targetKey());
  COMP.pickerOpen = false;
  COMP.pickerQuery = "";
  if (COMP.target.type === "tess") {
    const ans = answerOrgQuestion(q);
    if (ans) {
      ans._state = S.homeState;
      CMDK.aiAns = ans;
      CMDK.aiHistory = [ans, ...CMDK.aiHistory.filter((x) => x.q.toLowerCase() !== ans.q.toLowerCase())];
    }
  } else {
    const resolved = resolveChannelForTarget();
    if (resolved) {
      resolved.ch.msgs.push({ id: `u-${Date.now()}`, who: "you", time: "now", text: q });
      if (!isOfflineMode() && COMP.target.type === "member") replyFromAgent(COMP.target.id, q, resolved.id);
    }
  }
  renderCompanion();
  document.getElementById("tess-panel-input")?.focus({ preventScroll: true });
}

function setCompanionTarget(key) {
  COMP.pickerOpen = false;
  COMP.pickerQuery = "";
  COMP.unreadReply = false;
  if (key === "tess") {
    COMP.target = { type: "tess" };
  } else {
    const [type, id] = key.split(":");
    COMP.target = { type, id };
    COMP.lastNonTess = { type, id };
  }
  renderCompanion();
  onStateChange?.();
  document.getElementById("tess-panel-input")?.focus({ preventScroll: true });
}

function openCompanion(initialQuestion = "", trigger = null) {
  if (CMDK.open) closeCmdk(false);
  if (window.innerWidth < 900) closeNotifHook(false);
  if (COMP.mode === "closed") COMP.returnTo = trigger || document.getElementById("tess-trigger");
  COMP.mode = COMP.mode === "expanded" ? "expanded" : "open";
  COMP.unreadReply = false;
  if (initialQuestion && String(initialQuestion).trim()) {
    COMP.target = { type: "tess" };
    sendCompanionMessage(initialQuestion);
  } else {
    renderCompanion();
  }
  onStateChange?.();
  document.getElementById("tess-panel-input")?.focus({ preventScroll: true });
}

function closeCompanion(restoreFocus = false) {
  if (COMP.mode === "closed") return;
  COMP.mode = "closed";
  COMP.pickerOpen = false;
  COMP.pickerQuery = "";
  renderCompanion();
  onStateChange?.();
  if (restoreFocus && COMP.returnTo && document.body.contains(COMP.returnTo)) COMP.returnTo.focus();
  COMP.returnTo = null;
}

function toggleCompanion(trigger) {
  if (COMP.currentRoute[0] === "chat") {
    document.getElementById("compose")?.focus();
    return;
  }
  if (COMP.mode === "closed" || COMP.mode === "minimized") { openCompanion("", trigger); return; }
  const panel = document.getElementById("tess-panel");
  if (panel?.contains(document.activeElement) || trigger?.id === "tess-trigger") closeCompanion(true);
  else document.getElementById("tess-panel-input")?.focus({ preventScroll: true });
}

function minimizeCompanion() {
  if (COMP.mode === "closed") return;
  COMP.mode = "minimized";
  COMP.pickerOpen = false;
  renderCompanion();
  onStateChange?.();
}

function toggleExpandCompanion() {
  if (COMP.mode === "closed") return;
  COMP.mode = COMP.mode === "expanded" ? "open" : "expanded";
  renderCompanion();
  document.getElementById("tess-panel-input")?.focus({ preventScroll: true });
}

function toggleCompanionPicker() {
  COMP.pickerOpen = !COMP.pickerOpen;
  COMP.pickerQuery = "";
  renderCompanion();
  if (COMP.pickerOpen) document.getElementById("cmp-picker-input")?.focus({ preventScroll: true });
}

function handleCompanionInput(target) {
  if (target.id === "cmp-picker-input") {
    COMP.pickerQuery = target.value;
    const list = document.querySelector("#tess-panel .cmp-picker");
    if (list) {
      const sel = [target.selectionStart, target.selectionEnd];
      renderCompanion();
      const next = document.getElementById("cmp-picker-input");
      next?.focus({ preventScroll: true });
      if (next && sel[0] != null) next.setSelectionRange(...sel);
    }
    return true;
  }
  if (target.id === "tess-panel-input") {
    const val = target.value;
    COMP.drafts.set(targetKey(), val);
    if (val.startsWith("@") && !val.includes(" ")) {
      COMP.pickerOpen = true;
      COMP.pickerQuery = val.slice(1);
      const sel = [target.selectionStart, target.selectionEnd];
      renderCompanion();
      const next = document.getElementById("tess-panel-input");
      next?.focus({ preventScroll: true });
      if (next && sel[0] != null) next.setSelectionRange(...sel);
    } else if (COMP.pickerOpen && !val.startsWith("@")) {
      COMP.pickerOpen = false;
      COMP.pickerQuery = "";
      renderCompanion();
      document.getElementById("tess-panel-input")?.focus({ preventScroll: true });
    }
    return true;
  }
  return false;
}

function handleCompanionEscape() {
  if (COMP.mode === "closed") return false;
  if (COMP.pickerOpen) { COMP.pickerOpen = false; COMP.pickerQuery = ""; renderCompanion(); document.getElementById("tess-panel-input")?.focus(); return true; }
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
  if (COMP.mode !== "closed") renderCompanion();
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
  minimizeCompanion,
  openCompanion,
  registerCompanionHooks,
  renderCompanion,
  sendCompanionMessage,
  setCompanionTarget,
  syncCompanionRoute,
  toggleCompanion,
  toggleCompanionPicker,
  toggleExpandCompanion,
};
