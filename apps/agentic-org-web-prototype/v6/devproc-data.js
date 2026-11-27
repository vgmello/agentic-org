/* Development processes for the mock (development process design doc-32, organization data 18 doc-86).
   A development process is content: a manifest (name, summary, roles with seeds, kinds, folders, tabs, stages),
   role definitions and skills. origin is built_in (the product's copy, never edited) or organization (a folder in
   organization Git). A project follows at most one. It is not an organization process and is never resolved
   with them (doc-54 decision 7). */
const FEATURE_DELIVERY = {
  id: "0193-dp-feature-delivery", key: "feature-delivery", origin: "built_in", name: "Feature delivery", version_label: "1.0", status: "current",
  owner: null, revision: 1, copied_from: null,
  summary: "Software delivery by features: requests are scoped into features, features are built by tasks and shipped in releases.",
  roles: [
    { key: "product-lead", name: "Product lead", seed: true, hats: ["product-owner"] },
    { key: "engineering-lead", name: "Engineering lead", seed: true, hats: ["staff-engineer"] },
    { key: "software-engineer", name: "Software engineer", seed: false, hats: [] },
    { key: "code-reviewer", name: "Code reviewer", seed: false, hats: [] },
  ],
  kinds: [],
  folders: [["decisions", "Decisions"], ["designs", "Designs"], ["reports", "Reports"]],
  tabs: [["features", "Features", "features"], ["requests", "Requests", "work"], ["bugs", "Bugs", "work"], ["board", "Board", "work"], ["releases", "Releases", "releases"]],
  stages: [
    { key: "scope", title: "Scope the request", lead: "product-lead", summary: "Turn a request into a feature with acceptance criteria, or extend one." },
    { key: "break-down", title: "Break the feature into tasks", lead: "engineering-lead", summary: "Draft tasks a person can review before they are created." },
    { key: "build", title: "Build and review", lead: "engineering-lead", summary: "Each task is built, reviewed and merged." },
    { key: "ship", title: "Plan and ship the release", lead: "product-lead", summary: "Group finished features into a release and ask for sign-off." },
  ],
  skills: [
    { key: "scope-a-request", title: "Scope a request", roles: ["product-lead"], description: "Use when a request arrives: decide whether it is a new feature or extends one, and draft its definition." },
    { key: "write-a-feature", title: "Write a feature definition", roles: ["product-lead"], description: "Use when a feature is created: purpose, how it works, rules, limits, non-goals and acceptance criteria." },
    { key: "break-into-tasks", title: "Break a feature into tasks", roles: ["engineering-lead"], description: "Use when a feature is Planned and someone asks for its tasks." },
    { key: "build-a-task", title: "Build a task", roles: ["software-engineer", "engineering-lead"], description: "Use when you are assigned a task linked to a feature: read its definition first." },
    { key: "review-a-change", title: "Review a change", roles: ["code-reviewer"], description: "Use when asked to review: check the change against the feature's acceptance criteria." },
    { key: "plan-a-release", title: "Plan a release", roles: ["product-lead", "engineering-lead"], description: "Use when features are close to done: group them into a release with milestones." },
    { key: "hand-work-on", title: "Hand work on", roles: [], description: "Use when your part is done: tell the next person in the task, by name." },
  ],
};

