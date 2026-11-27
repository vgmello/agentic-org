/* Project › Docs: the project's documentation tree from its index (doc-86 "Project documentation tree", doc-32 §8),
   one document's page with issued versions, code links and history, and the project's processes and standards
   (doc-54 §2: "a Processes tab ... in each project's Docs"). */
import { M } from "../data.js";
import { projectDevProcess } from "../devproc-data.js";
import {
  KIND_ICON, featureDocs, featureLabel, getEntry, hasUnissued, isVersioned, kindTitle, latestVersion, projectSections, sectionEntries, treeEntries, versionTasks,
} from "../docs-data.js";
import { featureRef, getFeature, isFinished, projectFeatures } from "../plan-data.js";
import { S } from "../state.js";
import { av, empty, esc, ic, pill, searchRow, searchToggle, sectionHead } from "../ui.js";
import { md } from "./md.js";
import { planFact, planHead } from "./project-plan-ui.js";
import { projectProcesses } from "./processes.js";
import { placeRows } from "../proc-data.js";

Object.assign(S, { docQuery: "", docEditing: null });

const docHref = (e) => `#/projects/${encodeURIComponent(e.project_id)}/docs/${encodeURIComponent(e.id)}`;
const ownerHtml = (id) => (id && M[id] ? `<span class="proc-owner">${av(id, 24)}${esc(M[id].name)}</span>` : "");
const sizeText = (n) => (n >= 1024 ? `${(n / 1024).toFixed(1)} KB` : `${n} B`);

function docBadges(e) {
  const out = [];
  if (e.type === "diagram") out.push(`<span class="proc-badge">${ic("image", "ic14")}Diagram</span>`);
  const v = latestVersion(e);
  if (v) out.push(`<span class="proc-badge is-version">${ic("doc-check", "ic14")}v${v.version}</span>`);
  if (hasUnissued(e)) out.push(`<span class="proc-badge is-warn">${ic("pencil", "ic14")}Unissued changes</span>`);
  if (isVersioned(e.project_id, e.kind) && !v) out.push(`<span class="proc-badge">${ic("clock", "ic14")}Not issued yet</span>`);
  if (e.status === "draft") out.push(pill("Draft", "gray", "pencil"));
  return out.join("");
}

function docRow(e, showFeature = false) {
  const meta = e.type === "diagram"
    ? [`<span>${esc(e.media_type === "text/vnd.mermaid" ? "Mermaid" : e.media_type)} · ${sizeText(e.bytes)}</span>`]
    : [`<span>${esc(kindTitle(e.project_id, e.kind))}</span>`, `<span>rev ${e.revision}</span>`];
  if (showFeature && e.feature_id) meta.unshift(`<span>${ic("star", "ic14")}${esc(featureLabel(e.feature_id))}</span>`);
  return `<li class="linked plan-row proc-row doc-row">
    <span class="proc-row-ic" aria-hidden="true">${ic(KIND_ICON[e.kind] || "file-text", "ic20")}</span>
    <div class="grow"><p class="rt"><a class="stretch" href="${docHref(e)}">${esc(e.title)}</a></p>
      ${e.summary ? `<p class="dev-summary">${esc(e.summary)}</p>` : ""}
      <div class="meta">${meta.join("")}${ownerHtml(e.owner)}<span>Updated ${esc(e.updated)}</span><code class="proc-key">${esc(e.path)}</code></div></div>
    <div class="proc-badges">${docBadges(e)}</div>${ic("chev-right", "ic20 chev")}</li>`;
}

