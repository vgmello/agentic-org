/* Project documentation for the mock (organization data 18 doc-86, development process design doc-32 §8, features
   UX doc-43). A project's tree is indexed by docs/index.yaml: sections (path, title, summary, kinds) holding
   documents and media. A document has a stable id, kind, status, owner, title, summary, path, revision and, for a
   versioned kind, issued versions written once. A feature's definition and its supporting documents stay beside
   the feature (projects/<p>/features/<f>/). Nothing here is stored beyond this session. */
import { W } from "./data.js";
import { PROJECT_DEV, getDevProcess, projectDevProcess } from "./devproc-data.js";
import { featureRef, getFeature, projectFeatures } from "./plan-data.js";

const BUILT_IN_KINDS = { decision: "Decision", design: "Design", report: "Report", brief: "Brief", changelog: "Changelog" };
const KIND_ICON = { decision: "scale", design: "image", report: "file-text", "product-dss": "doc-check", "technical-dss": "doc-check", runbook: "terminal", diagram: "image" };
const SECTIONS = {};
const ENTRIES = [];
const h = (rev, by, at, note) => ({ rev, by, at, note });
let seq = 0;

function kindTitle(pid, kind) {
  if (BUILT_IN_KINDS[kind]) return BUILT_IN_KINDS[kind];
  const dp = projectDevProcess(pid), k = dp?.kinds.find((x) => x.key === kind);
  return k ? k.title : kind;
}
function isVersioned(pid, kind) { return Boolean(projectDevProcess(pid)?.kinds.find((x) => x.key === kind)?.versioned); }

function add(pid, fields) {
  const e = {
    id: `0193-doc-${(++seq).toString(36)}`, project_id: pid, feature_id: null, type: "document", status: "current",
    summary: "", body: "", code_links: [], versions: [], history: [], ...fields,
  };
  e.revision = e.history.length ? e.history[0].rev : 1;
  e.updated = e.history[0]?.at || "Nov 20";
  e.updated_by = e.history[0]?.by || e.owner;
  ENTRIES.push(e);
  return e;
}
const section = (pid, path, title, summary, kinds = []) => (SECTIONS[pid] ||= []).push({ path, title, summary, kinds });

/* Agentic Org v1.0: follows dss. It started on Feature delivery, so its decisions, designs and reports sections and
   its features stay (changing the development process removes nothing, doc-32 decision 15). */
section("agency-v1", "product", "Product documentation", "What the product does, by product domain and area.", ["product-dss"]);
section("agency-v1", "technical", "Technical documentation", "How the system is built, with its diagrams and images.", ["technical-dss"]);
section("agency-v1", "decisions", "Decisions", "Choices that shape v1.0, with the reasons and what was ruled out.", ["decision"]);
section("agency-v1", "designs", "Designs", "How a part of the product is built, across features.", ["design"]);
section("agency-v1", "reports", "Reports", "Findings from runs, reviews and audits.", ["report"]);
add("agency-v1", { kind: "decision", owner: "ada", title: "One office on a Mac for v1.0", path: "decisions/one-office-on-a-mac.md",
  summary: "Why v1.0 runs on one office and the second office waits for v0.9 federation work.",
  body: "## Context\nTwo engineers want to work inside the product as early as it can carry them.\n\n## Decision\nv1.0 ships with one office on the owner's Mac. A second office is the Office federation project.\n\n## Consequences\n- Sync is designed now and switched on later.\n- The codec split keeps previous-version satellites syncing for one release.",
  history: [h(3, "ada", "Nov 21", "Consequences for the codec split"), h(2, "you", "Nov 17", "Owner's direction added"), h(1, "ada", "Nov 15", "First version")] });
add("agency-v1", { kind: "decision", owner: "tomas", title: "Tier A changes wait for the owner", path: "decisions/tier-a-changes.md",
  summary: "Which merges need the owner's go after a READY review.",
  body: "## Decision\nChanges to the wire codec, credentials or release signing are tier A: after a READY verdict they wait for the owner's go before merging.",
  history: [h(1, "tomas", "Nov 23", "First version")] });