const DSS = {
  id: "0193-dp-dss", key: "dss", origin: "built_in", name: "Desired State Specification", version_label: "1.0", status: "current",
  owner: null, revision: 1, copied_from: null,
  summary: "Specify the product, then the system, as desired state. Plan, staff and build from the issued versions, and bring the code back in line whenever a version changes.",
  roles: [
    { key: "product-lead", name: "Product Owner / Principal Designer", seed: true, hats: ["product-owner", "principal-designer"] },
    { key: "technical-lead", name: "System Architect / Senior Staff Engineer", seed: true, hats: ["system-architect", "staff-engineer"] },
    { key: "backend-engineer", name: "Backend engineer", seed: false, hats: [] },
  ],
  kinds: [{ key: "product-dss", title: "Product DSS", versioned: true }, { key: "technical-dss", title: "Technical DSS", versioned: true }],
  folders: [["product", "Product documentation"], ["technical", "Technical documentation"]],
  tabs: [["product", "Product", "documents"], ["technical", "Technical", "documents"], ["releases", "Releases", "releases"]],
  stages: [
    { key: "specify", title: "Specify the product", lead: "product-lead", summary: "Talk with the person until the behaviour is clear, then issue it." },
    { key: "derive", title: "Derive the system", lead: "technical-lead", summary: "Issue a Technical DSS version that follows the Product DSS version." },
    { key: "converge", title: "Bring the code in line", lead: "technical-lead", summary: "Tasks realize issued versions; a version converges when its tasks finish." },
  ],
  skills: [
    { key: "specify-the-product", title: "Specify the product", roles: ["product-lead"], description: "Use when a person describes what they want: write it as a Product DSS and issue it once it is clear." },
    { key: "derive-technical-dss", title: "Derive the Technical DSS", roles: ["technical-lead"], description: "Use when a Product DSS version has been issued and the system specification has to follow it." },
    { key: "plan-convergence", title: "Plan convergence", roles: ["technical-lead"], description: "Use after issuing a version: create tasks that realize it and put them in a release." },
    { key: "realize-a-version", title: "Realize a version", roles: ["backend-engineer", "technical-lead"], description: "Use when your task realizes a version: read that version, not the working text." },
  ],
};

const FD_COPY = {
  ...FEATURE_DELIVERY,
  id: "0193-dp-fd-agentic", key: "feature-delivery-ao", origin: "organization", name: "Feature delivery (Agentic Org)", version_label: "1.1 draft",
  status: "draft", owner: "ada", revision: 3, copied_from: FEATURE_DELIVERY.id,
  summary: "Our copy of Feature delivery: the same stages, plus a release sign-off skill and a runbook kind issued in versions.",
  kinds: [{ key: "runbook", title: "Runbook", versioned: true }],
  folders: [...FEATURE_DELIVERY.folders, ["runbooks", "Runbooks"]],
  skills: [...FEATURE_DELIVERY.skills, { key: "prepare-sign-off", title: "Prepare release sign-off", roles: ["engineering-lead"], description: "Use when a release's tasks are finished: gather gates, notes and the notarized build, then ask the owner." }],
};

const DEV_PROCESSES = [FEATURE_DELIVERY, DSS, FD_COPY];
/* project.yaml development_process_id; null follows none (doc-86 "A project's process") */
const PROJECT_DEV = { "agency-v1": FEATURE_DELIVERY.id, "office-federation": DSS.id, "look-feel": null, "proto-agency": null };

const getDevProcess = (id) => DEV_PROCESSES.find((d) => d.id === id) || null;
const projectDevProcess = (pid) => getDevProcess(PROJECT_DEV[pid]);
const projectsFollowing = (id) => Object.keys(PROJECT_DEV).filter((p) => PROJECT_DEV[p] === id);
const DEV_STATUS = { draft: "Draft", current: "Current", deprecated: "Deprecated" };

function copyDevProcess(id, name, key) {
  const src = getDevProcess(id);
  const copy = { ...structuredClone(src), id: `0193-dp-${key}`, key, name, origin: "organization", status: "draft", owner: "you", revision: 1, copied_from: src.id, version_label: "" };
  DEV_PROCESSES.push(copy);
  return copy;
}
function importDevProcess(fileName) {
  const base = fileName.replace(/\.[a-z.]+$/i, "").toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "") || "imported";
  let key = base, n = 2;
  while (DEV_PROCESSES.some((d) => d.key === key)) key = `${base}-${n++}`;
  const copy = copyDevProcess(DSS.id, `Imported: ${fileName}`, key);
  copy.copied_from = null;
  copy.summary = "Imported from a process file. Validated and added as a draft with a new ID.";
  return copy;
}

export { DEV_PROCESSES, DEV_STATUS, PROJECT_DEV, copyDevProcess, getDevProcess, importDevProcess, projectDevProcess, projectsFollowing };
