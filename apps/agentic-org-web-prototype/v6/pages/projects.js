import { CH, IN, KIND, M, OPEN, P, TEAMS, W, needsMe } from "../data.js";
import { pageActivity, projectReleaseSummary } from "./activity.js";
import { msgHtml } from "./chat.js";
import { projectInsights } from "./insights.js";
import { notFound } from "./settings.js";
import { scopedWorkToolbar, scopedWorkWorkspace } from "./work.js";
import { render } from "../shell/router.js";
import { S } from "../state.js";
import { acts, av, avStack, crumbs, esc, feed, hdr, ic, itemActions, kindWord, leadOf, leadTxt, linkTabs, menuItem, moreMenu, needsCard, pill, presPill, progress, projMembers, projPill, recommendationCallout, searchRow, searchToggle, sectionHead, sectionTabs, SEP, showMenu, statusLine, toast } from "../ui.js";

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
      <div class="grow"><p class="rt"><a class="stretch" href="#/projects/${p}">${esc(pr.name)}</a>${leadOf(p) ? `<span class="sec">${esc(leadTxt(p))}</span>` : ""}</p>
      <div class="meta"><span>${pr.teams.map((t) => `<a href="#/teams/${t}">${esc(TEAMS[t].name)}</a>`).join(", ")}</span>${SEP}<span>${openN} open</span>${pr.due ? SEP + `<span>Due ${esc(pr.due)}</span>` : ""}${SEP}<span>Updated ${esc(pr.updated)}</span></div>
      <div style="max-width:320px;margin-top:8px">${progress(p)}</div></div>
      <span class="hide-sm">${avStack(projMembers(p), 4)}</span>${projPill(p)}${ic("chev-right", "ic20 chev")}</li>`;
  }).join("")}</ul>`;
}

function pageProject(id, tab) {
  const pr = P[id]; if (!pr) return notFound();
  tab = tab || "overview";
  const ws = W.filter((w) => w.proj === id);
  const tabs = [
    ["", "Overview"],
    ["activity", "Activity"],
    ["work", "Work", ws.filter(OPEN).length],
    ["insights", "Insights"],
    ["conversations", "Conversations"],
    ["team", "Team", projMembers(id).length],
  ];
  const more = moreMenu("pmore", "More actions for " + pr.name, menuItem("health", id, "flag", "Set health…", ` data-id="${id}"`) + menuItem("proto", "Edit " + pr.name, "pencil", "Edit"), "ghost");
  return crumbs([["#/projects", "Projects"], [null, pr.name]]) + `<div class="section-header project-header">
    ${hdr(pr.name, leadOf(id) ? esc(leadTxt(id)) : "", "", projPill(id))}
    ${sectionTabs(linkTabs("#/projects/" + id, tabs, tab === "overview" ? "" : tab), more)}
  </div>` + (tab === "activity" ? "" : projectReleaseSummary(id)) + projectBody(id, tab);
}
function projectBody(id, tab) {
  const pr = P[id], ws = W.filter((w) => w.proj === id);
  if (tab === "overview") {
    const needs = IN.filter((i) => i.proj === id && (!i.handled || i.handledNow));
    const pending = needs.filter((i) => !i.handled).length;
    return `<div class="pj-overview">
      ${projectSummaryHtml(id, ws)}
      <section class="project-sec" aria-labelledby="pj-goal-h">${sectionHead({ id: "pj-goal-h", title: "Goal" })}<p class="why">${esc(pr.goal)}</p>
        ${pr.risk ? `<div class="callout" style="margin-top:12px">${ic("warn", "ic20")}<span>${esc(pr.risk)}</span></div>` : ""}</section>
      <section class="project-sec" aria-labelledby="pj-needs-h">${sectionHead({ id: "pj-needs-h", title: "Needs your attention", count: pending ? `${pending} pending` : "", right: `<a class="link" href="#/inbox">Open inbox${ic("chev-right", "ic16")}</a>` })}${needs.length ? projectDecisionsHtml(needs) : `<p class="muted">Nothing needs you on this project.</p>`}</section>
      <section class="project-sec" aria-labelledby="pj-act-h">${sectionHead({ id: "pj-act-h", title: "Recent activity", right: `<a class="link" href="#/projects/${esc(id)}/activity">All activity${ic("chev-right", "ic16")}</a>` })}${feed(acts().filter((a) => a.proj === id).slice(0, 6), { noProj: true })}</section>
    </div>`;
  }
  if (tab === "work") return scopedWorkToolbar("project", id, ws) + scopedWorkWorkspace();
  if (tab === "team") return projectTeams(id) + memberRows(projMembers(id), { emptyText: "No members yet." });
  if (tab === "activity") return pageActivity(id);
  if (tab === "insights") return projectInsights(id);
  if (tab === "conversations") return `<div class="section">${conversationPreview(pr.channel)}</div>`;
  return notFound();
}