add("agency-v1", { kind: "design", owner: "rowan", title: "Release pipeline", path: "designs/release-pipeline.md",
  summary: "From a green v1 commit to a notarized download, with the gates in between.",
  body: "## Overview\nA release commit runs the full suite and the CSP gates, then signs and notarizes the app.\n\n## Steps\n1. Tag the release commit.\n2. Build and sign on HQ.\n3. Notarize with Apple; staple the ticket.\n4. Publish the notes with the download.",
  code_links: [{ repo: "agentic-org", path: "scripts/release/notarize.sh", symbol: "", note: "Notarization and stapling." }],
  history: [h(2, "rowan", "Nov 24", "Notarization step"), h(1, "rowan", "Nov 18", "First version")] });
add("agency-v1", { kind: "report", owner: "avery", title: "CSP report-only run, all themes", path: "reports/csp-report-only-run.md",
  summary: "Clean on all seven themes; enforcing mode waits on the gateway test.",
  body: "## Findings\n- No violations in report-only mode across 27 routes and 7 themes.\n- Enforcing mode is blocked on the gateway test permission (T-A124).",
  history: [h(1, "avery", "Nov 26", "First run")] });
add("agency-v1", { type: "diagram", kind: "diagram", owner: "rowan", title: "Release pipeline", path: "designs/release-pipeline.mmd", media_type: "text/vnd.mermaid", bytes: 1840,
  summary: "Build, gates, sign, notarize, publish.", body: "flowchart LR\n  commit --> suite --> csp[CSP gates] --> sign --> notarize --> publish",
  history: [h(1, "rowan", "Nov 18", "First version")] });

const GETTING_IN = "## Purpose\nA person gets into their organization on this Mac, and an agent is hired into it through HR.\n\n## Behaviour\n- The owner signs in with the Mac's account; there is no separate password.\n- A person invited by the owner signs in with a one-time link.\n- An agent never signs in; it is provisioned with a binding when the hire is confirmed.\n\n## Rules\n- A link expires after 24 hours or one use.\n\n## Out of scope\n- Single sign-on.";
const gettingIn = add("agency-v1", { kind: "product-dss", owner: "emery", title: "Getting in", path: "product/identity-and-access/getting-in.md",
  summary: "How the owner, invited people and hired agents get into the organization. Read before changing sign-in.", body: GETTING_IN,
  versions: [
    { version: 1, revision: 2, at: "Nov 11", by: "emery", note: "First issue", body: GETTING_IN.replace("\n\n## Rules\n- A link expires after 24 hours or one use.", "") },
    { version: 2, revision: 4, at: "Nov 19", by: "emery", note: "Invitation links expire", body: GETTING_IN },
  ],
  history: [h(4, "emery", "Nov 19", "Issued v2"), h(3, "emery", "Nov 18", "Link expiry"), h(2, "emery", "Nov 11", "Issued v1")] });
const MESSAGES = "## Purpose\nPeople and agents talk in channels and direct messages, and ask each other questions that wait for an answer.\n\n## Behaviour\n- A message reaches every office of the organization.\n- A required ask suspends the asking agent until it is answered.\n\n## Rules\n- A satellite one codec version behind still receives every message.";
const messages = add("agency-v1", { kind: "product-dss", owner: "emery", title: "Messages and channels", path: "product/collaboration/messages-and-channels.md",
  summary: "Channels, direct messages and asks across offices.", body: MESSAGES + "\n- Chat keeps its place after sending.",
  versions: [{ version: 1, revision: 3, at: "Nov 16", by: "emery", note: "First issue, with the previous-version rule", body: MESSAGES }],
  history: [h(4, "jules", "Nov 26", "Chat keeps its place (unissued)"), h(3, "emery", "Nov 16", "Issued v1")] });
add("agency-v1", { kind: "product-dss", owner: "emery", title: "Tasks", path: "product/work/tasks.md", status: "draft",
  summary: "Tasks, their states and who they wait on. Not issued yet.", body: "## Purpose\nWork is tracked as tasks with one accountable team.\n\n## Open questions\n- Do closed tasks archive after 7 or 14 days?",
  history: [h(1, "emery", "Nov 25", "First draft")] });
