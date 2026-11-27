import { CH, IN, KIND, M, OPEN, P, TEAMS, W, needsMe } from "../data.js";
import { pageActivity, projectReleaseSummary } from "./activity.js";
import { calendarHtml } from "./calendar.js";
import { msgHtml } from "./chat.js";
import { INSIGHT_DAY, INSIGHT_TODAY, insightCompletions, projectInsights } from "./insights.js";
import { notFound } from "./settings.js";
import { scopedWorkToolbar, scopedWorkWorkspace } from "./work.js";
import { pageFeature, pageFeatures } from "./project-plan-features.js";
import { pageRelease, pageReleases } from "./project-plan-releases.js";
import { docCrumbs, docsCount, docsSectionBar, pageDocs, setDocsExtra } from "./docs.js";
import { processTabBody, processTabs, tabCount } from "./project-tabs.js";
import { featureRef, getFeature, getRelease, projectFeatures, projectReleases, releaseRef } from "../plan-data.js";
import { render } from "../shell/router.js";
import { S } from "../state.js";
import { acts, av, avStack, crumbs, esc, feed, hdr, ic, itemActions, kindWord, leadOf, leadTxt, linkTabs, menuItem, moreMenu, needsCard, pill, plural, presPill, progress, projMembers, projPill, recommendationCallout, searchRow, searchToggle, sectionHead, sectionTabs, SEP, showMenu, statusLine, toast } from "../ui.js";

/* A star toggles a project in the sidebar's Favorite projects (owner); filled when it is a favorite */
function favStar(p, cls = "") {
  const on = S.favProjs.includes(p), name = esc(P[p].name);
  return `<button class="btn ghost icon proj-fav${on ? " is-fav" : ""}${cls ? " " + cls : ""}" type="button" data-act="fav-proj" data-id="${p}" aria-pressed="${on}" aria-label="${on ? "Remove " + name + " from favorites" : "Add " + name + " to favorites"}" title="${on ? "Remove from favorites" : "Add to favorites"}">${ic("star", "ic16")}</button>`;
}
function pageProjects() {
  const q = S.projectQuery.trim().toLowerCase();
  const matchesQuery = (p) => !q || [P[p].name, leadTxt(p), ...P[p].teams.map((t) => TEAMS[t].name)].join(" ").toLowerCase().includes(q);
  const ids = Object.keys(P).filter((p) => (S.showArchived || !P[p].archived) && matchesQuery(p));
  const active = Object.keys(P).filter((p) => !P[p].archived).length;
  const filter = showMenu("pshow", [["showArchived", "Show", [[false, "Active", active], [true, "All, with archived", Object.keys(P).length]]]],
    { icon: "filter", label: "Filter projects", active: S.showArchived });
  const search = searchToggle("projects-search", "Search projects", S.projectQuery);
  const empty = ids.length ? "" : `<p class="empty-row">No projects match “${esc(S.projectQuery.trim())}”.</p>`;
  return hdr("Projects", `${active} active · ${Object.keys(P).length - active} archived`, search + filter) +
  searchRow("projects-search", "Search projects", S.projectQuery, "Search projects by name, lead or team… (Esc to close)", "work-search-row") + empty +
  `<ul class="rows" role="list">${ids.map((p) => {
    const pr = P[p], ws = W.filter((w) => w.proj === p), openN = ws.filter(OPEN).length;
    return `<li class="linked"><span class="chip-i" style="margin-left:0;width:32px;height:32px;font-size:12px" aria-hidden="true">${esc(pr.name[0])}</span>
      <div class="grow"><p class="rt"><a class="stretch" href="#/projects/${p}">${esc(pr.name)}</a>${pr.archived ? "" : favStar(p, "proj-fav-row")}${leadOf(p) ? `<span class="sec">${esc(leadTxt(p))}</span>` : ""}</p>
      <div class="meta"><span>${pr.teams.map((t) => `<a href="#/teams/${t}">${esc(TEAMS[t].name)}</a>`).join(", ")}</span>${SEP}<span>${openN} open</span>${pr.due ? SEP + `<span>Due ${esc(pr.due)}</span>` : ""}${SEP}<span>Updated ${esc(pr.updated)}</span></div>
      <div style="max-width:320px;margin-top:8px">${progress(p)}</div></div>
      <span class="hide-sm">${avStack(projMembers(p), 4)}</span>${projPill(p)}${ic("chev-right", "ic20 chev")}</li>`;
  }).join("")}</ul>`;
}

