import { IN, M, OPEN, P, W, blockedOnOwner } from "../data.js";
import { own, route } from "../shell/router.js";
import { S } from "../state.js";
import { acts, av, displayCheckbox, esc, feed, hash, ic, iconWithTooltip, mlink, plural, projMembers, queueHeadline, showMenu, toast, unhandled } from "../ui.js";

const ACTIVITY_STAGES = [
  ["queue", "Queue"], ["agents", "Agents"], ["ci", "CI"], ["review", "Human review"], ["merged", "Merged"]
];
const ASSEMBLY_FEATURES = {
  "agency-v1": [["Office sync & release", ["T-A121", "T-A140", "T-A142", "T-A146"]], ["UI quality & onboarding", ["T-A112", "T-A124", "T-A144", "T-A70", "T-A86", "T-A88"]]],
  "office-federation": [["Distributed file access", ["T-A118", "T-A135", "T-A81"]], ["Office enrollment", ["T-A138"]]],
  "look-feel": [["Navigation & presence", ["T-A130", "T-A131"]], ["Theme & accessibility", ["T-A133", "T-A76", "T-A84"]]]
};
const ACTIVITY_TRIAGE = { a2: "in1", a5: "in2", a6: "in5", a7: "in3", a13: "in4" };
const DELIVERY_HISTORY = {
  "agency-v1": [2, 4, 3, 2], "office-federation": [0, 2, 2, 1], "look-feel": [1, 2, 1, 1]
};
const RELEASE_GATES = { in1: "Release sign-off", in5: "Security review", in2: "File access ruling", in17: "Design sign-off" };
const ACTIVITY_PANELS = {
  release: { label: "Release gates", icon: "flag" },
  simulation: { label: "Simulation", icon: "clock" },
};
const ASSEMBLY_SPEEDS = [1, 3, 8];
const ASSEMBLY_DAY_HOURS = 24;
const ASSEMBLY_MOVES = {
  queue: ["agents", "started agent work"],
  agents: ["ci", "entered CI"],
  ci: ["review", "passed CI and entered review"],
  review: ["merged", "passed review and merged"],
};
const assemblySimulation = {
  hours: 0, run: 0, speed: 1, playing: true, timer: null, progress: new Map(), samples: [], events: [],
};
const assemblyDay = (hours) => Math.floor(hours / ASSEMBLY_DAY_HOURS);
const assemblyHours = new Intl.NumberFormat(undefined, { maximumFractionDigits: 2 });
const formatAssemblyHours = (hours) => assemblyHours.format(hours) + " h";

function projectActivityId([page, project, tab] = route()) {
  return page === "projects" && tab === "activity" && own(P, project) ? project : null;
}
const activityProject = () => projectActivityId() || S.actProject;
const activityViewKey = (project) => project ? "projectActView" : "actView";
const activityFilterKey = (project) => project ? "projectActFilter" : "actFilter";

function assemblyReview(work) {
  return IN.find((item) => !item.handled && item.task === work.id && item.proj === work.proj
    && (item.kind === "approval" || item.kind === "decision" || blockedOnOwner(work)));
}
function assemblyStage(work, review) {
  if (!OPEN(work)) return "merged";
  if (review || blockedOnOwner(work) || /review/i.test(work.status)) return "review";
  if (work.status === "In progress" && work.pipeline === "ci") return "ci";
  return work.who && work.status === "In progress" ? "agents" : "queue";
}
function assemblySource(work) {
  const signOff = IN.find((item) => item.task === work.id && item.proj === work.proj && item.sub === "Release sign-off");
  if (signOff?.handled) {
    return signOff.outcome === "Approved" ? { stage: "merged" } : { stage: "review", review: signOff };
  }
  const review = assemblyReview(work);
  return { stage: assemblyStage(work, review), review };
}
function assemblyWork(simulated) {
  const work = W.filter((item) => item.kind !== "request");
  return simulated ? [...work, ...assemblySimulation.samples] : work;
}

function assemblyCanAdvance(work, review, stage) {
  return stage !== "merged" && !review && work.status !== "Blocked";
}

function assemblyStageDuration(work, stage) {
  if (stage === "queue") return 1 + (hash(work.id) % 8) / 4;
  if (stage === "agents") return Math.max(1, work.etaHours ?? 3);
  return stage === "ci" ? 1 : 0.75;
}

function advanceAssemblySimulation() {
  const sim = assemblySimulation;
  const previous = sim.hours;
  sim.hours += sim.speed / 4;
  const events = [];
  const at = Date.now();
  let active = false;
  for (const work of assemblyWork(true)) {
    if (P[work.proj].archived) continue;
    const { stage: initial, review } = assemblySource(work);
    if (!assemblyCanAdvance(work, review, initial)) {
      sim.progress.delete(work.id);
      continue;
    }
    let progress = sim.progress.get(work.id);
    if (!progress || progress.initial !== initial) {
      progress = { initial, stage: initial, started: previous, entered: previous, due: previous + assemblyStageDuration(work, initial) };
      sim.progress.set(work.id, progress);
    }
    while (progress.stage !== "merged" && progress.due <= sim.hours) {
      const [stage, action] = ASSEMBLY_MOVES[progress.stage];
      events.push({ hour: progress.due, at, started: progress.started, work, stage, action });
      progress.stage = stage;
      progress.entered = progress.due;
      progress.due += assemblyStageDuration(work, stage);
    }
    active ||= progress.stage !== "merged";
  }
  if (events.length) {
    events.sort((a, b) => b.hour - a.hour);
    sim.events = [...events, ...sim.events];
  }
  if (events.length || projectActivityId() && assemblyDay(previous) !== assemblyDay(sim.hours)) {
    refreshAssemblySimulation(Boolean(events.length));
  }
  if (!active) sim.playing = false;
  updateAssemblyPlayback();
}