const CODEC = "## Scope\nThe wire codec between offices.\n\n## Design\nCodec 3 splits the envelope from the payload (2a.3.2b). HQ reads codec 2 and 3 and writes 3 once every office reports 3.\n\n## Decisions\n- Dual read for one release: from the standard Codec compatibility window for v1.0.";
const codec = add("agency-v1", { kind: "technical-dss", owner: "rowan", title: "Wire codec", path: "technical/wire-codec.md",
  summary: "Envelope and payload encoding between offices. Follows Messages and channels.", body: CODEC,
  code_links: [{ repo: "agentic-org", path: "crates/org-proto/src/codec/v3.rs", symbol: "encode_envelope", note: "Codec 3 envelope." }],
  versions: [
    { version: 2, revision: 4, at: "Nov 17", by: "rowan", note: "Codec 3 envelope", body: CODEC.replace(" HQ reads codec 2 and 3 and writes 3 once every office reports 3.", ""), derived: [[messages.id, 1]] },
    { version: 3, revision: 6, at: "Nov 24", by: "rowan", note: "Dual read for one release", body: CODEC, derived: [[messages.id, 1]] },
  ],
  history: [h(6, "rowan", "Nov 24", "Issued v3"), h(5, "kai", "Nov 23", "Dual read window"), h(4, "rowan", "Nov 17", "Issued v2")] });
const CSP = "## Scope\nThe browser UI's Content Security Policy.\n\n## Design\n- No inline script or style; nonces for the one bootstrap script.\n- The gate suite runs report-only, then enforcing.\n\n## Decisions\n- From the organization standard Content Security Policy gates (not overridable).";
const csp = add("agency-v1", { kind: "technical-dss", owner: "rowan", title: "Content Security Policy", path: "technical/content-security-policy.md",
  summary: "What the UI may load and run, and how the gates check it.", body: CSP,
  versions: [{ version: 1, revision: 2, at: "Nov 20", by: "rowan", note: "First issue", body: CSP, derived: [[gettingIn.id, 2]] }],
  history: [h(2, "rowan", "Nov 20", "Issued v1"), h(1, "rowan", "Nov 19", "First draft")] });
const SIGN = "## Scope\nSigning and notarizing the Mac app.\n\n## Design\nBuild on HQ, sign with the organization's Developer ID, notarize, staple, then publish with the notes.";
const signing = add("agency-v1", { kind: "technical-dss", owner: "rowan", title: "Release signing and notarization", path: "technical/release-signing.md",
  summary: "From a green commit to a notarized download.", body: SIGN,
  versions: [{ version: 1, revision: 1, at: "Nov 24", by: "rowan", note: "First issue", body: SIGN }], history: [h(1, "rowan", "Nov 24", "Issued v1")] });

/* Office federation: Desired State Specification (versioned Product and Technical DSS) --------------- */
section("office-federation", "product", "Product documentation", "What the product does, by product domain and area.", ["product-dss"]);
section("office-federation", "technical", "Technical documentation", "How the system is built, with its diagrams and images.", ["technical-dss"]);
section("office-federation", "decisions", "Decisions", "Added when the first decision was saved.", ["decision"]);
const SYNC_V2 = "## Behaviour\nTwo offices keep one record of the organization. An office that sleeps or restarts resumes syncing without a manual reset.\n\n## Rules\n- One writer holds a file at a time.\n- A satellite on the previous version keeps syncing for one release.";
const productSync = add("office-federation", { kind: "product-dss", owner: "rowan", title: "Office sync", path: "product/offices/office-sync.md",
  summary: "What a person sees when two offices share one organization. Read before changing sync.",
  body: SYNC_V2 + "\n- An offline office shows when it was last seen.",
  versions: [
    { version: 1, revision: 3, at: "Nov 12", by: "rowan", note: "First issue", body: "## Behaviour\nTwo offices keep one record of the organization." },
    { version: 2, revision: 6, at: "Nov 20", by: "rowan", note: "Resume after sleep; previous-version satellites", body: SYNC_V2 },
  ],
  history: [h(7, "emery", "Nov 26", "Offline office shows last seen (unissued)"), h(6, "rowan", "Nov 20", "Issued v2"), h(5, "rowan", "Nov 19", "Rules for previous-version satellites"), h(3, "rowan", "Nov 12", "Issued v1")] });
