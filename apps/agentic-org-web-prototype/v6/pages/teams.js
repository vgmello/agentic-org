import { calendarHtml } from "./calendar.js";
import { orgChartBody, orgSummary } from "./org-chart.js";
import { CH, KNOWLEDGE, M, NOTES, OFFICES, OPEN, P, TEAMS, W } from "../data.js";
import { activityEventTime, activityTimeline, formatTelemetryHours } from "./activity.js";
import { agentRailHtml, agentSessionAvailability, getConsoleSession } from "./agent-profile.js";
import { agentConversationsTab } from "./agent-conversations.js";
import { agentConversations } from "./chat-model.js";
import { INSIGHT_COMPLETIONS, INSIGHT_DAY, INSIGHT_TODAY, insightBuckets, insightChart, insightCompletions, insightDayLabel, insightInRange, insightMedian } from "./insights.js";
import { conversationPreview, memberRows } from "./projects.js";
import { healthMark, notFound } from "./settings.js";
import { scopedWorkToolbar, scopedWorkWorkspace, teamWork, visibleWorkItems, workGroups, workList } from "./work.js";
import { S } from "../state.js";
import { SEP, acts, av, avStack, crumbs, esc, feed, hireMenu, ic, isPerson, joinSep, kindWord, leadOf, leadTxt, linkTabs, memberIds, menuItem, mlink, moreMenu, needsCard, openFrom, pill, plink, plural, presInline, presPill, progress, projPill, queueHeadline, sectionTabs, showMenu, statusLine } from "../ui.js";

/* ---------------------------------------------------------------- teams */
function teamsHeader(title, count, current, controls = "", actions = "") {
  const tabs = linkTabs("#/teams", [["", "Teams", Object.keys(TEAMS).length], ["members", "Members", memberIds().length], ["org-chart", "Org chart"]], current, "Teams views");
  return `<div class="section-header org-directory-header">${queueHeadline(title, count)}${sectionTabs(tabs, controls, actions)}</div>`;
}
function ownerCard() {
  const reports = memberIds().filter((k) => M[k].mgr === "you");
  return `<div class="ocard">${av("you", 40)}<span><span class="nm">You</span> <span class="muted">· Owner</span><span class="rl">Runs Agentic Org · ${plural(reports.length, "member reports", "members report")} to you: ${reports.map((k) => esc(M[k].name)).join(", ")}</span></span></div>`;
}
const teamProjects = (id) => Object.keys(P).filter((project) => P[project].teams.includes(id));
const teamActivity = (id) => acts().filter(({ who }) => TEAMS[id].members.includes(who));

