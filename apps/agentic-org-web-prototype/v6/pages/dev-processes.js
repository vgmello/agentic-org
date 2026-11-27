/* Settings › Development processes: the catalogue headed "Development processes: how a project builds software"
   (doc-54 decision 7, doc-32 §2). Built-in ones first; each shows its summary, seed roles, tabs and starting sections.
   A built-in one is copied to change it; a process file is imported as a draft. Never listed with organization
   processes, and never resolved with them. */
import { M, P } from "../data.js";
import { DEV_PROCESSES, DEV_STATUS, copyDevProcess, getDevProcess, importDevProcess, projectsFollowing } from "../devproc-data.js";
import { closeSheet, openDialog, setSheetReturn } from "../shell/dialogs.js";
import { render } from "../shell/router.js";
import { S, qs } from "../state.js";
import { av, esc, hdr, ic, pill, toast } from "../ui.js";
import { closeProcPanel, factList, openProcPanel, panelSection } from "./proc-panel.js";

Object.assign(S, { devView: "current" });
const VIEWS = [["current", "Current"], ["draft", "Drafts"], ["deprecated", "Deprecated"]];
const originBadge = (d) => (d.origin === "built_in" ? `<span class="proc-badge">${ic("shield-check", "ic14")}Built in</span>` : `<span class="proc-badge" data-level="org">${ic("office", "ic14")}Organization</span>`);
const statusPill = (d) => (d.status === "current" ? pill("Current", "green", "check-circle") : d.status === "draft" ? pill("Draft", "gray", "pencil") : pill("Deprecated", "outline", "archive"));
const ordered = () => [...DEV_PROCESSES].sort((a, b) => (a.origin === b.origin ? 0 : a.origin === "built_in" ? -1 : 1));

function rowHtml(d) {
  const seeds = d.roles.filter((r) => r.seed).map((r) => r.name).join(", ");
  const used = projectsFollowing(d.id);
  return `<li class="linked plan-row proc-row dev-row">
    <span class="proc-row-ic" aria-hidden="true">${ic("files", "ic20")}</span>
    <div class="grow"><p class="rt"><button class="stretch proc-open" type="button" data-dev-act="open" data-id="${esc(d.id)}">${esc(d.name)}</button>${d.version_label ? `<span class="plan-version">${esc(d.version_label)}</span>` : ""}</p>
      <p class="dev-summary">${esc(d.summary)}</p>
      <div class="meta"><span>Seed roles: ${esc(seeds)}</span><span>Tabs: ${esc(d.tabs.map((t) => t[1]).join(", "))}</span><span>Starts with ${esc(d.folders.map((f) => f[1]).join(", "))}</span>${used.length ? `<span>Followed by ${used.map((p) => esc(P[p].name)).join(", ")}</span>` : "<span>No project follows it</span>"}</div></div>
    <div class="proc-badges">${originBadge(d)}${statusPill(d)}</div>${ic("chev-right", "ic20 chev")}</li>`;
}

function pageDevProcesses() {
  const view = VIEWS.some(([v]) => v === S.devView) ? S.devView : "current";
  const rows = ordered().filter((d) => d.status === view);
  const tabs = VIEWS.map(([v, l]) => `<button class="queue-tab" type="button" data-act="set" data-k="devView" data-v="${v}" aria-pressed="${v === view}" aria-controls="dev-results">${l}<span>${DEV_PROCESSES.filter((d) => d.status === v).length}</span></button>`).join("");
  const header = hdr("Development processes", "How a project builds software");
  const body = `<p class="desc proc-note">A project follows at most one development process: which roles start it, what each role does, which documents it produces and which tabs the project has. It is content the product shows to people and agents; nothing runs it. Organization processes and standards are separate: see <a class="alink" href="#/settings/processes">Processes</a>.</p>
    <div class="work-subbar-header proc-subbar"><div class="work-toolbar queue-toolbar proc-toolbar">
      <div class="primary-tab-row"><div class="queue-tabs" role="group" aria-label="Lifecycle">${tabs}</div></div>
      <div class="proc-toolbar-end"><button class="btn outline" type="button" data-dev-act="import">${ic("download", "ic16")}Import process file</button><button class="btn primary" type="button" data-dev-act="copy-pick">${ic("copy", "ic16")}Copy one</button></div>
    </div></div>
    <div class="plan-results proc-results" id="dev-results">${rows.length ? `<ul class="rows plan-rows" role="list">${rows.map(rowHtml).join("")}</ul>` : `<p class="empty-row">No ${VIEWS.find(([v]) => v === view)[1].toLowerCase()} development processes.</p>`}</div>`;
  return { header, body };
}

