/* What the planning screens do: dialogs to create and edit features, releases and milestones, membership
   changes, and their undo. Session-only changes for the mock; controls carry data-plan-act. */
import { M } from "../data.js";
import {
  FEATURE_STATUS_LABEL, WORK_TYPES, addMilestone, createFeature, createRelease, deleteMilestone, featureRef, getFeature, getMilestone, getRelease,
  milestoneRef, milestoneTasks, projectFeatures, projectReleases, projectTasks, releaseName, releaseRef, restoreMilestone, retireFeature,
  setFeatureStatus, setReleaseFeatures, setTaskFeature, setTaskMilestone, slugify, updateFeature, updateMilestone, updateRelease,
} from "../plan-data.js";
import { closeSheet, openDialog, setSheetReturn } from "../shell/dialogs.js";
import { render } from "../shell/router.js";
import { S } from "../state.js";
import { esc, toast } from "../ui.js";
import { featureHref, featureLabel, milestoneLabel, releaseHref } from "./project-plan-ui.js";

const cancelBtn = '<button class="btn outline" type="button" data-act="close-sheet">Cancel</button>';
const submitBtn = (label, variant = "primary") => `<button class="btn ${variant}" type="submit" form="plan-form" id="plan-submit">${esc(label)}</button>`;
const field = (label, control, hint = "") => `<label>${label}${control}${hint ? `<span class="plan-hint">${hint}</span>` : ""}</label>`;
const text = (name, value, attrs = "") => `<input class="field" name="${name}" value="${esc(value ?? "")}" autocomplete="off"${attrs}>`;
const area = (name, value, placeholder) => `<textarea class="field" name="${name}" rows="4" placeholder="${esc(placeholder)}">${esc(value ?? "")}</textarea>`;
const select = (name, options, current) => `<select class="field" name="${name}">${options.map(([v, l]) => `<option value="${esc(v)}"${v === (current ?? "") ? " selected" : ""}>${esc(l)}</option>`).join("")}</select>`;
const ownerOptions = (none) => [["", none], ...Object.keys(M).filter((k) => k !== "tess").map((k) => [k, `${M[k].name} · ${M[k].kind === "agent" ? "Agent" : "Person"}`])];

function formDialog(trigger, title, desc, kind, id, fields, submit, variant) {
  openDialog(trigger, title, desc, `<form class="work-task-form plan-form" id="plan-form" data-plan-form="${kind}" data-id="${esc(id)}" novalidate>${fields}</form>`, cancelBtn + submitBtn(submit, variant));
  document.querySelector(".sheet")?.classList.add("plan-dialog");
  document.querySelector("#plan-form :is(input:not([type=checkbox]), textarea)")?.focus();
}
function done(focusSelector) {
  setSheetReturn(null); closeSheet(); render(false);
  requestAnimationFrame(() => (document.querySelector(focusSelector) || document.querySelector("#plan-title, #main h1"))?.focus({ preventScroll: false }));
}
function invalid(input, message) { input.setCustomValidity(message); input.reportValidity(); return false; }
const value = (form, name) => (form.elements[name]?.value ?? "").trim();

