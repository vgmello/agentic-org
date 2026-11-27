/* New project (owner): a small dialog from the Projects page. A new project gets a lead, an accountable team (a new
   team named after it by default, TEAM-8) and optionally a development process. Session-only for the mock. */
import { M, P, TEAMS } from "../data.js";
import { DEV_PROCESSES, PROJECT_DEV } from "../devproc-data.js";
import { S } from "../state.js";
import { esc, ic, toast } from "../ui.js";
import { closeSheet, openDialog } from "../shell/dialogs.js";

const cancelBtn = '<button class="btn outline" type="button" data-act="close-sheet">Cancel</button>';

const opt = (v, l, sel) => `<option value="${esc(v)}"${v === sel ? " selected" : ""}>${esc(l)}</option>`;
const slug = (name) => name.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "").slice(0, 40) || "project";

function openNewProject(trigger) {
  const leads = Object.keys(M).filter((k) => !M[k].owner && !M[k].builtin && !M[k].offboarded);
  const dps = DEV_PROCESSES.filter((d) => d.status === "current");
  openDialog(trigger, "New project", "Projects group work toward a goal. You can change any of this later.",
    `<form class="work-task-form plan-form" id="new-project-form" novalidate>
      <div class="work-task-fields">
        <label>Name<input class="field" name="name" required autocomplete="off" placeholder="Billing v1"></label>
        <p class="plan-err" id="np-err" role="alert" hidden></p>
        <label><span>Goal <span class="muted">· optional</span></span><input class="field" name="goal" autocomplete="off" placeholder="What done looks like"></label>
        <label>Lead<select class="field" name="lead">${opt("", "No lead yet", "")}${leads.map((k) => opt(k, `${M[k].name} · ${M[k].role}`, "")).join("")}</select></label>
        <label>Accountable team<select class="field" name="team">${opt("__new", "A new team named after the project", "__new")}${Object.keys(TEAMS).map((t) => opt(t, TEAMS[t].name, "")).join("")}</select></label>
        <label>Development process<select class="field" name="dp">${opt("", "None", "")}${dps.map((d) => opt(d.id, d.name, "")).join("")}</select></label>
        <label><span>Due date <span class="muted">· optional</span></span><input class="field" name="due" type="date"></label>
      </div></form>`,
    cancelBtn + `<button class="btn primary" type="submit" form="new-project-form">${ic("plus", "ic16")}Create project</button>`);
  document.querySelector(".sheet")?.classList.add("plan-dialog", "np-dialog");
  document.querySelector("#new-project-form input[name=name]")?.focus();
}

function nameError(name) {
  if (!name) return "Enter a name for the project.";
  return Object.values(P).some((p) => p.name.toLowerCase() === name.toLowerCase()) ? `A project named “${name}” already exists.` : "";
}
function newId(name) {
  let id = slug(name);
  while (P[id]) id += "-2";
  return id;
}
function dueFields(value) {
  if (!value) return {};
  const due = new Date(value + "T12:00:00"), mon = due.toLocaleString("en", { month: "short" });
  return { due: `${mon} ${due.getDate()}`, dueMonth: mon, dueDay: String(due.getDate()) };
}
function accountableTeam(choice, id, name, lead) {
  if (choice !== "__new") return choice;
  TEAMS[id] = { name, lead, purpose: `Delivers ${name}.`, members: lead ? [lead] : [], channel: "general" };
  return id;
}
function createProject(form) {
  const d = Object.fromEntries(new FormData(form)), name = String(d.name || "").trim(), err = form.querySelector("#np-err");
  const problem = nameError(name);
  if (problem) { err.textContent = problem; err.hidden = false; form.querySelector("input[name=name]").focus(); return; }
  const id = newId(name), lead = d.lead || null;
  const team = accountableTeam(d.team, id, name, lead);
  P[id] = { name, health: "On track", upd: 0, lead, teams: [team], goal: String(d.goal || "").trim(), updated: "just now", channel: "general", ...dueFields(d.due) };
  PROJECT_DEV[id] = d.dp || null;
  if (!S.favProjs.includes(id)) S.favProjs.push(id);
  closeSheet();
  location.hash = `#/projects/${id}`;
  toast(`Created ${name}.`);
}

document.addEventListener("click", (e) => { const t = e.target.closest?.('[data-act="new-project"]'); if (t) openNewProject(t); });
document.addEventListener("submit", (e) => { if (e.target.id === "new-project-form") { e.preventDefault(); createProject(e.target); } });

export { openNewProject };
