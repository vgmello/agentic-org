/* What the documentation screens do: edit a document or a feature's definition in place, issue a version, and
   create a document in a section or beside a feature. Session-only for the mock; controls carry data-doc-act. */
import { projectDevProcess } from "../devproc-data.js";
import {
  BUILT_IN_KINDS, createEntry, getEntry, isVersioned, issueVersion, kindTitle, latestVersion, projectSections, saveEntry, saveFeatureDefinition, treeEntries,
} from "../docs-data.js";
import { featureRef, getFeature, slugify } from "../plan-data.js";
import { closeSheet, openDialog, setSheetReturn } from "../shell/dialogs.js";
import { render } from "../shell/router.js";
import { S } from "../state.js";
import { esc, toast } from "../ui.js";
import { docHref } from "./docs.js";

const cancelBtn = '<button class="btn outline" type="button" data-act="close-sheet">Cancel</button>';
const opt = (v, l, cur) => `<option value="${esc(v)}"${v === cur ? " selected" : ""}>${esc(l)}</option>`;
const TEMPLATES = {
  decision: "## Context\n\n## Decision\n\n## Consequences\n",
  design: "## Overview\n\n## How it works\n\n## Risks\n",
  report: "## Findings\n\n## What to do next\n",
  "product-dss": "## Behaviour\n\n## Rules\n",
  "technical-dss": "## Architecture\n\n## Interfaces\n",
};
const focusLater = (sel) => requestAnimationFrame(() => document.querySelector(sel)?.focus());

function openNewDoc(trigger, pid, fid) {
  const dp = projectDevProcess(pid);
  const kinds = [...Object.entries(BUILT_IN_KINDS).filter(([k]) => ["decision", "design", "report"].includes(k)), ...(dp?.kinds || []).map((k) => [k.key, k.title])];
  const where = fid ? `<input type="hidden" name="feature" value="${esc(fid)}"><p class="plan-hint">Saved beside ${esc(featureRef(getFeature(fid)))} ${esc(getFeature(fid).title)}, outside the tree.</p>`
    : `<label>Section<select class="field" name="section">${projectSections(pid).map((s) => opt(s.path, `${s.title} (docs/${s.path}/)`, "")).join("")}</select></label>`;
  openDialog(trigger, fid ? "New document beside the feature" : "New document", "Starts from its kind's template. The index lists it as soon as it is saved.",
    `<form class="work-task-form plan-form" id="doc-form" data-doc-form="new" data-id="${esc(pid)}" novalidate>
      <div class="work-task-fields"><label>Kind<select class="field" name="kind">${kinds.map(([k, l]) => opt(k, l, fid ? "design" : "")).join("")}</select></label>${fid ? "" : where}</div>
      <label>Title<input class="field" name="title" required autocomplete="off" placeholder="Release pipeline"></label>
      <label>Summary<input class="field" name="summary" autocomplete="off" placeholder="One line on why to open it"></label>
      ${fid ? where : ""}</form>`,
    cancelBtn + `<button class="btn primary" type="submit" form="doc-form">Create and edit</button>`);
  document.querySelector(".sheet")?.classList.add("plan-dialog");
  document.querySelector("#doc-form input[name=title]")?.focus();
}
function submitNewDoc(form) {
  const pid = form.dataset.id, title = form.elements.title.value.trim();
  if (!title) { form.elements.title.setCustomValidity("Enter a title."); form.elements.title.reportValidity(); return; }
  const kind = form.elements.kind.value, fid = form.elements.feature?.value || null;
  const folder = fid ? `features/${fid}/${kind === "decision" ? "decisions" : kind === "report" ? "reports" : "designs"}` : form.elements.section.value;
  const e = createEntry(pid, { kind, feature_id: fid, owner: "you", title, summary: form.elements.summary.value.trim(), path: `${folder}/${slugify(title) || "untitled"}.md`, body: TEMPLATES[kind] || "## Purpose\n", status: isVersioned(pid, kind) ? "draft" : "current" });
  setSheetReturn(null); closeSheet();
  S.docEditing = e.id;
  location.hash = docHref(e);
  toast(`${title} created as rev 1.`, "check-circle");
  focusLater("#doc-edit-form textarea");
}