function assemblyProjects(simulated, scope = activityProject()) {
  const workItems = assemblyWork(simulated);
  return Object.entries(P).filter(([id, project]) => scope === "all" ? !project.archived : scope === id).map(([id, project]) => {
    const entries = workItems.filter((work) => work.proj === id).map((work) => {
      const { stage: initial, review } = assemblySource(work);
      const progress = simulated ? assemblySimulation.progress.get(work.id) : null;
      const stage = progress?.initial === initial && assemblyCanAdvance(work, review, initial) ? progress.stage : initial;
      return { work, review, stage };
    });
    const groups = ASSEMBLY_FEATURES[id] || [];
    const used = new Set(groups.flatMap(([, ids]) => ids));
    const features = [
      ...groups.map(([label, ids]) => ({ label, entries: entries.filter(({ work }) => ids.includes(work.id)) })),
      { label: "Other work", entries: entries.filter(({ work }) => !used.has(work.id)) }
    ].filter((group) => group.entries.length);
    return { id, project, entries, features };
  });
}
function assemblySnapshot(simulated, scope) {
  const projects = assemblyProjects(simulated, scope);
  const entries = projects.flatMap((project) => project.entries);
  const counts = Object.fromEntries(ACTIVITY_STAGES.map(([stage]) =>
    [stage, entries.filter((entry) => entry.stage === stage).length]));
  return { projects, entries, counts };
}
const assemblyMerged = (entries) => entries.filter((entry) => entry.stage === "merged").length;
const assemblyPct = (entries) => entries.length ? Math.round(100 * assemblyMerged(entries) / entries.length) : 0;
function assemblyProgressBar(done, total, label) {
  return `<progress class="assembly-progress-bar" value="${done}" max="${total || 1}" aria-label="${esc(label)}"></progress>`;
}
function assemblyProgress(entries, label, features) {
  const done = assemblyMerged(entries), pct = assemblyPct(entries);
  const detail = features
    ? ` tasks · ${features.filter((feature) => assemblyMerged(feature.entries) === feature.entries.length).length}/${features.length} features`
    : "";
  const complete = features && entries.length > 0 && done === entries.length ? " · Complete" : "";
  return `<div class="assembly-progress">${assemblyProgressBar(done, entries.length, label)}<span>${pct}% · ${done}/${entries.length}${detail}${complete}</span></div>`;
}
function assemblyChip(entry) {
  const { work, review, stage } = entry;
  const label = stage === "agents" && work.who ? work.id + " · " + M[work.who].name : work.id;
  const title = esc(work.id + " · " + work.title + (review?.handled ? " · " + review.outcome : ""));
  const chip = `class="assembly-chip" data-stage="${stage}" data-assembly-work="${esc(work.id)}" title="${title}"`;
  if (stage === "review" && review && !review.handled) return `<button ${chip} type="button" data-act="activity-review" data-id="${review.id}" aria-label="Review ${esc(work.id)}">${ic("flag", "ic14")}${esc(label)}</button>`;
  if (work.sample) return `<span ${chip}>${esc(label)}</span>`;
  return `<a ${chip} href="#/projects/${work.proj}/work?item=${encodeURIComponent(work.id)}">${esc(label)}</a>`;
}
function releaseGates(projectIds) {
  return IN.filter((item) => own(RELEASE_GATES, item.id) && projectIds.includes(item.proj));
}
const releaseGateBlocked = (outcome) => /^(denied|declined|dismissed|changes requested)\b/i.test(outcome);
function releaseGateState(item) {
  if (!item.handled) return "pending";
  return releaseGateBlocked(item.outcome) ? "blocked" : "cleared";
}
const releaseReadinessLabel = (entries, merged) => `Readiness ${assemblyPct(entries)}% · ${merged}/${entries.length} tasks merged`;
function projectReleaseSummary(id) {
  const release = P[id].release;
  if (!release) return "";
  const { entries, counts: { merged } } = assemblySnapshot(true, id);
  const gates = releaseGates([id]);
  const gateStates = { pending: ["clock", "Pending"], blocked: ["deny", "Blocked"], cleared: ["check", "Cleared"] };
  return `<section class="assembly-release" id="project-release-summary" aria-label="Release summary">
    <div class="assembly-release-overview">
      <div><p class="assembly-release-label">Release</p><p class="assembly-release-name">${esc(release)}</p></div>
      <div class="assembly-release-readiness">
        <p class="assembly-release-label" id="project-release-readiness">${releaseReadinessLabel(entries, merged)}</p>
        ${assemblyProgressBar(merged, entries.length, "Release readiness")}
      </div>
    </div>
    ${gates.length ? `<ul class="assembly-release-gates" aria-label="Release gates">${gates.map((item) => {
      const state = releaseGateState(item);
      const [icon, status] = gateStates[state];
      return `<li><a href="#/inbox/${item.id}" data-gate-state="${state}" title="${status}">
        ${ic(icon, "ic14")}${esc(RELEASE_GATES[item.id])}<span class="sr-only"> · ${status}</span>
      </a></li>`;
    }).join("")}</ul>` : ""}
  </section>`;
}
function assemblyProjectBodyHtml({ id, project, entries: projectEntries, features }) {
  const milestoneTitle = esc(project.milestoneTitle || project.name) + (project.release ? " · " + esc(project.release) : "");
  const rows = features.length ? features.map((feature) => `<tr>
          <th scope="row">${esc(feature.label)}<small>${plural(feature.entries.length, "task", "tasks")}</small></th>
          ${ACTIVITY_STAGES.map(([stage]) => {
            const cells = feature.entries.filter((entry) => entry.stage === stage);
            return cells.length ? `<td><div class="assembly-cell">${cells.map(assemblyChip).join("")}</div></td>` : '<td class="assembly-dash">—</td>';
          }).join("")}<td>${assemblyProgress(feature.entries, feature.label + " progress")}</td>
        </tr>`).join("") : '<tr><td colspan="7" class="assembly-dash">No tasks on this assembly line.</td></tr>';
  return `<tbody>
        <tr class="assembly-milestone"><th colspan="7" scope="rowgroup"><div><a class="alink" href="#/projects/${id}">${milestoneTitle}</a>${assemblyProgress(projectEntries, project.name + " progress", features)}</div></th></tr>
        ${rows}
      </tbody>`;
}
function assemblyMatrix(projects, entries, counts) {
  const project = projects.length === 1 ? projects[0].project : null;
  const projectId = projectActivityId();
  const scopeLabel = project ? project.release : "All projects";
  const title = projectId ? "Assembly line" : project?.name || "Portfolio Assembly";
  const featureCount = projects[0]?.features.length ?? 0;
  const cycleTime = projectId ? averageAssemblyCycle(assemblyLineEvents().filter(({ stage }) => stage === "merged")) : null;
  const headings = [["Milestone › Feature", projectId ? plural(featureCount, "feature", "features") : plural(projects.length, "project", "projects")],
    ...ACTIVITY_STAGES.map(([stage, label]) => [label, String(counts[stage])]), ["Into release", "Tasks merged"]];
  const busyAgents = new Set(entries.filter(({ stage, work }) => stage === "agents" && M[work.who]?.kind === "agent").map(({ work }) => work.who)).size;
  const cycleMetric = projectId ? `<div><dt title="Time from the first simulated stage to merge">Avg. cycle time</dt><dd data-assembly-cycle>${cycleTime === null ? "—" : formatTelemetryHours(cycleTime)}</dd></div>` : "";
  return `<section class="assembly-view" aria-label="Assembly line">
    <div class="assembly-summary">
      <div>${scopeLabel ? `<p class="assembly-eyebrow">${esc(scopeLabel)}</p>` : ""}<h2>${esc(title)}</h2></div>
      <dl class="assembly-metrics"><div><dt>Readiness</dt><dd>${assemblyPct(entries)}%${assemblyProgressBar(counts.merged, entries.length, projectId ? "Project readiness" : "Portfolio readiness")}</dd></div>
      <div><dt>Merged</dt><dd>${counts.merged} / ${entries.length}</dd></div>
      <div><dt>Agents busy</dt><dd>${busyAgents}</dd></div>
      <div><dt>Needs review</dt><dd class="assembly-review-text">${counts.review}</dd></div>
      ${cycleMetric}</dl>
    </div>
    <div class="assembly-table-wrap" tabindex="0" role="region" aria-label="Delivery stages, scroll horizontally on smaller screens">
      <table class="assembly-table"><thead><tr>${headings.map(([label, sub]) => `<th scope="col">${label}<small>${sub}</small></th>`).join("")}</tr></thead>
      ${projects.map(assemblyProjectBodyHtml).join("")}</table>
    </div>
    <div class="assembly-legend" aria-label="Delivery stages">${ACTIVITY_STAGES.map(([stage, label]) => `<span data-stage="${stage}"><i aria-hidden="true"></i>${label}</span>`).join("")}
      ${projectId ? "" : assemblyProgress(entries, "Release readiness")}</div>
  </section>`;
}
function activityThroughput(projects) {
  const days = ["3 days ago", "2 days ago", "Yesterday", "Today"];
  const totals = days.map((_, day) => projects.reduce((total, { id }) => total + (DELIVERY_HISTORY[id]?.[day] || 0), 0));
  const peak = Math.max(1, ...totals);
  return `<section><h3>Throughput</h3><p class="activity-rail-note">Tasks merged over the last four days</p><ul class="activity-throughput">${totals.map((count, index) => `
    <li><span>${count}</span><i style="height:${Math.max(2, count * 36 / peak)}px" aria-hidden="true"></i><span class="sr-only">${days[index]}</span></li>`).join("")}</ul></section>`;
}
function assemblyLineEvents() {
  const scope = activityProject();
  return assemblySimulation.events.filter(({ work }) => scope === "all" || work.proj === scope);
}