/* The panel: the manifest as people read it ------------------------------------------------------- */
function manifestHtml(d) {
  const roles = `<ul class="dev-list" role="list">${d.roles.map((r) => `<li><b>${esc(r.name)}</b>${r.seed ? ` <span class="proc-badge is-add">${ic("star", "ic14")}Seed</span>` : ""}<span class="proc-muted"><code>${esc(r.key)}</code>${r.hats.length ? ` · covers ${esc(r.hats.join(", "))}` : ""}</span></li>`).join("")}</ul>`;
  const stages = `<ol class="proc-stages" role="list">${d.stages.map((s) => `<li><b>${esc(s.title)}</b> <span class="proc-muted">${esc(s.summary)} Led by ${esc(d.roles.find((r) => r.key === s.lead)?.name || "anyone")}.</span></li>`).join("")}</ol>`;
  const skills = `<ul class="proc-instr-list" role="list">${d.skills.map((s) => `<li><b>${esc(s.title)}</b> <span class="proc-muted">for ${esc(s.roles.length ? s.roles.map((k) => d.roles.find((r) => r.key === k)?.name || k).join(", ") : "every role")}</span><p>${esc(s.description)}</p></li>`).join("")}</ul>`;
  const tabs = `<p>Overview · Work · ${d.tabs.map((t) => `<b>${esc(t[1])}</b>`).join(" · ")} · Docs · Team · Activity · Conversations</p><p class="proc-muted">The process's tabs sit between Work and Docs.</p>`;
  const kinds = d.kinds.length ? `<ul class="dev-list" role="list">${d.kinds.map((k) => `<li><b>${esc(k.title)}</b><span class="proc-muted"><code>${esc(k.key)}</code>${k.versioned ? " · issued in versions" : ""}</span></li>`).join("")}</ul>` : `<p class="proc-muted">None. It uses decisions, designs and reports.</p>`;
  const folders = `<ul class="dev-list" role="list">${d.folders.map(([p, t]) => `<li><b>${esc(t)}</b><span class="proc-muted"><code>docs/${esc(p)}/</code></span></li>`).join("")}</ul>`;
  return panelSection("dev-roles", "Roles", roles) + panelSection("dev-stages", "Stages", stages) + panelSection("dev-skills", "Skills", skills)
    + panelSection("dev-tabs", "Project tabs", tabs) + panelSection("dev-kinds", "Document kinds it adds", kinds) + panelSection("dev-folders", "Starting sections", folders);
}

function openDev(id, from) {
  const d = getDevProcess(id); if (!d) return;
  const used = projectsFollowing(d.id), src = d.copied_from && getDevProcess(d.copied_from);
  const facts = factList([
    ["Key", `<code>${esc(d.key)}</code>`],
    ["Origin", d.origin === "built_in" ? "Ships with the product. Copy it to change it." : `Organization Git <code>development-processes/${esc(d.id)}/</code>`],
    src ? ["Copied from", esc(src.name)] : null,
    ["Revision", `rev ${d.revision}${d.version_label ? ` · ${esc(d.version_label)}` : ""}`],
    ["Accountable owner", d.owner ? `<span class="proc-owner">${av(d.owner, 24)}${esc(M[d.owner].name)}</span>` : "The product"],
    ["Followed by", used.length ? used.map((p) => `<a class="alink" href="#/projects/${esc(p)}/docs">${esc(P[p].name)}</a>`).join(", ") : "No project"],
  ]);
  const btn = (act, icon, label) => `<button class="btn outline sm" type="button" data-dev-act="${act}" data-id="${esc(d.id)}">${ic(icon, "ic16")}${label}</button>`;
  const actions = (d.origin === "built_in" ? btn("copy", "copy", "Copy to change it") : btn("edit", "pencil", "Edit") + btn("copy", "copy", "Copy"))
    + btn("download", "download", "Download process file")
    + (d.origin !== "built_in" && d.status === "draft" ? btn("make-current", "check-circle", "Make current") : "");
  const draftNote = d.status === "draft" ? `<div class="callout proc-callout" role="note">${ic("info", "ic20")}<div><p>A draft can't be chosen for a project until it's Current.</p></div></div>` : "";
  openProcPanel({ eyebrow: `${ic("files", "ic16")}Development process`, title: d.name, badges: originBadge(d) + statusPill(d), actions }, draftNote + `<p class="proc-lead">${esc(d.summary)}</p>` + facts + manifestHtml(d), from, true);
}

