/* Organization processes and standards (processes UX doc-54 §2): the effective set for one place, matched by key,
   with the level each document comes from, what it replaces, and the problems that keep a document out.
   Used by Settings › Processes (any place, chosen with Show) and by a project's Docs › Processes and Standards. */
import { M, P, TEAMS } from "../data.js";
import { projectDevProcess } from "../devproc-data.js";
import {
  KIND_WORDS, PROJECT_TEAM, agentWork, levelName, levelOf, parsePlace, placeKey, placeLabel, placeRows, places, sourceText, teamChain,
} from "../proc-data.js";
import { S, qs } from "../state.js";
import { av, empty, esc, hdr, ic, pill, searchRow, searchToggle, showMenu, skel } from "../ui.js";

Object.assign(S, { procQuery: "", procState: "live", procKind: "all" });

/* "all" lists both kinds in one list (a project's Docs, with the Kind filter); Settings shows one kind per tab */
const kindsOf = (kind) => (kind === "all" ? ["process", "standard"] : [kind]);
function rowsFor(pl, kind) {
  const parts = kindsOf(kind).map((k) => placeRows(pl, k));
  return { entries: parts.flatMap((p) => p.entries), problems: parts.flatMap((p) => p.problems) };
}
const kindBadge = (d) => `<span class="proc-badge is-kind">${ic(KIND_WORDS[d.kind].icon, "ic14")}${d.kind === "process" ? "Process" : "Standard"}</span>`;

const LEVEL_ICON = { org: "office", team: "user-group", project: "folder" };
const LEVEL_WORD = { org: "Organization", team: "Team", project: "Project" };

/* Badges: a word plus an icon, never colour alone (PROC-18) */
const levelBadge = (d) => { const lv = levelOf(d); return `<span class="proc-badge" data-level="${lv.type}" title="${esc(levelName(lv))}">${ic(LEVEL_ICON[lv.type], "ic14")}${LEVEL_WORD[lv.type]}</span>`; };
const lockBadge = (d) => (d.overridable === false ? `<span class="proc-badge is-lock">${ic("lock", "ic14")}Not overridable</span>` : "");
function relationBadge(entry, pl) {
  const d = entry.winner, lv = levelOf(d);
  if (entry.replaced.length) return `<span class="proc-badge is-replace">${ic("refresh", "ic14")}Replaces ${esc(sourceText(entry.replaced[entry.replaced.length - 1]))}</span>`;
  if (lv.type === "org") return "";
  const here = pl.type === lv.type && pl.id === lv.id;
  return `<span class="proc-badge is-add">${ic("plus", "ic14")}${here ? "Added here" : `Added by ${esc(levelName(lv))}`}</span>`;
}
const statusPill = (d) => (d.status === "draft" ? pill("Draft", "gray", "pencil") : d.status === "deprecated" ? pill("Deprecated", "outline", "archive") : "");
const ownerHtml = (id) => (id && M[id] ? `<span class="proc-owner">${av(id, 24)}${esc(M[id].name)}</span>` : `<span>No owner</span>`);

function rowHtml(entry, pl) {
  const d = entry.winner;
  return `<li class="linked plan-row proc-row" data-doc="${esc(d.id)}">
    <span class="proc-row-ic" aria-hidden="true">${ic(KIND_WORDS[d.kind].icon, "ic20")}</span>
    <div class="grow"><p class="rt"><button class="stretch proc-open" type="button" data-proc-act="open" data-id="${esc(d.id)}">${esc(d.title)}</button></p>
      <div class="meta"><code class="proc-key">${esc(d.key)}</code><span>From ${esc(sourceText(d))}</span>${ownerHtml(d.owner_employee_id)}<span>Updated ${esc(d.updated)}</span></div></div>
    <div class="proc-badges">${S.procKindBadges ? kindBadge(d) : ""}${levelBadge(d)}${relationBadge(entry, pl)}${lockBadge(d)}${statusPill(d)}</div>
    ${ic("chev-right", "ic20 chev")}</li>`;
}

