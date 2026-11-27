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
  tabs: [
    { key: "features", title: "Features", view: "features" },
    { key: "requests", title: "Requests", view: "work", types: ["request"], layout: "list" },
    { key: "bugs", title: "Bugs", view: "work", types: ["bug"], layout: "list" },
    { key: "board", title: "Board", view: "work", types: ["task", "bug", "request"], layout: "board", lanes: "release" },
    { key: "releases", title: "Releases", view: "releases" },
  ],
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

/* The built-in dss development process, as the desired state design (doc-31 §1) declares it */
const DSS = {
  id: "0193-dp-dss", key: "dss", origin: "built_in", name: "Desired State Specification", version_label: "1.0", status: "current",
  owner: null, revision: 1, copied_from: null,
  summary: "Specify the product, then the system, as desired state. Plan, staff and build from the issued versions, and bring the code back in line whenever a version changes.",
  roles: [
    { key: "product-lead", name: "Product Owner / Principal Designer", seed: true, hats: ["product-owner", "principal-designer"] },
    { key: "technical-lead", name: "System Architect / Senior Staff Engineer", seed: true, hats: ["system-architect", "staff-engineer"] },
    { key: "backend-engineer", name: "REST Backend Engineer", seed: false, hats: [] },
    { key: "frontend-engineer", name: "Frontend Engineer", seed: false, hats: [] },
  ],
  kinds: [{ key: "product-dss", title: "Product DSS", versioned: true }, { key: "technical-dss", title: "Technical DSS", versioned: true }],
  folders: [["product", "Product documentation"], ["technical", "Technical documentation"]],
  tabs: [
    { key: "product", title: "Product", view: "documents", section: "product", kinds: ["product-dss"] },
    { key: "technical", title: "Technical", view: "documents", section: "technical" },
    { key: "releases", title: "Releases", view: "releases" },
  ],
  stages: [
    { key: "specify", title: "Specify the product", lead: "product-lead", summary: "Talk with the person until the behaviour is clear, then issue it." },
    { key: "derive", title: "Derive the technical specification", lead: "technical-lead", summary: "Standards first, then the person; issue it derived from the Product DSS versions." },
    { key: "plan", title: "Plan and staff", lead: "technical-lead", summary: "Tasks that realize the versions, in a release; hire the roles the plan needs." },
    { key: "build", title: "Build and answer questions", lead: null, summary: "Engineers build from the issued versions and ask the technical lead." },
    { key: "reconcile", title: "Keep the code in line with the specification", lead: null, summary: "Each new version gets the tasks that realize it; a version converges when they finish." },
  ],
  skills: [
    { key: "how-dss-works", title: "How DSS works", roles: [], description: "The two layers, what an issued version is, where documents live (read the index first), and whom to ask what." },
    { key: "write-product-dss", title: "Write the Product DSS", roles: ["product-lead"], description: "Talk with the person until the behaviour is clear; write it under its product domain; confirm; issue." },
    { key: "derive-technical-dss", title: "Derive the Technical DSS", roles: ["technical-lead"], description: "From an issued Product DSS version, update the Technical DSS: standards first, then the person." },
    { key: "record-a-standard", title: "Record a standard", roles: ["technical-lead"], description: "When a decision will be needed again, write or update a project standard." },
    { key: "plan-and-staff", title: "Plan and staff", roles: ["product-lead", "technical-lead"], description: "Break both layers into tasks; put them in a release, with milestones if useful; decide the roles needed; ask HR." },
    { key: "answer-architecture-questions", title: "Answer architecture questions", roles: ["technical-lead"], description: "Answer engineers from the Technical DSS and standards; change the specification when the answer changes it." },
    { key: "build-from-the-specification", title: "Build from the specification", roles: [], description: "Read the versions a task realizes; ask the technical lead; keep code links current." },
    { key: "reconcile-a-new-version", title: "Reconcile a new version", roles: ["product-lead", "technical-lead"], description: "When a version is issued over an earlier one, find what it changes and plan the work." },
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
const PROJECT_DEV = { "agency-v1": DSS.id, "office-federation": DSS.id, "look-feel": FEATURE_DELIVERY.id, "proto-agency": null };

/* The organization's roles (organization data 01) and who holds each, for seed rosters. A process role whose key
   the organization lacks is created from the process's definition when the process is chosen (doc-32 §3). */
const ORG_ROLE_HOLDERS = {
  "product-lead": ["emery", "eden"], "technical-lead": ["rowan"], "engineering-lead": ["ada"],
  "software-engineer": ["kai", "sage", "quinn", "jules", "avery"], "code-reviewer": ["tomas"],
};

const getDevProcess = (id) => DEV_PROCESSES.find((d) => d.id === id) || null;
const projectDevProcess = (pid) => getDevProcess(PROJECT_DEV[pid]);
const projectsFollowing = (id) => Object.keys(PROJECT_DEV).filter((p) => PROJECT_DEV[p] === id);
const DEV_STATUS = { draft: "Draft", current: "Current", deprecated: "Deprecated" };

function seedRoster(dp, teamMembers) {
  return dp.roles.filter((r) => r.seed).map((r) => ({ role: r, holder: (ORG_ROLE_HOLDERS[r.key] || []).find((m) => teamMembers.includes(m)) || null }));
}
function roleMatch(dp) {
  return { created: dp.roles.filter((r) => !ORG_ROLE_HOLDERS[r.key]), matched: dp.roles.filter((r) => ORG_ROLE_HOLDERS[r.key]) };
}

function copyDevProcess(id, name, key) {
  const src = getDevProcess(id);
  const copy = { ...structuredClone(src), id: `0193-dp-${key}`, key, name, origin: "organization", status: "draft", owner: "you", revision: 1, copied_from: src.id, version_label: "" };
  DEV_PROCESSES.push(copy);
  return copy;
}
/* A process file: the folder's content as one file (doc-32 §2 "Importing"). Identity, lifecycle and owner are not
   in it: importing gives a new ID and a draft. */
const MANIFEST_FIELDS = ["key", "name", "summary", "version_label", "roles", "kinds", "folders", "tabs", "stages", "skills"];
const BUILT_IN_KIND_KEYS = ["brief", "decision", "design", "report", "changelog"];
const FIXED_TAB_KEYS = ["overview", "work", "docs", "team", "activity", "conversations"];
const VIEWS = ["documents", "work", "features", "releases"];
const MAX_FILE_BYTES = 5 * 1024 * 1024;

function exportDevProcess(id) {
  const d = getDevProcess(id);
  const manifest = Object.fromEntries(MANIFEST_FIELDS.map((f) => [f, d[f] ?? null]));
  return JSON.stringify({ format: "agentic-org.development-process", schema_version: 1, process: manifest }, null, 2);
}

/* Validation at import, as at every write (doc-32 §2): a closed field list, skills name declared roles, no
   built-in kind, valid tabs. Returns the checks it ran and the first error, by contract code. */
const MANIFEST_RULES = [
  (m) => { const k = Object.keys(m).find((f) => !MANIFEST_FIELDS.includes(f)); return k && `Unknown field ${k}. A development process names no tool, model, credential, budget or approver.`; },
  (m) => !/^[a-z0-9][a-z0-9-]{0,63}$/.test(m.key || "") && "The key must be 1 to 64 lowercase letters, digits or hyphens.",
  (m) => (!m.name || !Array.isArray(m.roles) || !m.roles.length) && "A development process needs a name and at least one role.",
  (m) => { const roles = new Set(m.roles.map((r) => r.key)), sk = (m.skills || []).find((x) => (x.roles || []).some((r) => !roles.has(r))); return sk && `The skill ${sk.key} names a role the process doesn't declare.`; },
  (m) => { const k = (m.kinds || []).find((x) => BUILT_IN_KIND_KEYS.includes(x.key)); return k && `The kind ${k.key} is a built-in kind and can't be redefined.`; },
  (m) => { const t = (m.tabs || []).find((x) => FIXED_TAB_KEYS.includes(x.key) || !VIEWS.includes(x.view)); return t && `The tab ${t.key} uses a fixed tab's key or an unknown view.`; },
];
function manifestErrors(m) {
  for (const rule of MANIFEST_RULES) { const error = rule(m); if (error) return error; }
  return null;
}
function validateProcessFile(text, bytes, keyOverride) {
  const checks = [];
  if (bytes > MAX_FILE_BYTES) return { code: "too_large", message: `The file is ${(bytes / 1048576).toFixed(1)} MiB; a process file is at most 5 MiB.`, checks };
  checks.push("Size within 5 MiB");
  let file = null;
  try { file = JSON.parse(text); } catch (e) { file = null; }
  if (!file || file.format !== "agentic-org.development-process" || !file.process) return { code: "invalid_request", message: "This isn't a development process file.", checks };
  checks.push("Recognized as a development process file");
  const m = { ...file.process, ...(keyOverride ? { key: keyOverride } : {}) };
  const error = manifestErrors(m);
  if (error) return { code: "invalid_request", message: error, checks };
  checks.push(`Manifest valid: ${m.roles.length} roles, ${(m.skills || []).length} skills, ${(m.tabs || []).length} tabs`);
  if (DEV_PROCESSES.some((d) => d.key === m.key)) return { code: "already_exists", message: `The key ${m.key} is taken in this organization. Choose another key.`, checks, manifest: m };
  checks.push(`Key ${m.key} is free`);
  return { code: null, checks, manifest: m };
}
function importDevProcess(manifest) {
  const d = { ...structuredClone(manifest), id: `0193-dp-${manifest.key}`, origin: "organization", status: "draft", owner: "you", revision: 1, copied_from: null, imported: true };
  d.kinds ||= []; d.folders ||= []; d.tabs ||= []; d.stages ||= []; d.skills ||= [];
  DEV_PROCESSES.push(d);
  return d;
}

export {
  DEV_PROCESSES, DEV_STATUS, PROJECT_DEV, copyDevProcess, exportDevProcess, getDevProcess, importDevProcess, projectDevProcess, projectsFollowing, roleMatch,
  seedRoster, validateProcessFile,
};
