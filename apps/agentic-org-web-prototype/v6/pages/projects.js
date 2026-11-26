import { CH, IN, M, OPEN, P, TEAMS, W } from "../data.js";
import { activityViewControls, pageActivity, projectReleaseSummary } from "./activity.js";
import { msgHtml } from "./chat.js";
import { projectInsights } from "./insights.js";
import { notFound } from "./settings.js";
import { scopedWorkToolbar, scopedWorkWorkspace } from "./work.js";
import { render } from "../shell/router.js";
import { S } from "../state.js";
import { SEP, acts, av, avStack, crumbs, esc, feed, hdr, ic, kindWord, leadOf, leadTxt, linkTabs, menuItem, moreMenu, needsCard, pill, presPill, progress, projMembers, projPill, sectionTabs, showMenu, statusLine, toast } from "../ui.js";

function pageProjects() {
  const ids = Object.keys(P).filter((p) => S.showArchived || !P[p].archived);
  const active = Object.keys(P).filter((p) => !P[p].archived).length;
  const filter = showMenu("pshow", [["showArchived", "Show", [[false, "Active", active], [true, "All, with archived", Object.keys(P).length]]]],
    { icon: "filter", label: "Filter projects", active: S.showArchived });
  return hdr("Projects", `${active} active · ${Object.keys(P).length - active} archived`, filter) +
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
  const activityControls = tab === "activity" ? `<div class="queue-controls project-activity-controls">${activityViewControls(id)}</div>` : "";
  return crumbs([["#/projects", "Projects"], [null, pr.name]]) + `<div class="section-header project-header">
    ${hdr(pr.name, leadOf(id) ? esc(leadTxt(id)) : "", "", projPill(id))}
    ${sectionTabs(linkTabs("#/projects/" + id, tabs, tab === "overview" ? "" : tab), activityControls + more)}
  </div>` + (tab === "overview" ? projectReleaseSummary(id) : "") + projectBody(id, tab);
}
function projectBody(id, tab) {
  const pr = P[id], ws = W.filter((w) => w.proj === id);
  if (tab === "overview") {
    const needs = IN.filter((i) => i.proj === id && (!i.handled || i.handledNow));
    return `<div class="grid2"><div>
      <section class="section"><div class="sec-h"><h2>Goal</h2></div><p class="why" style="font-size:14px">${esc(pr.goal)}</p>
        <div class="stats" style="margin-top:12px"><span><b>${ws.filter(OPEN).length}</b> open</span><span><b>${ws.filter((w) => w.status === "Blocked").length}</b> blocked</span></div>
        ${pr.risk ? `<div class="callout" style="margin-top:16px">${ic("warn", "ic20")}<span>${esc(pr.risk)}</span></div>` : ""}</section>
      <section class="section"><div class="sec-h"><h2>${ic("bell", "ic20 c-amber")}Needs you for this project</h2></div>${needs.length ? `<div class="cards">${needs.map(needsCard).join("")}</div>` : `<p class="muted" style="padding-bottom:24px">Nothing needs you on this project.</p>`}</section>
    </div><aside class="compact"><div class="sec-h"><h2 style="font-size:14px;font-weight:600">Recent activity</h2></div>${feed(acts().filter((a) => a.proj === id).slice(0, 5), { noProj: true })}</aside></div>`;
  }
  if (tab === "work") return scopedWorkToolbar("project", id, ws) + scopedWorkWorkspace();
  if (tab === "team") return projectTeams(id) + memberRows(projMembers(id), { emptyText: "No members yet." });
  if (tab === "activity") return pageActivity(id);
  if (tab === "insights") return projectInsights(id);
  if (tab === "conversations") return `<div class="section">${conversationPreview(pr.channel)}</div>`;
  return notFound();
}

function projectTeams(id) {
  const pr = P[id], ts = pr.teams;
  const chips = ts.map((t) => `<span class="tchip"><a href="#/teams/${t}">${esc(TEAMS[t].name)}</a><button class="rm" type="button" data-act="team-rm" data-id="${id}" data-v="${t}" aria-label="Remove ${esc(TEAMS[t].name)} from ${esc(pr.name)}" title="Remove ${esc(TEAMS[t].name)}">${ic("x", "ic16")}</button></span>`).join("");
  const addLbl = ts.length ? "Add team…" : "Assign team…", addName = (ts.length ? "Add team to " : "Assign team to ") + pr.name;
  const canAdd = Object.keys(TEAMS).some((t) => !ts.includes(t));
  return `<div class="tchips" role="group" aria-label="Teams on ${esc(pr.name)}">${ts.length ? chips : `<span class="muted">No team</span>`}${ts.length >= 2 ? pill("Shared", "gray") : ""}${canAdd ? `<button class="btn outline xs" id="team-add" type="button" data-act="team-add" data-id="${id}" aria-label="${esc(addName)}">${ic("plus", "ic16")}${addLbl}</button>` : ""}</div>`;
}
function removeTeam(pid, t) {
  const ts = P[pid].teams, i = ts.indexOf(t); if (i < 0) return;
  const name = TEAMS[t].name;
  ts.splice(i, 1); render(false);
  const rms = [...document.querySelectorAll('.tchip .rm')];
  const to = rms[i] || rms[i - 1] || document.getElementById("team-add"); if (to) to.focus();
  toast(`${name} removed from ${P[pid].name}.`, "check-circle");
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

export {
  conversationPreview,
  memberRows,
  pageProject,
  pageProjects,
  removeTeam,
};
