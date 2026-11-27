/* Importing and downloading a development process (doc-32 §2 "Importing", dev_process.import and .export, PKG-16).
   A downloaded file imports again unchanged; importing validates it, gives it a new ID and adds it as a draft.
   Errors use the contract's codes: invalid_request, already_exists, too_large. People's calls only. */
import { exportDevProcess, getDevProcess, importDevProcess, validateProcessFile } from "../devproc-data.js";
import { closeSheet, openDialog, setSheetReturn } from "../shell/dialogs.js";
import { render } from "../shell/router.js";
import { S } from "../state.js";
import { esc, ic, toast } from "../ui.js";
import { openDev } from "./dev-processes.js";

const cancelBtn = '<button class="btn outline" type="button" data-act="close-sheet">Cancel</button>';
let pending = null; /* { text, bytes, name } of the chosen file */

function download(id) {
  const d = getDevProcess(id);
  const url = URL.createObjectURL(new Blob([exportDevProcess(id)], { type: "application/json" }));
  const a = Object.assign(document.createElement("a"), { href: url, download: `${d.key}.agentic-process` });
  document.body.append(a); a.click(); a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
  toast(`${d.key}.agentic-process downloaded. Importing it gives a copy with a new ID, as a draft.`, "download");
}

function openImport(trigger) {
  pending = null;
  openDialog(trigger, "Import a development process", "Choose a process file. It is validated like any write, gets a new ID and arrives as a draft. Nothing links it to the organization it came from.",
    `<form class="work-task-form plan-form" id="dev-form" data-dev-form="import" novalidate>
      <label>Process file<input class="field" type="file" name="file" accept=".agentic-process,application/json" required><span class="plan-hint">A file downloaded from any organization's Development processes. At most 5 MiB.</span></label>
      <div class="dev-import-result" id="dev-import-result" role="status" aria-live="polite"></div>
    </form>`,
    cancelBtn + `<button class="btn primary" type="submit" form="dev-form" id="dev-import-go" disabled>Import as draft</button>`);
  document.querySelector(".sheet")?.classList.add("plan-dialog", "dev-import-dialog");
  document.querySelector("#dev-form input[type=file]")?.focus();
}

function resultHtml(r) {
  const checks = r.checks.map((c) => `<li>${ic("check-circle", "ic16")}<span>${esc(c)}</span></li>`).join("");
  const error = r.code ? `<li class="is-error">${ic("x-circle", "ic16")}<span><code>${esc(r.code)}</code> ${esc(r.message)}</span></li>` : "";
  const keyField = r.code === "already_exists" ? `<label>New key<input class="field" name="key" autocomplete="off" spellcheck="false" value="${esc(`${r.manifest.key}-2`)}"><span class="plan-hint">Unique in the organization. The rest of the file is kept as it is.</span></label>` : "";
  const ok = r.code ? "" : `<p class="dev-import-ok">${ic("check-circle", "ic16")}Ready to import <b>${esc(r.manifest.name)}</b> as a draft.</p>`;
  return `<ul class="dev-checks" role="list">${checks}${error}</ul>${keyField}${ok}`;
}
function showResult(form, r) {
  document.getElementById("dev-import-result").innerHTML = resultHtml(r);
  document.getElementById("dev-import-go").disabled = Boolean(r.code) && r.code !== "already_exists";
  form.dataset.state = r.code || "ok";
}
function validate(form) {
  if (!pending) return null;
  const r = validateProcessFile(pending.text, pending.bytes, form.elements.key?.value.trim());
  showResult(form, r);
  if (form.elements.key && r.code === "already_exists") form.elements.key.focus();
  return r;
}
async function readChosen(form) {
  const file = form.elements.file.files?.[0];
  if (!file) return;
  /* too_large is decided from the size alone; the file is not read */
  pending = { name: file.name, bytes: file.size, text: file.size > 5 * 1024 * 1024 ? "" : await file.text() };
  validate(form);
}
function submitImport(form) {
  const r = validate(form);
  if (!r) { form.elements.file.setCustomValidity("Choose a process file."); form.elements.file.reportValidity(); return; }
  if (r.code) return;
  const d = importDevProcess(r.manifest);
  S.devView = "draft"; setSheetReturn(null); closeSheet(); render(false);
  toast(`${d.name} imported as a draft with a new ID.`, "check-circle");
  openDev(d.id, document.querySelector(`[data-dev-act="open"][data-id="${CSS.escape(d.id)}"]`));
}

document.addEventListener("click", (e) => {
  const t = e.target.closest?.("[data-dev-act=import], [data-dev-act=download]");
  if (!t) return;
  if (t.dataset.devAct === "import") openImport(t);
  else download(t.dataset.id);
});
document.addEventListener("change", (e) => {
  const form = e.target.closest?.("form[data-dev-form=import]");
  if (form && e.target.name === "file") readChosen(form);
});
document.addEventListener("input", (e) => {
  const form = e.target.closest?.("form[data-dev-form=import]");
  if (form && e.target.name === "key") { const r = validateProcessFile(pending.text, pending.bytes, e.target.value.trim()); document.getElementById("dev-import-go").disabled = Boolean(r.code); }
});
document.addEventListener("submit", (e) => {
  const form = e.target.closest?.("form[data-dev-form=import]");
  if (!form) return;
  e.preventDefault();
  submitImport(form);
});