const LOCK_V2 = "## Architecture\nCustody is a lease held by one office. The holder renews it while writing; a lapsed lease is re-read before any write.\n\n## Interfaces\n- `custody.acquire(path, lease)`\n- `custody.renew(token)`";
const techCustody = add("office-federation", { kind: "technical-dss", owner: "sage", title: "File custody", path: "technical/custody.md",
  summary: "How one writer holds a file across offices. Follows Office sync.",
  body: LOCK_V2,
  code_links: [{ repo: "agentic-org", path: "crates/org-office/src/custody/lease.rs", symbol: "acquire", note: "Lease acquisition." },
    { repo: "agentic-org", path: "crates/org-office/src/custody/mod.rs", symbol: "", note: "Custody service." }],
  versions: [
    { version: 1, revision: 2, at: "Nov 14", by: "sage", note: "Per-office lock", body: "## Architecture\nEach office holds a lock per file.", derived: [[productSync.id, 1]] },
    { version: 2, revision: 5, at: "Nov 22", by: "sage", note: "Lease instead of per-office lock", body: LOCK_V2, derived: [[productSync.id, 2]] },
  ],
  history: [h(5, "sage", "Nov 22", "Issued v2"), h(4, "sage", "Nov 21", "Lease interfaces"), h(2, "sage", "Nov 14", "Issued v1")] });
add("office-federation", { type: "diagram", kind: "diagram", owner: "sage", title: "Custody lease sequence", path: "technical/diagrams/custody-lease.mmd", media_type: "text/vnd.mermaid", bytes: 2210,
  summary: "HQ, satellite and the lease during a transfer.", body: "sequenceDiagram\n  HQ->>Sat: lease(path)\n  Sat-->>HQ: renew\n  HQ->>Sat: release", history: [h(1, "sage", "Nov 22", "First version")] });
add("office-federation", { kind: "technical-dss", owner: "rowan", title: "Office enrollment", path: "technical/enrollment.md", status: "draft",
  summary: "Certificates the owner approves for a new office. Not issued yet.",
  body: "## Architecture\nA new office presents a certificate signing request; the owner approves it in Inbox.", history: [h(1, "rowan", "Nov 25", "First draft")] });
add("office-federation", { kind: "decision", owner: "rowan", title: "Lease, not per-office lock", path: "decisions/lease-not-lock.md",
  summary: "Waiting on the owner's ruling; both variants are written behind one interface.",
  body: "## Decision\nProposed: a lease is safer across sleep and restarts. The owner rules between lease and per-office lock.", status: "draft",
  history: [h(1, "rowan", "Nov 24", "Proposed")] });
/* Tasks that realize issued versions (doc-86 "Tasks that realize a version") */
const realize = (taskId, d, version) => { const w = W.find((x) => x.id === taskId); if (w) (w.realizes ||= []).push({ document_id: d.id, version }); };
realize("T-A121", codec, 3); realize("T-A124", csp, 1); realize("T-A147", csp, 1); realize("T-A146", signing, 1); realize("T-A142", signing, 1); realize("T-A112", gettingIn, 2);
realize("T-A118", techCustody, 2); realize("T-A81", productSync, 2); realize("T-A135", productSync, 2); realize("T-A135", techCustody, 1);

/* Look & feel refresh: no development process (decisions, designs, reports) ------------------------- */
section("look-feel", "decisions", "Decisions", "Choices that shape the refresh.", ["decision"]);
section("look-feel", "designs", "Designs", "Screens and patterns.", ["design"]);
section("look-feel", "reports", "Reports", "Audits and reviews.", ["report"]);
add("look-feel", { kind: "design", owner: "eden", title: "Presence vocabulary", path: "designs/presence-vocabulary.md",
  summary: "The words and icons for where someone is, used on every screen.",
  body: "## Words\n- Working, With you, Available, Asleep, Offline.\n\n## Rules\n- A word plus an icon, never colour alone.", history: [h(2, "eden", "Nov 24", "Never colour alone"), h(1, "eden", "Nov 19", "First version")] });