const match = (e, q) => !q || [e.title, e.summary, e.path, e.kind].join(" ").toLowerCase().includes(q);
function sectionGroup(pid, s, q) {
  const list = sectionEntries(pid, s.path).filter((e) => match(e, q));
  if (q && !list.length) return "";
  const id = `doc-s-${s.path.replace(/[^a-z0-9]/g, "-")}`;
  return `<section class="work-group plan-group proc-group" aria-labelledby="${id}">
    <h3 class="work-group-title" id="${id}">${ic("folder", "ic16")}${esc(s.title)}<span class="cnt">${list.length}</span><code class="proc-key doc-sec-path">docs/${esc(s.path)}/</code></h3>
    ${s.summary ? `<p class="proc-group-note">${esc(s.summary)}</p>` : ""}
    ${list.length ? `<ul class="rows plan-rows" role="list">${list.map((e) => docRow(e)).join("")}</ul>` : `<p class="plan-empty doc-sec-empty">Nothing here yet. Saving a document into this section lists it here.</p>`}</section>`;
}
function featureGroup(pid, q) {
  const list = projectFeatures(pid).flatMap((f) => featureDocs(f.id)).filter((e) => match(e, q));
  if (!list.length) return "";
  return `<section class="work-group plan-group proc-group" aria-labelledby="doc-s-features">
    <h3 class="work-group-title" id="doc-s-features">${ic("star", "ic16")}Beside features<span class="cnt">${list.length}</span></h3>
    <p class="proc-group-note">Designs, decisions and reports that belong to one feature stay beside it, outside the tree. Listed here so you find them in one place.</p>
    <ul class="rows plan-rows" role="list">${list.map((e) => docRow(e, true)).join("")}</ul></section>`;
}

/* Docs holds the project's documentation in one place (owner): Documents, then the sections its development
   process declares (Product, Technical, Features…), then Processes and standards. `extra` is those middle links. */
let docsExtra = "";
const setDocsExtra = (html) => { docsExtra = html; };
function docsTabs(pid, cur) {
  const n = (k) => placeRows({ type: "project", id: pid }, k).entries.length;
  const tab = (key, label, count) => `<a class="queue-tab" href="#/projects/${esc(pid)}/docs${key ? "/" + key : ""}"${cur === key ? ' aria-current="page"' : ""}>${label}<span>${count}</span></a>`;
  return { documents: tab("", "Documents", treeEntries(pid).length) + docsExtra, rest: tab("processes", "Processes and standards", n("process") + n("standard")) };
}
/* The same section bar above a section that lives under Docs (Product, Technical, Features) */
function docsSectionBar(pid, action = "") {
  const t = docsTabs(pid, null);
  return `<div class="work-subbar-header proc-subbar docs-section-bar"><div class="work-toolbar queue-toolbar proc-toolbar">
      <div class="primary-tab-row"><nav class="queue-tabs" aria-label="Documentation">${t.documents}${t.rest}</nav></div>${action ? `<div class="proc-toolbar-end">${action}</div>` : ""}</div></div>`;
}

function devLine(pid) {
  const dp = projectDevProcess(pid);
  const text = dp ? `Sections from <a class="alink" href="#/dev-processes?open=${esc(dp.id)}">${esc(dp.name)}</a>, the development process this project follows. People and agents add sections as the work needs them.`
    : "This project follows no development process, so it starts with Decisions, Designs and Reports. Saving into a new folder adds a section.";
  return `<p class="proc-devline">${ic("files", "ic16")}<span>${text}</span></p>`;
}

function documentsView(pid) {
  const q = S.docQuery.trim().toLowerCase(), t = docsTabs(pid, "");
  const label = "Search documents";
  const toolbar = `<div class="work-subbar-header proc-subbar"><div class="work-toolbar queue-toolbar proc-toolbar">
      <div class="primary-tab-row"><nav class="queue-tabs" aria-label="Documentation">${t.documents}${t.rest}</nav></div>
      <div class="queue-controls" role="group" aria-label="View controls">${searchToggle("doc-search", label, S.docQuery)}</div>
      <div class="proc-toolbar-end"><button class="btn primary" type="button" data-doc-act="new" data-id="${esc(pid)}">${ic("plus", "ic16")}New document</button></div>
    </div></div>${searchRow("doc-search", label, S.docQuery, "Search title, summary or path… (Esc to close)", "work-search-row")}`;
  const groups = projectSections(pid).map((s) => sectionGroup(pid, s, q)).join("") + featureGroup(pid, q);
  const body = groups || (q ? `<p class="empty-row">Nothing matches “${esc(S.docQuery.trim())}”.</p>` : empty("files", "No documents yet", "Decisions, designs and reports for this project live here.", "", `<button class="btn primary" type="button" data-doc-act="new" data-id="${esc(pid)}">${ic("plus", "ic16")}New document</button>`));
  return toolbar + `<div class="proc-context">${devLine(pid)}</div><div class="plan-results proc-results" id="doc-results">${body}</div>`;
}