function assemblyThroughput() {
  const merged = assemblyLineEvents().filter(({ stage }) => stage === "merged").length;
  const rate = assemblySimulation.hours ? merged / assemblySimulation.hours : 0;
  return { merged, rate: assemblyHours.format(rate) };
}

function assemblyEventLog() {
  const events = assemblyLineEvents().slice(0, 30);
  return `<section class="assembly-events" aria-labelledby="assembly-events-title">
    <h3 id="assembly-events-title">Line events</h3>
    ${events.length ? `<ol class="assembly-event-log" tabindex="0" aria-labelledby="assembly-events-title">${events.map(({ hour, work, stage, action }) => `
      <li data-stage="${stage}"><span><strong>${esc(work.id)}</strong> ${action}</span><time datetime="PT${hour}H">${formatAssemblyHours(hour)}</time></li>`).join("")}</ol>`
      : '<p class="activity-rail-note">Events appear as tasks move through the line.</p>'}
  </section>`;
}

const telemetryHours = new Intl.NumberFormat(undefined, { minimumFractionDigits: 1, maximumFractionDigits: 1 });
const formatTelemetryHours = (hours) => telemetryHours.format(hours) + " h";
function averageAssemblyCycle(merges) {
  return merges.length ? merges.reduce((sum, { hour, started }) => sum + hour - started, 0) / merges.length : null;
}