function planTabCount(id, t) {
  if (t.view === "features") return projectFeatures(id).filter((f) => f.status !== "retired").length || null;
  if (t.view === "releases") return projectReleases(id).length || null;
  return tabCount(id, t);
}

/* Features and Releases (TASK-244): a detail page sits under its tab, named in the breadcrumb. */
function planCrumbs(id, tab, sub) {
  const base = "#/projects/" + id;
  if (tab !== "features" && tab !== "releases") return [];
  const label = tab === "features" ? "Features" : "Releases";
  if (!sub) return [[null, label]];
  const rec = tab === "features" ? getFeature(sub) : getRelease(sub);
  const ref = rec ? (tab === "features" ? featureRef(rec) : releaseRef(rec)) : "Not found";
  return [[`${base}/${tab}`, label], [null, ref]];
}
const inDocs = (t) => t.view === "documents" || t.view === "features";
function pageProject(id, tab, sub) {
  const pr = P[id]; if (!pr) return notFound();
  tab = tab || "overview";
  const ws = W.filter((w) => w.proj === id);
  const tabs = [
    ["", "Overview"],
    ["activity", "Activity"],
    ["work", "Work", `${ws.filter(OPEN).length} open`],
    /* Between Work and Docs: the tabs the project's development process declares (doc-32 §4) */
    ...processTabs(id).filter((t) => !inDocs(t)).map((t) => [t.key, t.title, planTabCount(id, t)]),
    ["docs", "Docs", docsCount(id) || null],
    ["calendar", "Calendar"],
    ["insights", "Insights"],
    ["conversations", "Conversations"],
    ["team", "Team", projMembers(id).length],
  ];
  /* Product, Technical and Features are sections of Docs, not tabs of their own (owner) */
  const docsKeys = processTabs(id).filter(inDocs).map((t) => t.key);
  setDocsExtra(processTabs(id).filter(inDocs).map((t) => `<a class="queue-tab" href="#/projects/${esc(id)}/${esc(t.key)}"${tab === t.key ? ' aria-current="page"' : ""}>${esc(t.title)}<span>${planTabCount(id, t) ?? ""}</span></a>`).join(""));
  const more = moreMenu("pmore", "More actions for " + pr.name, menuItem("health", id, "flag", "Set health…", ` data-id="${id}"`) + menuItem("proto", "Edit " + pr.name, "pencil", "Edit") + `<button role="menuitem" type="button" data-dev-act="choose" data-id="${esc(id)}">${ic("files", "ic20")}Development process…</button>`, "ghost");
  const trail = tab === "docs" ? docCrumbs(id, sub) : planCrumbs(id, tab, sub);
  return crumbs([["#/projects", "Projects"], [trail.length ? "#/projects/" + id : null, pr.name], ...trail]) + `<div class="section-header project-header">
    ${hdr(pr.name, leadOf(id) ? esc(leadTxt(id)) : "", "", (pr.archived ? "" : favStar(id)) + projPill(id))}
    ${sectionTabs(linkTabs("#/projects/" + id, tabs, tab === "overview" ? "" : docsKeys.includes(tab) ? "docs" : tab), more)}
  </div>` + (tab === "activity" || tab === "releases" || tab === "docs" ? "" : projectReleaseSummary(id)) + projectBody(id, tab, sub);
}
function projectBody(id, tab, sub) {
  const pr = P[id], ws = W.filter((w) => w.proj === id);
  if (tab === "overview") {
    const needs = IN.filter((i) => i.proj === id && (!i.handled || i.handledNow));
    const pending = needs.filter((i) => !i.handled).length;
    return `<div class="pj-overview">
      <p class="pj-goal"><span>Goal</span>${esc(pr.goal)}</p>
      ${projectSummaryHtml(id, ws)}
      <section class="project-sec" aria-labelledby="pj-needs-h">${sectionHead({ id: "pj-needs-h", title: "Needs your attention", count: pending ? `${pending} pending` : "", right: `<a class="link" href="#/inbox">Open inbox${ic("chev-right", "ic16")}</a>` })}${needs.length ? projectDecisionsHtml(needs) : `<p class="muted">Nothing needs you on this project.</p>`}</section>
      <section class="project-sec" aria-labelledby="pj-act-h">${sectionHead({ id: "pj-act-h", title: "Recent activity", right: `<a class="link" href="#/projects/${esc(id)}/activity">All activity${ic("chev-right", "ic16")}</a>` })}${feed(acts().filter((a) => a.proj === id).slice(0, 6), { noProj: true })}</section>
    </div>`;
  }
  if (tab === "work") return scopedWorkToolbar("project", id, ws) + scopedWorkWorkspace();
  if (tab === "calendar") return calendarHtml("project", id);
  if (tab === "team") return `<div class="pj-team">${projectTeams(id)}${memberRows(projMembers(id), { emptyText: "No members yet." })}</div>`;
  if (tab === "activity") return pageActivity(id);
  if (tab === "insights") return projectInsights(id);
  if (tab === "features") return sub ? pageFeature(id, sub) : docsSectionBar(id) + pageFeatures(id);
  if (tab === "releases") return sub ? pageRelease(id, sub) : pageReleases(id);
  if (tab === "docs") return pageDocs(id, sub);
  const processTab = processTabBody(id, tab);
  if (processTab != null) return (processTabs(id).some((t) => t.key === tab && inDocs(t)) ? docsSectionBar(id) : "") + processTab;
  if (tab === "conversations") return `<div class="section">${conversationPreview(pr.channel)}</div>`;
  return notFound();
}

