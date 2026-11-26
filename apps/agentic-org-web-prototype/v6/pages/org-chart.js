/* Organization chart (Teams › Org chart): pan/zoom canvas of teams or people, a detail
   panel, search, and an outline fallback on narrow screens. Teams nest by reporting
   line: a team's parent is the team its lead's manager belongs to. */
import { M, P, TEAMS } from "../data.js";
import { av, esc, ic, kindWord, memberIds, plural, presPill, searchRow, searchToggle } from "../ui.js";

const OWNER = "you";
const GAP_X = 28, GAP_Y = 56, MIN_K = 0.3, MAX_K = 1.8;
const ORG = { view: "teams", expanded: { teams: null, people: null }, selected: null, panel: true, camera: null, query: "", layout: null };

/* ---------------------------------------------------------------- model */
const teamIds = () => Object.keys(TEAMS);
const teamsOf = (id) => teamIds().filter((t) => TEAMS[t].members.includes(id));
function teamParent(t) {
  const mgr = M[TEAMS[t].lead]?.mgr;
  if (!mgr || mgr === OWNER) return null;
  return teamIds().find((x) => x !== t && TEAMS[x].members.includes(mgr)) ?? null;
}
function teamTree() {
  const seen = new Set();
  const kids = (parent) => teamIds().filter((t) => teamParent(t) === parent && !seen.has(t)).map((t) => {
    seen.add(t);
    return { id: "team:" + t, kind: "team", ref: t, children: kids(t) };
  });
  return { id: "org", kind: "org", ref: OWNER, children: kids(null) };
}
function peopleTree() {
  const seen = new Set([OWNER]);
  const kids = (mgr) => memberIds().filter((k) => M[k].mgr === mgr && !seen.has(k)).map((k) => {
    seen.add(k);
    return { id: "person:" + k, kind: "person", ref: k, children: kids(k) };
  });
  return { id: "person:" + OWNER, kind: "person", ref: OWNER, children: kids(OWNER) };
}
const treeFor = (view) => (view === "teams" ? teamTree() : peopleTree());
function walk(root, fn, parent = null, depth = 0) {
  fn(root, parent, depth);
  for (const k of root.children) walk(k, fn, root, depth + 1);
}
function index(root) {
  const byId = new Map(), parentOf = new Map();
  walk(root, (n, p, d) => { byId.set(n.id, { node: n, depth: d }); if (p) parentOf.set(n.id, p.id); });
  return { byId, parentOf };
}
function expandedSet(view, root) {
  if (!ORG.expanded[view]) {
    ORG.expanded[view] = new Set();
    walk(root, (n) => { if (n.children.length) ORG.expanded[view].add(n.id); });
  }
  return ORG.expanded[view];
}
const visibleKids = (n) => (ORG.expanded[ORG.view].has(n.id) ? n.children : []);
function stats() {
  const t = teamTree();
  let levels = 0;
  walk(t, (_n, _p, d) => { levels = Math.max(levels, d); });
  const ids = memberIds();
  const agents = ids.filter((k) => M[k].kind === "agent").length;
  return { members: ids.length, agents, people: ids.length - agents, teams: teamIds().length, levels, leads: new Set(teamIds().map((x) => TEAMS[x].lead)).size };
}

/* ---------------------------------------------------------------- matching */
function nodeText(n) {
  if (n.kind === "team") {
    const t = TEAMS[n.ref];
    return [t.name, ...t.members.map((k) => M[k].name + " " + M[k].role)].join(" ");
  }
  if (n.kind === "org") return "Agentic Org";
  return M[n.ref].name + " " + M[n.ref].role;
}
const matches = (n, q) => !q || nodeText(n).toLowerCase().includes(q);
function searchResults(q) {
  if (!q) return [];
  const out = [];
  for (const t of teamIds()) if (TEAMS[t].name.toLowerCase().includes(q)) out.push({ id: "team:" + t, label: TEAMS[t].name, sub: "Team · " + plural(TEAMS[t].members.length, "member", "members"), icon: "user-group" });
  for (const k of [OWNER, ...memberIds()]) if ((M[k].name + " " + M[k].role).toLowerCase().includes(q)) out.push({ id: "person:" + k, label: M[k].name, sub: M[k].role, person: k });
  return out.slice(0, 8);
}

