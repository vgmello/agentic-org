import { IN, M, P, TEAMS } from "../data.js";
import { esc, memberIds } from "../ui.js";

/* ---------------------------------------------------------------- dialogs */
let sheetReturn = null;
function setSheetReturn(el) { sheetReturn = el; }
function openDialog(trigger, title, desc, body, foot) {
  sheetReturn = trigger;
  document.getElementById("overlay-root").innerHTML = `<div class="overlay" data-act="close-sheet"></div><div class="sheet dialog" role="dialog" aria-modal="true" aria-labelledby="dlg-t"${desc ? ' aria-describedby="dlg-d"' : ""}>
    <h2 id="dlg-t">${esc(title)}</h2>${desc ? `<p class="dsc" id="dlg-d">${esc(desc)}</p>` : ""}${body}<div class="dfoot">${foot}</div></div>`;
  const f = document.querySelector(".sheet input:checked, .sheet select, .sheet button"); if (f) f.focus();
}
const cancelBtn = '<button class="btn outline" type="button" data-act="close-sheet">Cancel</button>';
function openHealth(pid, trigger) {
  const pr = P[pid], cur = pr.health || "none";
  const opts = [["On track", "On track"], ["At risk", "At risk"], ["Off track", "Off track"], ["none", "Not set"]];
  openDialog(trigger, `Set health for ${pr.name}`, "", `<fieldset><legend class="sr-only">Health</legend>${opts.map(([v, l]) => `<label class="opt"><input type="radio" name="health" value="${esc(v)}" ${v === cur ? "checked" : ""}>${esc(l)}</label>`).join("")}</fieldset>`,
    cancelBtn + `<button class="btn primary" type="button" data-act="health-save" data-id="${pid}">Set health</button>`);
}
function openConfirmRemove(pid, t, trigger) {
  openDialog(trigger, `Remove ${TEAMS[t].name} from ${P[pid].name}?`, `${P[pid].name} will have no team.`, "",
    cancelBtn + `<button class="btn danger" type="button" data-act="team-rm-confirm" data-id="${pid}" data-v="${t}">Remove ${esc(TEAMS[t].name)}</button>`);
}
function openConfirmOffboard(id, trigger) {
  const m = M[id]; if (!m) return;
  openDialog(trigger, `Offboard ${m.name}?`, `${m.name} will be paused and stopped. Its journal, memory, and records remain read-only.`, "",
    cancelBtn + `<button class="btn danger" type="button" data-act="offboard-confirm" data-id="${esc(id)}">Offboard ${esc(m.name)}</button>`);
}
function openAddTeam(pid, trigger) {
  const free = Object.keys(TEAMS).filter((t) => !P[pid].teams.includes(t));
  const verb = P[pid].teams.length ? "Add" : "Assign";
  openDialog(trigger, `${verb} a team to ${P[pid].name}`, "", `<div style="margin-top:16px"><label for="add-team" style="display:block;font-weight:500;margin-bottom:6px">Team</label><select class="field" id="add-team" style="width:100%">${free.map((t) => `<option value="${t}">${esc(TEAMS[t].name)}</option>`).join("")}</select></div>`,
    cancelBtn + `<button class="btn primary" type="button" data-act="team-add-save" data-id="${pid}">${verb} team</button>`);
}
function openAssign(itemId, trigger) {
  const it = IN.find((i) => i.id === itemId); if (!it) return;
  const opts = memberIds();
  openDialog(trigger, `Assign ${it.task}`, "It leaves your Inbox once someone is assigned.", `<div style="margin-top:16px"><label for="assign-to" style="display:block;font-weight:500;margin-bottom:6px">Assign to</label><select class="field" id="assign-to" style="width:100%">${opts.map((k) => `<option value="${k}" ${k === "rowan" ? "selected" : ""}>${esc(M[k].name)} · ${esc(M[k].role)}</option>`).join("")}</select></div>`,
    cancelBtn + `<button class="btn primary" type="button" data-act="assign-save" data-id="${esc(itemId)}">Assign</button>`);
}
function closeSheet() { document.getElementById("overlay-root").innerHTML = ""; if (sheetReturn) sheetReturn.focus(); }
document.addEventListener("keydown", (e) => {
  if (e.key !== "Tab") return;
  const sh = document.querySelector(".sheet"); if (!sh) return;
  const f = sh.querySelectorAll("button, a[href], input, select, textarea"); if (!f.length) return;
  const first = f[0], last = f[f.length - 1];
  if (e.shiftKey && document.activeElement === first) { e.preventDefault(); last.focus(); }
  else if (!e.shiftKey && document.activeElement === last) { e.preventDefault(); first.focus(); }
});

export {
  closeSheet,
  openAddTeam,
  openAssign,
  openConfirmOffboard,
  openConfirmRemove,
  openHealth,
  setSheetReturn,
};
