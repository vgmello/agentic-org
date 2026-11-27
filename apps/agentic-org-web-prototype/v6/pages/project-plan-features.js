/* Project › Features: the capability map (features UX §2) and one feature's page. */
import { M } from "../data.js";
import {
  FEATURE_STATUSES, featureProgress, featureRef, featureTasks, getFeature, isFinished, projectFeatures, releaseName, releaseRef, releasesWithFeature,
} from "../plan-data.js";
import { S } from "../state.js";
import { featureDefinitionHtml, featureHistoryHtml, featureSupportHtml } from "./feature-docs.js";
import { av, empty, esc, ic, plural, searchRow, searchToggle, sectionHead, SEP } from "../ui.js";
import {
  featureHref, featurePill, featureWords, iconButton, planBar, planButton, planFact, planHead, planMenu, planTaskRow, releaseHref,
} from "./project-plan-ui.js";

Object.assign(S, { planFeatureView: "active", planFeatureQuery: "", planLiveOpen: false });

const FEATURE_VIEWS = [["active", "All"], ["in_progress", "In progress"], ["planned", "Planned"], ["live", "Live"], ["retired", "Retired"]];
const inView = (f, view) => (view === "active" ? f.status !== "retired" : f.status === view);
function matchesQuery(f, q) {
  if (!q) return true;
  return [featureRef(f), f.title, f.local_slug].join(" ").toLowerCase().includes(q);
}

function featuresToolbar(pid, list, view) {
  const label = "Search features", value = S.planFeatureQuery;
  return `<div class="work-subbar-header plan-subbar">
    <div class="queue-toolbar plan-toolbar">
      <div class="primary-tab-row">
        <div class="queue-tabs" role="group" aria-label="Feature status">
          ${FEATURE_VIEWS.map(([v, l]) => `<button class="queue-tab" type="button" data-act="set" data-k="planFeatureView" data-v="${v}" aria-pressed="${view === v}" aria-controls="plan-results">${l}<span>${list.filter((f) => inView(f, v)).length}</span></button>`).join("")}
        </div>
      </div>
      <div class="queue-controls" role="group" aria-label="Feature tools">${searchToggle("plan-feature-search", label, value)}</div>
      ${planButton("feature-new", pid, "plus", "New feature", "primary")}
    </div>
  </div>${searchRow("plan-feature-search", label, value, "Search F- reference, title or slug… (Esc to close)", "work-search-row")}`;
}

function featureRow(f) {
  const p = featureProgress(f.id), rels = releasesWithFeature(f.id);
  const owner = f.owner_employee_id && M[f.owner_employee_id]
    ? `<span class="plan-owner">${av(f.owner_employee_id, 24)}${esc(M[f.owner_employee_id].name)}</span>` : `<span>Unassigned</span>`;
  const relText = rels.length ? `<span>In ${rels.map((r) => `${esc(releaseRef(r))} ${esc(releaseName(r))}`).join(", ")}</span>` : `<span>No release</span>`;
  const cancelled = p.cancelled ? SEP + `<span>${p.cancelled} cancelled, not counted</span>` : "";
  return `<li class="linked plan-row">
    <span class="plan-row-ref" aria-hidden="true">${esc(featureRef(f))}</span>
    <div class="grow"><p class="rt"><a class="stretch" href="${featureHref(f)}"><span class="sr-only">${esc(featureRef(f))} </span>${esc(f.title)}</a></p>
      <div class="meta">${owner}${SEP}${relText}${cancelled}</div></div>
    <div class="plan-row-prog">${planBar(p.done, p.total, `${f.title} task progress`, featureWords(p))}</div>
    ${featurePill(f.status)}${ic("chev-right", "ic20 chev")}</li>`;
}

/* Grouped by status. Live is collapsed by default; Retired shows only when selected (FTR-10). */
function featureGroups(rows, view) {
  const statuses = view === "active" ? FEATURE_STATUSES.filter(([s]) => s !== "retired") : FEATURE_STATUSES.filter(([s]) => s === view);
  return statuses.map(([status, label]) => {
    const list = rows.filter((f) => f.status === status);
    if (!list.length) return "";
    const collapsible = status === "live" && view === "active", open = !collapsible || S.planLiveOpen;
    const title = collapsible
      ? `<button class="plan-group-toggle" type="button" data-plan-act="live-toggle" data-id="live" aria-expanded="${open}" aria-controls="plan-live">${ic(open ? "chev-down" : "chev-right", "ic16")}${label}<span class="cnt">${list.length}</span></button>`
      : `${label}<span class="cnt">${list.length}</span>`;
    return `<div class="work-group plan-group"><h3 class="work-group-title">${title}</h3>
      ${open ? `<ul class="rows plan-rows" role="list"${collapsible ? ' id="plan-live"' : ""}>${list.map(featureRow).join("")}</ul>` : ""}</div>`;
  }).join("");
}