function pageTeams() {
  const all = memberIds(), ppl = all.filter(isPerson).length;
  const create = `<button class="btn primary" type="button" data-act="proto" data-v="New team">${ic("plus", "ic16")}New team</button>`;
  return teamsHeader("Teams", `${plural(Object.keys(TEAMS).length, "team", "teams")} · ${plural(ppl, "person", "people")} · ${plural(all.length - ppl, "agent", "agents")}`, "", "", create) +
  `<div class="owner-wrap">${ownerCard()}</div>
  <ul class="rows" role="list" aria-label="Teams">${Object.entries(TEAMS).map(([k, t]) => {
    const people = t.members.filter(isPerson).length, agents = t.members.length - people;
    const projs = teamProjects(k).filter((p) => !P[p].archived);
    return `<li class="linked"><span class="chip-i" style="margin-left:0;width:32px;height:32px;font-size:12px" aria-hidden="true">${esc(t.name[0])}</span>
      <div class="grow"><p class="rt"><a class="stretch" href="#/teams/${k}">${esc(t.name)}</a><span class="sec">Lead ${esc(M[t.lead].name)}</span></p>
      <p class="rowline">${esc(t.purpose)}</p>
      <div class="meta"><span>${plural(t.members.length, "member", "members")} · ${plural(people, "person", "people")} · ${plural(agents, "agent", "agents")}</span>${SEP}<span>${projs.length ? "Projects: " + projs.map((p) => esc(P[p].name)).join(", ") : "No active projects"}</span></div></div>
      <span class="hide-sm">${avStack(t.members, 4)}</span>${ic("chev-right", "ic20 chev")}</li>`;
  }).join("")}</ul>`;
}
function teamOverview(id, items) {
  const team = TEAMS[id];
  const base = "#/teams/" + id;
  const end = INSIGHT_TODAY;
  const start = end - 27 * INSIGHT_DAY;
  const done = Object.keys(INSIGHT_COMPLETIONS).flatMap(insightCompletions)
    .filter((item) => item.team === id && insightInRange(item, start, end));
  const open = items.filter(OPEN);
  const timing = (field) => {
    const hours = insightMedian(done.map((item) => item[field]));
    return hours === null ? "—" : formatTelemetryHours(hours);
  };
  const enoughSamples = done.length >= 5;
  const stats = [
    ["Completed", done.length, "Tasks and bugs in the last 28 days"],
    ["Median cycle time", timing("cycle"), enoughSamples ? "From start to completion" : "At least 5 completions needed"],
    ["Median approval wait", timing("wait"), enoughSamples ? "Time waiting on you per completed item" : "At least 5 completions needed"],
    ["Blocked now", open.filter(({ status }) => status === "Blocked").length, "Of " + plural(open.length, "open item", "open items")],
  ];
  const types = ["Task", "Bug"];
  const weekly = insightBuckets(start, end, 7).map(({ first, last, label }) => {
    const completed = done.filter((item) => insightInRange(item, first, last));
    return { label, values: types.map((kind) => completed.filter((item) => item.kind === kind).length) };
  });
  const statuses = workGroups(open, "status").map(({ label, rows }) => ({ label, values: [rows.length] }));
  const contributions = team.members.map((member) => ({
    member,
    completed: done.filter((item) => item.member === member).length,
    open: open.filter(({ who }) => who === member).length,
    blocked: open.filter(({ who, status }) => who === member && status === "Blocked").length,
  })).filter((row) => row.completed || row.open)
    .sort((a, b) => M[a.member].name.localeCompare(M[b.member].name));
  const projects = teamProjects(id).filter((project) => !P[project].archived);
  const now = new Date();
  const recent = teamActivity(id).map((event) => ({ event, age: activityEventTime(event.time, now).age }))
    .sort((a, b) => a.age - b.age).slice(0, 3).map(({ event }) => event);
  return `<section class="team-overview" aria-label="Team overview">
    <div class="team-overview-intro"><p>${esc(team.purpose)} <span>Lead ${mlink(team.lead, "insight-link")}</span></p>
      <small>Demo data · ${insightDayLabel(start)}–${insightDayLabel(end)}</small></div>
    <dl class="insight-kpis">${stats.map(([label, value, context]) => `<div><dt>${label}</dt><dd>${esc(value)}</dd><dd class="insight-kpi-note">${esc(context)}</dd></div>`).join("")}</dl>
    <div class="insights-grid">
      ${insightChart("team-throughput", "Weekly completions", "Tasks and bugs completed each week", types, weekly, { columns: true })}
      ${insightChart("team-workload", "Open work by status", "Current assignments for this team", ["Items"], statuses, {
        control: `<a class="insight-link" href="${base}/work">View work</a>`, empty: "No open work assigned.",
      })}
    </div>
    <div class="insights-grid">
      <section class="insight-card" aria-labelledby="team-projects-title">
        <header class="insight-card-head"><div><h3 id="team-projects-title">Active projects</h3><p>Progress across each project</p></div><a class="insight-link" href="${base}/projects">All projects</a></header>
        ${projects.length ? `<ul class="team-overview-projects" role="list">${projects.map((project) => `<li>
          <div>${plink(project)}${projPill(project)}</div>
          ${progress(project)}
        </li>`).join("")}</ul>` : '<p class="insight-empty">No active projects for this team.</p>'}
      </section>
      <section class="insight-card" aria-labelledby="team-contributions-title">
        <header class="insight-card-head"><div><h3 id="team-contributions-title">Contributions</h3><p>Completions in the last 28 days and current assignments</p></div></header>
        ${contributions.length ? `<div class="insight-table-scroll"><table aria-labelledby="team-contributions-title">
          <thead><tr><th scope="col">Contributor</th><th scope="col">Completed</th><th scope="col">Open</th><th scope="col">Blocked</th></tr></thead>
          <tbody>${contributions.map(({ member, completed, open, blocked }) => `<tr><th scope="row">${mlink(member, "insight-link")}</th><td>${completed}</td><td>${open}</td><td>${blocked}</td></tr>`).join("")}</tbody>
        </table></div>` : '<p class="insight-empty">No completions or current assignments.</p>'}
      </section>
    </div>
    <section class="insight-card" aria-labelledby="team-recent-title">
      <header class="insight-card-head"><h3 id="team-recent-title">Recent activity</h3><a class="insight-link" href="${base}/activity">View activity</a></header>
      ${recent.length ? `<ul class="team-overview-events" role="list">${recent.map(({ who, text, proj, time }) =>
        `<li><span>${mlink(who, "insight-link")} ${esc(text)} · ${plink(proj)}</span><time>${esc(time)}</time></li>`).join("")}</ul>`
        : '<p class="insight-empty">No recent team activity.</p>'}
    </section>
  </section>`;
}

