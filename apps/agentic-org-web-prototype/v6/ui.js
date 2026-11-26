import { A, CH, HEALTH, IN, M, OPEN, P, RECOMMENDATION_TONES, TEAMS, W } from "./data.js";
import { itemActionsV4 } from "./pages/home.js";
import { workHref } from "./pages/work.js";
import { S } from "./state.js";

/* ---------------------------------------------------------------- helpers */
const esc = (s) => String(s == null ? "" : s).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
const ic = (n, cls = "") => `<svg class="ic ${cls}" aria-hidden="true"><use href="#i-${n}"/></svg>`;
const SEP = '<span class="sep" aria-hidden="true"></span>';
const TINTS = ["#a3b3ff", "#5ee9b5", "#74d4ff", "#f4a8ff", "#ffa1ad", "#c4b4ff", "#46ecd5"];
function hash(s) { let h = 7; for (const c of s) h = (h * 31 + c.charCodeAt(0)) >>> 0; return h; }
function initials(n) { const w = n.trim().split(/\s+/); return (w.length > 1 ? w[0][0] + w[1][0] : w[0][0]).toUpperCase(); }
const memberIds = () => Object.keys(M).filter((k) => !M[k].owner);
const agentIds = () => Object.keys(M).filter((k) => M[k].kind === "agent");
const isPerson = (k) => M[k].kind === "person";
const unhandled = () => IN.filter((i) => !i.handled);
const openFrom = (id) => IN.filter((i) => !i.handled && i.from === id);
const plural = (n, one, many) => `${n} ${n === 1 ? one : many}`;

const PRES = {
  needs:     { label: "Needs you", icon: "bell", tone: "amber" },
  withyou:   { label: "With you", icon: "user", tone: "indigo" },
  working:   { label: "Working", icon: "bolt", tone: "indigo" },
  paused:    { label: "Paused", icon: "pause", tone: "gray" },
  available: { label: "Available", icon: "check-circle", tone: "green" },
  asleep:    { label: "Asleep", icon: "moon", tone: "gray" },
  offline:   { label: "Offline", icon: "offline", tone: "outline" },
  unknown:   { label: "Status unknown", icon: "question", tone: "outline" }
};
function presence(id) {
  const m = M[id]; if (!m || m.owner || m.kind === "person") return null;
  if (openFrom(id).length) return "needs";
  if (m.live === "withyou") return "withyou";
  if (m.live === "working") return "working";
  if (m.paused) return "paused";
  return PRES[m.live] ? m.live : "unknown";
}
function av(id, size = 32, mode = false) {
  const m = M[id]; if (!m) return `<span class="av av${size} empty" aria-hidden="true">${ic("user", "ic16")}</span>`;
  const p = mode ? presence(id) : null;
  const dot = p && p !== "unknown" ? `<span class="avdot p-${p}"></span>` : "";
  const tip = dot && mode === "tip";
  return `<span class="av av${size}" style="--av-tint:${TINTS[hash(id) % 7]}" aria-hidden="true"${tip ? ` title="${esc(m.name)} · ${PRES[p].label}"` : ""}>${esc(initials(m.name))}${dot}</span>${tip ? `<span class="sr-only">(${PRES[p].label})</span>` : ""}`;
}
const pill = (text, tone = "gray", icon = null, chip = false) => `<span class="pill t-${tone}${chip ? " chip" : ""}">${icon ? ic(icon, "ic14") : ""}${esc(text)}</span>`;
const presPill = (id, profile = false) => { const p = presence(id); return p && (p !== "unknown" || profile) ? pill(PRES[p].label, PRES[p].tone, PRES[p].icon) : ""; };
const presInline = (id) => { const p = presence(id); return p && p !== "unknown" ? `<span class="pin c-${PRES[p].tone}">${ic(PRES[p].icon, "ic14")}${PRES[p].label}</span>` : ""; };
const kindWord = (id) => (M[id].owner ? "Owner" : M[id].kind === "agent" ? "Agent" : "Person");
const mlink = (id, cls = "alink") => !M[id] ? "Unassigned" : M[id].owner ? esc(M[id].name) : `<a class="${cls}" href="#/members/${id}">${esc(M[id].name)}</a>`;
const plink = (p) => `<a class="alink" href="#/projects/${p}">${esc(P[p].name)}</a>`;
const joinSep = (parts) => parts.filter(Boolean).join(SEP);