/* Features ----------------------------------------------------------------------------------------- */
function featureFields(pid, f = {}, withRelease = false) {
  const releases = projectReleases(pid);
  return field("Title", text("title", f.title, ' required placeholder="Quick capture of requests"'))
    + field("Local slug", text("local_slug", f.local_slug, ' required placeholder="quick-capture" spellcheck="false" data-touched="' + (f.local_slug ? "1" : "") + '"'), `Public slug: <code data-slug-preview>${esc(pid)}/${esc(f.local_slug || "…")}</code>`)
    + field("Definition", area("body_markdown", f.body_markdown, "What the capability does, for whom, its rules and known limits"))
    + `<div class="work-task-fields">${field(`Accountable owner${f.status && f.status !== "planned" ? "" : " (optional while Planned)"}`, select("owner_employee_id", ownerOptions("Unassigned"), f.owner_employee_id))}`
    + (withRelease ? field("Release (optional)", select("release", [["", "No release"], ...releases.map((r) => [r.id, `${releaseRef(r)} · ${releaseName(r)}`])], ""))
      : field("Work type", select("work_type", WORK_TYPES, f.work_type))) + "</div>";
}
function openNewFeature(pid, trigger) {
  formDialog(trigger, "New feature", "Creates a Planned capability definition. Tasks can link to it once it exists.", "feature-new", pid, featureFields(pid, {}, true), "Create feature");
}
function openEditFeature(fid, trigger) {
  const f = getFeature(fid);
  formDialog(trigger, `Edit ${featureRef(f)}`, "", "feature-edit", fid, featureFields(f.project_id, f), "Save feature");
}
function readFeature(form, pid, selfId) {
  const title = form.elements.title, slug = form.elements.local_slug;
  if (!value(form, "title")) return invalid(title, "Enter a title for the feature.");
  const local = slugify(value(form, "local_slug") || value(form, "title"));
  if (!local) return invalid(slug, "Use letters or numbers in the slug.");
  if (projectFeatures(pid).some((f) => f.local_slug === local && f.id !== selfId)) return invalid(slug, "Another feature in this project uses this slug.");
  const owner = value(form, "owner_employee_id") || null;
  const current = selfId && getFeature(selfId);
  if (current && current.status !== "planned" && current.status !== "retired" && !owner) return invalid(form.elements.owner_employee_id, `${FEATURE_STATUS_LABEL[current.status]} features need an accountable owner.`);
  return { title: value(form, "title"), local_slug: local, body_markdown: form.elements.body_markdown.value.trim(), owner_employee_id: owner };
}
function submitNewFeature(form) {
  const pid = form.dataset.id, fields = readFeature(form, pid);
  if (!fields) return;
  const f = createFeature(pid, { ...fields, status: "planned" });
  const rid = value(form, "release");
  if (rid) setReleaseFeatures(rid, [...getRelease(rid).feature_ids, f.id]);
  setSheetReturn(null); closeSheet();
  location.hash = featureHref(f);
  toast(`${featureRef(f)} created as Planned.`, "check-circle");
}
function submitEditFeature(form) {
  const f = getFeature(form.dataset.id), fields = readFeature(form, f.project_id, f.id);
  if (!fields) return;
  updateFeature(f.id, { ...fields, work_type: value(form, "work_type") });
  done("#plan-title");
  toast(`${featureRef(f)} saved.`, "check-circle");
}

/* Status: In progress and Live need an accountable owner (organization data 03). Asked for, not refused. */
function changeFeatureStatus(fid, status, trigger) {
  const f = getFeature(fid), before = { status: f.status, owner: f.owner_employee_id, delivered: f.delivered_at };
  if (status === f.status) return;
  if (status !== "planned" && !f.owner_employee_id) {
    formDialog(trigger, `Who owns ${featureRef(f)}?`, `${FEATURE_STATUS_LABEL[status]} features need an accountable owner. Choose one to move ${f.title} to ${FEATURE_STATUS_LABEL[status]}.`,
      "feature-status", fid, field("Accountable owner", select("owner_employee_id", ownerOptions("Choose an owner…"), "")) + `<input type="hidden" name="status" value="${status}">`, `Set to ${FEATURE_STATUS_LABEL[status]}`);
    document.querySelector("#plan-form select")?.focus();
    return;
  }
  applyFeatureStatus(f, status, null, before);
}
function applyFeatureStatus(f, status, owner, before) {
  setFeatureStatus(f.id, status, owner);
  done('[aria-controls="menu-plan-fstatus"]');
  toast(`${featureRef(f)} is ${FEATURE_STATUS_LABEL[status]}.`, "check-circle", () => {
    Object.assign(f, { status: before.status, owner_employee_id: before.owner, delivered_at: before.delivered }); render(false);
  });
}
function submitFeatureStatus(form) {
  const f = getFeature(form.dataset.id), owner = value(form, "owner_employee_id");
  if (!owner) return invalid(form.elements.owner_employee_id, "Choose an accountable owner.");
  applyFeatureStatus(f, value(form, "status"), owner, { status: f.status, owner: f.owner_employee_id, delivered: f.delivered_at });
}

