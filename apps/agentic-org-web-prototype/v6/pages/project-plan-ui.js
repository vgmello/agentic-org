/* Shared pieces of the project planning screens (Features, Releases): progress lines, status words,
   compact task rows and the page heading. Markup reuses the Work list and Projects list classes. */
import { M, P } from "../data.js";
import { FEATURE_STATUS_LABEL, featureRef, getFeature, getMilestone, getRelease, milestoneRef, releaseRef } from "../plan-data.js";
import { av, esc, ic, mlink, pill } from "../ui.js";
import { workStatus } from "./work.js";

const FEATURE_TONE = { planned: ["gray", null], in_progress: ["amber", null], live: ["green", "check-circle"], retired: ["outline", null] };
const featurePill = (status) => { const [tone, icon] = FEATURE_TONE[status] || ["gray", null]; return pill(FEATURE_STATUS_LABEL[status] || status, tone, icon); };

/* Releases and milestones have no stored status; this is the derived word (organization data 02). */
const PROGRESS_STATE = {
  empty: ["No tasks yet", "outline", null],
  todo: ["Not started", "gray", null],
  active: ["In progress", "amber", null],
  done: ["Done", "green", "check-circle"],
};
const statePill = (state) => { const [label, tone, icon] = PROGRESS_STATE[state]; return pill(label, tone, icon); };

/* The Projects list's progress line, with the counted words beside the bar. */
function planBar(done, total, label, words) {
  const pct = total ? Math.round((done * 100) / total) : 0;
  return `<div class="prog-line plan-prog"><div class="progress${total && done === total ? " done" : ""}" role="progressbar" aria-label="${esc(label)}" aria-valuemin="0" aria-valuemax="${total}" aria-valuenow="${done}" aria-valuetext="${esc(words)}"><span style="width:${pct}%"></span></div><span class="tnum">${esc(words)}</span></div>`;
}
const finishedWords = (p) => (p.total ? `${p.finished} of ${p.total} finished` : "No tasks yet");
const featureWords = (p) => (p.total ? `${p.done} of ${p.total} done` : "No tasks yet");

const workHref = (w) => `#/projects/${encodeURIComponent(w.proj)}/work?item=${encodeURIComponent(w.id)}`;

/* A compact Work row: status, reference, title (opens the task on Work), assignee and an optional remove. */
function planTaskRow(w, { remove = "", hideOwner = false } = {}) {
  return `<li class="work-item"><div class="work-row plan-task-row" data-work-id="${esc(w.id)}">
    <div class="work-summary">
      <span class="work-ident"><span class="work-ref">${esc(w.id)}</span></span>
      <span class="work-heading"><span class="work-markers">${workStatus(w.status)}</span><span><a class="work-title" href="${workHref(w)}">${esc(w.title)}</a></span></span>
    </div>
    <div class="work-facts">${hideOwner ? "" : `<span class="work-owner">${w.who && M[w.who] ? av(w.who, 24) : ""}${mlink(w.who)}</span>`}${remove}</div>
  </div></li>`;
}
const iconButton = (act, id, v, icon, label, cls = "") => `<button class="btn ghost icon plan-icon-btn${cls ? " " + cls : ""}" type="button" data-plan-act="${act}" data-id="${esc(id)}"${v != null ? ` data-v="${esc(v)}"` : ""} aria-label="${esc(label)}" title="${esc(label)}">${ic(icon, "ic20")}</button>`;
const planButton = (act, id, icon, label, variant = "outline", v = null) => `<button class="btn ${variant} plan-btn" type="button" data-plan-act="${act}" data-id="${esc(id)}"${v != null ? ` data-v="${esc(v)}"` : ""}>${icon ? ic(icon, "ic16") : ""}${esc(label)}</button>`;

/* A plan menu: the shared menu markup, with items that dispatch plan actions. */
function planMenu(id, trigger, items, cls = "outline") {
  return `<div class="menu-wrap"><button class="btn ${cls} plan-btn" type="button" data-act="menu" data-id="${id}" aria-haspopup="menu" aria-expanded="false" aria-controls="menu-${id}"${trigger.label ? ` aria-label="${esc(trigger.label)}" title="${esc(trigger.label)}"` : ""}>${trigger.html}</button>
    <div class="menu" id="menu-${id}" role="menu" hidden>${items.map(([act, itemId, v, icon, label, checked]) =>
      `<button role="${checked == null ? "menuitem" : "menuitemradio"}"${checked == null ? "" : ` aria-checked="${checked}"`} type="button" data-plan-act="${act}" data-id="${esc(itemId)}" data-v="${esc(v)}">${ic(icon, "ic20")}${esc(label)}</button>`).join("")}</div></div>`;
}

/* The detail heading inside a project tab: reference, title and state on one line, actions at its end. */
function planHead(ref, title, statusHtml, actions) {
  return `<header class="plan-head"><div class="plan-head-t"><span class="plan-ref">${esc(ref)}</span><h2 id="plan-title">${esc(title)}</h2>${statusHtml}</div>
    ${actions ? `<div class="plan-head-actions">${actions}</div>` : ""}</header>`;
}
const planFact = (dt, dd) => `<div><dt>${esc(dt)}</dt><dd>${dd}</dd></div>`;
/* Markdown bodies are shown as paragraphs; the mock keeps them as plain text. */
const planBody = (text, empty) => (text ? `<div class="plan-body">${text.split(/\n{2,}/).map((p) => `<p>${esc(p)}</p>`).join("")}</div>` : `<p class="plan-body is-empty">${esc(empty)}</p>`);

const featureHref = (f) => `#/projects/${encodeURIComponent(f.project_id)}/features/${encodeURIComponent(f.id)}`;
const releaseHref = (r) => `#/projects/${encodeURIComponent(r.project_id)}/releases/${encodeURIComponent(r.id)}`;
const featureLabel = (fid) => { const f = getFeature(fid); return f ? `${featureRef(f)} ${f.title}` : ""; };
const milestoneLabel = (mid) => { const m = getMilestone(mid), r = m && getRelease(m.release_id); return m ? `${releaseRef(r)} › ${milestoneRef(m)} ${m.title}` : ""; };
const ownerName = (id) => (id && M[id] ? M[id].name : "Unassigned");
const projectName = (pid) => P[pid]?.name || "";

export {
  featureHref,
  featureLabel,
  featurePill,
  featureWords,
  finishedWords,
  iconButton,
  milestoneLabel,
  ownerName,
  planBar,
  planBody,
  planButton,
  planFact,
  planHead,
  planMenu,
  planTaskRow,
  projectName,
  releaseHref,
  statePill,
};