const PROBLEM_ACTION = { invalid: ["proc-fix", "Fix", "wrench"], clash: ["proc-rename", "Rename…", "pencil"], sealed: ["open", "Open", "eye"] };
function problemRow(p) {
  const d = p.doc, [act, label, icon] = PROBLEM_ACTION[p.type];
  const tag = { invalid: "Details not recognized", clash: "Key clash", sealed: "Replacement refused" }[p.type];
  return `<li class="plan-row proc-row is-problem" data-doc="${esc(d.id)}">
    <span class="proc-row-ic" aria-hidden="true">${ic("warn", "ic20")}</span>
    <div class="grow"><p class="rt"><span class="proc-title">${esc(d.title)}</span></p>
      <div class="meta"><code class="proc-key">${esc(d.key)}</code><span>${esc(sourceText(d))}</span>${ownerHtml(d.owner_employee_id)}</div>
      <p class="proc-problem">${esc(p.message)}${p.field ? ` Field: <code>${esc(p.field)}</code>.` : ""}</p></div>
    <div class="proc-badges">${levelBadge(d)}<span class="proc-badge is-warn">${ic("warn", "ic14")}${tag}</span></div>
    <button class="btn outline sm proc-row-btn" type="button" data-proc-act="${act}" data-id="${esc(d.id)}">${ic(icon, "ic16")}${label}<span class="sr-only"> ${esc(d.title)}</span></button></li>`;
}

const GROUP_NOTE = {
  must: "Delivered in full to every agent this applies to. No level below can replace them.",
  guide: { process: "Agents get each one's summary and retrieve the full text when they need it.", standard: "Agents look these up when a process or task calls for them.", all: "Processes reach agents as summaries with the full text retrievable; standards are looked up when a process or task calls for them." },
  draft: "Not delivered to agents until marked Current.",
  deprecated: "No longer delivered. A document stops applying when its own level deprecates it.",
  problems: "Left out of the effective set until fixed.",
};
function group(id, title, note, items) {
  if (!items) return "";
  return `<section class="work-group plan-group proc-group" aria-labelledby="proc-g-${id}">
    <h3 class="work-group-title" id="proc-g-${id}">${title}<span class="cnt">${items.length}</span></h3>
    <p class="proc-group-note">${esc(note)}</p><ul class="rows plan-rows" role="list">${items.html}</ul></section>`;
}
const listOf = (arr, fn) => (arr.length ? { length: arr.length, html: arr.map(fn).join("") } : null);

function matches(d, q) { return !q || [d.title, d.key, sourceText(d)].join(" ").toLowerCase().includes(q); }

function groupsHtml(pl, kind) {
  const q = S.procQuery.trim().toLowerCase(), { entries, problems } = pl.type === "org" || kind !== "all" ? placeRows(pl, kind) : rowsFor(pl, kind);
  const shown = entries.filter((e) => matches(e.winner, q)), probs = problems.filter((p) => matches(p.doc, q));
  const current = shown.filter((e) => e.winner.status === "current");
  const row = (e) => rowHtml(e, pl);
  const html = group("must", "Must follow", GROUP_NOTE.must, listOf(current.filter((e) => e.winner.overridable === false), row))
    + group("guide", "Guidance", GROUP_NOTE.guide[kind], listOf(current.filter((e) => e.winner.overridable !== false), row))
    + group("draft", "Drafts", GROUP_NOTE.draft, listOf(shown.filter((e) => e.winner.status === "draft"), row))
    + group("deprecated", "Deprecated", GROUP_NOTE.deprecated, listOf(shown.filter((e) => e.winner.status === "deprecated"), row))
    + group("problems", "Not applied", GROUP_NOTE.problems, listOf(probs, problemRow));
  if (html) return html;
  return q ? `<p class="empty-row">Nothing matches “${esc(S.procQuery.trim())}”.</p>` : emptyHtml(kind === "all" ? "process" : kind, pl);
}

function emptyHtml(kind, pl) {
  const label = kind === "process" ? "Use the review-and-merge template" : "Use the technology stack template";
  const tpl = kind === "process" ? "review-and-merge" : "stack";
  const text = kind === "process" ? "Write down how your agents work. Start with how changes get reviewed and merged." : "Write down what you build with and how you run it. Start with your technology stack.";
  return empty(KIND_WORDS[kind].icon, `No ${KIND_WORDS[kind].title.toLowerCase()} yet`, text, "proc-empty",
    `<button class="btn primary" type="button" data-proc-act="new" data-v="${tpl}" data-place="${esc(placeKey(pl))}">${ic("file-text", "ic16")}${label}</button>`);
}