function pageTeam(id, tab = "overview") {
  const t = TEAMS[id];
  const people = t.members.filter(isPerson).length;
  const count = `${plural(t.members.length, "member", "members")} · ${plural(people, "person", "people")} · ${plural(t.members.length - people, "agent", "agents")}`;
  const actions = hireMenu(id) + moreMenu("tmore", "More actions for " + t.name, menuItem("proto", "Edit team " + t.name, "pencil", "Edit team") + menuItem("proto", "Move team " + t.name + " (choose its parent team)", "user-group", "Move team…") + menuItem("proto", "Change manager of " + t.name, "user", "Change manager…"));
  const ws = teamWork(id);
  const tabs = linkTabs("#/teams/" + id, [
    ["", "Overview"], ["members", "Members", t.members.length], ["work", "Work", `${ws.filter(OPEN).length} open`], ["calendar", "Calendar"],
    ["projects", "Projects"], ["activity", "Activity"], ["conversations", "Conversations"],
  ], tab === "overview" ? "" : tab, "Team sections");
  const header = crumbs([["#/teams", "Teams"], [null, t.name]]) + `<div class="section-header">${queueHeadline(t.name, count)}${sectionTabs(tabs, "", actions)}</div>`;
  if (tab === "overview") return header + teamOverview(id, ws);
  if (tab === "members") return header + `<section aria-labelledby="h-mem"><h2 class="team-roster-title" id="h-mem">All members</h2>${memberRows(t.members, { noTeam: true })}</section>`;
  if (tab === "work") return header + scopedWorkToolbar("team", id, ws) + scopedWorkWorkspace();
  if (tab === "calendar") return header + calendarHtml("team", id);
  if (tab === "projects") {
    const projects = teamProjects(id);
    return header + (projects.length ? `<ul class="rows" role="list" aria-label="Team projects">${projects.map((p) => `<li class="linked"><div class="grow"><p class="rt"><a class="stretch" href="#/projects/${p}">${esc(P[p].name)}</a>${leadOf(p) ? `<span class="sec">${esc(leadTxt(p))}</span>` : ""}</p><div style="max-width:320px;margin-top:8px">${progress(p)}</div></div>${projPill(p)}</li>`).join("")}</ul>` : '<p class="empty-row">No projects for this team.</p>');
  }
  if (tab === "activity") return header + activityTimeline(teamActivity(id));
  if (tab === "conversations") return header + `<div class="section">${conversationPreview(t.channel)}</div>`;
  return notFound();
}
function pageMembersDir() {
  const all = memberIds(), ppl = all.filter(isPerson).length;
  const ids = all.filter((k) => S.people === "all" || (S.people === "people" ? isPerson(k) : !isPerson(k)));
  const show = showMenu("mshow", [["people", "Show", [["all", "All", all.length], ["people", "People", ppl], ["agents", "Agents", all.length - ppl]]]],
    { icon: "filter", label: "Filter members", active: S.people !== "all" });
  return teamsHeader("Members", `${plural(all.length, "member", "members")} · ${plural(ppl, "person", "people")} · ${plural(all.length - ppl, "agent", "agents")}`, "members", show, hireMenu()) +
    memberRows(ids, { emptyText: "No members match this filter." });
}
function pageOrgChart() {
  return teamsHeader("Org chart", orgSummary(), "org-chart", "", hireMenu()) + orgChartBody();
}