const leadOf = (p) => (P[p].lead && M[P[p].lead] ? M[P[p].lead].name : "");
const leadTxt = (p) => (leadOf(p) ? "Lead " + leadOf(p) : "");

const projMembers = (p) => [...new Set(P[p].teams.flatMap((t) => TEAMS[t].members))];
const acts = () => A;

function statusLine(id) {
  const m = M[id], p = presence(id);
  const task = m.task ? `Working on ${esc(m.task)}${m.proj ? " · " + plink(m.proj) : ""}` : "";
  switch (p) {
    case "needs": { const it = openFrom(id)[0]; return `Waiting on you: <a class="alink" href="#/inbox/${it.id}">${esc(it.what)}</a>`; }
    case "withyou": return "In a session with you";
    case "working": return task;
    case "paused": return "Its work waits until it's resumed";
    case "available": return m.kind === "agent" ? "Idle, ready for work" : "Signed in";
    case "asleep": { const how = m.setup ? m.setup.how : "Wakes when called"; return esc(how === "Wakes when called" ? how : "Wakes " + how.charAt(0).toLowerCase() + how.slice(1)); }
    case "offline": return esc(m.last || "No session");
    default: return task;
  }
}
function hdr(title, count, trailing = "", extra = "") {
  return `<header class="ph"><div class="ph-t"><h1 tabindex="-1">${esc(title)}</h1>${extra}${count ? `<p class="cnt">${count}</p>` : ""}</div>${trailing ? `<div class="ph-actions">${trailing}</div>` : ""}</header>`;
}
function queueHeadline(title, count, announce = false) {
  return `<div class="ph queue-headline"><div class="ph-t"><h1 tabindex="-1">${esc(title)}</h1>
    <span class="cnt queue-count"${announce ? ' role="status"' : ""}><i aria-hidden="true"></i>${esc(count)}</span></div></div>`;
}
/* Section header shared by Home, project Overview and other pages. repTitle swaps the label in Report themes. */
function sectionHead({ id, title, repTitle = title, count = "", right = "", cls = "" }) {
  const label = repTitle === title ? title : `<span class="lbl-std">${title}</span><span class="lbl-rep">${repTitle}</span>`;
  return `<div class="sec-head${cls ? " " + cls : ""}"><div class="sec-head-l"><h2 id="${id}">${label}</h2>${count ? `<span class="sec-head-count">${count}</span>` : ""}</div>${right}</div>`;
}
/* Inline page search: a toolbar icon reveals a search row above the results ("/" opens it too). */
const searchIsOpen = (inputId, value) => Boolean(S.searchOpen?.[inputId] || String(value ?? "").trim());
const searchToggle = (inputId, label, value) => `<button class="btn ghost icon search-toggle${searchIsOpen(inputId, value) ? " is-active" : ""}" type="button" data-act="search-toggle" data-v="${inputId}" aria-label="${esc(label)}" title="${esc(label)} (/)" aria-expanded="${searchIsOpen(inputId, value)}" aria-controls="${inputId}-row" aria-keyshortcuts="/">${ic("search", "ic20")}</button>`;
const searchRow = (inputId, label, value, placeholder, cls = "") => searchIsOpen(inputId, value)
  ? `<div class="inline-search${cls ? " " + cls : ""}" id="${inputId}-row"><label>${ic("search", "ic16")}<span class="sr-only">${esc(label)}</span><input id="${inputId}" class="inline-search-input" type="search" value="${esc(value ?? "")}" placeholder="${esc(placeholder)}" autocomplete="off" spellcheck="false"></label><kbd aria-hidden="true">Esc</kbd></div>`
  : "";