function openRetire(fid, trigger) {
  const f = getFeature(fid), others = projectFeatures(f.project_id).filter((o) => o.id !== fid && o.status !== "retired");
  formDialog(trigger, `Retire ${featureRef(f)} ${f.title}?`, "Its tasks stay linked and open tasks stay open. Retiring keeps the definition and its history.", "feature-retire", fid,
    field("Reason", area("retired_reason", "", "Why is this capability being retired?") .replace("<textarea", "<textarea required"))
    + field("Replaced by (optional)", select("replaced_by_feature_id", [["", "No replacement"], ...others.map((o) => [o.id, `${featureRef(o)} ${o.title}`])], "")),
    "Retire feature", "danger");
}
function submitRetire(form) {
  const f = getFeature(form.dataset.id), reason = value(form, "retired_reason");
  if (!reason) return invalid(form.elements.retired_reason, "Say why the feature is retired.");
  retireFeature(f.id, reason, value(form, "replaced_by_feature_id"));
  done("#plan-title");
  toast(`${featureRef(f)} retired.`, "check-circle");
}

/* Membership pickers: one list of the project's tasks with where each one is now. */
function pickList(items) {
  return `<fieldset class="plan-pick"><legend class="sr-only">Tasks</legend><ul role="list">${items.map(({ id, checked, title, ref, note, move }) =>
    `<li><label><input type="checkbox" name="pick" value="${esc(id)}"${checked ? " checked" : ""}><span class="plan-pick-ref">${esc(ref)}</span><span class="plan-pick-t">${esc(title)}${note ? `<span class="plan-pick-note${move ? " is-move" : ""}">${esc(note)}</span>` : ""}</span></label></li>`).join("")}</ul></fieldset>`;
}
const picked = (form) => [...form.querySelectorAll('input[name="pick"]:checked')].map((i) => i.value);
function syncPickCount(form) {
  const button = document.getElementById("plan-submit"), n = picked(form).length, verb = form.dataset.verb;
  if (button && verb) button.textContent = verb.replace("{n}", n).replace("{s}", n === 1 ? "" : "s");
}

function openLinkTasks(fid, trigger) {
  const f = getFeature(fid), tasks = projectTasks(f.project_id).filter((w) => w.feature !== fid);
  if (!tasks.length) { toast("Every task in this project already links to this feature."); return; }
  formDialog(trigger, `Link tasks to ${featureRef(f)}`, "A task links to at most one feature. Linking keeps the task's history and does not change either feature's status.", "feature-link", fid,
    pickList(tasks.map((w) => ({ id: w.id, ref: w.id, title: w.title, note: w.feature ? `In ${featureLabel(w.feature)}; linking moves it to ${featureRef(f)}` : "", move: Boolean(w.feature) }))), "Link 0 tasks");
  const form = document.getElementById("plan-form"); form.dataset.verb = "Link {n} task{s}";
  form.querySelector("input")?.focus();
}
function submitLinkTasks(form) {
  const f = getFeature(form.dataset.id), ids = picked(form);
  if (!ids.length) { closeSheet(); return; }
  const before = ids.map((id) => [id, projectTasks(f.project_id).find((w) => w.id === id).feature]);
  ids.forEach((id) => setTaskFeature(id, f.id));
  done('[data-plan-act="feature-link"]');
  toast(`${ids.length === 1 ? ids[0] : ids.length + " tasks"} linked to ${featureRef(f)}.`, "check-circle", () => { before.forEach(([id, prev]) => setTaskFeature(id, prev)); render(false); });
}
function unlinkTask(fid, taskId) {
  const f = getFeature(fid);
  setTaskFeature(taskId, null); render(false);
  document.querySelector('[data-plan-act="feature-link"]')?.focus();
  toast(`${taskId} unlinked from ${featureRef(f)}. The task is unchanged.`, "check-circle", () => { setTaskFeature(taskId, fid); render(false); });
}