/* One document ------------------------------------------------------------------------------------- */
function versionState(tasks) {
  if (!tasks.length) return `<span class="proc-badge">${ic("clock", "ic14")}No task realizes it</span>`;
  return tasks.every(isFinished) ? `<span class="proc-badge is-ok">${ic("check-circle", "ic14")}Converged</span>` : `<span class="proc-badge is-warn">${ic("refresh", "ic14")}Converging</span>`;
}
const derivedLinks = (v) => (v.derived || []).map(([id, n]) => { const src = getEntry(id); return src ? `<a class="alink" href="${docHref(src)}">${esc(src.title)} v${n}</a>` : ""; }).join(", ");
const taskLinks = (tasks) => tasks.map((w) => `<a class="alink" href="#/projects/${esc(w.proj)}/work?item=${encodeURIComponent(w.id)}">${esc(w.id)}</a> <span class="proc-muted">${esc(w.status)}</span>`).join(", ");
function versionRow(e, v) {
  const tasks = versionTasks(e, v.version), derived = derivedLinks(v);
  return `<li class="doc-version"><div class="doc-version-h"><b>v${v.version}</b><span class="proc-muted">issued ${esc(v.at)} by ${esc(M[v.by]?.name || v.by)} · from rev ${v.revision}</span>${versionState(tasks)}</div>
      <p>${esc(v.note)}</p>${derived ? `<p class="proc-muted">Derived from ${derived}</p>` : ""}
      ${tasks.length ? `<p class="doc-version-tasks">Realized by ${taskLinks(tasks)}</p>` : ""}</li>`;
}
function versionsHtml(e) {
  if (!isVersioned(e.project_id, e.kind)) return "";
  const rows = [...e.versions].reverse().map((v) => versionRow(e, v)).join("");
  return `<section class="plan-sec" aria-labelledby="doc-ver-h">${sectionHead({ id: "doc-ver-h", title: "Issued versions", count: e.versions.length ? `latest v${latestVersion(e).version}` : "" })}
    ${rows ? `<ol class="doc-versions" role="list">${rows}</ol>` : `<p class="plan-empty">Not issued yet. Work is done against an issued version; issue it when the text is ready.</p>`}</section>`;
}

const historyHtml = (list) => `<ol class="proc-history" role="list">${list.map((r) => `<li><span class="proc-rev">rev ${r.rev}</span>${av(r.by, 24)}<span class="proc-hist-t"><b>${esc(M[r.by]?.name || r.by)}</b> ${esc(r.note)}</span><time>${esc(r.at)}</time></li>`).join("")}</ol>`;

function codeLinks(e) {
  if (!e.code_links.length) return "None";
  return `<ul class="doc-code" role="list">${e.code_links.map((c) => `<li><code>${esc(c.repo)}/${esc(c.path)}${c.symbol ? `#${esc(c.symbol)}` : ""}</code><span class="proc-muted">${esc(c.note)}</span></li>`).join("")}</ul>`;
}

function editorHtml(e) {
  return `<form class="work-task-form plan-form doc-editor" id="doc-edit-form" data-doc-form="edit" data-id="${esc(e.id)}" novalidate aria-label="Edit ${esc(e.title)}">
    <label>Title<input class="field" name="title" value="${esc(e.title)}" required autocomplete="off"></label>
    <label>Summary<input class="field" name="summary" value="${esc(e.summary)}" autocomplete="off"><span class="plan-hint">One line on why to open it. It is copied to the index.</span></label>
    <label>Text<textarea class="field proc-editor" name="body" rows="14">${esc(e.body)}</textarea></label>
    <label>What changed<input class="field" name="note" autocomplete="off" placeholder="One line for the history"></label>
    <div class="doc-editor-actions"><button class="btn outline" type="button" data-doc-act="edit-cancel" data-id="${esc(e.id)}">Cancel</button><button class="btn primary" type="submit">Save rev ${e.revision + 1}</button></div>
  </form>`;
}