add("look-feel", { kind: "decision", owner: "emery", title: "Icons, no emoji", path: "decisions/icons-no-emoji.md",
  summary: "One icon set across the app.", body: "## Decision\nUse the Lucide set at 16 and 20 px. No emoji in product text.", history: [h(1, "emery", "Nov 18", "First version")] });

/* Feature definitions and the documents beside them (doc-43 §1) -------------------------------------- */
const DEFINITIONS = {
  "office-sync": { rev: 4, body: "## Purpose\nOffices keep one shared record of the organization, so people and agents see the same tasks, messages and people wherever they work.\n\n## How it works\nHQ holds the record. A satellite keeps a replica and sends its writes to HQ. Every release has notes for the people who install it.\n\n## Rules\n- A satellite on the previous version keeps syncing while the codec split lands.\n- An office that cannot reach HQ says so and keeps its last copy readable.\n\n## Known limitations\n- One HQ; HQ failover is not in v1.0.\n\n## Non-goals\n- Syncing between organizations.\n\n## Acceptance criteria\n- A 2a.3.1 satellite syncs with the v1.0 HQ for a full day.\n- Release notes list the codec version.",
    history: [h(4, "kai", "Nov 24", "Rules: previous-version satellites keep syncing"), h(3, "ada", "Nov 20", "Acceptance criteria"), h(2, "ada", "Nov 16", "Known limitations"), h(1, "ada", "Nov 12", "Created as Planned")] },
  "ui-quality": { rev: 3, body: "## Purpose\nNew people find what needs them on the first screen, and the UI is safe to run.\n\n## How it works\nHome leads with what needs you. The UI passes the CSP gates; chat keeps its place after sending.\n\n## Rules\n- No inline scripts or styles (Content Security Policy gates).\n- Every fixed bug has an end-to-end test that fails before the fix.\n\n## Known limitations\n- Onboarding covers people, not agents.\n\n## Non-goals\n- A guided tour.\n\n## Acceptance criteria\n- The CSP gate suite is green in enforcing mode.\n- T-A70, T-A86 and T-A88 each have a failing-first test.",
    history: [h(3, "jules", "Nov 25", "Failing-first tests"), h(2, "ada", "Nov 19", "CSP rules"), h(1, "ada", "Nov 12", "Created as Planned")] },
  "agent-journal": { rev: 1, body: "## Purpose\nA person can see why an agent did what it did.\n\n## How it works\nA read-only view of an agent's journal and memory on its profile.\n\n## Rules\n\n## Known limitations\n\n## Non-goals\n- Editing an agent's memory.",
    history: [h(1, "emery", "Nov 22", "Created as Planned")] },
};
const FEATURE_META = new Map();
function seedFeatureDocs() {
  for (const f of projectFeatures("agency-v1")) {
    const def = DEFINITIONS[f.local_slug];
    FEATURE_META.set(f.id, { revision: def?.rev || 1, history: def?.history || [h(1, f.owner_employee_id || "ada", "Nov 12", "Created as Planned")] });
    if (def) f.body_markdown = def.body;
  }
  const bySlug = (s) => projectFeatures("agency-v1").find((f) => f.local_slug === s);
  const sync = bySlug("office-sync"), ui = bySlug("ui-quality");
  if (sync) {
    add("agency-v1", { feature_id: sync.id, kind: "design", owner: "kai", title: "Sync while the codec splits", path: `features/${sync.id}/designs/sync-during-codec-split.md`,
      summary: "How HQ reads two codec versions and writes one.", body: "## Overview\nHQ reads codec 2 and 3 and writes 3 once every office reports 3.\n\n## Risks\n- A satellite that never upgrades holds HQ on dual read.",
      history: [h(2, "kai", "Nov 24", "Dual read window"), h(1, "kai", "Nov 21", "First version")] });
    add("agency-v1", { feature_id: sync.id, kind: "decision", owner: "ada", title: "Previous-version satellites sync for one release", path: `features/${sync.id}/decisions/previous-version-window.md`,
      summary: "One release of overlap, not two.", body: "## Decision\nA satellite one codec version behind keeps syncing for one release, then must update.", history: [h(1, "ada", "Nov 20", "First version")] });
  }
  if (ui) {
    add("agency-v1", { feature_id: ui.id, kind: "report", owner: "tomas", title: "Review of the onboarding checklist", path: `features/${ui.id}/reports/onboarding-review.md`,
      summary: "CHANGES NEEDED: the checklist hides agents' first tasks.", body: "## Verdict\nCHANGES NEEDED.\n\n## Findings\n1. The checklist hides what an agent is working on.\n2. The empty state has no next step.", history: [h(1, "tomas", "Nov 25", "Review")] });
  }
}
seedFeatureDocs();