/* ---------------------------------------------------------------- cards */
const isSelected = (id) => ORG.selected === id;
function selectedTeamRefs() {
  const s = ORG.selected;
  if (!s?.startsWith("person:")) return new Set();
  return new Set(teamsOf(s.slice(7)));
}
function toggleBtn(n, label) {
  if (!n.children.length) return "";
  const open = ORG.expanded[ORG.view].has(n.id);
  return `<button type="button" class="org-toggle" data-org-act="toggle" data-v="${esc(n.id)}" aria-expanded="${open}" aria-label="${open ? "Hide" : "Show"} ${esc(label)}" title="${open ? "Hide" : "Show"} ${esc(label)}">${open ? "−" : "+"} ${n.children.length}</button>`;
}
function personLine(id, cls = "") {
  return `<div class="org-person ${cls}">${av(id, cls === "is-lead" ? 32 : 24, "tip")}<span class="org-person-t"><b>${esc(M[id].name)}</b>${cls === "is-lead" ? `<small>${esc(M[id].role)}</small>` : ""}</span>${cls === "is-lead" ? '<span class="org-badge">Lead</span>' : `<small class="org-person-role">${esc(M[id].role)}</small>`}</div>`;
}
function cardShell(n, inner, extraCls, label) {
  const dim = ORG.query && !matches(n, ORG.query) ? " is-dim" : "";
  const sel = isSelected(n.id) || (n.kind === "team" && selectedTeamRefs().has(n.ref)) ? " is-selected" : "";
  return `<article class="card org-card ${extraCls}${sel}${dim}" data-node="${esc(n.id)}">
    <button type="button" class="org-card-sel" data-org-act="select" data-v="${esc(n.id)}" aria-pressed="${isSelected(n.id)}" aria-label="${esc(label)}"></button>
    ${inner}</article>`;
}
function teamCard(n, depth) {
  const t = TEAMS[n.ref], others = t.members.filter((k) => k !== t.lead);
  const sub = n.children.length ? `<p class="org-card-foot">${ic("user-group", "ic14")}<span>${plural(n.children.length, "sub-team", "sub-teams")}: ${n.children.map((c) => esc(TEAMS[c.ref].name)).join(" · ")}</span></p>` : "";
  return cardShell(n, `
    <header class="org-card-hd"><span class="org-card-ic" aria-hidden="true">${ic("user-group", "ic16")}</span><div class="org-card-tt"><h3>${esc(t.name)}</h3><p>${plural(t.members.length, "member", "members")} · Level ${depth}</p></div>${toggleBtn(n, "sub-teams")}</header>
    ${M[t.lead] ? personLine(t.lead, "is-lead") : ""}
    ${others.length ? `<div class="org-members">${others.map((k) => personLine(k)).join("")}</div>` : ""}
    ${sub}`, "is-team", `${t.name} team, ${plural(t.members.length, "member", "members")}`);
}
function orgCard(n) {
  const loose = memberIds().filter((k) => !teamsOf(k).length);
  const s = stats();
  return cardShell(n, `
    <header class="org-card-hd"><span class="org-card-ic is-org" aria-hidden="true">${ic("office", "ic16")}</span><div class="org-card-tt"><h3>Agentic Org</h3><p>${plural(s.teams, "team", "teams")} · ${plural(s.members, "member", "members")}</p></div>${toggleBtn(n, "teams")}</header>
    ${personLine(OWNER, "is-lead").replace('<span class="org-badge">Lead</span>', '<span class="org-badge">Owner</span>')}
    ${loose.length ? `<p class="org-card-foot">${ic("user", "ic14")}<span>Not on a team: ${loose.map((k) => esc(M[k].name)).join(", ")}</span></p>` : ""}`, "is-org", "Agentic Org, owned by you");
}
function personCard(n) {
  const id = n.ref, m = M[id], teams = teamsOf(id);
  const kind = m.owner ? "Owner" : kindWord(id);
  return cardShell(n, `
    <header class="org-card-hd">${av(id, 32, "tip")}<div class="org-card-tt"><h3>${esc(m.name)}</h3><p>${esc(m.owner ? "Runs Agentic Org" : m.role)}</p></div>${TEAMS[teams[0]]?.lead === id || m.owner ? `<span class="org-badge">${m.owner ? "Owner" : "Lead"}</span>` : ""}</header>
    <div class="org-card-meta">${m.owner ? "" : `<span class="org-kind">${esc(kind)}</span>`}${teams.map((t) => `<span class="org-team-chip">${esc(TEAMS[t].name)}</span>`).join("")}${toggleBtn(n, "direct reports")}</div>`,
    "is-person", `${m.name}, ${m.owner ? "Owner" : m.role}`);
}
function cardFor(n, depth) {
  if (n.kind === "team") return teamCard(n, depth);
  if (n.kind === "org") return orgCard(n);
  return personCard(n);
}

