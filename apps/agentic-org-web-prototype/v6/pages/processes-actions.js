/* What the processes screens do: write, override, fix, rename and deprecate documents (doc-54 §2, doc-34 §5),
   the Show select, the agent preview and the reader panel. Session-only for the mock; controls carry data-proc-act. */
import { M, P, TEAMS } from "../data.js";
import {
  BLANK_BODY, KIND_WORDS, TEMPLATES, docPath, getDoc, keyTaken, levelFields, levelName, levelOf, parsePlace, placeKey, saveDoc, sealedAbove, writeLevel,
} from "../proc-data.js";
import { closeSheet, openDialog, setSheetReturn } from "../shell/dialogs.js";
import { render } from "../shell/router.js";
import { S } from "../state.js";
import { esc, ic, toast } from "../ui.js";
import { currentPlace, openDocPanel, openPreview } from "./proc-reader.js";
import { closeProcPanel, procPanelOpen } from "./proc-panel.js";

const cancelBtn = '<button class="btn outline" type="button" data-act="close-sheet">Cancel</button>';
const submitBtn = (label, variant = "primary") => `<button class="btn ${variant}" type="submit" form="proc-form">${esc(label)}</button>`;
const field = (label, control, hint = "") => `<label>${label}${control}${hint ? `<span class="plan-hint">${hint}</span>` : ""}</label>`;
const opt = (v, l, cur) => `<option value="${esc(v)}"${String(v) === String(cur ?? "") ? " selected" : ""}>${esc(l)}</option>`;
const KEY_RE = /^[a-z0-9][a-z0-9-]{0,63}$/;

const scopeOptions = (cur) => [["org", "Organization"], ...Object.keys(TEAMS).map((t) => [`team.${t}`, `${TEAMS[t].name} (team)`]),
  ...Object.keys(P).filter((p) => !P[p].archived).map((p) => [`project.${p}`, `${P[p].name} (project)`])].map(([v, l]) => opt(v, l, cur)).join("");
const ownerOptions = (cur) => opt("", "No owner", cur) + Object.keys(M).filter((k) => k !== "tess").map((k) => opt(k, `${M[k].name} · ${M[k].kind === "agent" ? "Agent" : "Person"}`, cur)).join("");
const levelKey = (lv) => (lv.type === "org" ? "org" : `${lv.type}.${lv.id}`);