function pageFeatures(pid) {
  const list = projectFeatures(pid), view = FEATURE_VIEWS.some(([v]) => v === S.planFeatureView) ? S.planFeatureView : "active";
  const q = S.planFeatureQuery.trim().toLowerCase();
  const rows = list.filter((f) => inView(f, view) && matchesQuery(f, q));
  const action = planButton("feature-new", pid, "plus", "New feature", "primary");
  const body = !list.length
    ? empty("star", "No features defined yet", "A feature describes one capability this project delivers. Tasks can link to one; a project can also run on tasks alone.", "", action)
    : rows.length ? featureGroups(rows, view)
      : `<p class="empty-row">No features match${q ? ` “${esc(S.planFeatureQuery.trim())}”` : " this status"}. Try another status or search term.</p>`;
  return (list.length ? featuresToolbar(pid, list, view) : "") + `<div class="plan-results" id="plan-results">${body}</div>`;
}

/* One feature ------------------------------------------------------------------------------------- */
function featureFacts(f) {
  const rels = releasesWithFeature(f.id), owner = f.owner_employee_id && M[f.owner_employee_id]
    ? `<span class="plan-owner">${av(f.owner_employee_id, 24)}<a class="alink" href="#/members/${esc(f.owner_employee_id)}" data-person="${esc(f.owner_employee_id)}">${esc(M[f.owner_employee_id].name)}</a></span>`
    : "Unassigned";
  const relLinks = rels.length ? rels.map((r) => `<a class="alink" href="${releaseHref(r)}">${esc(releaseRef(r))} · ${esc(releaseName(r))}</a>`).join(", ") : "Not listed in any release yet";
  const workType = { build: "Build", run: "Run" }[f.work_type] || "Not set";
  return `<dl class="triage-fields pj-fields plan-facts">
    ${planFact("Accountable owner", owner)}
    ${planFact("Included in release", relLinks)}
    ${planFact("Work type", esc(workType))}
    ${planFact("Public slug", `<code>${esc(f.project_id)}/${esc(f.local_slug)}</code>`)}
    ${f.delivered_at ? planFact("Delivered", esc(f.delivered_at)) : ""}
  </dl>`;
}

function featureStatusMenu(f) {
  const items = FEATURE_STATUSES.filter(([s]) => s !== "retired").map(([s, l]) => ["feature-status", f.id, s, s === f.status ? "check" : "flag", l, s === f.status]);
  return planMenu("plan-fstatus", { html: `Change status${ic("chev-down", "ic16")}`, label: "" }, items);
}

function featureTaskList(f) {
  const tasks = featureTasks(f.id), open = tasks.filter((w) => !isFinished(w)), done = tasks.filter(isFinished);
  const remove = (w) => iconButton("feature-unlink", f.id, w.id, "x", `Unlink ${w.id} from ${featureRef(f)}`);
  const group = (label, list) => (list.length ? `<div class="work-group"><h4 class="work-group-title">${label}<span class="cnt">${list.length}</span></h4>
    <ul class="work-list" role="list">${list.map((w) => planTaskRow(w, { remove: remove(w) })).join("")}</ul></div>` : "");
  const right = planButton("feature-link", f.id, "plus", "Link tasks", "outline");
  return `<section class="plan-sec" aria-labelledby="plan-tasks-h">${sectionHead({ id: "plan-tasks-h", title: "Tasks", count: plural(tasks.length, "task", "tasks"), right })}
    ${tasks.length ? `<div class="work-items plan-work">${group("Open", open)}${group("Finished", done)}</div>`
      : `<p class="plan-empty">No tasks link to this feature yet. Linking a task does not change the feature's status.</p>`}</section>`;
}

function pageFeature(pid, fid) {
  const f = getFeature(fid);
  if (!f || f.project_id !== pid) return empty("star", "Feature not found", "It may have been removed, or the link points to another project.", "", `<a class="btn outline" href="#/projects/${esc(pid)}/features">All features</a>`);
  const p = featureProgress(f.id), retired = f.status === "retired";
  const replacement = f.replaced_by_feature_id && getFeature(f.replaced_by_feature_id);
  const actions = (retired ? "" : featureStatusMenu(f)) + planButton("feature-edit", f.id, "pencil", "Edit", "outline")
    + (retired ? "" : planMenu("plan-fmore", { html: ic("ellipsis", "ic20"), label: `More actions for ${featureRef(f)}` }, [["feature-retire", f.id, "", "archive", "Retire feature…"]], "ghost icon"));
  const retiredNote = retired ? `<div class="callout plan-callout" role="note">${ic("archive", "ic20")}<div><p><b>Retired.</b> ${esc(f.retired_reason || "")}</p>${replacement ? `<p>Replaced by <a class="alink" href="${featureHref(replacement)}">${esc(featureRef(replacement))} ${esc(replacement.title)}</a></p>` : ""}<p class="muted">Its tasks stay linked; open tasks stay open.</p></div></div>` : "";
  const cancelled = p.cancelled ? ` · ${p.cancelled} cancelled, not counted` : "";
  return `<article class="plan-page" aria-labelledby="plan-title">
    ${planHead(featureRef(f), f.title, featurePill(f.status), actions)}
    ${retiredNote}
    <div class="plan-summary">${planBar(p.done, p.total, "Feature task progress", featureWords(p) + cancelled)}</div>
    ${featureFacts(f)}
    ${featureDefinitionHtml(f)}
    ${featureTaskList(f)}
    ${featureSupportHtml(f)}
    ${featureHistoryHtml(f)}
  </article>`;
}

export { pageFeature, pageFeatures };
