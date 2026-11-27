/* Project › Releases: the project's releases and one release with its milestones, features and tasks.
   Release and milestone state is derived from task status (organization data 02); nothing is stored. */
import {
  featureProgress, featureRef, getFeature, getRelease, milestoneProgress, milestoneRef, milestoneTasks, progressState, projectReleases,
  releaseMilestones, releaseProgress, releaseRef, releaseTasks,
} from "../plan-data.js";
import { empty, esc, ic, plural, sectionHead, SEP } from "../ui.js";
import {
  featureHref, featurePill, featureWords, finishedWords, iconButton, planBar, planBody, planButton, planHead, planMenu, planTaskRow, releaseHref, statePill,
} from "./project-plan-ui.js";

function releaseRow(r) {
  const p = releaseProgress(r.id);
  const facts = [plural(p.milestones, "milestone", "milestones"), plural(r.feature_ids.length, "feature", "features"), plural(p.total, "task", "tasks")];
  return `<li class="linked plan-row">
    <span class="plan-row-ref" aria-hidden="true">${esc(releaseRef(r))}</span>
    <div class="grow"><p class="rt"><a class="stretch" href="${releaseHref(r)}"><span class="sr-only">${esc(releaseRef(r))} </span>${esc(r.title)}</a>${r.version_label ? `<span class="plan-version">${esc(r.version_label)}</span>` : ""}</p>
      <div class="meta">${facts.map((t) => `<span>${t}</span>`).join(SEP)}</div></div>
    <div class="plan-row-prog">${p.total ? planBar(p.finished, p.total, `${r.title} release readiness`, finishedWords(p)) : ""}</div>
    ${statePill(progressState(p))}${ic("chev-right", "ic20 chev")}</li>`;
}

function pageReleases(pid) {
  const list = projectReleases(pid);
  const action = planButton("release-new", pid, "plus", "New release", "primary");
  if (!list.length) return `<div class="plan-results">${empty("flag", "No releases yet", "A release groups milestones, features and tasks you plan to deliver together. Nothing requires work to be in a release.", "", action)}</div>`;
  return `<div class="work-subbar-header plan-subbar is-plain"><div class="queue-toolbar plan-toolbar">
      <p class="plan-toolbar-note">${plural(list.length, "release", "releases")} · done when every task is finished and every milestone is done</p>
      ${action}</div></div>
    <div class="plan-results" id="plan-results"><ul class="rows plan-rows" role="list">${list.map(releaseRow).join("")}</ul></div>`;
}

/* One release ------------------------------------------------------------------------------------- */
function milestoneBlock(m) {
  const p = milestoneProgress(m.id), tasks = milestoneTasks(m.id), ref = milestoneRef(m);
  const remove = (w) => iconButton("milestone-unassign", m.id, w.id, "x", `Remove ${w.id} from ${ref}`);
  const more = planMenu(`plan-ms-${m.id}`, { html: ic("ellipsis", "ic20"), label: `More actions for ${ref}` }, [
    ["milestone-edit", m.id, "", "pencil", "Edit milestone…"],
    ["milestone-delete", m.id, "", "x-circle", "Delete milestone…"],
  ], "ghost icon");
  return `<li class="plan-ms" id="plan-ms-${esc(m.id)}">
    <div class="plan-ms-head">
      <div class="plan-ms-t"><span class="plan-ref">${esc(ref)}</span><h4 id="plan-ms-${esc(m.id)}-t">${esc(m.title)}</h4>${statePill(progressState(p))}</div>
      <div class="plan-ms-actions">${planButton("milestone-assign", m.id, "plus", "Assign tasks", "outline")}${more}</div>
    </div>
    ${m.body_markdown ? `<p class="plan-ms-body">${esc(m.body_markdown)}</p>` : ""}
    <div class="plan-ms-prog">${planBar(p.finished, p.total, `${ref} progress`, finishedWords(p))}</div>
    ${tasks.length ? `<ul class="work-list plan-ms-tasks" role="list">${tasks.map((w) => planTaskRow(w, { remove: remove(w) })).join("")}</ul>`
      : `<p class="plan-empty">No tasks yet. A milestone is done once it has tasks and every one is finished.</p>`}
  </li>`;
}