/* Releases ----------------------------------------------------------------------------------------- */
const releaseFields = (r = {}) => field("Title", text("title", r.title, ' required placeholder="Checkout v2.2 rollout"'))
  + field("Version label (optional)", text("version_label", r.version_label, ' placeholder="2.2.0" spellcheck="false"'), "Any label: 1.4.0 or Spring launch. The release is still known by its R- number.")
  + field("Release notes", area("body_markdown", r.body_markdown, "Purpose, compatibility notes and known limitations"));
function openNewRelease(pid, trigger) {
  formDialog(trigger, "New release", "Add milestones, features and tasks once it exists.", "release-new", pid, releaseFields(), "Create release");
}
function openEditRelease(rid, trigger) {
  const r = getRelease(rid);
  formDialog(trigger, `Edit ${releaseRef(r)}`, "", "release-edit", rid, releaseFields(r), "Save release");
}
function readRelease(form) {
  if (!value(form, "title")) return invalid(form.elements.title, "Enter a title for the release.");
  return { title: value(form, "title"), version_label: value(form, "version_label") || null, body_markdown: form.elements.body_markdown.value.trim() };
}
function submitRelease(form, isNew) {
  const fields = readRelease(form); if (!fields) return;
  if (isNew) {
    const r = createRelease(form.dataset.id, fields);
    setSheetReturn(null); closeSheet(); location.hash = releaseHref(r);
    toast(`${releaseRef(r)} created.`, "check-circle");
    return;
  }
  const r = updateRelease(form.dataset.id, fields);
  done("#plan-title"); toast(`${releaseRef(r)} saved.`, "check-circle");
}
function openReleaseFeatures(rid, trigger) {
  const r = getRelease(rid), list = projectFeatures(r.project_id).filter((f) => f.status !== "retired" || r.feature_ids.includes(f.id));
  if (!list.length) { toast("This project has no features yet. Create one on the Features tab."); return; }
  formDialog(trigger, `Features in ${releaseRef(r)} ${releaseName(r)}`, "Selecting a feature lists it in the release's content. A feature can be in several releases.", "release-features", rid,
    pickList(list.map((f) => ({ id: f.id, ref: featureRef(f), title: f.title, checked: r.feature_ids.includes(f.id), note: FEATURE_STATUS_LABEL[f.status] }))), "Save features");
  document.querySelector("#plan-form input")?.focus();
}
function submitReleaseFeatures(form) {
  const r = getRelease(form.dataset.id), before = [...r.feature_ids];
  setReleaseFeatures(r.id, picked(form));
  done('[data-plan-act="release-features"]');
  toast(`${releaseRef(r)} lists ${r.feature_ids.length === 1 ? "1 feature" : r.feature_ids.length + " features"}.`, "check-circle", () => { setReleaseFeatures(r.id, before); render(false); });
}
function removeReleaseFeature(rid, fid) {
  const r = getRelease(rid), before = [...r.feature_ids], f = getFeature(fid);
  setReleaseFeatures(rid, before.filter((id) => id !== fid)); render(false);
  document.querySelector('[data-plan-act="release-features"]')?.focus();
  toast(`${featureRef(f)} removed from ${releaseRef(r)}.`, "check-circle", () => { setReleaseFeatures(rid, before); render(false); });
}

/* Milestones --------------------------------------------------------------------------------------- */
const milestoneFields = (m = {}) => field("Title", text("title", m.title, ' required placeholder="Second office enrollment"'))
  + field("Description (optional)", area("body_markdown", m.body_markdown, "What finishing this milestone means"));