function projectThroughput(id, events, waiting, gates) {
  const history = DELIVERY_HISTORY[id] || [];
  const sim = assemblySimulation;
  const day = assemblyDay(sim.hours);
  const first = Math.max(Math.min(0, 1 - history.length), day - 7);
  const merges = events.filter(({ stage }) => stage === "merged");
  const signOffs = gates.filter((item) => item.sub === "Release sign-off" && item.outcome === "Approved")
    .map((item) => currentAssemblyHour(item.activity?.at(-1)));
  const mergeTimes = merges.map(({ hour }) => hour).concat(signOffs);
  const days = Array.from({ length: day - first + 1 }, (_, index) => {
    const offset = first + index;
    const label = offset < 0 ? `${Math.abs(offset)}d ago` : `D${offset + 1}`;
    const count = (history[history.length - 1 + offset] ?? 0) + mergeTimes.filter((hour) => assemblyDay(hour) === offset).length;
    return { label, count };
  });
  const peak = Math.max(4, ...days.map(({ count }) => count));
  const cycle = averageAssemblyCycle(merges);
  const now = new Date();
  const waitingSince = waiting.filter(({ review }) => !review?.handled).map(({ work, review }) => {
    const age = review && activityEventTime(review.time, now).age;
    if (Number.isFinite(age)) {
      const reopened = review.activity?.findLast(({ icon }) => icon === "refresh");
      return reopened ? currentAssemblyHour(reopened) : -age / 60;
    }
    return assemblySimulation.progress.get(work.id)?.entered ?? 0;
  });
  const waitOffset = waitingSince.reduce((sum, hour) => sum - hour, 0);
  const wait = Math.max(0, sim.hours * waitingSince.length + waitOffset);
  return `<section class="project-telemetry" aria-labelledby="project-throughput-title">
    <div class="project-telemetry-header"><h3 id="project-throughput-title">Throughput</h3><p class="activity-rail-note">Tasks merged per day</p></div>
    <ul class="project-throughput" role="list" aria-label="Daily merge counts">${days.map(({ label, count }) => `<li>
      <div class="project-throughput-plot" aria-hidden="true"><div style="height:${count * 100 / peak}%">${count ? `<span>${count}</span>` : ""}</div></div>
      <span>${label}</span><span class="sr-only"> · ${plural(count, "task", "tasks")} merged</span>
    </li>`).join("")}</ul>
    <dl class="project-throughput-metrics">
      <div><dt title="Time from the first simulated stage to merge">Avg. cycle time</dt><dd>${cycle === null ? "—" : formatTelemetryHours(cycle)}</dd></div>
      <div><dt title="Total time across tasks currently waiting for review">Waiting on people now</dt><dd id="project-human-wait" data-count="${waitingSince.length}" data-offset="${waitOffset}">${formatTelemetryHours(wait)}</dd></div>
    </dl>
  </section>`;
}

function currentAssemblyHour(event) {
  return event?.assemblyRun === assemblySimulation.run ? event.assemblyHour : 0;
}

function projectLineLog(gates, events) {
  const now = new Date();
  const requests = gates.flatMap((item) => {
    const label = RELEASE_GATES[item.id];
    const age = activityEventTime(item.time, now).age;
    return [{ at: now.getTime() - age * 60_000, time: item.time, tone: "warn", text: `${label} requested` },
      ...(item.activity || []).filter(({ at }) => Number.isFinite(at)).map(({ at, icon, text }) => ({
        at, time: new Date(at).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" }),
        tone: icon === "refresh" ? "warn" : releaseGateBlocked(text) ? "bad" : "ok",
        text: `${label} · ${text}`,
      }))];
  });
  const moves = events.map(({ hour, at, work, action, stage }) => {
    const minutes = Math.floor(hour * 60) % (ASSEMBLY_DAY_HOURS * 60);
    const clock = [Math.floor(minutes / 60), minutes % 60].map((value) => String(value).padStart(2, "0")).join(":");
    return { at, time: `D${assemblyDay(hour) + 1} · ${clock}`, tone: stage === "merged" ? "ok" : stage === "review" ? "warn" : "", text: `${work.id} ${action}` };
  });
  const log = [...moves, ...requests].sort((left, right) => right.at - left.at).slice(0, 30);
  return `<section class="project-telemetry" aria-labelledby="project-line-events-title"><h3 id="project-line-events-title">Line events</h3>
    ${log.length ? `<ol class="assembly-event-log project-line-log" role="list" tabindex="0" aria-label="Line events">${log.map(({ time, tone, text }) => `
      <li data-tone="${tone}"><span class="project-line-time">${esc(time)}</span><span>${esc(text)}</span></li>`).join("")}</ol>`
      : '<p class="activity-rail-note">Events appear as tasks and gates move through the line.</p>'}
  </section>`;
}