function agentSetupFields(id, inProfile = false) {
  const m = M[id], s = m.setup;
  return `<div class="form">
    <div><label for="s-rt">Runtime</label><select class="field" id="s-rt" style="width:100%"><option ${s.runtime === "Claude Code" ? "selected" : ""}>Claude Code</option><option ${s.runtime === "Codex" ? "selected" : ""}>Codex</option></select></div>
    <div><label for="s-model">Model</label><input class="field" id="s-model" value="${esc(s.model)}"></div>
    <div><label for="s-how">How it runs</label><select class="field" id="s-how" style="width:100%">${["Always on", "Wakes when called", "On a schedule (weekdays 09:00)"].map((o) => `<option ${o === s.how ? "selected" : ""}>${o}</option>`).join("")}</select><p class="help">Wakes when called: asleep until a message, a task or a schedule wakes it.</p></div>
    <div><label for="s-office">Office</label><select class="field" id="s-office" style="width:100%">${s.office ? "" : '<option value="" selected>Not placed yet</option>'}${Object.entries(OFFICES).map(([k, o]) => `<option value="${k}" ${k === s.office ? "selected" : ""}>${esc(o.name)} · ${esc(o.machine)}</option>`).join("")}</select>${s.office ? "" : '<p class="help">Choose an office to place this agent; until then it can\'t pick up work.</p>'}</div>
    <div><label for="s-home">Home folder</label><input class="field" id="s-home" value="${esc(s.home)}" disabled><p class="help">Where its journal, memory, knowledge and backlog live.</p></div>
    <div class="setup"><h2>Role &amp; reporting</h2><dl class="dl"><dt>Role</dt><dd>${esc(m.role)}</dd><dt>Teams</dt><dd>${m.teams.map((t) => esc(TEAMS[t].name)).join(", ") || "No team"}</dd><dt>Reports to</dt><dd>${m.mgr ? mlink(m.mgr) : "Owner"}</dd></dl>${inProfile ? "" : `<p style="margin-top:12px"><a class="link" href="#/settings/roles">Open in Roles${ic("chev-right", "ic16")}</a></p>`}</div>
    <div style="display:flex;flex-wrap:wrap;gap:12px;align-items:center">
      <button class="btn primary" type="button" data-act="save-setup" data-id="${id}">Save setup</button>
      <button class="btn outline" type="button" data-act="pause" data-id="${id}">${ic(m.paused ? "play" : "pause", "ic20")}${m.paused ? "Resume agent" : "Pause agent"}</button>
      ${inProfile ? `<a class="link" style="margin-left:auto" href="#/settings/agents/${id}">Open in Organization settings${ic("chev-right", "ic16")}</a>` : ""}
    </div>
    <div class="form-danger-row">
      <p class="muted" style="font-size:0.75rem">Offboarding pauses the agent and keeps its journal, memory, and records read-only.</p>
      <button class="btn danger" type="button" data-act="offboard" data-id="${id}">Offboard</button>
    </div>
  </div>`;
}

function memberChannels(id) {
  return Object.keys(CH).filter((k) => CH[k].msgs.some((x) => x.who === id || (x.thread || []).some((r) => r.who === id)));
}

function personConversationsHtml(id) {
  const chs = memberChannels(id);
  if (!chs.length) return `<p class="empty-row">No conversations yet.</p>`;
  return `<ul class="rows" role="list">${chs.map((k) => {
    const c = CH[k], last = c.msgs[c.msgs.length - 1];
    const icon = c.kind === "channel" ? ic("hash", "ic20 chev") : ic(c.kind === "dm" ? "chat" : "eye", "ic20 chev");
    return `<li class="linked">${icon}<div class="grow"><p class="rt"><a class="stretch" href="#/chat/${k}">${c.kind === "channel" ? "#" : ""}${esc(c.name)}</a></p><p class="meta">${esc(M[last.who].name)}: ${esc(last.text)}</p></div><span class="muted" style="font-size:0.75rem">${esc(last.time)}</span></li>`;
  }).join("")}</ul>`;
}