/* Breadcrumb trail: ancestors as links, then the current page. items = [[href, label], ..., [null, currentLabel]] */
const crumbs = (items) => `<nav class="back crumbs" aria-label="Breadcrumb"><ol>${items.map(([href, label], i) => {
  const last = i === items.length - 1;
  return `<li>${last ? `<span aria-current="page">${esc(label)}</span>` : `<a href="${href}">${esc(label)}</a>${ic("chev-right", "ic14 crumb-sep")}`}</li>`;
}).join("")}</ol></nav>`;
function linkTabs(base, tabs, cur, label = "Sections") {
  return `<nav class="tabs" aria-label="${esc(label)}">${tabs.map(([k, l, n]) => `<a class="tab" data-menu-label="${esc(l)}" href="${base}${k ? "/" + k : ""}" ${cur === k ? 'aria-current="page"' : ""}>${esc(l)}${n != null ? `<span class="tcount">${n}</span>` : ""}</a>`).join("")}</nav>`;
}
function seg(key, opts, label) {
  return `<span class="seg-label" aria-hidden="true">${esc(label)}</span><div class="seg${key === "theme" ? " theme-options" : ""}" role="group" aria-label="${esc(label)}">${opts.map(([v, l, i]) => {
    const icon = typeof i === "string" && i ? ic(i, "ic16") : "";
    const count = typeof i === "number" ? `<span class="tcount">${i}</span>` : "";
    return `<button type="button" data-act="set" data-k="${key}" data-v="${v}" aria-pressed="${S[key] === v}">${icon}${esc(l)}${count}</button>`;
  }).join("")}</div>`;
}
function displayCheckbox(action, label, checked) {
  return `<button role="menuitemcheckbox" tabindex="-1" aria-checked="${checked}" type="button" data-act="${action}">${ic("check", "ic20")}${label}</button>`;
}