function startField(d, isNew, kind) {
  if (!isNew) return "";
  const options = TEMPLATES.filter((t) => t.kind === kind).map((t) => opt(t.id, `Template: ${t.title}`, d.template)).join("");
  return field("Start from", `<select class="field" name="template">${opt("", "Blank", "")}${options}</select>`);
}
function keyField(d, lockScope) {
  const hint = lockScope ? "Same key as the document it replaces." : "A lower level replaces a document by using its key.";
  return field("Key", `<input class="field" name="key" value="${esc(d.key || "")}" required autocomplete="off" spellcheck="false"${lockScope ? " readonly" : ""} placeholder="review-and-merge">`, hint);
}
function scopeField(d, lockScope) {
  if (!lockScope) return field("Scope", `<select class="field" name="scope">${scopeOptions(d.scope)}</select>`);
  return field("Scope", `<select class="field" name="scope-view" disabled>${scopeOptions(d.scope)}</select><input type="hidden" name="scope" value="${esc(d.scope)}">`);
}
const OVER_VALUE = { false: "no", true: "yes" };
function overridableField(d) {
  const over = OVER_VALUE[String(d.overridable)] || "";
  const hint = d.invalidField ? `<span class="proc-warn-ink">The file says <code>${esc(String(d.overridable))}</code>, which isn't recognized. Choose one.</span>` : "Only the owner marks an organization document not overridable.";
  return field("Overridable", `<select class="field" name="overridable"${d.invalidField ? ' aria-invalid="true"' : ""}>${over ? "" : opt("", "Choose…", "")}${opt("yes", "Yes: a team or project may replace it", over)}${opt("no", "No: not overridable below this level", over)}</select>`, hint);
}
function pathNote(d, isNew, kind) {
  const path = d.path || `${kind === "standard" ? "standards" : "processes"}/<new id>.md`;
  return `<p class="plan-hint proc-path">${ic("folder", "ic14")}Saves <code>${esc(path)}</code> in organization Git, as rev ${isNew ? 1 : (d.revision || 0) + 1}.</p>`;
}
function editorFields(d, { isNew, lockScope, kind }) {
  return startField(d, isNew, kind)
    + field("Title", `<input class="field" name="title" value="${esc(d.title || "")}" required autocomplete="off" placeholder="Review and merge">`)
    + `<div class="work-task-fields">${keyField(d, lockScope)}${scopeField(d, lockScope)}`
    + field("Accountable owner", `<select class="field" name="owner">${ownerOptions(d.owner_employee_id)}</select>`, "Required while Current.")
    + field("Lifecycle", `<select class="field" name="status">${opt("draft", "Draft", d.status)}${opt("current", "Current", d.status)}${opt("deprecated", "Deprecated", d.status)}</select>`)
    + `</div>${overridableField(d)}`
    + field("Text", `<textarea class="field proc-editor" name="body" rows="12" spellcheck="true">${esc(d.body || BLANK_BODY)}</textarea>`, "Markdown. Agents read the first paragraph of Purpose as its summary.")
    + field("What changed", `<input class="field" name="note" autocomplete="off" placeholder="${isNew ? "First version" : "One line for the history"}">`)
    + pathNote(d, isNew, kind);
}

function openEditor(trigger, title, desc, state, fields) {
  openDialog(trigger, title, desc, `<form class="work-task-form plan-form proc-form" id="proc-form" data-proc-form="${state.mode}" data-id="${esc(state.id || "")}" data-kind="${state.kind}" novalidate>${fields}</form>`,
    cancelBtn + submitBtn(state.mode === "rename" ? "Rename" : "Save"));
  document.querySelector(".sheet")?.classList.add("plan-dialog", "proc-dialog");
  document.querySelector(`#proc-form ${state.focus || "input:not([readonly]):not([type=hidden]), select"}`)?.focus();
}

function openNew(trigger, kind, placeText, templateId) {
  const tpl = TEMPLATES.find((t) => t.id === templateId);
  kind = tpl?.kind || kind || (location.hash.includes("standard") ? "standard" : "process");
  const lv = writeLevel(parsePlace(placeText));
  const d = { title: tpl?.title || "", key: tpl?.key || "", body: tpl?.body || BLANK_BODY, scope: levelKey(lv), owner_employee_id: "you", status: "current", overridable: true, template: tpl?.id || "" };
  openEditor(trigger, `New ${KIND_WORDS[kind].one}`, `A ${KIND_WORDS[kind].one} ${kind === "process" ? "says how the organization handles something, whatever is being built" : "says what the organization uses and how it builds and runs things"}.`, { mode: "new", kind }, editorFields(d, { isNew: true, kind }));
}
function openEdit(trigger, id, focus) {
  const d = getDoc(id);
  const state = { ...d, scope: levelKey(levelOf(d)), path: docPath(d), invalidField: d.invalid?.field === "overridable" };
  openEditor(trigger, `Edit ${d.title}`, `${KIND_WORDS[d.kind].single === "Process" ? "Organization process" : "Standard"} · ${levelName(levelOf(d))} · rev ${d.revision}`, { mode: "edit", id, kind: d.kind, focus }, editorFields(state, { kind: d.kind }));
}
function openOverride(trigger, id, placeText) {
  const d = getDoc(id), lv = writeLevel(parsePlace(placeText));
  const state = { title: d.title, key: d.key, body: d.body, scope: levelKey(lv), owner_employee_id: "you", status: "current", overridable: true };
  openEditor(trigger, `Override ${d.title} for ${levelName(lv)}`, `Starts from ${levelName(levelOf(d))} rev ${d.revision}. The override has its own ID and the same key, and replaces it for ${levelName(lv)} and below.`,
    { mode: "new", kind: d.kind, focus: "textarea" }, editorFields(state, { isNew: false, lockScope: true, kind: d.kind }).replace(/as rev \d+/, "as rev 1"));
}
function openRename(trigger, id) {
  const d = getDoc(id);
  openEditor(trigger, `Rename the key of ${d.title}`, `Two documents in ${levelName(levelOf(d))} use the key ${d.key}. Neither applies until one is renamed.`, { mode: "rename", id, kind: d.kind },
    field("New key", `<input class="field" name="key" value="${esc(d.key)}" required autocomplete="off" spellcheck="false">`, "Lowercase letters, digits and hyphens."));
}