function personOverviewHtml(m, owner, myW, myActs) {
  const working = m.task
    ? `<p class="why" style="font-size:0.875rem">${esc(m.task)}${m.proj ? " · " + plink(m.proj) : ""}</p>${m.goal ? `<p class="meta"><span class="pin wrap">${ic("flag", "ic14")}<span>Goal: ${esc(m.goal)}</span></span></p>` : ""}`
    : `<p class="muted">Not working on anything right now.</p>`;
  const left = owner
    ? `<section class="section"><div class="sec-h"><h2>Assigned to you</h2></div></section><div class="compact" style="padding:0 var(--gutter)">${workList(visibleWorkItems(myW), { hideOwner: true })}</div>`
    : `<section class="section"><div class="sec-h"><h2>Working on</h2></div>${working}</section><section class="section compact"><div class="sec-h"><h2>Recent activity</h2></div>${feed(myActs.slice(0, 5))}</section>`;
  const aside = `<div class="setup"><h2>About</h2><dl class="dl"><dt>Kind</dt><dd>${owner ? "Owner" : "Person"}</dd><dt>Role</dt><dd>${esc(m.role)}</dd><dt>Access</dt><dd>${owner ? "Owner · not a member of any team" : "Member"}</dd></dl></div>`;
  return `<div class="grid2"><div>${left}</div><aside>${aside}</aside></div>`;
}

function personMemberView(id, tab, m, owner, myW, myActs) {
  const tabs = [["", "Overview"], ["work", "Work", `${myW.filter(OPEN).length} open`], ["conversations", "Conversations"], ["activity", "Activity"]];
  if (tab === "work") return { tabs, controls: "", body: scopedWorkToolbar("person", id, myW) + scopedWorkWorkspace() };
  if (tab === "activity") return { tabs, controls: "", body: activityTimeline(myActs) };
  if (tab === "conversations") return { tabs, controls: "", body: personConversationsHtml(id) };
  if (tab === "overview") return { tabs, controls: "", body: personOverviewHtml(m, owner, myW, myActs) };
  return { tabs, controls: "", body: notFound() };
}