function openNewMilestone(rid, trigger) {
  const r = getRelease(rid);
  formDialog(trigger, `Add milestone to ${releaseRef(r)} ${releaseName(r)}`, "It is done once it has tasks and every one is finished. Assign tasks after adding it.", "milestone-new", rid, milestoneFields(), "Add milestone");
}
function openEditMilestone(mid, trigger) {
  const m = getMilestone(mid);
  formDialog(trigger, `Edit ${milestoneRef(m)}`, "", "milestone-edit", mid, milestoneFields(m), "Save milestone");
}
function submitMilestone(form, isNew) {
  if (!value(form, "title")) return invalid(form.elements.title, "Enter a title for the milestone.");
  const fields = { title: value(form, "title"), body_markdown: form.elements.body_markdown.value.trim() };
  const m = isNew ? addMilestone(form.dataset.id, fields) : updateMilestone(form.dataset.id, fields);
  done(`#plan-ms-${CSS.escape(m.id)} [data-plan-act="milestone-assign"]`);
  toast(`${milestoneRef(m)} ${isNew ? "added" : "saved"}.`, "check-circle");
}
function openDeleteMilestone(mid, trigger) {
  const m = getMilestone(mid), n = milestoneTasks(mid).length, r = getRelease(m.release_id);
  const effect = n ? `Its ${n === 1 ? "task stays" : n + " tasks stay"} in the project without a milestone and stop counting toward ${releaseRef(r)}, unless the release lists ${n === 1 ? "it" : "them"} directly.`
    : "It has no tasks.";
  openDialog(trigger, `Delete ${milestoneRef(m)} ${m.title}?`, effect, "",
    cancelBtn + `<button class="btn danger" type="button" data-plan-act="milestone-delete-confirm" data-id="${esc(mid)}">Delete milestone</button>`);
}
function confirmDeleteMilestone(mid) {
  const removed = deleteMilestone(mid);
  done('[data-plan-act="milestone-new"]');
  toast(`${milestoneRef(removed.milestone)} deleted.`, "check-circle", () => { restoreMilestone(removed); render(false); });
}
function openAssignTasks(mid, trigger) {
  const m = getMilestone(mid), r = getRelease(m.release_id), tasks = projectTasks(m.project_id);
  formDialog(trigger, `Tasks in ${milestoneRef(m)} ${m.title}`, `A task belongs to at most one milestone. Checked tasks count toward ${releaseRef(r)} ${releaseName(r)}.`, "milestone-assign", mid,
    pickList(tasks.map((w) => ({ id: w.id, ref: w.id, title: w.title, checked: w.milestone === mid,
      note: w.milestone && w.milestone !== mid ? `In ${milestoneLabel(w.milestone)}; checking moves it here` : "", move: Boolean(w.milestone && w.milestone !== mid) }))), "Save tasks");
  document.querySelector("#plan-form input")?.focus();
}
function submitAssignTasks(form) {
  const m = getMilestone(form.dataset.id), ids = new Set(picked(form)), tasks = projectTasks(m.project_id);
  const before = tasks.map((w) => [w.id, w.milestone]);
  for (const w of tasks) {
    if (ids.has(w.id) && w.milestone !== m.id) setTaskMilestone(w.id, m.id);
    if (!ids.has(w.id) && w.milestone === m.id) setTaskMilestone(w.id, null);
  }
  done(`#plan-ms-${CSS.escape(m.id)} [data-plan-act="milestone-assign"]`);
  toast(`${milestoneRef(m)} has ${ids.size === 1 ? "1 task" : ids.size + " tasks"}.`, "check-circle", () => { before.forEach(([id, prev]) => setTaskMilestone(id, prev)); render(false); });
}
function unassignTask(mid, taskId) {
  const m = getMilestone(mid);
  setTaskMilestone(taskId, null); render(false);
  document.querySelector(`#plan-ms-${CSS.escape(mid)} [data-plan-act="milestone-assign"]`)?.focus();
  toast(`${taskId} removed from ${milestoneRef(m)}. The task is unchanged.`, "check-circle", () => { setTaskMilestone(taskId, mid); render(false); });
}