function releaseOperations({ projects, entries, counts }) {
  const projectId = projectActivityId();
  const gates = releaseGates(projects.map(({ id }) => id));
  const gateList = `<ul>${gates.map((item) => {
    const tone = !item.handled ? "assembly-review-text"
      : !projectId ? "muted" : releaseGateState(item) === "blocked" ? "tone-bad" : "tone-ok";
    return `<li><a href="#/inbox/${item.id}">${esc(RELEASE_GATES[item.id])}</a><span class="${tone}">${item.handled ? esc(item.outcome) : "Pending"}</span></li>`;
  }).join("") || "<li>No gates in this scope</li>"}</ul>`;
  if (projectId) return projectReleaseOperations(projects[0], counts, gates, gateList);
  const agents = [...new Set(entries.filter((entry) => ["agents", "ci", "review"].includes(entry.stage) && entry.work.who)
    .map((entry) => entry.work.who))].filter((id) => M[id].kind === "agent");
  return `<div class="activity-rail">
    <section aria-label="Gate status">${gateList}</section>
    <section><h3>Stations</h3><dl>${ACTIVITY_STAGES.map(([stage, label]) => `<div><dt>${label}</dt><dd>${counts[stage]}</dd></div>`).join("")}</dl></section>
    ${activityThroughput(projects)}
    <section><h3>Active agents</h3><ul class="activity-agents">${agents.map((agent) => {
      const entry = entries.find(({ work, stage }) => work.who === agent && ["agents", "ci", "review"].includes(stage));
      return `<li><div>${av(agent, 24)}${mlink(agent)}</div><span class="muted">${entry.work.id} · ${ACTIVITY_STAGES.find(([stage]) => stage === entry.stage)[1]}</span></li>`;
    }).join("") || "<li>No agents at these stations</li>"}</ul></section>
  </div>`;
}

function projectReleaseOperations({ id, project, features, entries }, counts, gates, gateList) {
  const agents = projMembers(id).filter((member) => M[member]?.kind === "agent" && !M[member].offboarded);
  const busy = new Set(entries.filter(({ stage }) => stage === "agents").map(({ work }) => work.who));
  const agentCount = agents.filter((member) => busy.has(member)).length;
  const stationCounts = {
    queue: `${counts.queue} waiting`,
    agents: `${agentCount} / ${agents.length} busy`,
    ci: `${counts.ci} checking`,
    review: `${counts.review} waiting`,
    merged: `${counts.merged} of ${entries.length}`,
  };
  const waiting = entries.filter(({ stage }) => stage === "review");
  const events = assemblyLineEvents();
  return `<div class="activity-rail project-release-rail">
    <section aria-labelledby="project-stations-title"><h3 id="project-stations-title">Stations</h3>
      <dl>${ACTIVITY_STAGES.map(([stage, label]) => `<div><dt>${label}</dt><dd>
        ${stage === "agents" ? `<span class="project-station-slots" aria-hidden="true">${agents.map((member) => `<i${busy.has(member) ? " data-busy" : ""}></i>`).join("")}</span>` : ""}
        <span>${stationCounts[stage]}</span>
      </dd></div>`).join("")}</dl>
    </section>
    <section aria-labelledby="project-milestones-title"><h3 id="project-milestones-title">Milestones</h3>
      ${project.milestoneTitle ? `<p class="project-milestone-name">${esc(project.milestoneTitle)}</p>${assemblyProgress(entries, project.milestoneTitle + " progress", features)}` : '<p class="activity-rail-note">No release milestone.</p>'}
    </section>
    <section aria-labelledby="project-human-title"><h3 id="project-human-title">Needs a human <span>${waiting.length}</span></h3>
      ${waiting.length ? `<ul>${waiting.map(({ work, review }) => `<li><a href="${review && !review.handled ? "#/inbox/" + review.id : "#/projects/" + id + "/work?item=" + encodeURIComponent(work.id)}">${esc(work.title)}</a></li>`).join("")}</ul>`
        : '<p class="activity-rail-note">Nothing is waiting on people.</p>'}
    </section>
    <section aria-labelledby="project-gate-status-title"><h3 id="project-gate-status-title">Gate status</h3>${gateList}</section>
    ${projectThroughput(id, events, waiting, gates)}
    ${projectLineLog(gates, events)}
  </div>`;
}

function simulationDetails() {
  const { merged, rate } = assemblyThroughput();
  return `<div class="activity-rail">
    ${assemblyEventLog()}
    <section><h3>Simulation throughput</h3><dl><div><dt>Merged this run</dt><dd>${merged}</dd></div><div><dt>Tasks per hour</dt><dd id="assembly-rate">${rate}</dd></div></dl></section>
  </div>`;
}

function updateActivityPanelScroll() {
  const more = document.getElementById("activity-panel-more");
  if (!more) return;
  const scroll = document.getElementById("activity-panel-scroll");
  const remaining = scroll.scrollHeight - scroll.clientHeight - scroll.scrollTop;
  const show = S.actPanel === "release" && scroll.clientHeight > 0 && remaining > 1;
  if (!show && document.activeElement === more) scroll.focus({ preventScroll: true });
  more.hidden = !show;
}
const activityPanelResize = new ResizeObserver(updateActivityPanelScroll);

function refreshActivityPanel(snapshot) {
  const panel = document.getElementById("activity-panel");
  if (!panel || !S.actPanel) return;
  const body = panel.querySelector(".activity-panel-body");
  const scroller = document.getElementById("activity-panel-scroll");
  const scroll = scroller.scrollTop;
  const log = body.querySelector(".assembly-event-log");
  const logScroll = log?.scrollTop ?? 0;
  const active = document.activeElement;
  const href = body.contains(active) && active.getAttribute("href");
  const showSimulation = Boolean(projectActivityId()) || S.actView === "assembly";
  body.innerHTML = S.actPanel === "simulation"
    ? simulationDetails()
    : releaseOperations(snapshot ?? assemblySnapshot(showSimulation));
  const nextLog = body.querySelector(".assembly-event-log");
  if (nextLog) nextLog.scrollTop = logScroll;
  const focus = href ? body.querySelector(`a[href="${CSS.escape(href)}"]`) : active === log ? nextLog : null;
  if (body.contains(focus)) focus.focus({ preventScroll: true });
  else if (!active.isConnected) panel.querySelector(".side-panel-header button").focus({ preventScroll: true });
  scroller.scrollTop = scroll;
  updateActivityPanelScroll();
}