/* The chain that produced this place's set (doc-34 §3.1), with the words agents get */
function chainHtml(pl) {
  const hop = (icon, text) => `<li>${ic(icon, "ic14")}<span>${esc(text)}</span></li>`;
  const items = [hop("office", "Organization")];
  const lv = pl.type === "agent" ? (agentWork(pl.id) ? { type: "project", id: agentWork(pl.id).project } : null) : pl;
  if (lv?.type === "team") teamChain(lv.id).forEach((t) => items.push(hop("user-group", TEAMS[t].name)));
  if (lv?.type === "project") { teamChain(PROJECT_TEAM[lv.id]).forEach((t) => items.push(hop("user-group", TEAMS[t].name))); items.push(hop("folder", P[lv.id].name)); }
  return `<ol class="proc-chain" aria-label="Resolution order, organization first">${items.join(`<li aria-hidden="true">${ic("chev-right", "ic14")}</li>`)}</ol>`;
}

function contextHtml(pl, kind) {
  let lead = "";
  if (pl.type === "org") lead = "These apply to every team and project. A team or project can add its own, or replace one by using its key, unless it's marked not overridable.";
  if (pl.type === "team") lead = `What ${TEAMS[pl.id].name} follows for its projectless work. A project's own documents are added for work in that project.`;
  if (pl.type === "project") lead = `What every agent working in ${P[pl.id].name} follows, through the project's owning team, ${TEAMS[PROJECT_TEAM[pl.id]].name}. Assignment and an agent's home team add nothing.`;
  if (pl.type === "agent") lead = agentLead(pl.id);
  const define = kindsOf(kind).map((k) => `<p class="proc-define">${ic(KIND_WORDS[k].icon, "ic16")}<span><b>${KIND_WORDS[k].title}:</b> ${esc(KIND_WORDS[k].define)}.</span></p>`).join("");
  return `<div class="proc-context">${define}
    <p class="proc-lead">${esc(lead)}</p>${pl.type === "org" ? "" : chainHtml(pl)}${inheritedNote(pl)}${devProcessLine(pl)}</div>`;
}
/* Inherited documents can't be switched off below (doc-54 PROC-4, doc-31 §4) */
const inheritedNote = (pl) => (pl.type === "project" ? `<p class="proc-lead">${ic("lock", "ic14")} Documents inherited from the organization or a team can't be marked not applicable here. The project adds its own, or replaces an overridable one by using its key.</p>` : "");
function agentLead(aid) {
  const w = agentWork(aid), m = M[aid];
  if (!w) return "No current work. Showing the organization's processes. Team and project processes apply when it starts a task.";
  const task = w.task ? `${w.task.id} ${w.task.title}` : "its current work";
  return `What ${m.name} follows for ${task} in ${P[w.project].name}. Resolved through the project's owning team, ${TEAMS[PROJECT_TEAM[w.project]].name}; ${m.name}'s home team adds nothing to project work.`;
}
/* PROC-35: a project that follows a development process names it; its skills are not listed here */
function devProcessLine(pl) {
  const pid = pl.type === "project" ? pl.id : pl.type === "agent" ? agentWork(pl.id)?.project : null;
  const dp = pid && projectDevProcess(pid);
  if (!dp) return "";
  return `<p class="proc-devline">${ic("files", "ic16")}<span>This project follows <a class="alink" href="#/settings/dev-processes?open=${esc(dp.id)}">${esc(dp.name)}</a>. Its skills are shown there.</span></p>`;
}

function stateBody(pl, kind) {
  if (S.procState === "loading") return `<div class="proc-loading">${skel(4, `Loading ${KIND_WORDS[kind].title.toLowerCase()}`, false)}</div>`;
  if (S.procState === "empty" && pl.type === "org") return emptyHtml(kind, pl);
  const stale = S.procState === "stale"
    ? `<div class="callout proc-callout" role="status">${ic("offline", "ic20")}<div><p><b>Can't reach the organization repository.</b> Showing the last copy from 09:12.</p><p class="muted">Changes are saved when it's back.</p></div></div>` : "";
  return stale + `<div class="plan-results proc-results" id="proc-results">${groupsHtml(pl, kind)}</div>`;
}

const displayMenu = () => showMenu("proc-display", [["procState", "Preview state", [["live", "Live"], ["loading", "Loading"], ["stale", "Repository unreachable"], ["empty", "No documents yet"]]]],
  { icon: "adjustments", label: "Display", text: "Display", align: "right" });

