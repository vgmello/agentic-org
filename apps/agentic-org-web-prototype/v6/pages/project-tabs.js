/* A project's tabs (development process design doc-32 §4, PKG-15): the fixed tabs, and between Work and Docs the
   tabs the project's development process declares, each one of the product's generic views (documents, work,
   features, releases). A project with no development process gets Features and Releases; a project that holds
   features or releases its process gives no tab for keeps a tab for them. Nothing about tabs is stored. */
import { OPEN, W } from "../data.js";
import { projectDevProcess } from "../devproc-data.js";
import { KIND_ICON, isVersioned, projectSections, sectionEntries } from "../docs-data.js";
import { projectFeatures, projectReleases } from "../plan-data.js";
import { esc, ic, sectionHead } from "../ui.js";
import { docRow } from "./docs.js";
import { planTaskRow } from "./project-plan-ui.js";

const DEFAULT_TABS = [{ key: "features", title: "Features", view: "features" }, { key: "releases", title: "Releases", view: "releases" }];

function processTabs(pid) {
  const dp = projectDevProcess(pid), tabs = dp ? [...dp.tabs] : [...DEFAULT_TABS];
  if (projectFeatures(pid).length && !tabs.some((t) => t.view === "features")) tabs.push(DEFAULT_TABS[0]);
  if (projectReleases(pid).length && !tabs.some((t) => t.view === "releases")) tabs.push(DEFAULT_TABS[1]);
  return tabs;
}
const typeOf = (w) => w.kind || "task";
const tabTasks = (pid, t) => W.filter((w) => w.proj === pid && (!t.types || t.types.includes(typeOf(w))));
const tabDocs = (pid, t) => sectionEntries(pid, t.section).filter((e) => !t.kinds || !t.kinds.length || t.kinds.includes(e.kind) || e.type === "diagram");

function tabCount(pid, t) {
  if (t.view === "documents") return tabDocs(pid, t).length || null;
  if (t.view === "work") return tabTasks(pid, t).filter(OPEN).length || null;
  return null;
}

/* documents view: one section, grouped by its product domain or sub-folder */
function byDomain(list, section) {
  const groups = new Map();
  for (const e of list) {
    const rest = e.path.slice(section.length + 1).split("/"), g = rest.length > 1 ? rest[0] : "";
    if (!groups.has(g)) groups.set(g, []);
    groups.get(g).push(e);
  }
  return groups;
}
const domainTitle = (g, fallback) => (g ? g.replace(/-/g, " ").replace(/^./, (c) => c.toUpperCase()) : fallback);
function domainGroup(t, g, rows, title, kind) {
  return `<section class="work-group plan-group proc-group" aria-label="${esc(title)}">
      <h3 class="work-group-title">${ic(g ? "folder" : KIND_ICON[kind] || "files", "ic16")}${esc(title)}<span class="cnt">${rows.length}</span><code class="proc-key doc-sec-path">docs/${esc(t.section)}/${g ? esc(g) + "/" : ""}</code></h3>
      <ul class="rows plan-rows" role="list">${rows.map((e) => docRow(e)).join("")}</ul></section>`;
}
function documentsTab(pid, t) {
  const sec = projectSections(pid).find((s) => s.path === t.section) || {};
  const list = tabDocs(pid, t), kind = t.kinds?.[0] || (sec.kinds || [])[0] || "", name = sec.title || t.title;
  const newBtn = `<button class="btn primary" type="button" data-doc-act="new" data-id="${esc(pid)}" data-section="${esc(t.section)}" data-kind="${esc(kind)}">${ic("plus", "ic16")}New ${esc(t.title.toLowerCase())} document</button>`;
  const body = list.length ? [...byDomain(list, t.section)].map(([g, rows]) => domainGroup(t, g, rows, domainTitle(g, name), kind)).join("")
    : `<p class="plan-empty doc-sec-empty">Nothing in ${esc(name)} yet.</p>`;
  const versioned = isVersioned(pid, kind) ? " Documents here are issued in versions; agents build from the latest issued version." : "";
  return `<div class="work-subbar-header proc-subbar"><div class="work-toolbar queue-toolbar proc-toolbar">
      <p class="plan-toolbar-note">${esc(sec.summary || "")}${versioned}</p><div class="proc-toolbar-end">${newBtn}</div></div></div>
    <div class="plan-results proc-results">${body}</div>`;
}

/* work view: the project's tasks of the tab's types */
function workTab(pid, t) {
  const tasks = tabTasks(pid, t), open = tasks.filter(OPEN), done = tasks.filter((w) => !OPEN(w));
  const group = (label, list) => (list.length ? `<div class="work-group"><h4 class="work-group-title">${label}<span class="cnt">${list.length}</span></h4><ul class="work-list" role="list">${list.map((w) => planTaskRow(w)).join("")}</ul></div>` : "");
  const board = t.layout === "board" ? `<a class="btn outline" href="#/projects/${esc(pid)}/work">${ic("board", "ic16")}Open as a board in Work</a>` : "";
  return `<article class="plan-page"><div>${sectionHead({ id: "ptab-h", title: t.title, count: `${open.length} open`, right: board })}</div>
    ${tasks.length ? `<div class="work-items plan-work">${group("Open", open)}${group("Finished", done)}</div>` : `<p class="plan-empty">No ${esc(t.title.toLowerCase())} yet.</p>`}</article>`;
}

function processTabBody(pid, key) {
  const t = processTabs(pid).find((x) => x.key === key);
  if (!t) return null;
  if (t.view === "documents") return documentsTab(pid, t);
  if (t.view === "work") return workTab(pid, t);
  return null;
}
const processTabView = (pid, key) => processTabs(pid).find((x) => x.key === key)?.view || null;

export { processTabBody, processTabView, processTabs, tabCount };