function agentOverviewTab(id, m, avail, myW, myActs) {
  const needs = openFrom(id), sess = getConsoleSession(id), chs = memberChannels(id);
  const lastEntry = (avail.active || sess.entries.length) ? sess.entries[sess.entries.length - 1] : null;
  const needsHtml = needs.length ? `<section class="section" aria-labelledby="h-ag-needs"><div class="sec-h"><h2 id="h-ag-needs">${ic("bell", "ic20 c-amber")}Needs you</h2><a class="link" href="#/inbox">Open Inbox${ic("chev-right", "ic16")}</a></div><div class="cards">${needs.map(needsCard).join("")}</div></section>` : "";
  const workingHtml = m.task
    ? `<p class="why" style="font-size:0.875rem">${esc(m.task)}${m.proj ? " · " + plink(m.proj) : ""}</p>${m.goal ? `<p class="meta"><span class="pin wrap">${ic("flag", "ic14")}<span>Goal: ${esc(m.goal)}</span></span></p>` : ""}`
    : `<p class="muted">${avail.active ? "Not working on anything right now." : esc(avail.reason)}</p>`;
  const latestHtml = lastEntry ? `<div class="k-doc" style="margin-top:16px">
            <div style="display:flex;align-items:center;justify-content:space-between;gap:8px;margin-bottom:6px;flex-wrap:wrap">
              <span style="font-weight:600;color:var(--white);display:inline-flex;align-items:center;gap:6px">${ic("terminal", "ic16")}Latest session output</span>
              <a class="link" style="font-size:0.75rem" href="#/members/${id}/sessions">Open Sessions${ic("chev-right", "ic16")}</a>
            </div>
            <p class="muted" style="font-size:0.75rem;margin-bottom:6px">${esc(M[lastEntry.from] ? M[lastEntry.from].name : lastEntry.from)} · ${esc(lastEntry.time)} — “${esc(lastEntry.text)}”</p>
            <p style="color:var(--gray-300)">${esc(lastEntry.reply.split("\n")[0])}</p>
          </div>` : "";
  const officeLabel = OFFICES[m.setup.office] ? esc(OFFICES[m.setup.office].name) + " · " + esc(OFFICES[m.setup.office].machine) : "Not placed yet";
  const convRow = chs.length ? `<dt>Conversations</dt><dd>${chs.map((k) => `<a href="#/chat/${k}">${CH[k].kind === "channel" ? "#" : ""}${esc(CH[k].name)}</a>`).join(" · ")}</dd>` : "";
  return `${needsHtml}
        <section class="section"><div class="sec-h"><h2>Working on</h2><a class="link" href="#/members/${id}/tasks">All tasks (${myW.length})${ic("chev-right", "ic16")}</a></div>
          ${workingHtml}${latestHtml}
        </section>
        <section class="section compact"><div class="sec-h"><h2>Recent activity</h2><a class="link" href="#/members/${id}/sessions">Live stream${ic("chev-right", "ic16")}</a></div>${feed(myActs.slice(0, 4))}</section>
        <section class="section"><div class="sec-h"><h2>Setup &amp; channels</h2><a class="link" href="#/members/${id}/settings">Settings${ic("chev-right", "ic16")}</a></div>
          <dl class="dl" style="max-width:640px">
            <dt>Runtime</dt><dd>${esc(m.setup.runtime)} · ${esc(m.setup.model)}</dd>
            <dt>How it runs</dt><dd>${esc(m.setup.how)}</dd>
            <dt>Office</dt><dd>${officeLabel}</dd>
            <dt>Home folder</dt><dd><code>${esc(m.setup.home)}</code></dd>
            ${convRow}
          </dl>
        </section>`;
}

function agentKnowledgeTab(id, m) {
  const docs = KNOWLEDGE[id] || KNOWLEDGE.default(id);
  const rows = docs.map((doc) => {
    const open = !!S.openKnow[id + ":" + doc.id];
    return `<li>${ic("doc-check", "ic20 chev")}<div class="grow">
            <p class="rt"><button type="button" style="text-align:left;font-weight:600" data-act="toggle-know" data-id="${id}" data-v="${doc.id}" aria-expanded="${open}">${esc(doc.title)}</button>${pill(doc.folder + "/", "gray", null, true)}</p>
            <p class="rowline">${esc(doc.summary)}</p>
            <div class="meta"><code>${esc(doc.path)}</code>${SEP}<span>Updated ${esc(doc.updated)}</span></div>
            ${open ? `<div class="k-doc"><h3>${esc(doc.title)}</h3><p style="white-space:pre-wrap">${esc(doc.body)}</p></div>` : ""}
          </div><button class="btn ghost sm" type="button" data-act="toggle-know" data-id="${id}" data-v="${doc.id}">${open ? "Hide" : "Read"}</button></li>`;
  }).join("");
  return `<div class="section" style="padding-bottom:16px"><h2 class="sr-only">Knowledge documents</h2><div class="callout">${ic("folder", "ic20")}<span>Reference material in <code>${esc(m.setup.home)}/knowledge/</code> and shared project briefs. Select an item to inspect its contents.</span></div></div><ul class="rows" role="list">${rows}</ul>`;
}