function sectionTabs(tabs, controls = "", actions = "", actionClass = "") {
  return `<div class="section-toolbar">
    <div class="primary-tab-row" data-page-menu>${tabs}</div>
    ${controls ? `<div class="queue-controls" role="group" aria-label="View controls">${controls}</div>` : ""}
    ${actions ? `<div class="ph-actions section-actions ${actionClass}">${actions}</div>` : ""}
  </div>`;
}
function iconWithTooltip(label, icon, size = "ic16") {
  return ic(icon, size) + `<span class="icon-tooltip" aria-hidden="true">${esc(label)}</span>`;
}
function showMenu(id, groups, { align = "", icon, label, active, focusTooltip = false, footer = "" } = {}) {
  const cur = groups.map(([k, , opts]) => { const o = opts.find((x) => String(x[0]) === String(S[k])); return o && o[0] !== "all" ? o[1] : null; }).filter(Boolean);
  const tooltip = focusTooltip ? "" : ` title="${esc(label)}"`;
  const triggerAttrs = icon
    ? `class="btn ghost icon${focusTooltip ? " icon-with-tooltip" : ""}" aria-label="${esc(label)}"${tooltip}${active ? " data-active" : ""}`
    : 'class="btn textbtn"';
  const triggerContent = icon ? (focusTooltip ? iconWithTooltip(label, icon, "ic20") : ic(icon, "ic20"))
    : `Show <span class="cur">${esc(cur.length ? cur.join(", ") : "all")}</span>${ic("chev-down", "ic16 muted")}`;
  return `<div class="menu-wrap"><button ${triggerAttrs} type="button" data-act="menu" data-id="${id}" aria-haspopup="menu" aria-expanded="false" aria-controls="menu-${id}">${triggerContent}</button>
    <div class="menu ${align}" id="menu-${id}" role="menu"${label ? ` aria-label="${esc(label)}"` : ""} hidden>${groups.map(([k, title, opts], gi) => `${gi ? "<hr>" : ""}<p class="menu-h" id="mh-${id}-${gi}">${esc(title)}</p><div role="group" aria-labelledby="mh-${id}-${gi}">${opts.map(([v, l, n]) => { const on = String(S[k]) === String(v); return `<button role="menuitemradio" aria-checked="${on}" type="button" data-act="set" data-k="${k}" data-v="${v}">${on ? ic("check", "ic20") : '<span class="spacer20" aria-hidden="true"></span>'}<span style="flex:1">${esc(l)}</span>${n != null ? `<span class="muted tnum">${n}</span>` : ""}</button>`; }).join("")}</div>`).join("")}${footer}</div></div>`;
}
const empty = (icon, title, text, cls = "", action = "") => `<div class="empty ${cls}">${ic(icon, "ic48")}<h3>${esc(title)}</h3><p>${esc(text)}</p>${action ? `<div style="margin-top:24px">${action}</div>` : ""}</div>`;
const skel = (n, label, avatar = true) => `<div class="skel" aria-busy="true"><span class="sr-only">${esc(label)}</span>${Array.from({ length: n }, (_, i) => `<div class="skel-row" aria-hidden="true">${avatar ? '<span class="skel-c"></span>' : ""}<span style="flex:1;display:flex;flex-direction:column;gap:8px"><span class="skel-b" style="width:${60 - i * 8}%"></span><span class="skel-b" style="width:${35 + i * 5}%;height:10px"></span></span></div>`).join("")}</div>`;
const officeDown = () => `<div class="section" style="padding-bottom:0"><div class="callout" role="alert">${ic("warn", "ic20 c-amber")}<div><p>Couldn't reach the office. Your data is safe.</p><button class="btn outline sm" type="button" data-act="set" data-k="homeState" data-v="filled">${ic("refresh", "ic16")}Try again</button></div></div></div>`;
const SPOT_ORDER = ["in1", "in18", "in5", "in2", "in17", "in16"];
const spotQueue = () => SPOT_ORDER.map((id) => IN.find((i) => i.id === id)).filter((i) => i && (!i.handled || i.handledNow));
function effectiveHealth(p) {
  if (p === "agency-v1") {
    const pending = ["in1", "in5"].map((id) => IN.find((i) => i.id === id)).filter((i) => i && !i.handled);
    if (!pending.length) return "On track";
  }
  return P[p] ? P[p].health : "";
}
function projPill(p) {
  const pr = P[p];
  if (pr.archived) return pill("Archived", "outline");
  if (!pr.health) return "";
  const h = HEALTH[pr.health]; return pill(pr.health, h.tone, h.icon);
}
function progress(p) {
  const ws = W.filter((w) => w.proj === p), total = ws.length, done = ws.filter((w) => !OPEN(w)).length, pct = total ? Math.round(done * 100 / total) : 0;
  if (!total) return `<p class="prog-line">No work items yet</p>`;
  return `<div class="prog-line"><div class="progress${P[p].archived ? " done" : ""}" role="progressbar" aria-label="${esc(P[p].name)} progress" aria-valuemin="0" aria-valuemax="${total}" aria-valuenow="${done}" aria-valuetext="${done} of ${total} work items done"><span style="width:${pct}%"></span></div><span class="tnum">${done} of ${total} done</span></div>`;
}
function avStack(ids, max = 5) {
  return `<span class="avstack">${ids.slice(0, max).map((i) => av(i, 24)).join("")}${ids.length > max ? `<span class="more">+${ids.length - max}</span>` : ""}</span><span class="sr-only">${ids.map((i) => M[i].name).join(", ")}</span>`;
}
function hireMenu(team) {
  const t = team ? TEAMS[team].name : "";
  return `<div class="menu-wrap"><button class="btn outline" type="button" data-act="menu" data-id="add" aria-haspopup="menu" aria-expanded="false" aria-controls="menu-add">${ic("user-plus", "ic20")}Hire or add${ic("chev-down", "ic16")}</button>
    <div class="menu" id="menu-add" role="menu" hidden><button role="menuitem" type="button" data-act="proto" data-v="Hire an agent${t ? " for " + esc(t) : ""}">${ic("user-plus", "ic20")}Hire an agent</button><button role="menuitem" type="button" data-act="proto" data-v="Add a person${t ? " to " + esc(t) : ""}">${ic("user-plus", "ic20")}Add a person</button></div></div>`;
}
function moreMenu(id, label, items, variant = "outline") {
  return `<div class="menu-wrap"><button class="btn ${variant} icon" type="button" data-act="menu" data-id="${id}" aria-haspopup="menu" aria-expanded="false" aria-controls="menu-${id}" aria-label="${esc(label)}" title="More actions">${ic("ellipsis", "ic20")}</button><div class="menu" id="menu-${id}" role="menu" hidden>${items}</div></div>`;
}
const menuItem = (act, v, icon, label, extra = "") => `<button role="menuitem" type="button" data-act="${act}" data-v="${esc(v)}"${extra}>${ic(icon, "ic20")}${esc(label)}</button>`;