function setActivityPanel(next) {
  const previous = S.actPanel;
  S.actPanel = next;
  const panel = document.getElementById("activity-panel");
  const open = Boolean(next);
  panel.parentElement.toggleAttribute("data-panel-open", open);
  panel.inert = !open;
  for (const button of document.querySelectorAll(".activity-tool")) {
    button.setAttribute("aria-expanded", String(button.dataset.v === next));
  }
  if (open) {
    const title = document.getElementById("activity-panel-title");
    title.textContent = ACTIVITY_PANELS[next].label;
    panel.querySelector(".activity-panel-controls").innerHTML = next === "simulation" ? assemblyPlayback() : "";
    refreshActivityPanel();
    document.getElementById("activity-panel-scroll").scrollTop = 0;
    title.focus({ preventScroll: true });
  } else {
    document.querySelector(`.activity-tool[data-v="${previous}"]`)?.focus({ preventScroll: true });
  }
  updateActivityPanelScroll();
}
function activityNeedsFeed(items, waiting) {
  return waiting.map((item) => items.find((entry) => entry.inbox === item.id) || {
    id: "activity-" + item.id, inbox: item.id, type: item.kind === "question" ? "conversation" : "decision",
    who: item.from, time: item.time, proj: item.proj, text: item.what
  });
}
function activityEventTime(time, now) {
  const relative = time.match(/^(\d+)([mh])$/);
  if (relative) {
    return { day: "Today", age: Number(relative[1]) * (relative[2] === "h" ? 60 : 1) };
  }
  if (time === "Yesterday") return { day: time, age: 24 * 60 };
  const date = new Date(`${time} ${now.getUTCFullYear()} 00:00:00 GMT`);
  if (date > now) date.setUTCFullYear(date.getUTCFullYear() - 1);
  return { day: time, age: Number.isNaN(date.getTime()) ? Infinity : (now - date) / 60_000 };
}

function activityTimeline(items, options = {}) {
  if (!items.length) return feed(items);
  const now = new Date(), groups = new Map();
  const events = items.map((event) => ({ event, ...activityEventTime(event.time, now) }))
    .sort((left, right) => left.age - right.age);
  for (const { day, event } of events) {
    if (!groups.has(day)) groups.set(day, []);
    groups.get(day).push(event);
  }
  return [...groups].map(([day, events]) => `<section class="activity-day">
    <h2>${esc(day)}</h2>${feed(events, { triage: true, ...options, day })}
  </section>`).join("");
}

function assemblyPlayback() {
  const { playing, hours, speed } = assemblySimulation;
  const playLabel = playing ? "Pause simulation" : "Play simulation";
  return `<div class="assembly-playback" id="assembly-playback" role="group" aria-label="Assembly simulation">
    <div class="assembly-clock">
      <span class="assembly-play-state" id="assembly-play-state" data-playing="${playing}" role="status">${playing ? "Running" : "Paused"}</span>
      <span>Elapsed <strong id="assembly-clock">${formatAssemblyHours(hours)}</strong></span>
    </div>
    <div class="assembly-controls">
      <button class="btn outline icon icon-with-tooltip" id="assembly-play" type="button" data-act="assembly-play" aria-label="${playLabel}">${iconWithTooltip(playLabel, playing ? "pause" : "play")}</button>
      <div class="seg" role="group" aria-label="Simulation speed">${ASSEMBLY_SPEEDS.map((rate) => `<button type="button" data-act="assembly-speed" data-v="${rate}" aria-pressed="${rate === speed}">${rate}×</button>`).join("")}</div>
      <button class="btn ghost icon icon-with-tooltip" type="button" data-act="assembly-restart" aria-label="Reset simulation">${iconWithTooltip("Reset simulation", "refresh")}</button>
      <button class="btn ghost icon icon-with-tooltip" type="button" data-act="assembly-add-sample" aria-label="Add sample task">${iconWithTooltip("Add sample task", "plus")}</button>
    </div>
  </div>`;
}

function updateAssemblyPlayback() {
  const sim = assemblySimulation;
  const clock = document.getElementById("assembly-clock");
  if (clock) {
    clock.textContent = formatAssemblyHours(sim.hours);
    const state = document.getElementById("assembly-play-state");
    if (state.dataset.playing !== String(sim.playing)) {
      state.dataset.playing = String(sim.playing);
      state.textContent = sim.playing ? "Running" : "Paused";
      const play = document.getElementById("assembly-play");
      const label = sim.playing ? "Pause simulation" : "Play simulation";
      play.setAttribute("aria-label", label);
      play.innerHTML = iconWithTooltip(label, sim.playing ? "pause" : "play");
    }
  }
  const rate = document.getElementById("assembly-rate");
  if (rate) rate.textContent = assemblyThroughput().rate;
  const wait = document.getElementById("project-human-wait");
  if (wait) wait.textContent = formatTelemetryHours(Math.max(0, sim.hours * Number(wait.dataset.count) + Number(wait.dataset.offset)));
  syncAssemblySimulationTimer();
}