/* ---------------------------------------------------------------- inspector */
const personChip = (id) => `<button type="button" class="org-chip" data-org-act="jump" data-v="person:${esc(id)}">${av(id, 24)}<span>${esc(M[id].name)}</span></button>`;
const teamChip = (t) => `<button type="button" class="org-chip" data-org-act="jump" data-v="team:${esc(t)}">${ic("user-group", "ic14")}<span>${esc(TEAMS[t].name)}</span></button>`;
const dlRow = (term, value) => `<dt>${term}</dt><dd>${value}</dd>`;
function teamInspector(t) {
  const team = TEAMS[t], parent = teamParent(t), subs = teamIds().filter((x) => teamParent(x) === t);
  const projects = Object.keys(P).filter((p) => !P[p].archived && P[p].teams.includes(t));
  return `<header class="org-insp-hd"><span class="org-card-ic" aria-hidden="true">${ic("user-group", "ic20")}</span><div><p class="org-eyebrow">Team</p><h2 id="org-insp-h">${esc(team.name)}</h2></div></header>
    <p class="org-insp-lede">${esc(team.purpose)}</p>
    <dl class="org-dl">
      ${dlRow("Lead", personChip(team.lead))}
      ${dlRow("Reports into", parent ? teamChip(parent) : `<span class="org-muted">Agentic Org (top level)</span>`)}
      ${dlRow(`Members <span class="org-count">${team.members.length}</span>`, `<div class="org-chips">${team.members.map(personChip).join("")}</div>`)}
      ${dlRow("Sub-teams", subs.length ? `<div class="org-chips">${subs.map(teamChip).join("")}</div>` : `<span class="org-muted">None</span>`)}
      ${dlRow("Projects", projects.length ? projects.map((p) => `<a class="alink" href="#/projects/${esc(p)}">${esc(P[p].name)}</a>`).join("<br>") : `<span class="org-muted">None</span>`)}
    </dl>
    <div class="org-insp-acts"><a class="btn primary" href="#/teams/${esc(t)}">Open team${ic("chev-right", "ic16")}</a><a class="btn outline" href="#/chat/${esc(team.channel)}">${ic("hash", "ic16")}${esc(team.channel)}</a></div>`;
}
const chipsOr = (items, render, empty) => (items.length ? `<div class="org-chips">${items.map(render).join("")}</div>` : `<span class="org-muted">${empty}</span>`);
function managerChip(m) {
  if (m.mgr && M[m.mgr]) return personChip(m.mgr);
  return `<span class="org-muted">${m.owner ? "None (owner)" : "Unassigned"}</span>`;
}
function personInspector(id) {
  const m = M[id], reports = memberIds().filter((k) => M[k].mgr === id);
  const eyebrow = m.owner ? "Owner" : kindWord(id), role = m.owner ? "Runs Agentic Org" : m.role;
  const presence = m.owner ? "" : `<p class="org-insp-pres">${presPill(id, true)}</p>`;
  const task = m.task ? `<p class="org-insp-lede">Working on: ${esc(m.task)}</p>` : "";
  const open = m.owner ? "" : `<div class="org-insp-acts"><a class="btn primary" href="#/members/${esc(id)}">Open profile${ic("chev-right", "ic16")}</a></div>`;
  return `<header class="org-insp-hd">${av(id, 40)}<div><p class="org-eyebrow">${esc(eyebrow)}</p><h2 id="org-insp-h">${esc(m.name)}</h2><p class="org-insp-role">${esc(role)}</p></div></header>
    ${presence}${task}
    <dl class="org-dl">
      ${dlRow("Reports to", managerChip(m))}
      ${dlRow(`Direct reports <span class="org-count">${reports.length}</span>`, chipsOr(reports, personChip, "None"))}
      ${dlRow("Teams", chipsOr(teamsOf(id), teamChip, "Not on a team"))}
    </dl>
    ${open}`;
}
function orgInspector() {
  const s = stats();
  return `<header class="org-insp-hd"><span class="org-card-ic is-org" aria-hidden="true">${ic("office", "ic20")}</span><div><p class="org-eyebrow">Organization</p><h2 id="org-insp-h">Agentic Org</h2></div></header>
    <p class="org-insp-lede">${plural(s.members, "member", "members")} (${plural(s.agents, "agent", "agents")}, ${plural(s.people, "person", "people")}) across ${plural(s.teams, "team", "teams")} and ${plural(s.levels, "level", "levels")}.</p>
    <dl class="org-dl">${dlRow("Owner", personChip(OWNER))}${dlRow("Top-level teams", `<div class="org-chips">${teamIds().filter((t) => !teamParent(t)).map(teamChip).join("")}</div>`)}</dl>`;
}
function inspectorHtml() {
  const s = ORG.selected;
  let body = `<h2 id="org-insp-h" class="org-insp-empty-t">Explore your organization</h2><p class="org-insp-lede">Select a team or person to see who leads it, who reports where, and what they work on.</p>
    <p class="org-insp-tip">Teams nest by reporting line: a team sits under the team its lead's manager belongs to.</p>`;
  if (s === "org") body = orgInspector();
  else if (s?.startsWith("team:") && TEAMS[s.slice(5)]) body = teamInspector(s.slice(5));
  else if (s?.startsWith("person:") && M[s.slice(7)]) body = personInspector(s.slice(7));
  return `<aside class="org-inspector" aria-labelledby="org-insp-h"${ORG.panel ? "" : " hidden"}>
    <button type="button" class="btn ghost icon sm org-insp-close" data-org-act="panel" aria-label="Close detail panel">${ic("x", "ic16")}</button>${body}</aside>`;
}