/* Lookups ------------------------------------------------------------------------------------------ */
const getEntry = (id) => ENTRIES.find((e) => e.id === id) || null;
const projectSections = (pid) => SECTIONS[pid] || [];
const treeEntries = (pid) => ENTRIES.filter((e) => e.project_id === pid && !e.feature_id);
const sectionEntries = (pid, path) => treeEntries(pid).filter((e) => e.path.startsWith(path + "/"));
const featureDocs = (fid) => ENTRIES.filter((e) => e.feature_id === fid);
const latestVersion = (e) => e.versions[e.versions.length - 1] || null;
/* A document has unissued changes when its body differs from its latest issued version (doc-86 "Issued versions") */
const hasUnissued = (e) => { const v = latestVersion(e); return Boolean(v) && v.body !== e.body; };
const featureMeta = (fid) => {
  if (!FEATURE_META.has(fid)) FEATURE_META.set(fid, { revision: 1, history: [h(1, "you", "Today", "Created as Planned")] });
  return FEATURE_META.get(fid);
};
const featurePath = (f) => `projects/${f.project_id}/features/${f.id}/feature.md`;
/* A version converges when at least one task realizes it and every one of those tasks is finished */
function versionTasks(e, version) { return W.filter((w) => (w.realizes || []).some((r) => r.document_id === e.id && r.version === version)); }

/* Changes ------------------------------------------------------------------------------------------ */
function saveEntry(id, fields, note) {
  const e = getEntry(id), rev = e.revision + 1;
  Object.assign(e, fields, { revision: rev, updated: "Nov 27", updated_by: "you" });
  e.history.unshift(h(rev, "you", "Nov 27", note || "Edited"));
  return e;
}
function issueVersion(id, note, derived) {
  const e = getEntry(id), version = (latestVersion(e)?.version || 0) + 1;
  e.versions.push({ version, revision: e.revision, at: "Nov 27", by: "you", note, body: e.body, derived });
  e.status = "current";
  saveEntry(id, {}, `Issued v${version}`);
  return version;
}
function createEntry(pid, fields) {
  return add(pid, { ...fields, history: [h(1, "you", "Nov 27", "First version")] });
}
function saveFeatureDefinition(f, body, note) {
  const meta = featureMeta(f.id);
  f.body_markdown = body;
  meta.revision += 1;
  meta.history.unshift(h(meta.revision, "you", "Nov 27", note || "Definition edited"));
}
/* project.set_development_process (doc-32 §3): sets the field, adds the sections the process declares and the
   project lacks, and creates missing roles. Nothing is removed and no agent is started. */
function chooseDevProcess(pid, id) {
  const dp = getDevProcess(id), have = new Set(projectSections(pid).map((s) => s.path));
  const added = (dp?.folders || []).filter(([path]) => !have.has(path));
  added.forEach(([path, title]) => section(pid, path, title, "Added when the development process was chosen.", []));
  PROJECT_DEV[pid] = id || null;
  return added.map(([, title]) => title);
}
const sectionsToAdd = (pid, dp) => { const have = new Set(projectSections(pid).map((s) => s.path)); return (dp?.folders || []).filter(([path]) => !have.has(path)).map(([, t]) => t); };
const featureLabel = (fid) => { const f = getFeature(fid); return f ? `${featureRef(f)} ${f.title}` : ""; };

export {
  BUILT_IN_KINDS, KIND_ICON, chooseDevProcess, createEntry, sectionsToAdd, featureDocs, featureLabel, featureMeta, featurePath, getDevProcess, getEntry, hasUnissued, isVersioned, issueVersion,
  kindTitle, latestVersion, projectSections, saveEntry, saveFeatureDefinition, sectionEntries, treeEntries, versionTasks,
};
