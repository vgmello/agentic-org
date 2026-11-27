/* Project planning records for the mock: features, releases and milestones (organization data 02, 03 and 09).
   Field names follow the data contract. Nothing here is stored beyond this session.
   - features: project_id, number (F-<n>, project-scoped), local_slug, title, status, owner_employee_id, work_type,
     body_markdown, delivered_at, retired_reason, replaced_by_feature_id
   - releases: project_id, number (R-<n>, project-scoped), title, version_label, body_markdown,
     feature_ids[] and task_ids[] (the explicit release manifest, document 09)
   - milestones: project_id, release_id, number (M-<n>, release-scoped), title, body_markdown
   - tasks keep zero or one feature (W[].feature) and zero or one milestone (W[].milestone).
   Milestones and releases store no status: done is derived from their tasks (document 02). */
import { OPEN, P, W } from "./data.js";

const FEATURE_STATUSES = [["in_progress", "In progress"], ["planned", "Planned"], ["live", "Live"], ["retired", "Retired"]];
const FEATURE_STATUS_LABEL = Object.fromEntries(FEATURE_STATUSES);
const WORK_TYPES = [["", "Not set"], ["build", "Build"], ["run", "Run"]];

const FEATURES = [];
const RELEASES = [];
const MILESTONES = [];

/* Seed --------------------------------------------------------------------------------------------- */
const SEED = {
  "agency-v1": {
    features: [
      { slug: "office-sync", title: "Office sync & release", status: "in_progress", owner: "kai", work_type: "build", tasks: ["T-A121", "T-A140", "T-A142", "T-A146"],
        body: "Offices keep one shared record of the organization. A satellite on the previous version keeps syncing while the codec split lands, and every release has notes for the people who install it." },
      { slug: "ui-quality", title: "UI quality & onboarding", status: "in_progress", owner: "ada", work_type: "build", tasks: ["T-A112", "T-A124", "T-A144", "T-A70", "T-A86", "T-A88"],
        body: "New people find what needs them on the first screen, the UI passes its CSP gates, and chat keeps its place after sending." },
      { slug: "agent-journal", title: "Agent journal viewer", status: "planned", owner: null, work_type: "build", tasks: [],
        body: "A read-only view of an agent's journal and memory, so a person can see why an agent did what it did." },
      { slug: "hiring", title: "Hiring through HR", status: "live", owner: "maya", work_type: "build", tasks: [], delivered_at: "Nov 12",
        body: "People hire agents by asking HR, which drafts the role, checks the budget and asks for approval." },
    ],
    releases: [
      { title: "Agentic Org v1.0", version_label: "v1.0", features: ["office-sync", "ui-quality", "hiring"],
        body: "The first release: one office on a Mac, people and agents hired through HR, messages and tasks.",
        milestones: [
          { title: "Core loop on one office", body: "People and agents get in, chat and track tasks on one office.", tasks: ["T-A112", "T-A144", "T-A70", "T-A86"] },
          { title: "Release sign-off", body: "Everything the v1.0 sign-off waits on: sync, security gates and notarization.", tasks: ["T-A121", "T-A124", "T-A140", "T-A142", "T-A146", "T-A88"] },
        ] },
      { title: "Agentic Org v1.1", version_label: "v1.1", features: ["agent-journal"],
        body: "Follow-up release: see into an agent's work.",
        milestones: [{ title: "Agent journal", body: "Journal and memory are readable by the people who manage the agent.", tasks: [] }] },
    ],
  },
  "office-federation": {
    features: [
      { slug: "file-access", title: "Distributed file access", status: "in_progress", owner: "sage", work_type: "build", tasks: ["T-A118", "T-A135", "T-A81"],
        body: "One writer holds a file at a time across offices, and an office that comes back online resumes without a manual reset." },
      { slug: "office-enrollment", title: "Office enrollment", status: "in_progress", owner: "rowan", work_type: "build", tasks: ["T-A138"],
        body: "A new office enrolls with HQ using a certificate the owner approves." },
    ],
    releases: [
      { title: "Office federation", version_label: "v0.9", features: ["file-access", "office-enrollment"],
        body: "Run agents in a second office that syncs with HQ.",
        milestones: [
          { title: "Office sync test", body: "HQ and a satellite stay in sync through sleep, restarts and custody transfers.", tasks: ["T-A118", "T-A81", "T-A135"] },
          { title: "Second office enrollment", body: "", tasks: ["T-A138"] },
        ] },
    ],
  },
  "look-feel": {
    features: [
      { slug: "navigation-presence", title: "Navigation & presence", status: "in_progress", owner: "eden", work_type: "build", tasks: ["T-A130", "T-A131"],
        body: "One presence vocabulary and the IA v2 navigation on every screen." },
      { slug: "theme-accessibility", title: "Theme & accessibility", status: "in_progress", owner: "emery", work_type: "build", tasks: ["T-A133", "T-A76", "T-A84"],
        body: "Status pills, placeholders and contrast pass at every width in every theme." },
    ],
    releases: [
      { title: "Look & feel refresh", version_label: "v1.1", features: ["navigation-presence", "theme-accessibility"],
        body: "One calm, consistent look across every screen.",
        milestones: [{ title: "Look & feel refresh", body: "Final design polish signed off.", tasks: ["T-A130", "T-A131", "T-A133", "T-A76", "T-A84"] }] },
    ],
  },
};