function agentTabContent(id, tab, m, avail, myW, myActs, sub) {
  if (tab === "overview") return { controls: "", leftBody: agentOverviewTab(id, m, avail, myW, myActs) };
  if (tab === "tasks") {
    return { controls: "", leftBody: scopedWorkToolbar("agent", id, myW) + scopedWorkWorkspace() };
  }
  if (tab === "sessions") return { controls: "", leftBody: `<div id="agent-sessions-root" data-agent="${id}"></div>` };
  if (tab === "conversations") return { controls: "", leftBody: agentConversationsTab(id, sub) };
  if (tab === "journal") {
    const n = NOTES[id] || NOTES.default(id);
    return { controls: "", leftBody: `<div class="section"><h2 class="sr-only">Daily journal</h2><div class="callout" style="margin-bottom:20px">${ic("lock", "ic20")}<span>Read-only daily work log from <code>${esc(m.setup.home)}/journal/</code>. ${esc(m.name)} records progress at the end of each session.</span></div><div class="journal">${n.journal.map((j) => `<article><h2>${esc(j.d)}</h2><ul>${j.items.map((x) => `<li>${esc(x)}</li>`).join("")}</ul></article>`).join("")}</div></div>` };
  }
  if (tab === "memory") {
    const n = NOTES[id] || NOTES.default(id);
    const mem = Array.isArray(n.memory) ? { decisions: n.memory, context: [] } : n.memory;
    const ctx = mem.context?.length ? `<article><h2>Project &amp; operational context</h2><ul>${mem.context.map((x) => `<li>${esc(x)}</li>`).join("")}</ul></article>` : "";
    return { controls: "", leftBody: `<div class="section"><h2 class="sr-only">Durable memory</h2><div class="callout" style="margin-bottom:20px">${ic("lock", "ic20")}<span>Read-only durable memory from <code>${esc(m.setup.home)}/memory/MEMORY.md</code>. Loaded at the start of every session.</span></div><div class="journal"><article><h2>Decisions &amp; conventions</h2><ul>${mem.decisions.map((x) => `<li>${esc(x)}</li>`).join("")}</ul></article>${ctx}</div></div>` };
  }
  if (tab === "knowledge") return { controls: "", leftBody: agentKnowledgeTab(id, m) };
  if (tab === "settings") return { controls: "", leftBody: `<div class="section" style="padding-bottom:0"><div class="sec-h"><h2>Configuration</h2>${healthMark(id)}</div><p class="muted" style="font-size:0.8125rem">Runtime, schedule, office placement, and lifecycle controls for ${esc(m.name)}.</p></div>` + agentSetupFields(id, true) };
  return { controls: "", leftBody: notFound() };
}

function memberActionsHtml(id, m, agent, owner, tab) {
  if (owner) return "";
  const hasSecondaryPrimary = agent && (tab === "settings" || (tab === "sessions" && S.consoleView === "chat" && agentSessionAvailability(id).active));
  const pauseLabel = m.paused ? "Resume" : "Pause", pauseIcon = m.paused ? "play" : "pause";
  const more = agent
    ? `<button role="menuitem" type="button" data-act="pause" data-id="${id}">${ic(pauseIcon, "ic20")}${pauseLabel}</button><button role="menuitem" type="button" data-act="proto" data-v="Edit ${esc(m.name)}">${ic("pencil", "ic20")}Edit</button><hr><button role="menuitem" class="danger-item" type="button" data-act="offboard" data-id="${id}">${ic("logout", "ic20")}Offboard</button>`
    : `<button role="menuitem" type="button" data-act="proto" data-v="Edit ${esc(m.name)}">${ic("pencil", "ic20")}Edit</button>`;
  return `<button class="btn ${hasSecondaryPrimary ? "outline" : "primary"}" type="button" data-act="go" data-v="#/chat/dm-${id}">${ic("chat", "ic20")}Message</button>
      ${agent ? `<button class="btn outline" type="button" data-act="agent-focus-open" data-id="${id}">${ic("maximize", "ic20")}Analysis Mode</button>` : ""}
      <button class="btn outline" type="button" data-act="work-new-task" data-id="${id}">${ic("clipboard", "ic20")}Assign task</button>
      <div class="menu-wrap"><button class="btn outline icon" type="button" data-act="menu" data-id="more" aria-haspopup="menu" aria-expanded="false" aria-controls="menu-more" aria-label="More actions for ${esc(m.name)}" title="More actions">${ic("ellipsis", "ic20")}</button><div class="menu" id="menu-more" role="menu" hidden>${more}</div></div>`;
}