const docBtn = (e, act, icon, label, variant = "outline") => `<button class="btn ${variant} plan-btn" type="button" data-doc-act="${act}" data-id="${esc(e.id)}">${ic(icon, "ic16")}${label}</button>`;
function issueButton(e) {
  if (!isVersioned(e.project_id, e.kind)) return "";
  /* doc.issue refuses a text equal to its latest version (no_changes, doc-31 §6): offered only when there is one to issue */
  const v = latestVersion(e);
  if (v && !hasUnissued(e)) return "";
  return docBtn(e, "issue", "doc-check", v ? `Issue v${v.version + 1}` : "Issue v1", "primary");
}
function docActions(e) {
  if (S.docEditing === e.id) return "";
  return issueButton(e) + (e.type === "diagram" ? "" : docBtn(e, "edit", "pencil", "Edit"));
}
function unissuedNote(e) {
  if (!hasUnissued(e)) return "";
  const n = latestVersion(e).version;
  return `<div class="callout proc-callout is-warn" role="note">${ic("pencil", "ic20")}<div><p><b>Unissued changes since v${n}.</b> Agents working against this document read v${n} until the next version is issued.</p></div></div>`;
}
const desiredState = (e) => { const v = latestVersion(e); return v ? `v${v.version}, issued ${esc(v.at)}` : "Not issued yet"; };
function featureFact(e) {
  const f = e.feature_id && getFeature(e.feature_id);
  return f ? planFact("Feature", `<a class="alink" href="#/projects/${esc(e.project_id)}/features/${esc(f.id)}">${esc(featureRef(f))} ${esc(f.title)}</a>`) : "";
}
function docFacts(e) {
  const root = e.feature_id ? `projects/${e.project_id}/` : `projects/${e.project_id}/docs/`;
  const file = e.type === "diagram" ? planFact("File", `${esc(e.media_type)} · ${sizeText(e.bytes)}`) : planFact("Code links", codeLinks(e));
  return `<dl class="triage-fields pj-fields plan-facts">
    ${planFact("Path", `<code>${esc(root + e.path)}</code>`)}
    ${planFact("Owner", ownerHtml(e.owner) || "No owner")}
    ${planFact("Revision", `rev ${e.revision} · ${esc(e.updated)} by ${esc(M[e.updated_by]?.name || "")}`)}
    ${isVersioned(e.project_id, e.kind) ? planFact("Desired state", desiredState(e)) : ""}
    ${featureFact(e)}${file}
  </dl>`;
}
function docBody(e) {
  if (S.docEditing === e.id) return editorHtml(e);
  if (e.type === "diagram") return `<div class="doc-diagram"><p class="proc-muted">Diagram source. The product shows it as an image and never runs anything inside it.</p><pre><code>${esc(e.body)}</code></pre></div>`;
  return `<div class="proc-md doc-md">${md(e.body)}</div>`;
}

function docPage(pid, id) {
  const e = getEntry(id);
  if (!e || e.project_id !== pid) return empty("file", "Document not found", "It may have moved to another project, or the link is out of date.", "", `<a class="btn outline" href="#/projects/${esc(pid)}/docs">All documents</a>`);
  const editing = S.docEditing === e.id;
  return `<article class="plan-page doc-page" aria-labelledby="plan-title">
    ${planHead(kindTitle(pid, e.kind), e.title, docBadges(e), docActions(e))}
    ${e.summary && !editing ? `<p class="proc-lead">${esc(e.summary)}</p>` : ""}
    ${unissuedNote(e)}${docFacts(e)}
    <section class="plan-sec" aria-labelledby="doc-text-h">${sectionHead({ id: "doc-text-h", title: editing ? "Editing" : "Text" })}${docBody(e)}</section>
    ${versionsHtml(e)}
    <section class="plan-sec" aria-labelledby="doc-hist-h">${sectionHead({ id: "doc-hist-h", title: "History", count: `${e.history.length} revisions` })}${historyHtml(e.history)}</section>
  </article>`;
}

function pageDocs(pid, sub) {
  if (sub === "processes" || sub === "standards") { const t = docsTabs(pid, "processes"); return projectProcesses(pid, t.documents + t.rest); }
  return sub ? docPage(pid, sub) : documentsView(pid);
}
function docCrumbs(pid, sub) {
  const base = `#/projects/${pid}/docs`;
  if (!sub) return [[null, "Docs"]];
  if (sub === "processes" || sub === "standards") return [[base, "Docs"], [null, "Processes and standards"]];
  const e = getEntry(sub);
  return [[base, "Docs"], [null, e ? e.title : "Not found"]];
}
const docsCount = (pid) => treeEntries(pid).length;

export { docCrumbs, docHref, docRow, docsCount, docsSectionBar, historyHtml, pageDocs, setDocsExtra };