let seq = 0;
const newId = (kind) => `${kind}-${(++seq).toString(36)}`;
const task = (id) => W.find((w) => w.id === id);

function seedProject(pid, { features, releases }) {
  const bySlug = {};
  for (const f of features) {
    const feature = createFeature(pid, { title: f.title, local_slug: f.slug, status: f.status, owner_employee_id: f.owner, work_type: f.work_type, body_markdown: f.body });
    feature.delivered_at = f.delivered_at || null;
    bySlug[f.slug] = feature;
    for (const id of f.tasks) { const w = task(id); if (w) w.feature = feature.id; }
  }
  for (const r of releases) {
    const release = createRelease(pid, { title: r.title, version_label: r.version_label, body_markdown: r.body });
    release.feature_ids = r.features.map((slug) => bySlug[slug].id);
    for (const m of r.milestones) {
      const milestone = addMilestone(release.id, { title: m.title, body_markdown: m.body });
      for (const id of m.tasks) { const w = task(id); if (w) w.milestone = milestone.id; }
    }
  }
}

/* Lookups ------------------------------------------------------------------------------------------ */
const byId = (list, id) => list.find((item) => item.id === id) || null;
const getFeature = (id) => byId(FEATURES, id);
const getRelease = (id) => byId(RELEASES, id);
const getMilestone = (id) => byId(MILESTONES, id);
const projectFeatures = (pid) => FEATURES.filter((f) => f.project_id === pid);
const projectReleases = (pid) => RELEASES.filter((r) => r.project_id === pid);
const releaseMilestones = (rid) => MILESTONES.filter((m) => m.release_id === rid).sort((a, b) => a.number - b.number);
const projectTasks = (pid) => W.filter((w) => w.proj === pid);
const featureTasks = (fid) => W.filter((w) => w.feature === fid);
const milestoneTasks = (mid) => W.filter((w) => w.milestone === mid);
const featureRef = (f) => `F-${f.number}`;
const releaseRef = (r) => `R-${r.number}`;
const milestoneRef = (m) => `M-${m.number}`;
const releaseName = (r) => r.version_label || r.title;
const releasesWithFeature = (fid) => RELEASES.filter((r) => r.feature_ids.includes(fid));

/* The release's task set: its milestones' tasks united with its explicit task_ids, without duplicates. */
function releaseTasks(rid) {
  const r = getRelease(rid); if (!r) return [];
  const ids = new Set([...releaseMilestones(rid).flatMap((m) => milestoneTasks(m.id).map((w) => w.id)), ...r.task_ids]);
  return W.filter((w) => ids.has(w.id));
}

/* Completion (document 02): a task is finished when done or cancelled. The mock's task statuses read
   Done, Fixed, Closed, Approved and Declined as finished, as the rest of the prototype does. */
const isFinished = (w) => !OPEN(w) || w.status === "Cancelled";
const isCancelled = (w) => w.status === "Cancelled" || w.status === "Declined";
function taskCounts(tasks) {
  const finished = tasks.filter(isFinished).length;
  return { total: tasks.length, finished, open: tasks.length - finished };
}
function milestoneProgress(mid) {
  const c = taskCounts(milestoneTasks(mid));
  return { ...c, done: c.total > 0 && c.finished === c.total };
}
function releaseProgress(rid) {
  const c = taskCounts(releaseTasks(rid)), ms = releaseMilestones(rid).map((m) => milestoneProgress(m.id));
  const msDone = ms.filter((m) => m.done).length;
  return { ...c, milestones: ms.length, milestonesDone: msDone, done: c.total > 0 && c.finished === c.total && msDone === ms.length };
}
/* Features leave cancelled tasks out of the denominator (features UX, FTR-10). */
function featureProgress(fid) {
  const tasks = featureTasks(fid), cancelled = tasks.filter(isCancelled).length;
  const counted = tasks.filter((w) => !isCancelled(w)), done = counted.filter(isFinished).length;
  return { total: counted.length, done, open: counted.length - done, cancelled };
}
/* A derived word for a milestone or release, never stored. */
const progressState = (p) => (p.total === 0 ? "empty" : p.done ? "done" : p.finished > 0 ? "active" : "todo");