/* ---------------------------------------------------------------- inbox items */
function sourceOf(it) {
  if (it.src) return it.src;
  return it.task ? { href: workHref({ item: it.task }), label: "Open " + it.task } : { href: `#/chat/${P[it.proj].channel}`, label: "Open #" + CH[P[it.proj].channel].name };
}
function itemReplyComposer(it, first) {
  const changes = S.replyMode === "changes", who = esc(M[it.from].name);
  const lbl = changes ? `What should ${who} change?` : `Reply to ${who}`;
  return `<div class="reply"><label class="sr-only" for="r-${it.id}">${lbl}</label><textarea id="r-${it.id}" rows="2" placeholder="${lbl}…"></textarea>
      <div class="actions"><button class="btn ${first} sm" type="button" data-act="send-reply" data-id="${it.id}">${ic("send", "ic16")}${changes ? "Send changes" : "Send reply"}</button><button class="btn ghost sm" type="button" data-act="cancel-reply" data-id="${it.id}">Cancel</button></div></div>`;
}
function problemActionButtons(it, first, btn) {
  return [
    it.resume ? btn(first, "handle", "Goal resumed", "play", "Resume") : "",
    it.assign ? btn(first, "assign", "", "user-plus", "Assign…") : "",
    it.dismiss ? btn("outline", "handle", "Dismissed", "x", "Dismiss") : "",
  ].filter(Boolean);
}
function itemActionButtons(it, first) {
  const btn = (style, act, v, icon, label) => `<button class="btn ${style} sm" type="button" data-act="${act}" data-id="${it.id}"${v ? ` data-v="${esc(v)}"` : ""}>${icon ? ic(icon, "ic16") : ""}${esc(label)}</button>`;
  if (it.acts) return it.acts.map(([label, outcome, icon, mode], i) => mode === "changes" ? btn(i ? "outline" : first, "reply", "changes", icon, label) : btn(i ? "outline" : first, "handle", outcome, icon, label));
  if (it.kind === "approval") return [btn(first, "handle", "Approved", "check", "Approve"), btn("danger-outline", "handle", "Denied", "deny", "Deny")];
  if (it.kind === "question") return [btn(first, "reply", "", "reply", "Reply")];
  if (it.kind === "decision") return it.opts.map((o, i) => btn(i ? "outline" : first, "handle", "Chose: " + o, null, o));
  return it.kind === "problem" ? problemActionButtons(it, first, btn) : [];
}
function itemActions(it, where) {
  if (it.handled) return `<p class="handled" id="handled-${esc(it.id)}">${esc(it.outcome)}</p>`;
  const card = where === "card", first = "primary";
  if (S.replying === it.id) return itemReplyComposer(it, first);
  const s = card ? null : sourceOf(it);
  /* Cards open through their title link, so the action row is just the decision (one line). */
  const open = card ? "" : `<a class="btn ghost sm" href="${s.href}">${ic("open", "ic16")}${esc(s.label)}</a>`;
  return `<div class="actions">${[...itemActionButtons(it, first), open].join("")}</div>`;
}
function recommendationCallout(it, headingTag = "p") {
  const tone = it.recTone || "neutral";
  const [icon, label] = RECOMMENDATION_TONES[tone] || ["info", ""];
  return `<div class="recommendation" data-tone="${tone}">
    ${ic(icon, "ic20")}<div>
      <${headingTag} class="recommendation-label">Recommendation${label ? " · " + label : ""}</${headingTag}>
      <p>${esc(it.rec)}</p>
    </div>
  </div>`;
}
/* Decision card used inside "Needs you" sections (project Overview, member pages).
   The section already says it needs you, so only "Handled" is badged; hideProject drops the project link on project pages. */
function needsCard(it, opts = {}) {
  return `<article class="card${it.handled ? " done" : ""}" aria-label="${esc(it.sub)} from ${esc(M[it.from].name)}">
    <div class="top">${av(it.from, 24)}<span class="grow">${mlink(it.from)} <span class="muted">· ${esc(it.sub)} · ${esc(it.time)}</span></span>${it.handled ? pill("Handled", "gray", "check-circle") : ""}</div>
    <h3><a class="needs-title" href="#/inbox/${esc(it.id)}">${esc(it.what)}</a></h3>
    <p class="why">${esc(it.why)}</p>
    ${it.rec && !it.handled ? recommendationCallout(it) : ""}
    <div class="meta" style="margin-top:0">${joinSep([
      opts.hideProject ? "" : plink(it.proj),
      it.task ? `<span>${esc(it.task)}</span>` : "",
      it.expires && !it.handled ? `<span class="pin wrap">${ic("clock", "ic14")}${esc(it.expires)}</span>` : "",
    ])}</div>
    ${itemActions(it, "card")}
  </article>`;
}