function syncAssemblySimulationTimer() {
  const sim = assemblySimulation;
  const running = sim.playing && !document.hidden && document.querySelector(".activity-workspace");
  if (running && sim.timer === null) sim.timer = setInterval(advanceAssemblySimulation, 250);
  if (!running && sim.timer !== null) {
    clearInterval(sim.timer);
    sim.timer = null;
  }
}

function assemblyLayout({ projects, entries, counts } = assemblySnapshot(true)) {
  return `<div class="assembly-layout" id="assembly-layout">
    ${assemblyMatrix(projects, entries, counts)}
  </div>`;
}

function animateAssemblyCards(layout, previousPositions) {
  for (const card of layout.querySelectorAll("[data-assembly-work]")) {
    const previous = previousPositions.get(card.dataset.assemblyWork);
    if (!previous) continue;
    const current = card.getBoundingClientRect();
    const x = previous.left - current.left, y = previous.top - current.top;
    if (Math.abs(x) + Math.abs(y) < 1) continue;
    card.animate([
      { transform: `translate(${x}px, ${y}px)`, zIndex: 1 },
      { transform: "translate(0, 0)", zIndex: 1 }
    ], { duration: 230, easing: "cubic-bezier(.2, .7, .25, 1)" });
  }
}

function syncReleaseSummaryProgress(snapshot) {
  const readiness = document.querySelector("#project-release-summary progress");
  if (!readiness || !snapshot) return;
  const { entries, counts: { merged } } = snapshot;
  readiness.max = entries.length || 1;
  readiness.value = merged;
  document.getElementById("project-release-readiness").textContent = releaseReadinessLabel(entries, merged);
}

function captureAssemblyFocusSelector(active) {
  const workId = active?.dataset.assemblyWork;
  if (workId) return `[data-assembly-work="${CSS.escape(workId)}"]`;
  const href = active?.getAttribute("href");
  return href ? `a[href="${CSS.escape(href)}"]` : ".assembly-table-wrap";
}

function refreshAssemblySimulation(animate = false) {
  const layout = document.getElementById("assembly-layout");
  const projectId = projectActivityId();
  const snapshot = layout || projectId ? assemblySnapshot(true) : null;
  syncReleaseSummaryProgress(snapshot);
  if (!layout) {
    if (S.actPanel === "simulation" || projectId) refreshActivityPanel(snapshot);
    return;
  }
  const tableScroll = layout.querySelector(".assembly-table-wrap").scrollLeft;
  const content = layout.closest(".activity-main"), pageScroll = content.scrollTop;
  const active = document.activeElement, focus = layout.contains(active);
  const focusSelector = captureAssemblyFocusSelector(active);
  const shouldAnimate = animate && matchMedia("(prefers-reduced-motion: no-preference)").matches;
  const positions = shouldAnimate ? new Map([...layout.querySelectorAll("[data-assembly-work]")].map((card) => [card.dataset.assemblyWork, card.getBoundingClientRect()])) : null;
  layout.outerHTML = assemblyLayout(snapshot);
  const next = document.getElementById("assembly-layout"), table = next.querySelector(".assembly-table-wrap");
  table.scrollLeft = tableScroll;
  if (focus) (next.querySelector(focusSelector) || table).focus({ preventScroll: true });
  refreshActivityPanel(snapshot);
  content.scrollTop = pageScroll;
  if (positions) animateAssemblyCards(next, positions);
}

function addAssemblySample() {
  const sim = assemblySimulation;
  const scope = activityProject();
  const proj = scope === "all" ? "agency-v1" : scope;
  const who = projMembers(proj).find((id) => M[id]?.kind === "agent");
  const work = { id: "T-S" + (sim.samples.length + 1), title: "Verify release readiness",
    sample: true, kind: "task", status: "To do", proj, who, etaHours: 2 };
  sim.samples.push(work);
  sim.events.unshift({ hour: sim.hours, at: Date.now(), work, stage: "queue", action: "joined the queue" });
  refreshAssemblySimulation();
  toast(`${work.id} added to the simulation.`, "check-circle");
}

function activityViewTabs(viewKey) {
  const views = [["assembly", "Assembly line", "board"], ["feed", "Activity feed", "signal"]];
  return `<div class="queue-tabs" role="tablist" aria-label="Activity view">${views.map(([id, label, icon]) => {
    const selected = S[viewKey] === id;
    return `<button class="queue-tab" type="button" role="tab" id="activity-tab-${id}" data-act="set" data-k="${viewKey}" data-v="${id}" data-menu-label="${esc(label)}"
      aria-selected="${selected}" tabindex="${selected ? 0 : -1}" aria-controls="activity-view">${ic(icon, "ic16")}${label}</button>`;
  }).join("")}</div>`;
}

function activityFilterMenu(isFeed, waitingCount, projectId) {
  const projects = [["all", "All projects"], ...Object.entries(P)
    .filter(([, project]) => !project.archived).map(([id, project]) => [id, project.name + " · " + project.release])];
  const types = [["all", "All events"], ["needs", "Needs you", waitingCount], ["work", "Work"], ["conversation", "Conversations"],
    ["decision", "Decisions"], ["run", "Runs"], ["org", "Org changes"]];
  const groups = projectId ? [] : [["actProject", "Project", projects]];
  const filterKey = activityFilterKey(projectId);
  if (isFeed) groups.push([filterKey, "Event type", types]);
  return showMenu("activity-filter", groups, {
    icon: "filter", label: "Filter activity", focusTooltip: true,
    active: (!projectId && S.actProject !== "all") || (isFeed && S[filterKey] !== "all"),
  });
}