/* The project's current release: the first one that is not done, else the latest. */
function currentRelease(pid) {
  const list = projectReleases(pid);
  return list.find((r) => !releaseProgress(r.id).done) || list[list.length - 1] || null;
}
/* Keep the project's release label (used by Home, Insights, Calendar and the strip) in step with the records. */
function syncProjectRelease(pid) {
  const r = currentRelease(pid);
  if (P[pid]) P[pid].release = r ? releaseName(r) : "";
}

/* Changes ------------------------------------------------------------------------------------------ */
const nextNumber = (list) => list.reduce((n, item) => Math.max(n, item.number), 0) + 1;
const slugify = (s) => String(s).toLowerCase().normalize("NFKD").replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "").slice(0, 48);

function createFeature(pid, fields) {
  const feature = {
    id: newId("ftr"), project_id: pid, number: nextNumber(projectFeatures(pid)),
    title: fields.title, local_slug: fields.local_slug || slugify(fields.title), status: fields.status || "planned",
    owner_employee_id: fields.owner_employee_id || null, work_type: fields.work_type || "", body_markdown: fields.body_markdown || "",
    delivered_at: null, retired_reason: null, replaced_by_feature_id: null,
  };
  FEATURES.push(feature);
  return feature;
}
function updateFeature(fid, fields) {
  const f = getFeature(fid); if (!f) return null;
  Object.assign(f, fields);
  return f;
}
function setFeatureStatus(fid, status, owner) {
  const f = getFeature(fid); if (!f) return null;
  if (owner) f.owner_employee_id = owner;
  f.status = status;
  if (status === "live" && !f.delivered_at) f.delivered_at = "Today";
  return f;
}
function retireFeature(fid, reason, replacement) {
  return updateFeature(fid, { status: "retired", retired_reason: reason, replaced_by_feature_id: replacement || null });
}
function setTaskFeature(taskId, fid) { const w = task(taskId); if (w) w.feature = fid || null; return w; }

function createRelease(pid, fields) {
  const release = {
    id: newId("rel"), project_id: pid, number: nextNumber(projectReleases(pid)),
    title: fields.title, version_label: fields.version_label || null, body_markdown: fields.body_markdown || "",
    feature_ids: [], task_ids: [],
  };
  RELEASES.push(release);
  syncProjectRelease(pid);
  return release;
}
function updateRelease(rid, fields) {
  const r = getRelease(rid); if (!r) return null;
  Object.assign(r, fields);
  syncProjectRelease(r.project_id);
  return r;
}
function setReleaseFeatures(rid, ids) { const r = getRelease(rid); if (r) r.feature_ids = [...ids]; return r; }

function addMilestone(rid, fields) {
  const r = getRelease(rid); if (!r) return null;
  const milestone = { id: newId("ms"), project_id: r.project_id, release_id: rid, number: nextNumber(releaseMilestones(rid)), title: fields.title, body_markdown: fields.body_markdown || "" };
  MILESTONES.push(milestone);
  syncProjectRelease(r.project_id);
  return milestone;
}
function updateMilestone(mid, fields) { const m = getMilestone(mid); if (m) Object.assign(m, fields); return m; }
/* Deleting a milestone clears its tasks' milestone; the tasks themselves are untouched. */
function deleteMilestone(mid) {
  const m = getMilestone(mid); if (!m) return null;
  const freed = milestoneTasks(mid).map((w) => w.id);
  for (const id of freed) task(id).milestone = null;
  MILESTONES.splice(MILESTONES.indexOf(m), 1);
  syncProjectRelease(m.project_id);
  return { milestone: m, freed };
}
function restoreMilestone({ milestone, freed }) {
  MILESTONES.push(milestone);
  for (const id of freed) { const w = task(id); if (w) w.milestone = milestone.id; }
  syncProjectRelease(milestone.project_id);
}
function setTaskMilestone(taskId, mid) {
  const w = task(taskId); if (!w) return null;
  w.milestone = mid || null;
  syncProjectRelease(w.proj);
  return w;
}

for (const [pid, plan] of Object.entries(SEED)) { seedProject(pid, plan); syncProjectRelease(pid); }

export {
  FEATURE_STATUSES,
  FEATURE_STATUS_LABEL,
  WORK_TYPES,
  addMilestone,
  createFeature,
  createRelease,
  currentRelease,
  deleteMilestone,
  featureProgress,
  featureRef,
  featureTasks,
  getFeature,
  getMilestone,
  getRelease,
  isFinished,
  milestoneProgress,
  milestoneRef,
  milestoneTasks,
  progressState,
  projectFeatures,
  projectReleases,
  projectTasks,
  releaseMilestones,
  releaseName,
  releaseProgress,
  releaseRef,
  releaseTasks,
  releasesWithFeature,
  restoreMilestone,
  retireFeature,
  setFeatureStatus,
  setReleaseFeatures,
  setTaskFeature,
  setTaskMilestone,
  slugify,
  updateFeature,
  updateMilestone,
  updateRelease,
};