function openIssue(trigger, id) {
  const e = getEntry(id), v = latestVersion(e), next = (v?.version || 0) + 1;
  const sources = treeEntries(e.project_id).filter((o) => o.id !== e.id && latestVersion(o));
  openDialog(trigger, `Issue ${e.title} v${next}?`, `Fixes the working text, rev ${e.revision}, as version ${next}. It is written once and never changed; tasks then realize it.`,
    `<form class="work-task-form plan-form" id="doc-form" data-doc-form="issue" data-id="${esc(id)}" novalidate>
      <label>What this version changes<input class="field" name="note" required autocomplete="off" placeholder="Passwords now expire after 90 days"></label>
      ${sources.length ? `<fieldset class="plan-pick"><legend class="plan-hint">Derived from (optional)</legend><ul role="list">${sources.map((o) => `<li><label><input type="checkbox" name="derived" value="${esc(o.id)}"><span class="plan-pick-ref">v${latestVersion(o).version}</span><span class="plan-pick-t">${esc(o.title)}<span class="plan-pick-note">${esc(kindTitle(o.project_id, o.kind))}</span></span></label></li>`).join("")}</ul></fieldset>` : ""}
    </form>`, cancelBtn + `<button class="btn primary" type="submit" form="doc-form">Issue v${next}</button>`);
  document.querySelector(".sheet")?.classList.add("plan-dialog");
  document.querySelector("#doc-form input[name=note]")?.focus();
}
function submitIssue(form) {
  const note = form.elements.note.value.trim();
  if (!note) { form.elements.note.setCustomValidity("Say what this version changes."); form.elements.note.reportValidity(); return; }
  const derived = [...form.querySelectorAll('input[name="derived"]:checked')].map((i) => [i.value, latestVersion(getEntry(i.value)).version]);
  const version = issueVersion(form.dataset.id, note, derived);
  setSheetReturn(null); closeSheet(); render(false);
  toast(`Issued v${version}. Agents read it from their next start.`, "check-circle");
  focusLater("#doc-ver-h");
}

function saveDocEdit(form) {
  const title = form.elements.title.value.trim();
  if (!title) { form.elements.title.setCustomValidity("Enter a title."); form.elements.title.reportValidity(); return; }
  const e = saveEntry(form.dataset.id, { title, summary: form.elements.summary.value.trim(), body: form.elements.body.value }, form.elements.note.value.trim() || "Edited");
  S.docEditing = null; render(false);
  toast(`Saved rev ${e.revision}.${isVersioned(e.project_id, e.kind) ? " Issue a version when it's ready." : ""}`, "check-circle");
  focusLater('[data-doc-act="edit"]');
}
function saveFeatureEdit(form) {
  const f = getFeature(form.dataset.id);
  saveFeatureDefinition(f, form.elements.body.value, form.elements.note.value.trim() || "Definition edited");
  S.featureEditing = null; render(false);
  toast(`${featureRef(f)} definition saved.`, "check-circle");
  focusLater('[data-doc-act="feature-edit"]');
}

const CLICK = {
  new: (id, t) => openNewDoc(t, id, t.dataset.feature || null),
  edit: (id) => { S.docEditing = id; render(false); focusLater("#doc-edit-form textarea"); },
  "edit-cancel": () => { S.docEditing = null; render(false); focusLater('[data-doc-act="edit"]'); },
  issue: (id, t) => openIssue(t, id),
  "feature-edit": (id) => { S.featureEditing = id; render(false); focusLater("#feature-def-form textarea"); },
  "feature-cancel": () => { S.featureEditing = null; render(false); focusLater('[data-doc-act="feature-edit"]'); },
};
const SUBMIT = { new: submitNewDoc, issue: submitIssue, edit: saveDocEdit, feature: saveFeatureEdit };

document.addEventListener("click", (e) => {
  const t = e.target.closest?.("[data-doc-act]");
  if (!t || !Object.hasOwn(CLICK, t.dataset.docAct)) return;
  CLICK[t.dataset.docAct](t.dataset.id, t);
});
document.addEventListener("submit", (e) => {
  const form = e.target.closest?.("form[data-doc-form]");
  if (!form || !Object.hasOwn(SUBMIT, form.dataset.docForm)) return;
  e.preventDefault();
  SUBMIT[form.dataset.docForm](form);
});
document.addEventListener("input", (e) => {
  if (e.target.id === "doc-search") { S.docQuery = e.target.value; render(false); return; }
  if (e.target.form?.matches?.("form[data-doc-form]")) e.target.setCustomValidity?.("");
});
/* Leaving a page drops an unsaved in-place edit's mode (the typed text is the browser's until then) */
addEventListener("hashchange", () => { if (!location.hash.includes("/docs/")) S.docEditing = null; if (!location.hash.includes("/features/")) S.featureEditing = null; });