function invalid(input, message) { input.setCustomValidity(message); input.reportValidity(); return null; }
const val = (form, name) => (form.elements[name]?.value ?? "").trim();

/* doc-34 §5: valid fields, a key unique at its level and scope, no replacement of a non-overridable document */
function readEditor(form, selfId) {
  const kind = form.dataset.kind, lv = parsePlace(val(form, "scope")), key = val(form, "key");
  if (!val(form, "title")) return invalid(form.elements.title, "Enter a title.");
  if (!KEY_RE.test(key)) return invalid(form.elements.key, "Use 1 to 64 lowercase letters, digits or hyphens, starting with a letter or digit.");
  const sealed = sealedAbove(lv, key, kind);
  if (sealed) return invalid(form.elements.key, `Refused: ${levelName(levelOf(sealed)) === "Organization" ? "the organization" : levelName(levelOf(sealed))} marked ${sealed.title} not overridable. Use another key to add a ${KIND_WORDS[kind].one} instead.`);
  if (keyTaken(lv, key, kind, selfId)) return invalid(form.elements.key, `Another ${KIND_WORDS[kind].one} in ${levelName(lv)} uses the key ${key}. Keys are unique at each level.`);
  if (!val(form, "overridable")) return invalid(form.elements.overridable, "Choose whether a lower level may replace it.");
  if (val(form, "status") === "current" && !val(form, "owner")) return invalid(form.elements.owner, "A Current document needs an accountable owner.");
  return { kind, key, title: val(form, "title"), body: form.elements.body.value, status: val(form, "status"), owner_employee_id: val(form, "owner") || null,
    overridable: val(form, "overridable") !== "no", note: val(form, "note"), ...levelFields(lv) };
}

function afterSave(d, verb) {
  setSheetReturn(null); closeSheet(); render(false);
  toast(`${verb} ${d.title} (${levelName(levelOf(d))} rev ${d.revision}).`, "check-circle");
  const row = document.querySelector(`[data-proc-act="open"][data-id="${CSS.escape(d.id)}"]`);
  openDocPanel(d.id, row || document.querySelector("#main h1"));
}
const SUBMIT = {
  new: (form) => { const f = readEditor(form); if (f) afterSave(saveDoc(f), "Saved"); },
  edit: (form) => { const f = readEditor(form, form.dataset.id); if (f) afterSave(saveDoc(f, form.dataset.id), "Saved"); },
  rename: (form) => {
    const d = getDoc(form.dataset.id), key = val(form, "key");
    if (!KEY_RE.test(key)) return invalid(form.elements.key, "Use lowercase letters, digits or hyphens.");
    if (keyTaken(levelOf(d), key, d.kind, d.id)) return invalid(form.elements.key, `Another document in ${levelName(levelOf(d))} uses ${key}.`);
    afterSave(saveDoc({ key, note: `Key renamed to ${key}` }, d.id), "Renamed");
  },
};