/* Cycle time: how fast work closes. Tasks closed per day over the last 7 days (per hour when faster
   than one an hour), the change against the 7 days before, and the median open-to-closed cycle time. */
function projectCycle(id) {
  const done = insightCompletions(id), since = (days) => INSIGHT_TODAY - days * INSIGHT_DAY;
  const week = done.filter((c) => c.day > since(7)), prior = done.filter((c) => c.day > since(14) && c.day <= since(7));
  const perDay = week.length / 7, priorPerDay = prior.length / 7;
  /* Median cycle uses the same scope as Insights' default period (this month), so both show one number. */
  const today = new Date(INSIGHT_TODAY), monthStart = Date.UTC(today.getUTCFullYear(), today.getUTCMonth(), 1);
  const month = done.filter((c) => c.day >= monthStart);
  const cycles = (month.length ? month : done).map((c) => c.cycle);
  const sorted = [...cycles].sort((a, b) => a - b), median = sorted.length ? sorted[Math.ceil(sorted.length / 2) - 1] : null;
  const delta = priorPerDay ? Math.round(100 * (perDay - priorPerDay) / priorPerDay) : null;
  const rate = perDay >= 24 ? { value: (perDay / 24).toFixed(1), unit: "closed / hour" } : { value: perDay.toFixed(1), unit: "closed / day" };
  return { ...rate, median, delta, closed: week.length };
}
function cycleCard(id, card) {
  const c = projectCycle(id);
  if (!c.closed && c.median == null) return card("Cycle time", "clock", "—", "no closed tasks yet", "Shows once work closes", `#/projects/${id}/insights`);
  const trend = c.delta == null ? "" : c.delta === 0 ? `<span>Steady vs last week</span>`
    : `<span class="pj-pace ${c.delta > 0 ? "is-ok" : "is-warn"}">${c.delta > 0 ? "+" : ""}${c.delta}% vs last week</span>`;
  return card("Cycle time", "clock", c.value, c.unit, `<span>Median cycle ${fmtHours(c.median)}</span>${trend}`, `#/projects/${id}/insights`);
}
const fmtHours = (h) => (h == null ? "—" : h >= 48 ? `${(h / 24).toFixed(1)} d` : `${Math.round(h)} h`);