const AGENT_TAB_ALIASES = { work: "tasks", notes: "memory", activity: "sessions" };

function memberMetaFacts(id, m, owner) {
  if (owner) return `<span>Owner</span>${SEP}<span>Not a member of any team</span>`;
  const teams = m.teams.length ? `<span>${m.teams.map((t) => `<a href="#/teams/${t}">${esc(TEAMS[t].name)}</a>`).join(", ")}</span>` : "";
  const mgr = `<span>${m.mgr ? "Reports to " + mlink(m.mgr) : "Runs the organization"}</span>`;
  return joinSep([`<span>${kindWord(id)}</span>`, teams, mgr]);
}

function agentMemberView(id, resolvedTab, m, myW, myActs, sub) {
  const tabs = [["", "Overview"], ["tasks", "Tasks", `${myW.filter(OPEN).length} open`], ["sessions", "Sessions"], ["conversations", "Conversations", agentConversations(id).length], ["journal", "Journal"], ["memory", "Memory"], ["knowledge", "Knowledge"], ["settings", "Settings"]];
  const { controls, leftBody } = agentTabContent(id, resolvedTab, m, agentSessionAvailability(id), myW, myActs, sub);
  const body = `<div class="agent-shell" id="agent-shell" data-agent="${id}" data-tab="${resolvedTab}"><div class="agent-main">${leftBody}</div>${agentRailHtml(id, resolvedTab)}</div>`;
  return { tabs, controls, body };
}

/* `sub` is the part of the address after the tab: the open conversation on Conversations. */
function pageMember(id, tab, sub) {
  const m = M[id]; if (!m) return notFound();
  const agent = m.kind === "agent", owner = !!m.owner;
  const resolvedTab = agent ? (AGENT_TAB_ALIASES[tab] || tab || "overview") : (tab || "overview");
  const myW = W.filter((w) => w.who === id), myActs = acts().filter((a) => a.who === id);
  const { tabs, controls, body } = agent
    ? agentMemberView(id, resolvedTab, m, myW, myActs, sub)
    : personMemberView(id, resolvedTab, m, owner, myW, myActs);
  const actions = memberActionsHtml(id, m, agent, owner, resolvedTab);
  const line = owner ? "Runs Agentic Org" : statusLine(id);
  const pp = presPill(id, true);
  const nowLine = !agent && (pp || line) ? `<p class="now">${pp}${line ? `<span>${line}</span>` : ""}</p>` : "";
  const nav = linkTabs("#/members/" + id, tabs, resolvedTab === "overview" ? "" : resolvedTab, "Profile sections");
  const backLink = owner ? crumbs([["#/teams", "Teams"], [null, m.name]]) : crumbs([["#/teams", "Teams"], ["#/teams/members", "Members"], [null, m.name]]);
  return `${backLink}<div class="section-header"><header class="prof">${av(id, 40, "word")}<div class="grow"><h1 tabindex="-1">${esc(m.name)} <span class="role">${esc(m.role)}</span></h1>
    <p class="meta">${memberMetaFacts(id, m, owner)}</p>${nowLine}</div></header>${sectionTabs(nav, controls, actions, "profile-actions")}</div>${body}`;
}
function setupCard(id) {
  const s = M[id].setup;
  return `<div class="setup"><h2>Setup</h2><dl class="dl"><dt>Runtime</dt><dd>${esc(s.runtime)}</dd><dt>Model</dt><dd>${esc(s.model)}</dd><dt>How it runs</dt><dd>${esc(s.how)}</dd><dt>Office</dt><dd>${OFFICES[s.office] ? esc(OFFICES[s.office].name) + " · " + esc(OFFICES[s.office].machine) : "Not placed in an office yet"}</dd><dt>Home folder</dt><dd><code>${esc(s.home)}</code></dd></dl>
    <p style="margin-top:16px"><a class="link" href="#/settings/agents/${id}">Open in Agent setup${ic("chev-right", "ic16")}</a></p></div>`;
}

export {
  agentSetupFields,
  pageMember,
  pageMembersDir,
  pageOrgChart,
  pageTeam,
  pageTeams,
};