function activityViewControls(projectId, waitingCount = unhandled().filter((item) => item.proj === projectId).length) {
  const viewKey = activityViewKey(projectId);
  const isFeed = S[viewKey] === "feed";
  const filter = !projectId || isFeed ? activityFilterMenu(isFeed, waitingCount, projectId) : "";
  const display = isFeed ? "" : showMenu("assembly-display", [], {
    icon: "adjustments", label: "Assembly line display", focusTooltip: true, active: S.assemblyDividers,
    footer: displayCheckbox("assembly-dividers", "Row dividers", S.assemblyDividers),
  });
  const tabs = activityViewTabs(viewKey);
  return `${projectId ? tabs : `<div class="primary-tab-row" data-page-menu>${tabs}</div>`}
    <div class="queue-controls" role="group" aria-label="Activity view settings">${filter}${display}</div>`;
}

function activitySidebarPanelHtml(projectId, panels) {
  const tools = panels.map(([id, { label, icon }]) => `<button class="btn ghost icon icon-with-tooltip activity-tool" type="button" data-act="activity-panel" data-v="${id}" aria-label="${label}" aria-expanded="${S.actPanel === id}" aria-controls="activity-panel">${iconWithTooltip(label, icon, "ic20")}</button>`).join("");
  const moreBtn = projectId ? `<button class="activity-panel-more" id="activity-panel-more" type="button" data-act="activity-panel-more" aria-controls="activity-panel-scroll" hidden>Scroll for more${ic("chev-down", "ic16")}</button>` : "";
  return `<div class="activity-tools" role="group" aria-label="Activity sidebar tools">${tools}</div>
      <aside class="activity-panel" id="activity-panel" aria-labelledby="activity-panel-title"${S.actPanel ? "" : " inert"}>
        <div class="activity-panel-scroll" id="activity-panel-scroll" role="region" aria-label="Activity sidebar contents" tabindex="0">
          <header class="side-panel-header">
            <h2 id="activity-panel-title" tabindex="-1">${ACTIVITY_PANELS[S.actPanel]?.label || ""}</h2>
            <button class="btn ghost icon icon-with-tooltip" type="button" data-act="activity-panel-close" aria-label="Collapse Activity sidebar">${iconWithTooltip("Collapse Activity sidebar", "chev-right")}</button>
          </header>
          <div class="activity-panel-controls">${S.actPanel === "simulation" ? assemblyPlayback() : ""}</div>
          <div class="activity-panel-body"></div>
        </div>
        ${moreBtn}
      </aside>`;
}

function pageActivity(projectId = null) {
  const scope = activityProject();
  const inScope = (record) => scope === "all" || record.proj === scope;
  const all = acts().filter(inScope).map((event) => ({ ...event, inbox: ACTIVITY_TRIAGE[event.id] }));
  const waiting = unhandled().filter(inScope);
  const filter = S[activityFilterKey(projectId)];
  const items = filter === "needs" ? activityNeedsFeed(all, waiting) : all.filter((event) => filter === "all" || event.type === filter);
  const view = S[activityViewKey(projectId)], isFeed = view === "feed";
  const panels = projectId && P[projectId].archived ? [["release", ACTIVITY_PANELS.release]] : Object.entries(ACTIVITY_PANELS);
  const count = `${plural(all.length, "event", "events")} · ${plural(all.filter((event) => event.type === "run").length, "run", "runs")}`;
  const header = projectId ? projectReleaseSummary(projectId) : `<header class="activity-header queue-header">
      ${queueHeadline("Activity", count, true)}
      <div class="activity-toolbar queue-toolbar">${activityViewControls(null, waiting.length)}</div>
    </header>`;
  return `<section class="activity-workspace" data-activity-scope="${esc(projectId || "portfolio")}" aria-label="${projectId ? "Project activity" : "Activity command center"}">
    ${header}
    <div class="activity-body"${S.actPanel ? " data-panel-open" : ""}>
      <div class="activity-main" id="activity-view" role="tabpanel" aria-labelledby="activity-tab-${view}" tabindex="0" data-view="${view}">
        ${isFeed ? activityFeedLayout(items, Boolean(projectId)) : assemblyLayout()}
      </div>
      ${activitySidebarPanelHtml(projectId, panels)}
    </div>
  </section>`;
}

function activityFeedLayout(items, noProj) {
  const { counts } = assemblySnapshot(false);
  return `<div class="activity-layout">
        <section class="activity-feed" aria-label="Activity feed">
          <div class="activity-bottleneck"><span>Work by stage</span><div class="activity-stage-bar" aria-hidden="true">${ACTIVITY_STAGES.map(([stage]) => `<i data-stage="${stage}" style="flex-grow:${counts[stage]}"></i>`).join("")}</div><span>${counts.review} in review · ${counts.queue} queued</span></div>
          ${activityTimeline(items, { noProj })}</section>
    </div>`;
}

export {
  ACTIVITY_STAGES,
  RELEASE_GATES,
  activityEventTime,
  activityFilterKey,
  activityPanelResize,
  activityTimeline,
  activityViewControls,
  activityViewKey,
  addAssemblySample,
  assemblyMerged,
  assemblyPct,
  assemblyProgressBar,
  assemblySimulation,
  assemblySnapshot,
  formatTelemetryHours,
  pageActivity,
  projectActivityId,
  projectReleaseSummary,
  refreshActivityPanel,
  refreshAssemblySimulation,
  releaseGateState,
  releaseGates,
  setActivityPanel,
  syncAssemblySimulationTimer,
  updateActivityPanelScroll,
  updateAssemblyPlayback,
};
