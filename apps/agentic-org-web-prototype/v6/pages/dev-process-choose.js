/* Choosing a project's development process (doc-32 §3, project.set_development_process, PKG-4/5/13): the
   project's More menu › Development process… lists every Current development process and None. Before confirming,
   the person sees the seed roles and who fills them, which roles will be created and which existing ones are used,
   the sections that will be added and the tabs the project will have. Nothing is removed and no agent starts. */
import { M, P, TEAMS } from "../data.js";
import { DEV_PROCESSES, getDevProcess, projectDevProcess, roleMatch, seedRoster } from "../devproc-data.js";
import { chooseDevProcess, sectionsToAdd } from "../docs-data.js";
import { PROJECT_TEAM, teamChain } from "../proc-data.js";
import { closeSheet, openDialog, setSheetReturn } from "../shell/dialogs.js";
import { render } from "../shell/router.js";
import { esc, ic, toast } from "../ui.js";

const cancelBtn = '<button class="btn outline" type="button" data-act="close-sheet">Cancel</button>';
const FIXED_BEFORE = ["Overview", "Activity", "Work"], FIXED_AFTER = ["Docs", "Calendar", "Insights", "Conversations", "Team"];

/* Members of the owning team and the teams beneath it fill seed roles */
function teamMembers(pid) {
  const owner = PROJECT_TEAM[pid];
  return Object.keys(TEAMS).filter((t) => teamChain(t).includes(owner)).flatMap((t) => TEAMS[t].members);
}

function previewHtml(pid, id) {
  const dp = getDevProcess(id);
  if (!dp) return `<p class="proc-muted">The project follows no development process. Its tabs are the fixed ones with Features and Releases, and its documentation starts with Decisions, Designs and Reports. Nothing is removed.</p>`;
  const roster = seedRoster(dp, teamMembers(pid)).map(({ role, holder }) => `<li>${ic(holder ? "check-circle" : "user-plus", "ic16")}<span><b>${esc(role.name)}</b> ${holder ? `filled by ${esc(M[holder].name)}` : `<span class="proc-warn-ink">open: hire or assign one</span>`}</span></li>`).join("");
  const { created, matched } = roleMatch(dp), added = sectionsToAdd(pid, dp);
  const tabs = [...FIXED_BEFORE, ...dp.tabs.map((t) => `<b>${esc(t.title)}</b>`), ...FIXED_AFTER].join(" · ");
  return `<dl class="proc-facts dev-choose-facts">
      <div><dt>Seed team</dt><dd><ul class="dev-checks" role="list">${roster}</ul></dd></div>
      <div><dt>Roles</dt><dd>Uses ${esc(matched.map((r) => r.name).join(", ") || "none")}${created.length ? `. Creates ${esc(created.map((r) => r.name).join(", "))} from the process's definitions.` : "."}</dd></div>
      <div><dt>Sections added</dt><dd>${added.length ? esc(added.join(", ")) : "None: the project has them already."}</dd></div>
      <div><dt>Tabs</dt><dd>${tabs}</dd></div>
    </dl><p class="proc-muted">Nothing is removed: documents, sections, features and roles stay. Agents get the process's skills at their next start.</p>`;
}

function openChoose(trigger, pid) {
  const cur = projectDevProcess(pid)?.id || "";
  const options = [`<option value=""${cur ? "" : " selected"}>None</option>`, ...DEV_PROCESSES.map((d) => `<option value="${esc(d.id)}"${d.id === cur ? " selected" : ""}${d.status === "current" ? "" : " disabled"}>${esc(d.name)}${d.origin === "built_in" ? " (built in)" : ""}${d.status === "current" ? "" : ` (${d.status}, can't be chosen)`}</option>`)].join("");
  openDialog(trigger, `Development process for ${P[pid].name}`, "How this project builds software: the roles it starts with, the documents it produces and its tabs.",
    `<form class="work-task-form plan-form" id="dev-form" data-dev-form="choose" data-id="${esc(pid)}" novalidate>
      <label>Development process<select class="field" name="process">${options}</select></label>
      <div id="dev-choose-preview" aria-live="polite">${previewHtml(pid, cur)}</div></form>`,
    cancelBtn + `<button class="btn primary" type="submit" form="dev-form">Save</button>`);
  document.querySelector(".sheet")?.classList.add("plan-dialog", "proc-dialog");
  document.querySelector("#dev-form select")?.focus();
}

function submitChoose(form) {
  const pid = form.dataset.id, id = form.elements.process.value, dp = getDevProcess(id);
  const before = projectDevProcess(pid)?.id || "";
  if (id === before) { setSheetReturn(null); closeSheet(); return; }
  const added = chooseDevProcess(pid, id || null);
  setSheetReturn(null); closeSheet(); render(false);
  toast(dp ? `${P[pid].name} follows ${dp.name}.${added.length ? ` Added ${added.join(", ")}.` : ""} Agents get its skills at their next start.` : `${P[pid].name} follows no development process.`, "check-circle");
}

/* A menu item hands focus back to its menu button */
function menuButton(t) {
  const menu = t.closest(".menu");
  if (!menu) return t;
  menu.hidden = true;
  const button = document.querySelector(`[aria-controls="${menu.id}"]`);
  button?.setAttribute("aria-expanded", "false");
  return button || t;
}
document.addEventListener("click", (e) => {
  const t = e.target.closest?.("[data-dev-act=choose]");
  if (t) openChoose(menuButton(t), t.dataset.id);
});
document.addEventListener("change", (e) => {
  const form = e.target.closest?.("form[data-dev-form=choose]");
  if (form) document.getElementById("dev-choose-preview").innerHTML = previewHtml(form.dataset.id, e.target.value);
});
document.addEventListener("submit", (e) => {
  const form = e.target.closest?.("form[data-dev-form=choose]");
  if (!form) return;
  e.preventDefault();
  submitChoose(form);
});