function releaseFeatures(r) {
  const rows = r.feature_ids.map(getFeature).filter(Boolean).map((f) => {
    const p = featureProgress(f.id);
    return `<li class="work-item"><div class="work-row plan-task-row">
      <div class="work-summary"><span class="work-ident"><span class="work-ref">${esc(featureRef(f))}</span></span>
        <span class="work-heading plan-heading-plain"><span><a class="work-title" href="${featureHref(f)}">${esc(f.title)}</a></span></span></div>
      <div class="work-facts"><span class="tnum">${esc(featureWords(p))}</span>${featurePill(f.status)}${iconButton("release-feature-remove", r.id, f.id, "x", `Remove ${featureRef(f)} from ${releaseRef(r)}`)}</div>
    </div></li>`;
  }).join("");
  const right = planButton("release-features", r.id, "plus", "Choose features", "outline");
  return `<section class="plan-sec" aria-labelledby="plan-rf-h">${sectionHead({ id: "plan-rf-h", title: "Features", count: plural(r.feature_ids.length, "feature", "features"), right })}
    ${rows ? `<div class="work-items plan-work"><ul class="work-list" role="list">${rows}</ul></div>`
      : `<p class="plan-empty">No features selected. Listing a feature here says the release includes it; it does not change the feature's status.</p>`}</section>`;
}

/* Tasks the release lists explicitly (task_ids) that sit in none of its milestones. */
function releaseExtraTasks(r) {
  const inMilestones = new Set(releaseMilestones(r.id).flatMap((m) => milestoneTasks(m.id).map((w) => w.id)));
  const extra = releaseTasks(r.id).filter((w) => !inMilestones.has(w.id));
  if (!extra.length) return "";
  return `<section class="plan-sec" aria-labelledby="plan-rt-h">${sectionHead({ id: "plan-rt-h", title: "Other tasks in this release", count: String(extra.length) })}
    <div class="work-items plan-work"><ul class="work-list" role="list">${extra.map((w) => planTaskRow(w)).join("")}</ul></div></section>`;
}

function pageRelease(pid, rid) {
  const r = getRelease(rid);
  if (!r || r.project_id !== pid) return empty("flag", "Release not found", "It may have been removed, or the link points to another project.", "", `<a class="btn outline" href="#/projects/${esc(pid)}/releases">All releases</a>`);
  const p = releaseProgress(r.id), milestones = releaseMilestones(r.id);
  const pct = p.total ? Math.round((p.finished * 100) / p.total) : 0;
  const words = p.total ? `Release readiness ${pct}% · ${p.finished} of ${p.total} tasks finished · ${p.milestonesDone} of ${p.milestones} milestones done` : "No tasks yet: a release with no tasks is not done";
  const actions = planButton("release-edit", r.id, "pencil", "Edit", "outline");
  const addMs = planButton("milestone-new", r.id, "plus", "Add milestone", "primary");
  return `<article class="plan-page" aria-labelledby="plan-title">
    ${planHead(releaseRef(r) + (r.version_label ? " · " + r.version_label : ""), r.title, statePill(progressState(p)), actions)}
    <div class="plan-summary">${planBar(p.finished, p.total, "Release readiness", words)}</div>
    ${planBody(r.body_markdown, "No release notes written yet.")}
    <section class="plan-sec" aria-labelledby="plan-ms-h">${sectionHead({ id: "plan-ms-h", title: "Milestones", count: milestones.length ? `${p.milestonesDone} of ${milestones.length} done` : "", right: addMs })}
      ${milestones.length ? `<ol class="plan-ms-list" role="list">${milestones.map(milestoneBlock).join("")}</ol>`
        : `<p class="plan-empty">No milestones in this release. Milestones split a release into steps; each groups the tasks that finish it.</p>`}
    </section>
    ${releaseFeatures(r)}
    ${releaseExtraTasks(r)}
  </article>`;
}

export { pageRelease, pageReleases };