/* ---------------------------------------------------------------- outline (narrow) */
function outlineMeta(n) {
  if (n.kind === "team") {
    const t = TEAMS[n.ref];
    return { label: t.name, sub: `${M[t.lead]?.name ?? ""} · ${plural(t.members.length, "member", "members")}`, face: t.lead };
  }
  if (n.kind === "org") return { label: "Agentic Org", sub: "Owner: You", face: OWNER };
  return { label: M[n.ref].name, sub: M[n.ref].owner ? "Owner" : M[n.ref].role, face: n.ref };
}
function outlineHtml(root) {
  const item = (n, depth) => {
    const { label, sub, face } = outlineMeta(n);
    const kids = n.children.length ? `<ul>${n.children.map((c) => item(c, depth + 1)).join("")}</ul>` : "";
    return `<li><button type="button" class="org-out-item${isSelected(n.id) ? " is-selected" : ""}" data-org-act="select" data-v="${esc(n.id)}" style="--depth:${depth}">${av(face, 24)}<span><b>${esc(label)}</b><small>${esc(sub)}</small></span></button>${kids}</li>`;
  };
  return `<ul class="org-outline" aria-label="${ORG.view === "teams" ? "Team" : "People"} hierarchy">${item(root, 0)}</ul>`;
}

/* ---------------------------------------------------------------- page */
function toolbarHtml() {
  const seg = (v, icon, label) => `<button type="button" data-org-act="view" data-v="${v}" aria-pressed="${ORG.view === v}">${ic(icon, "ic16")}${label}</button>`;
  return `<div class="org-toolbar">
    <div class="org-seg" role="group" aria-label="Chart view">${seg("teams", "user-group", "Teams")}${seg("people", "user", "People")}</div>
    <p class="org-hint">${ORG.view === "teams" ? "A lead's manager determines their team's parent." : "People are arranged by who they report to."}</p>
    <div class="org-tools">
      ${searchToggle("org-search", "Search people and teams", ORG.query)}
      <button type="button" class="btn ghost sm" data-org-act="collapse">Collapse</button>
      <button type="button" class="btn ghost sm" data-org-act="expand">Expand all</button>
      <button type="button" class="btn ghost sm" data-org-act="fit">${ic("maximize", "ic16")}Fit view</button>
      <button type="button" class="btn ghost icon sm" data-org-act="panel" aria-pressed="${ORG.panel}" aria-label="${ORG.panel ? "Hide" : "Show"} detail panel" title="Detail panel">${ic("sidebar", "ic16")}</button>
    </div>
  </div>
  <div class="org-search-wrap">${searchRow("org-search", "Search people and teams", ORG.query, "Search people or teams… (Esc to close)", "org-search-row")}<div class="org-results" id="org-results" role="list"></div></div>`;
}
function stageHtml(root) {
  return `<div class="org-stage${ORG.panel ? " has-panel" : ""}">
    <div class="org-viewport" id="org-viewport" tabindex="0" aria-label="${ORG.view === "teams" ? "Team" : "People"} hierarchy canvas. Drag to pan, scroll to zoom, plus and minus keys to zoom, 0 to fit.">
      <div class="org-layer" id="org-layer"><svg class="org-edges" id="org-edges" aria-hidden="true"></svg><div id="org-cards"></div></div>
      <div class="org-zoom" role="group" aria-label="Zoom">
        <button type="button" data-org-act="home" aria-label="Back to top" title="Back to top">${ic("home", "ic16")}</button>
        <button type="button" data-org-act="zoom-out" aria-label="Zoom out" title="Zoom out">−</button>
        <span id="org-zoom-v" aria-live="polite">100%</span>
        <button type="button" data-org-act="zoom-in" aria-label="Zoom in" title="Zoom in">+</button>
        <button type="button" data-org-act="fit" aria-label="Fit to view" title="Fit to view">${ic("maximize", "ic16")}</button>
      </div>
      <p class="org-keys" aria-hidden="true"><kbd>Drag</kbd> to pan <kbd>Scroll</kbd> to zoom <kbd>Click</kbd> to inspect</p>
    </div>
    ${outlineHtml(root)}
    ${inspectorHtml()}
  </div>`;
}
function orgBodyHtml() {
  const root = treeFor(ORG.view);
  expandedSet(ORG.view, root);
  return toolbarHtml() + stageHtml(root);
}
function orgSummary() {
  const s = stats();
  return `${plural(s.members, "member", "members")} · ${plural(s.teams, "team", "teams")} · ${plural(s.levels, "level", "levels")} · ${plural(s.leads, "lead", "leads")}`;
}
const orgChartBody = () => `<div class="org" id="org-root">${orgBodyHtml()}</div>`;