/* Copy and import ----------------------------------------------------------------------------------- */
const cancelBtn = '<button class="btn outline" type="button" data-act="close-sheet">Cancel</button>';
function openCopy(trigger, id) {
  const src = id ? getDevProcess(id) : null;
  const pick = src ? `<input type="hidden" name="source" value="${esc(src.id)}">` : `<label>Copy<select class="field" name="source">${ordered().filter((d) => d.status === "current").map((d) => `<option value="${esc(d.id)}">${esc(d.name)}</option>`).join("")}</select></label>`;
  const name = src ? `Copy of ${src.name}` : "";
  openDialog(trigger, src ? `Copy ${src.name}` : "Copy a development process", "The copy is an organization development process with a new ID. It starts as a draft; projects keep following the original until someone chooses the copy.",
    `<form class="work-task-form plan-form" id="dev-form" data-dev-form="copy" novalidate>${pick}<label>Name<input class="field" name="name" value="${esc(name)}" required autocomplete="off"></label><label>Key<input class="field" name="key" value="" required autocomplete="off" spellcheck="false" placeholder="feature-delivery-payments"><span class="plan-hint">Unique in the organization. Lowercase letters, digits and hyphens.</span></label></form>`,
    cancelBtn + `<button class="btn primary" type="submit" form="dev-form">Copy as draft</button>`);
  document.querySelector(".sheet")?.classList.add("plan-dialog");
  document.querySelector("#dev-form input[name=name]")?.focus();
}
function submitCopy(form) {
  const name = form.elements.name.value.trim(), key = form.elements.key.value.trim();
  if (!name) { form.elements.name.setCustomValidity("Enter a name."); form.elements.name.reportValidity(); return; }
  if (!/^[a-z0-9][a-z0-9-]{0,63}$/.test(key) || DEV_PROCESSES.some((d) => d.key === key)) { form.elements.key.setCustomValidity(DEV_PROCESSES.some((d) => d.key === key) ? `The key ${key} is taken.` : "Use lowercase letters, digits and hyphens."); form.elements.key.reportValidity(); return; }
  const copy = copyDevProcess(form.elements.source.value, name, key);
  S.devView = "draft"; setSheetReturn(null); closeSheet(); render(false);
  toast(`${copy.name} added as a draft.`, "check-circle");
  openDev(copy.id, document.querySelector(`[data-dev-act="open"][data-id="${CSS.escape(copy.id)}"]`));
}
function openImport(trigger) {
  openDialog(trigger, "Import a process file", "A process file is an archive of a development process's folder. It is validated, gets a new ID and arrives as a draft. Nothing links it to the organization it came from.",
    `<form class="work-task-form plan-form" id="dev-form" data-dev-form="import" novalidate><label>Process file<input class="field" type="file" name="file" accept=".zip,.tar,.tgz,.agentic-process" required></label></form>`,
    cancelBtn + `<button class="btn primary" type="submit" form="dev-form">Import</button>`);
  document.querySelector(".sheet")?.classList.add("plan-dialog");
}
function submitImport(form) {
  const file = form.elements.file.files?.[0];
  if (!file) { form.elements.file.setCustomValidity("Choose a process file."); form.elements.file.reportValidity(); return; }
  const d = importDevProcess(file.name);
  S.devView = "draft"; setSheetReturn(null); closeSheet(); render(false);
  toast(`${d.name} imported as a draft.`, "check-circle");
}

/* A dialog opened from the panel replaces it; Cancel returns focus to the process's row */
function fromPanel(t, id) {
  if (!document.getElementById("proc-panel")?.contains(t)) return t;
  closeProcPanel(false);
  return document.querySelector(`[data-dev-act="open"][data-id="${CSS.escape(id)}"]`) || document.querySelector("#main h1");
}
const CLICK = {
  open: (id, t) => openDev(id, t),
  copy: (id, t) => openCopy(fromPanel(t, id), id),
  "copy-pick": (_id, t) => openCopy(t, null),
  import: (_id, t) => openImport(t),
  edit: () => toast("The development process editor isn't in this prototype."),
  download: (id) => toast(`${getDevProcess(id).key}.agentic-process downloaded.`, "download"),
  "make-current": (id, t) => { const d = getDevProcess(id); d.status = "current"; d.revision += 1; render(false); openDev(id, t); toast(`${d.name} is Current and can be chosen for a project.`, "check-circle"); },
};
document.addEventListener("click", (e) => {
  const t = e.target.closest?.("[data-dev-act]");
  if (!t || !Object.hasOwn(CLICK, t.dataset.devAct)) return;
  CLICK[t.dataset.devAct](t.dataset.id, t);
});
document.addEventListener("submit", (e) => {
  const form = e.target.closest?.("form[data-dev-form]");
  if (!form) return;
  e.preventDefault();
  (form.dataset.devForm === "copy" ? submitCopy : submitImport)(form);
});
document.addEventListener("input", (e) => { if (e.target.form?.matches?.("form[data-dev-form]")) e.target.setCustomValidity(""); });
document.addEventListener("change", (e) => { if (e.target.form?.matches?.("form[data-dev-form]")) e.target.setCustomValidity(""); });

/* #/settings/dev-processes?open=<id> opens that process (links from a project's processes) */
function mountDevProcesses() {
  const id = qs().get("open");
  if (id && location.hash.startsWith("#/settings/dev-processes")) requestAnimationFrame(() => openDev(id, document.querySelector(`[data-dev-act="open"][data-id="${CSS.escape(id)}"]`)));
}
addEventListener("hashchange", () => setTimeout(mountDevProcesses, 0));

export { mountDevProcesses, pageDevProcesses };