/* Project at a glance, in the style of Home's summary cards: progress, schedule, open work, cycle time, team. */
function projectSummaryHtml(id, ws) {
  const pr = P[id], done = ws.filter((w) => !OPEN(w)).length, total = ws.length, open = total - done;
  const blocked = ws.filter((w) => w.status === "Blocked").length, mine = ws.filter(needsMe).length;
  const members = projMembers(id), pct = total ? Math.round((done * 100) / total) : 0;
  const tone = { ok: "ok", warn: "warn", bad: "bad" }[pr.paceTone] || "";
  const card = (label, icon, value, unit, foot, href) => `<a class="pj-kpi" href="${href}">
      <span class="pj-kpi-l">${ic(icon, "ic14")}${label}</span>
      <span class="pj-kpi-v"><b>${value}</b>${unit ? `<span>${unit}</span>` : ""}</span>
      <span class="pj-kpi-f">${foot}</span></a>`;
  return `<section class="pj-kpis" aria-label="Project at a glance">
      ${card("All work", "check-circle", `${done}/${total}`, "tasks done", `<span class="pj-kpi-bar" aria-hidden="true"><i style="width:${pct}%"></i></span>`, `#/projects/${id}/work`)}
      ${card("Schedule", "calendar-check", pr.due ? esc(pr.due) : "—", pr.due ? "due" : "no due date", pr.pace ? `<span class="pj-pace${tone ? " is-" + tone : ""}">${esc(pr.pace)}</span>${pr.release ? ` · ${esc(pr.release)}` : ""}` : "", `#/projects/${id}/activity`)}
      ${card("Open work", "clipboard", String(open), "open", `${blocked} blocked · ${plural(mine, "task", "tasks")} waiting on you`, `#/projects/${id}/work`)}
      ${cycleCard(id, card)}
      ${card("Team", "user-group", String(members.length), members.length === 1 ? "member" : "members", `${avStack(members, 5)}<span class="pj-kpi-teams">${pr.teams.map((t) => esc(TEAMS[t].name)).join(", ")}</span>`, `#/projects/${id}/team`)}
    </section>`;
}

/* Inbox-style split: a priority queue on the left, the selected decision on the right.
   Narrow containers stack the decision above the queue. */
/* Kind reads "Approval · Release sign-off", but never repeats itself ("Suggestion", not "Suggestion · Tess suggestion"). */
function decisionKind(it) {
  const label = KIND[it.kind]?.label || "Request", sub = it.sub || "";
  return sub && !sub.toLowerCase().includes(label.toLowerCase()) ? `${label} · ${sub}` : label;
}
function decisionFields(it) {
  const field = (dt, dd, cls = "") => `<div><dt>${dt}</dt><dd${cls ? ` class="${cls}"` : ""}>${dd}</dd></div>`;
  const work = it.task ? field("Work item", `<a class="alink" href="#/projects/${esc(it.proj)}/work?item=${encodeURIComponent(it.task)}">${esc(it.task)}</a>`) : "";
  const expires = it.expires ? field("Expires", esc(it.expires), it.handled ? "" : "triage-expiry") : "";
  const clears = it.clears || KIND[it.kind]?.clears || "When handled";
  return `<dl class="triage-fields pj-fields">${field("Kind", esc(decisionKind(it)))}${work}${expires}${field("Clears", esc(clears))}</dl>`;
}
function decisionDetailHtml(it) {
  const rec = it.rec && !it.handled ? recommendationCallout(it) : "";
  return `<article class="pj-detail" aria-labelledby="pj-detail-t">
      <div class="pj-detail-top">${av(it.from, 24)}<span class="grow"><b>${esc(M[it.from].name)}</b> · ${esc(it.time)}</span></div>
      <h3 id="pj-detail-t" class="pj-detail-title"><a href="#/inbox/${esc(it.id)}" title="Open in Inbox">${esc(it.what)}</a></h3>
      ${it.why ? `<p class="why">${esc(it.why)}</p>` : ""}
      ${decisionFields(it)}
      ${rec}
      ${itemActions(it, "card")}
    </article>`;
}
/* Queue row state, as in the Inbox: handled, urgent (has a deadline) or pending. */
function queueState(it) {
  if (it.handled) return { state: "handled", icon: "check-circle", meta: `<span>${esc(it.outcome || "Handled")}</span>` };
  if (it.expires) return { state: "urgent", icon: "bell", meta: `<span class="triage-expiry">${ic("clock", "ic14")}${esc(it.expires)}</span>` };
  return { state: "pending", icon: KIND[it.kind]?.icon || "inbox", meta: "" };
}
/* Rows reuse the Inbox queue row markup (.triage-item) so both lists read as one component. */
function queueRowHtml(it, sel) {
  const { state, icon, meta } = queueState(it), current = it === sel;
  return `<li><button type="button" class="triage-item pj-q-item" data-act="set" data-k="pjDecision" data-v="${esc(it.id)}" aria-pressed="${current}"${current ? ' aria-current="true"' : ""}>
        <span class="triage-person">${av(it.from, 24)}<span class="triage-sender"><strong>${esc(M[it.from].name)}</strong><span class="triage-kind">${esc(it.sub)}</span></span><span class="triage-stamp"><span class="triage-state-icon" data-state="${state}">${ic(icon, "ic14")}</span><time>${esc(it.time)}</time></span></span>
        <span class="triage-title">${esc(it.what)}</span>${meta ? `<span class="triage-meta">${meta}</span>` : ""}</button></li>`;
}
function projectDecisionsHtml(needs) {
  const sel = needs.find((it) => it.id === S.pjDecision) || needs[0];
  return `<div class="pj-split">
      <nav class="pj-queue-pane" aria-label="Priority queue">
        <ul role="list" class="pj-q-list" tabindex="-1">${needs.map((it) => queueRowHtml(it, sel)).join("")}</ul>
        <button type="button" class="pj-q-more" hidden>${ic("chev-down", "ic16")}<span></span></button></nav>
      ${decisionDetailHtml(sel)}
    </div>`;
}

function projectTeams(id) {
  const pr = P[id], ts = pr.teams;
  const chips = ts.map((t) => `<span class="tchip"><a href="#/teams/${t}">${esc(TEAMS[t].name)}</a><button class="rm" type="button" data-act="team-rm" data-id="${id}" data-v="${t}" aria-label="Remove ${esc(TEAMS[t].name)} from ${esc(pr.name)}" title="Remove ${esc(TEAMS[t].name)}">${ic("x", "ic16")}</button></span>`).join("");
  const addLbl = ts.length ? "Add team…" : "Assign team…", addName = (ts.length ? "Add team to " : "Assign team to ") + pr.name;
  const canAdd = Object.keys(TEAMS).some((t) => !ts.includes(t));
  return `<div class="tchips" role="group" aria-label="Teams on ${esc(pr.name)}">${ts.length ? chips : `<span class="muted">No team</span>`}${ts.length >= 2 ? `<span class="tchips-note" title="More than one team works on this project">Shared by ${ts.length} teams</span>` : ""}${canAdd ? `<button class="btn outline xs" id="team-add" type="button" data-act="team-add" data-id="${id}" aria-label="${esc(addName)}">${ic("plus", "ic16")}${addLbl}</button>` : ""}</div>`;
}
function removeTeam(pid, t) {
  const ts = P[pid].teams, i = ts.indexOf(t); if (i < 0) return;
  const name = TEAMS[t].name;
  ts.splice(i, 1); render(false);
  const rms = [...document.querySelectorAll('.tchip .rm')];
  const to = rms[i] || rms[i - 1] || document.getElementById("team-add"); if (to) to.focus();
  toast(`${name} removed from ${P[pid].name}.`, "check-circle", () => { if (!ts.includes(t)) { ts.splice(i, 0, t); render(false); } });
}

function conversationPreview(chId) {
  const ch = CH[chId];
  return `<div class="sec-h"><h2>${ic("hash", "ic20")}${esc(ch.name)}</h2><a class="link" href="#/chat/${chId}">Open in Chat${ic("chev-right", "ic16")}</a></div>
    <div class="msgs" style="padding:0">${ch.msgs.slice(-3).map(msgHtml).join("")}</div>`;
}

function memberRows(ids, opts = {}) {
  if (!ids.length) return `<p class="empty-row">${esc(opts.emptyText || "No members yet. Hire an agent or add a person.")}</p>`;
  return `<ul class="rows" role="list">${ids.map((id) => {
    const m = M[id], line = statusLine(id);
    const facts = [`<span>${kindWord(id)}</span>`];
    if (!opts.noTeam) facts.push(`<span>${m.teams.map((t) => esc(TEAMS[t].name)).join(", ")}</span>`);
    if (m.mgr) facts.push(`<span>Reports to ${esc(M[m.mgr].name)}</span>`);
    if (!opts.noTeam && !m.teams.length) facts.splice(1, 1);
    return `<li class="linked">${av(id, 32, "word")}<div class="grow"><p class="rt"><a class="stretch" href="#/members/${id}" data-person="${esc(id)}">${esc(m.name)}</a><span class="sec">${esc(m.role)}</span></p>
      <div class="meta">${facts.join(SEP)}</div>${line ? `<p class="rowline">${line}</p>` : ""}</div>
      ${presPill(id)}${ic("chev-right", "ic20 chev")}</li>`;
  }).join("")}</ul>`;
}

/* "↓ N more" under the queue: counts rows below the visible area, scrolls to them, hides at the end. */
function bindQueueMore(list) {
  const more = list.parentElement.querySelector(".pj-q-more");
  if (!more) return;
  const hiddenBelow = () => [...list.children].filter((li) => li.offsetTop - list.offsetTop + li.offsetHeight > list.scrollTop + list.clientHeight + 2);
  const sync = () => {
    const n = hiddenBelow().length;
    more.hidden = n === 0;
    list.classList.toggle("is-scrolling", n > 0);
    more.querySelector("span").textContent = `${n} more`;
    more.setAttribute("aria-label", `Show ${n} more ${n === 1 ? "item" : "items"}`);
  };
  list.addEventListener("scroll", sync, { passive: true });
  more.addEventListener("click", () => {
    const next = hiddenBelow()[0];
    if (next) list.scrollTo({ top: next.offsetTop - list.offsetTop - 8, behavior: "smooth" });
  });
  sync();
}
/* Keep the selected queue item visible inside the scrolling queue after a re-render. */
function mountProjectQueue(main) {
  const list = main.querySelector(".pj-q-list"), cur = list?.querySelector('[aria-current="true"]');
  if (!list || !cur) return;
  /* Show exactly four whole rows; more scroll, with a fade at the bottom edge as the cue. */
  const rows = [...list.children];
  if (rows.length > 4) {
    const pad = parseFloat(getComputedStyle(list).paddingTop) || 0;
    list.style.maxHeight = `${rows[3].offsetTop - list.offsetTop + rows[3].offsetHeight + pad}px`;
    list.classList.add("is-scrolling");
  }
  const top = cur.offsetTop - list.offsetTop, bottom = top + cur.offsetHeight;
  if (top < list.scrollTop) list.scrollTop = top;
  else if (bottom > list.scrollTop + list.clientHeight) list.scrollTop = bottom - list.clientHeight;
  bindQueueMore(list);
}

export {
  mountProjectQueue,
  conversationPreview,
  memberRows,
  pageProject,
  pageProjects,
  removeTeam,
};