/* ---------------------------------------------------------------- layout + camera */
function measure(cards, visible) {
  cards.innerHTML = visible.map(([n, d]) => cardFor(n, d)).join("");
  const sizes = new Map();
  for (const el of cards.children) sizes.set(el.dataset.node, { w: el.offsetWidth, h: el.offsetHeight });
  return sizes;
}
/* Tidy top-down tree: each subtree is as wide as its widest of (own card, children row). */
function layoutTree(tree, visible, sizes) {
  const span = new Map();
  const width = (n) => {
    const kids = visibleKids(n);
    const kw = kids.reduce((s, k) => s + width(k), 0) + GAP_X * Math.max(0, kids.length - 1);
    span.set(n.id, Math.max(sizes.get(n.id).w, kw));
    return span.get(n.id);
  };
  width(tree);
  const rowH = [];
  for (const [n, d] of visible) rowH[d] = Math.max(rowH[d] ?? 0, sizes.get(n.id).h);
  const rowY = [];
  let y = 0;
  rowH.forEach((h, i) => { rowY[i] = y; y += h + GAP_Y; });
  const pos = new Map();
  const place = (n, d, left) => {
    const s = sizes.get(n.id), kids = visibleKids(n);
    pos.set(n.id, { x: left + (span.get(n.id) - s.w) / 2, y: rowY[d], w: s.w, h: s.h });
    const kw = kids.reduce((a, k) => a + span.get(k.id), 0) + GAP_X * Math.max(0, kids.length - 1);
    let l = left + (span.get(n.id) - kw) / 2;
    for (const k of kids) { place(k, d + 1, l); l += span.get(k.id) + GAP_X; }
  };
  place(tree, 0, 0);
  return { pos, W: span.get(tree.id), H: Math.max(0, y - GAP_Y) };
}
/* Nodes on the path from the root to the selection (a selected person in Teams view lights their team). */
function hotPath(tree, pos) {
  const { parentOf } = index(tree), sel = ORG.selected, hot = new Set();
  let cur = sel && pos.has(sel) ? sel : null;
  if (!cur && sel?.startsWith("person:")) cur = [...pos.keys()].find((k) => k.startsWith("team:") && teamsOf(sel.slice(7)).includes(k.slice(5))) ?? null;
  for (; cur; cur = parentOf.get(cur)) hot.add(cur);
  return hot;
}
function drawEdges(svg, visible, pos, hot, W, H) {
  const paths = [];
  for (const [n] of visible) {
    const p = pos.get(n.id), px = p.x + p.w / 2, pb = p.y + p.h, my = pb + GAP_Y / 2;
    for (const k of visibleKids(n)) {
      const c = pos.get(k.id);
      paths.push([hot.has(k.id), `<path${hot.has(k.id) ? ' class="is-hot"' : ""} d="M${px} ${pb} V${my} H${c.x + c.w / 2} V${c.y}"/>`]);
    }
  }
  svg.setAttribute("width", W);
  svg.setAttribute("height", H);
  svg.innerHTML = paths.sort((a, b) => a[0] - b[0]).map(([, d]) => d).join("");
}
function renderCanvas(root) {
  const cards = root.querySelector("#org-cards"), tree = treeFor(ORG.view), visible = [];
  const collect = (n, d) => { visible.push([n, d]); for (const k of visibleKids(n)) collect(k, d + 1); };
  collect(tree, 0);
  const { pos, W, H } = layoutTree(tree, visible, measure(cards, visible));
  for (const el of cards.children) {
    const p = pos.get(el.dataset.node);
    el.style.transform = `translate(${p.x}px, ${p.y}px)`;
  }
  drawEdges(root.querySelector("#org-edges"), visible, pos, hotPath(tree, pos), W, H);
  ORG.layout = { pos, W, H };
}
function applyCamera(root) {
  const c = ORG.camera;
  root.querySelector("#org-layer").style.transform = `translate(${c.x}px, ${c.y}px) scale(${c.k})`;
  root.querySelector("#org-zoom-v").textContent = Math.round(c.k * 100) + "%";
}
function frame(root, mode) {
  const v = root.querySelector("#org-viewport"), { W, H } = ORG.layout;
  const vw = v.clientWidth, vh = v.clientHeight;
  if (!vw || !vh) return;
  const fitK = Math.min((vw - 48) / W, (vh - 48) / H);
  const k = mode === "fit" ? Math.max(MIN_K, Math.min(1, fitK)) : Math.max(0.75, Math.min(1, (vw - 48) / W));
  /* Top framing keeps cards readable; when the chart is wider than the view, centre on the root card. */
  const rootPos = [...ORG.layout.pos.values()][0];
  const x = W * k <= vw - 48 || !rootPos ? (vw - W * k) / 2 : vw / 2 - (rootPos.x + rootPos.w / 2) * k;
  ORG.camera = { k, x, y: mode === "fit" && H * k < vh ? (vh - H * k) / 2 : 24 };
  applyCamera(root);
}
function zoomAt(root, factor, cx, cy) {
  const v = root.querySelector("#org-viewport"), c = ORG.camera;
  if (cx == null) { cx = v.clientWidth / 2; cy = v.clientHeight / 2; }
  const k = Math.max(MIN_K, Math.min(MAX_K, c.k * factor));
  ORG.camera = { k, x: cx - (cx - c.x) * (k / c.k), y: cy - (cy - c.y) * (k / c.k) };
  applyCamera(root);
}
function centerOn(root, id) {
  const p = ORG.layout?.pos.get(id), v = root.querySelector("#org-viewport");
  if (!p || !v.clientWidth) return;
  const k = ORG.camera.k;
  ORG.camera = { k, x: v.clientWidth / 2 - (p.x + p.w / 2) * k, y: Math.min(24, v.clientHeight / 3 - p.y * k) };
  applyCamera(root);
}
function sizeStage(root) {
  const stage = root.querySelector(".org-stage");
  if (!stage) return;
  const top = stage.getBoundingClientRect().top;
  stage.style.height = Math.max(460, window.innerHeight - top - 16) + "px";
}