/* ---------------------------------------------------------------- feed */
const ATYPE = { work: { label: "Work", icon: "clipboard" }, conversation: { label: "Conversation", icon: "chat" }, decision: { label: "Decision", icon: "flag" }, org: { label: "Org change", icon: "user-group" }, run: { label: "Run", icon: "terminal" } };
function feedRunLogHtml(a) {
  if (a.type !== "run") return "";
  const open = S.openRuns[a.id];
  return `<div class="runbtn"><button class="btn ghost sm" type="button" data-act="run" data-id="${a.id}" aria-expanded="${!!open}" aria-controls="log-${a.id}">${ic(open ? "chev-down" : "chev-right", "ic16")}${open ? "Hide run log" : "Show run log"}</button></div>
      <ol class="runlog" id="log-${a.id}" ${open ? "" : "hidden"}>${a.log.map(([tm, tx]) => `<li><time>${esc(tm)}</time><span>${esc(tx)}</span></li>`).join("")}</ol>`;
}
function feedItemHtml(a, opts) {
  const t = ATYPE[a.type];
  const inboxItem = opts.triage && a.inbox && IN.find((item) => item.id === a.inbox);
  const stamp = opts.day === a.time ? "" : `<time>${esc(a.time)}</time>`;
  const proj = opts.noProj ? "" : " · " + plink(a.proj);
  const summary = a.summary ? `<br><span class="muted">${esc(a.summary)}</span>` : "";
  const triage = inboxItem ? `<div class="activity-triage" id="activity-triage-${inboxItem.id}" tabindex="-1"><a class="alink" href="#/inbox/${inboxItem.id}">${esc(inboxItem.what)}</a>${itemActionsV4(inboxItem, { hideDetails: true })}</div>` : "";
  return `<li><div class="l1">${av(a.who, 24, opts.noDots ? false : "tip")}<span class="actor">${mlink(a.who)}<span class="kind">${ic(t.icon, "ic14")}${t.label}</span></span>${stamp}</div>
      <p class="l2">${esc(a.text)}${proj}${summary}</p>
      ${feedRunLogHtml(a)}
      ${triage}</li>`;
}
function feed(items, opts = {}) {
  if (!items.length) return `<p class="empty-row">No activity yet.</p>`;
  const tag = opts.day ? "ol" : "ul";
  return `<${tag} class="feed${opts.day ? " activity-timeline" : ""}" role="list">${items.map((a) => feedItemHtml(a, opts)).join("")}</${tag}>`;
}

function toast(text, icon = "info", action = null) {
  const t = document.createElement("div");
  t.className = "toast";
  t.innerHTML = `${ic(icon)}<p>${esc(text)}</p>`;
  if (typeof action === "function") {
    const b = document.createElement("button");
    b.type = "button"; b.className = "toast-undo"; b.textContent = "Undo";
    b.addEventListener("click", () => { t.remove(); action(); });
    t.appendChild(b);
  } else if (action && typeof action === "object" && action.label) {
    const el = document.createElement(action.href ? "a" : "button");
    el.className = "toast-undo";
    el.textContent = action.label;
    if (action.href) {
      el.href = action.href;
      el.addEventListener("click", () => t.remove());
    } else {
      el.type = "button";
      el.addEventListener("click", () => { t.remove(); action.onClick?.(); });
    }
    t.appendChild(el);
  }
  const box = document.querySelector(".work-panel[open] .toasts") || document.getElementById("toasts");
  box.appendChild(t);
  setTimeout(() => t.remove(), action ? 5500 : 3200);
}

export {
  ATYPE,
  SEP,
  acts,
  av,
  avStack,
  crumbs,
  searchRow,
  sectionHead,
  searchToggle,
  displayCheckbox,
  effectiveHealth,
  empty,
  esc,
  feed,
  hash,
  hdr,
  hireMenu,
  ic,
  iconWithTooltip,
  isPerson,
  joinSep,
  kindWord,
  leadOf,
  leadTxt,
  linkTabs,
  memberIds,
  menuItem,
  mlink,
  moreMenu,
  needsCard,
  openFrom,
  pill,
  plink,
  plural,
  presInline,
  presPill,
  presence,
  progress,
  projMembers,
  projPill,
  queueHeadline,
  itemActions,
  recommendationCallout,
  sectionTabs,
  seg,
  showMenu,
  skel,
  spotQueue,
  statusLine,
  toast,
  unhandled,
};