function openDeprecate(trigger, id) {
  const d = getDoc(id);
  openDialog(trigger, `Deprecate ${d.title}?`, `It stops applying at ${levelName(levelOf(d))} and below at each agent's next start. Overrides of it below keep their own text and become additions. The file and its history stay.`, "",
    cancelBtn + `<button class="btn danger" type="button" data-proc-act="deprecate-confirm" data-id="${esc(id)}">Deprecate</button>`);
}
function deprecate(id) {
  const d = getDoc(id), before = d.status;
  saveDoc({ status: "deprecated", note: "Deprecated" }, id);
  setSheetReturn(null); closeSheet(); closeProcPanel(false); render(false);
  toast(`${d.title} deprecated.`, "check-circle", () => { saveDoc({ status: before, note: "Deprecation undone" }, id); render(false); });
}

/* A dialog opened from the reader replaces it; Cancel returns focus to the document's row */
function fromPanel(t) {
  const el = document.getElementById("proc-panel");
  if (!el?.contains(t)) return t;
  const id = el.dataset.doc;
  closeProcPanel(false);
  return document.querySelector(`[data-proc-act="open"][data-id="${CSS.escape(id || "")}"]`) || document.querySelector("#main h1");
}
const CLICK = {
  open: (id, t) => openDocPanel(id, t),
  new: (_id, t) => { closeProcPanel(false); openNew(t, t.dataset.kind, t.dataset.place || placeKey(currentPlace()), t.dataset.v); },
  edit: (id, t) => openEdit(fromPanel(t), id),
  "proc-fix": (id, t) => openEdit(fromPanel(t), id, "select[name=overridable]"),
  "proc-rename": (id, t) => openRename(fromPanel(t), id),
  override: (id, t) => { const place = t.dataset.place; openOverride(fromPanel(t), id, place); },
  compare: (id, t) => openDocPanel(id, t, { compare: true }),
  deprecate: (id, t) => openDeprecate(fromPanel(t), id),
  "deprecate-confirm": (id) => deprecate(id),
  preview: (_id, t) => openPreview(t.dataset.place, t),
};

document.addEventListener("click", (e) => {
  const t = e.target.closest?.("[data-proc-act]");
  if (!t || !Object.hasOwn(CLICK, t.dataset.procAct)) return;
  CLICK[t.dataset.procAct](t.dataset.id, t);
});
document.addEventListener("submit", (e) => {
  const form = e.target.closest?.("form[data-proc-form]");
  if (!form || !Object.hasOwn(SUBMIT, form.dataset.procForm)) return;
  e.preventDefault();
  SUBMIT[form.dataset.procForm](form);
});
document.addEventListener("input", (e) => {
  const t = e.target;
  if (t.id === "proc-search") { S.procQuery = t.value; render(false); return; }
  if (t.form?.matches?.("form[data-proc-form]")) t.setCustomValidity?.("");
});
document.addEventListener("change", (e) => {
  const t = e.target;
  /* Choosing a place is a history entry; the URL carries it (PROC-31) */
  if (t.id === "proc-show") { location.hash = `#/settings/processes${location.hash.includes("/standards") ? "/standards" : ""}${t.value === "org" ? "" : `?show=${t.value}`}`; return; }
  if (t.id === "proc-agent") { openPreview(`project.${t.dataset.pid}`, null, t.value); document.getElementById("proc-agent")?.focus(); return; }
  const form = t.form?.closest?.("form[data-proc-form]");
  if (form && t.name === "template") applyTemplate(form, t.value);
  if (form) t.setCustomValidity?.("");
});
function applyTemplate(form, id) {
  const tpl = TEMPLATES.find((x) => x.id === id);
  form.elements.title.value = tpl?.title || "";
  form.elements.key.value = tpl?.key || "";
  form.elements.body.value = tpl?.body || BLANK_BODY;
}
/* Reopen a reader after an in-place re-render keeps its content in step */
export const refreshProcPanel = () => { if (procPanelOpen()) { const id = document.getElementById("proc-panel")?.dataset.doc; if (id) openDocPanel(id); } };