/* ---------------------------------------------------------------- interaction */
function refresh(root, { keepCamera = true, focus = null } = {}) {
  root.innerHTML = orgBodyHtml();
  sizeStage(root);
  renderCanvas(root);
  if (keepCamera && ORG.camera) applyCamera(root); else frame(root, "top");
  if (focus) root.querySelector(focus)?.focus({ preventScroll: true });
}
function revealAndSelect(root, id) {
  const view = id.startsWith("team:") || id === "org" ? "teams" : "people";
  if (ORG.view !== view) { ORG.view = view; ORG.camera = null; }
  const tree = treeFor(view), { parentOf } = index(tree);
  const set = expandedSet(view, tree);
  for (let p = parentOf.get(id); p; p = parentOf.get(p)) set.add(p);
  ORG.selected = id; ORG.panel = true;
  refresh(root);
  centerOn(root, id);
}
function renderResults(root) {
  const box = root.querySelector("#org-results");
  if (!box) return;
  const results = searchResults(ORG.query);
  box.innerHTML = ORG.query ? (results.length ? results.map((r) => `<button type="button" class="org-result" role="listitem" data-org-act="jump" data-v="${esc(r.id)}">${r.person ? av(r.person, 24) : `<span class="org-card-ic">${ic(r.icon, "ic14")}</span>`}<span><b>${esc(r.label)}</b><small>${esc(r.sub)}</small></span></button>`).join("") : `<p class="org-muted org-noresult">No people or teams match “${esc(ORG.query)}”.</p>`) : "";
  for (const el of root.querySelectorAll(".org-card")) {
    const id = el.dataset.node, node = index(treeFor(ORG.view)).byId.get(id)?.node;
    el.classList.toggle("is-dim", Boolean(ORG.query && node && !matches(node, ORG.query)));
  }
}
const ACTS = {
  select: (root, v) => { ORG.selected = ORG.selected === v ? null : v; if (ORG.selected) ORG.panel = true; refresh(root, { focus: `[data-org-act="select"][data-v="${CSS.escape(v)}"]` }); },
  jump: (root, v) => { ORG.query = ""; revealAndSelect(root, v); root.querySelector("#org-insp-h")?.setAttribute("tabindex", "-1"); root.querySelector("#org-insp-h")?.focus({ preventScroll: true }); },
  toggle: (root, v) => { const s = ORG.expanded[ORG.view]; if (s.has(v)) s.delete(v); else s.add(v); refresh(root, { focus: `[data-org-act="toggle"][data-v="${CSS.escape(v)}"]` }); },
  view: (root, v) => { if (ORG.view === v) return; ORG.view = v; ORG.camera = null; refresh(root, { keepCamera: false, focus: `[data-org-act="view"][data-v="${v}"]` }); },
  collapse: (root) => { const tree = treeFor(ORG.view); ORG.expanded[ORG.view] = new Set([tree.id]); refresh(root, { keepCamera: false }); },
  expand: (root) => { ORG.expanded[ORG.view] = null; refresh(root, { keepCamera: false }); frame(root, "fit"); },
  fit: (root) => frame(root, "fit"),
  home: (root) => frame(root, "top"),
  "zoom-in": (root) => zoomAt(root, 1.2),
  "zoom-out": (root) => zoomAt(root, 1 / 1.2),
  panel: (root) => { ORG.panel = !ORG.panel; refresh(root, { focus: '.org-tools [data-org-act="panel"]' }); },
};
let stopResize = null;
function onOrgClick(root, e) {
  if (root.dataset.dragged === "1" && e.target.closest(".org-viewport")) {
    root.dataset.dragged = "";
    e.preventDefault(); e.stopPropagation();
    return;
  }
  const t = e.target.closest("[data-org-act]");
  if (!t || !root.contains(t)) return;
  e.preventDefault();
  ACTS[t.dataset.orgAct]?.(root, t.dataset.v);
}
function onOrgInput(root, e) {
  if (e.target.id !== "org-search") return;
  ORG.query = e.target.value.trim().toLowerCase();
  renderResults(root);
}
const ZOOM_KEYS = { "+": (root) => zoomAt(root, 1.2), "=": (root) => zoomAt(root, 1.2), "-": (root) => zoomAt(root, 1 / 1.2), "0": (root) => frame(root, "fit") };
function onOrgKeydown(root, e) {
  if (e.target.id === "org-search" && e.key === "Enter") {
    const first = root.querySelector(".org-result");
    if (first) { e.preventDefault(); ACTS.jump(root, first.dataset.v); }
    return;
  }
  const zoom = ZOOM_KEYS[e.key];
  if (!zoom || !e.target.closest?.(".org-viewport") || e.metaKey || e.ctrlKey || e.altKey) return;
  e.preventDefault();
  zoom(root);
}
function onOrgWheel(root, e) {
  const v = e.target.closest?.(".org-viewport");
  if (!v) return;
  e.preventDefault();
  const r = v.getBoundingClientRect();
  zoomAt(root, Math.exp(-e.deltaY * (e.ctrlKey ? 0.01 : 0.0015)), e.clientX - r.left, e.clientY - r.top);
}
/* Drag to pan: a press becomes a pan after 4px; the click that ends a pan is swallowed. */
function bindPan(root) {
  let drag = null;
  root.addEventListener("pointerdown", (e) => {
    const v = e.target.closest?.(".org-viewport");
    if (!v || e.button !== 0 || e.target.closest(".org-zoom, .org-toggle")) return;
    drag = { id: e.pointerId, sx: e.clientX, sy: e.clientY, x: ORG.camera.x, y: ORG.camera.y, moved: false, v };
  });
  root.addEventListener("pointermove", (e) => {
    if (drag?.id !== e.pointerId) return;
    const dx = e.clientX - drag.sx, dy = e.clientY - drag.sy;
    if (!drag.moved && Math.hypot(dx, dy) < 4) return;
    if (!drag.moved) { drag.moved = true; drag.v.setPointerCapture(e.pointerId); drag.v.classList.add("is-panning"); }
    ORG.camera = { ...ORG.camera, x: drag.x + dx, y: drag.y + dy };
    applyCamera(root);
  });
  const end = (e) => {
    if (drag?.id !== e.pointerId) return;
    if (drag.moved) {
      root.dataset.dragged = "1";
      drag.v.classList.remove("is-panning");
      setTimeout(() => { root.dataset.dragged = ""; }, 0);
    }
    drag = null;
  };
  root.addEventListener("pointerup", end);
  root.addEventListener("pointercancel", end);
}
function bindResize(root) {
  stopResize?.();
  const onResize = () => { if (root.isConnected) sizeStage(root); else stopResize?.(); };
  window.addEventListener("resize", onResize);
  stopResize = () => { window.removeEventListener("resize", onResize); stopResize = null; };
}
function bindOrg(root) {
  root.addEventListener("click", (e) => onOrgClick(root, e), true);
  root.addEventListener("input", (e) => onOrgInput(root, e));
  root.addEventListener("keydown", (e) => onOrgKeydown(root, e));
  root.addEventListener("wheel", (e) => onOrgWheel(root, e), { passive: false });
  bindPan(root);
  bindResize(root);
}
function mountOrgChart(main) {
  const root = main.querySelector("#org-root");
  if (!root) { stopResize?.(); return; }
  sizeStage(root);
  renderCanvas(root);
  if (ORG.camera) applyCamera(root); else frame(root, "top");
  renderResults(root);
  bindOrg(root);
}

export { mountOrgChart, orgChartBody, orgSummary };