function kindTabs(hrefFor, pl, kind) {
  const count = (k) => placeRows(pl, k).entries.filter((e) => e.winner.status === "current").length;
  return ["process", "standard"].map((k) => `<a class="queue-tab" href="${hrefFor(k)}"${k === kind ? ' aria-current="page"' : ""}>${KIND_WORDS[k].title}<span>${count(k)}</span></a>`).join("");
}

/* The toolbar: Processes | Standards, then search and Display, then the agent preview and New */
const kindMenu = () => showMenu("proc-kind", [["procKind", "Kind", [["all", "Processes and standards"], ["process", "Processes"], ["standard", "Standards"]]]],
  { icon: "filter", label: "Filter by kind", text: "Kind", active: S.procKind !== "all" });
function procToolbar(pl, kind, tabsHtml) {
  const label = kind === "all" ? "Search processes and standards" : `Search ${KIND_WORDS[kind].title.toLowerCase()}`;
  const newKind = kind === "all" ? "process" : kind;
  const preview = pl.type === "project" || (pl.type === "agent" && agentWork(pl.id))
    ? `<button class="btn outline proc-preview-btn" type="button" data-proc-act="preview" data-place="${esc(placeKey(pl))}">${ic("eye", "ic16")}What agents receive</button>` : "";
  return `<div class="work-subbar-header proc-subbar"><div class="work-toolbar queue-toolbar proc-toolbar">
      <div class="primary-tab-row"><nav class="queue-tabs" aria-label="Kind of document">${tabsHtml}</nav></div>
      <div class="queue-controls" role="group" aria-label="View controls">${searchToggle("proc-search", label, S.procQuery)}${pl.type === "project" ? kindMenu() : ""}${displayMenu()}</div>
      <div class="proc-toolbar-end">${preview}<button class="btn primary" type="button" data-proc-act="new" data-v="" data-kind="${newKind}" data-place="${esc(placeKey(pl))}">${ic("plus", "ic16")}New ${kind === "all" ? "process or standard" : KIND_WORDS[kind].one}</button></div>
    </div></div>${searchRow("proc-search", label, S.procQuery, `Search title or key… (Esc to close)`, "work-search-row")}`;
}

/* Settings › Processes ------------------------------------------------------------------------------ */
const settingsHref = (kind, pl) => `#/settings/processes${kind === "standard" ? "/standards" : ""}${pl.type === "org" ? "" : `?show=${placeKey(pl)}`}`;
function showSelect(pl) {
  const opts = places().map((p) => `<option value="${esc(placeKey(p))}"${placeKey(p) === placeKey(pl) ? " selected" : ""}>${esc(placeLabel(p))}</option>`).join("");
  return `<div class="proc-show"><label for="proc-show">Show</label><select class="field" id="proc-show">${opts}</select></div>`;
}
function settingsProcesses(sub) {
  const kind = sub === "standards" ? "standard" : "process", pl = parsePlace(qs().get("show"));
  const header = hdr("Processes", placeLabel(pl), showSelect(pl));
  const note = `<p class="desc proc-note">Organization processes and standards say how agents work, written down and inherited. A project's development process is separate: see <a class="alink" href="#/settings/dev-processes">Development processes</a>.</p>`;
  S.procKindBadges = false;
  const body = note + procToolbar(pl, kind, kindTabs((k) => settingsHref(k, pl), pl, kind)) + contextHtml(pl, kind) + stateBody(pl, kind);
  return { header, body };
}
const processesTitle = () => `${placeLabel(parsePlace(qs().get("show")))} · Processes`;

/* A project's Docs › Processes and Standards ------------------------------------------------------- */
/* One list with a Kind filter (doc-31: "process list --kind standard"); each row names its kind and its source */
function projectProcesses(pid, tabsHtml) {
  const pl = { type: "project", id: pid }, kind = ["process", "standard"].includes(S.procKind) ? S.procKind : "all";
  S.procKindBadges = kind === "all";
  return procToolbar(pl, kind, tabsHtml) + contextHtml(pl, kind) + stateBody(pl, kind);
}
const projectProcessCount = (pid) => placeRows({ type: "project", id: pid }, "process").entries.length + placeRows({ type: "project", id: pid }, "standard").entries.length;

export { LEVEL_ICON, LEVEL_WORD, levelBadge, lockBadge, ownerHtml, processesTitle, projectProcessCount, projectProcesses, settingsHref, settingsProcesses, statusPill };