/* Wiring ------------------------------------------------------------------------------------------- */
const CLICK = {
  "feature-new": (id, _v, t) => openNewFeature(id, t),
  "feature-edit": (id, _v, t) => openEditFeature(id, t),
  "feature-status": (id, v, t) => changeFeatureStatus(id, v, t),
  "feature-retire": (id, _v, t) => openRetire(id, t),
  "feature-link": (id, _v, t) => openLinkTasks(id, t),
  "feature-unlink": (id, v) => unlinkTask(id, v),
  "release-new": (id, _v, t) => openNewRelease(id, t),
  "release-edit": (id, _v, t) => openEditRelease(id, t),
  "release-features": (id, _v, t) => openReleaseFeatures(id, t),
  "release-feature-remove": (id, v) => removeReleaseFeature(id, v),
  "milestone-new": (id, _v, t) => openNewMilestone(id, t),
  "milestone-edit": (id, _v, t) => openEditMilestone(id, t),
  "milestone-delete": (id, _v, t) => openDeleteMilestone(id, t),
  "milestone-delete-confirm": (id) => confirmDeleteMilestone(id),
  "milestone-assign": (id, _v, t) => openAssignTasks(id, t),
  "milestone-unassign": (id, v) => unassignTask(id, v),
  "live-toggle": () => { S.planLiveOpen = !S.planLiveOpen; render(false); document.querySelector('[data-plan-act="live-toggle"]')?.focus(); },
};
const SUBMIT = {
  "feature-new": submitNewFeature,
  "feature-edit": submitEditFeature,
  "feature-status": submitFeatureStatus,
  "feature-retire": submitRetire,
  "feature-link": submitLinkTasks,
  "release-new": (form) => submitRelease(form, true),
  "release-edit": (form) => submitRelease(form, false),
  "release-features": submitReleaseFeatures,
  "milestone-new": (form) => submitMilestone(form, true),
  "milestone-edit": (form) => submitMilestone(form, false),
  "milestone-assign": submitAssignTasks,
};

/* A menu item hands focus back to its menu button, which the dialog then returns to on Cancel. */
function menuTrigger(t) {
  const menu = t.closest(".menu");
  if (!menu) return t;
  menu.hidden = true;
  const button = document.querySelector(`[aria-controls="${menu.id}"]`);
  button?.setAttribute("aria-expanded", "false");
  return button || t;
}
document.addEventListener("click", (e) => {
  const t = e.target.closest?.("[data-plan-act]");
  if (!t || !Object.hasOwn(CLICK, t.dataset.planAct)) return;
  CLICK[t.dataset.planAct](t.dataset.id, t.dataset.v, menuTrigger(t));
});
document.addEventListener("submit", (e) => {
  const form = e.target.closest?.("form[data-plan-form]");
  if (!form || !Object.hasOwn(SUBMIT, form.dataset.planForm)) return;
  e.preventDefault();
  SUBMIT[form.dataset.planForm](form);
});
document.addEventListener("input", (e) => {
  const t = e.target;
  if (t.id === "plan-feature-search") { S.planFeatureQuery = t.value; render(false); return; }
  const form = t.form?.closest?.("form[data-plan-form]");
  if (!form) return;
  t.setCustomValidity?.("");
  const slug = form.elements.local_slug;
  if (!slug) return;
  if (t === slug) slug.dataset.touched = "1";
  if (t.name === "title" && !slug.dataset.touched) slug.value = slugify(t.value);
  const preview = form.querySelector("[data-slug-preview]");
  if (preview) preview.textContent = `${preview.textContent.split("/")[0]}/${slugify(slug.value) || "…"}`;
});
document.addEventListener("change", (e) => {
  const form = e.target.form?.closest?.("form[data-plan-form]");
  if (form && e.target.name === "pick") syncPickCount(form);
});