/* Project at a glance, in the style of Home's summary cards: progress, schedule, open work, team. */
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
      ${card("Progress", "check-circle", `${done}/${total}`, "tasks done", `<span class="pj-kpi-bar" aria-hidden="true"><i style="width:${pct}%"></i></span>`, `#/projects/${id}/work`)}
      ${card("Schedule", "calendar-check", pr.due ? esc(pr.due) : "—", pr.due ? "due" : "no due date", pr.pace ? `<span class="pj-pace${tone ? " is-" + tone : ""}">${esc(pr.pace)}</span>${pr.release ? ` · ${esc(pr.release)}` : ""}` : "", `#/projects/${id}/activity`)}
      ${card("Open work", "clipboard", String(open), "open", `${blocked} blocked · ${mine} need you`, `#/projects/${id}/work`)}
      ${card("Team", "user-group", String(members.length), members.length === 1 ? "member" : "members", `${avStack(members, 5)}<span class="pj-kpi-teams">${pr.teams.map((t) => esc(TEAMS[t].name)).join(", ")}</span>`, `#/projects/${id}/team`)}
    </section>`;
}

/* Inbox-style split: a priority queue on the left, the selected decision on the right.
   Narrow containers stack the decision above the queue. */
function decisionDetailHtml(it) {
  const exp = it.expires && !it.handled ? `<span class="pin pj-detail-exp">${ic("clock", "ic14")}${esc(it.expires)}</span>` : "";
  return `<article class="pj-detail" aria-labelledby="pj-detail-t">
      <div class="pj-detail-top">${av(it.from, 24)}<span class="grow"><b>${esc(M[it.from].name)}</b> · ${esc(it.sub)} · ${esc(it.time)}</span>${exp}</div>
      <h3 id="pj-detail-t" class="pj-detail-title">${esc(it.what)}</h3>
      <p class="why">${esc(it.why)}</p>
      ${it.rec && !it.handled ? recommendationCallout(it) : ""}
      ${itemActions(it, "card")}
      <div class="pj-detail-foot">${it.task ? `<span class="work-ref">${esc(it.task)}</span>` : "<span></span>"}<a class="link" href="#/inbox/${esc(it.id)}">Details${ic("open", "ic16")}</a></div>
    </article>`;
}
function projectDecisionsHtml(needs) {
  const sel = needs.find((it) => it.id === S.pjDecision) || needs[0];
  const item = (it) => `<li><button type="button" class="pj-q-item${it === sel ? " is-current" : ""}" data-act="set" data-k="pjDecision" data-v="${esc(it.id)}" aria-pressed="${it === sel}">
        <span class="pj-q-kind">${ic(KIND[it.kind]?.icon || "bell", "ic14")}<span class="grow">${esc(it.sub)}</span><time>${esc(it.time)}</time></span>
        <span class="pj-q-title">${esc(it.what)}</span>${it.handled ? `<span class="pj-q-done">${esc(it.outcome || "Handled")}</span>` : ""}</button></li>`;
  return `<div class="pj-split">
      <nav class="pj-queue-pane" aria-label="Priority queue"><p class="pj-queue-h">Priority queue <span class="sec-head-count">${needs.length}</span></p>
        <ul role="list" class="pj-q-list" tabindex="-1">${needs.map(item).join("")}</ul>
        <a class="pj-queue-all" href="#/inbox">View all in Inbox${ic("chev-right", "ic16")}</a></nav>
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
    return `<li class="linked">${av(id, 32, "word")}<div class="grow"><p class="rt"><a class="stretch" href="#/members/${id}">${esc(m.name)}</a><span class="sec">${esc(m.role)}</span></p>
      <div class="meta">${facts.join(SEP)}</div>${line ? `<p class="rowline">${line}</p>` : ""}</div>
      ${presPill(id)}${ic("chev-right", "ic20 chev")}</li>`;
  }).join("")}</ul>`;
}

/* Keep the selected queue item visible inside the scrolling queue after a re-render. */
function mountProjectQueue(main) {
  const list = main.querySelector(".pj-q-list"), cur = list?.querySelector(".is-current");
  if (!list || !cur) return;
  const top = cur.offsetTop - list.offsetTop, bottom = top + cur.offsetHeight;
  if (top < list.scrollTop) list.scrollTop = top;
  else if (bottom > list.scrollTop + list.clientHeight) list.scrollTop = bottom - list.clientHeight;
}

export {
  mountProjectQueue,
  conversationPreview,
  memberRows,
  pageProject,
  pageProjects,
  removeTeam,
};
