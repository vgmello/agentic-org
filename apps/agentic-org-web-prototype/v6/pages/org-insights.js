/* Organization Insights (#/insights): how the whole organization is doing across projects and teams.
   Read top to bottom it answers, in order: how much got done and how fast, whether quality and autonomy
   hold, where the time goes, what it cost, and which project or team to look at. Same period control,
   chart/table view and CSV export as a project's Insights tab (doc-51 §1 "Insights (nav, org-wide)"). */
import { P, TEAMS } from "../data.js";
import {
  INSIGHT_COMPLETIONS, INSIGHT_DAY, INSIGHT_GROUPS, INSIGHT_TODAY, INSIGHT_USAGE, buildProjectUsage, insightAverage, insightChange, insightChart,
  insightCompletions, insightControls, insightCost, insightCustomRange, insightDaily, insightDayLabel, insightHours, insightInRange, insightKpis,
  insightMedian, insightPeriod, insightRows, insightSelect, insightSeriesBuckets, insightShareText, insightSum, insightTable,
  insightTickUsd, insightTimeRows, insightUsd, insightWeeks,
} from "./insights.js";
import { own } from "../shell/router.js";
import { S, qs } from "../state.js";
import { esc, hdr, ic, plural, projPill, sectionHead } from "../ui.js";

const ORG_GROUPS = { project: "Project", ...INSIGHT_GROUPS };
const ORG_GROUP_FIELD = { project: "projectName" };
const ORG_MONTH_END = Date.UTC(2026, 10, 30);
const hours = insightHours;
const percent = (value) => Math.round(value) + "%";
const share = (items, flag) => (items.length ? 100 * items.filter((item) => item[flag]).length / items.length : null);
const medianOf = (items, field) => insightMedian(items.map((item) => item[field]));

/* ---------------------------------------------------------------- data */
function orgData(params) {
  const projects = [...new Set([...Object.keys(INSIGHT_USAGE), ...Object.keys(INSIGHT_COMPLETIONS)])].filter((id) => P[id] && !P[id].archived);
  const usage = projects.flatMap((id) => buildProjectUsage(id)).sort((a, b) => b.day - a.day);
  const completions = projects.flatMap((id) => insightCompletions(id));
  const { period, start, end } = insightPeriod(usage, completions, params);
  const priorStart = start - (end - start + INSIGHT_DAY);
  const current = (items) => items.filter((item) => insightInRange(item, start, end));
  const prior = (items) => items.filter((item) => insightInRange(item, priorStart, start - INSIGHT_DAY));
  return {
    projects, usage, completions, period, start, end,
    records: current(usage), done: current(completions), priorRecords: prior(usage), priorDone: prior(completions),
  };
}

/* Month-end pace from the last 7 days' spend, the same projection Home shows. */
function orgForecast(d, spend) {
  if (d.period !== "month") return null;
  const lastWeek = insightSum(d.usage.filter((item) => insightInRange(item, INSIGHT_TODAY - 6 * INSIGHT_DAY, INSIGHT_TODAY)), insightCost);
  return spend + lastWeek / 7 * ((ORG_MONTH_END - INSIGHT_TODAY) / INSIGHT_DAY);
}

/* ---------------------------------------------------------------- figures */
function orgRateStat(label, d, flag, what) {
  const now = share(d.done, flag), before = share(d.priorDone, flag);
  if (now === null) return { label, value: "—", sub: "No completed items" };
  const change = before === null ? { note: "No prior period data" } : insightChange(now, before, "up", true);
  return { label, value: percent(now), sub: `${d.done.filter((item) => item[flag]).length} of ${d.done.length} ${what}`, ...change };
}
function orgCycleStat(d) {
  const now = medianOf(d.done, "cycle"), before = medianOf(d.priorDone, "cycle");
  if (now === null) return { label: "Median cycle time", value: "—", sub: "At least 5 completed items needed" };
  return { label: "Median cycle time", value: hours(now), sub: `Lead time ${hours(medianOf(d.done, "lead"))}`, ...(before === null ? { note: "No prior period data" } : insightChange(now, before, "down")) };
}
function orgCostStats(d) {
  const spend = insightSum(d.records, insightCost), before = insightSum(d.priorRecords, insightCost);
  const forecast = orgForecast(d, spend);
  const perItem = d.done.length ? spend / d.done.length : null, perItemBefore = d.priorDone.length ? before / d.priorDone.length : null;
  const pace = forecast ? `Pacing to ≈ $${Math.round(forecast / 100)} by ${insightDayLabel(ORG_MONTH_END)}` : "API cost estimate";
  return [
    { label: "Period cost", value: d.records.length ? insightUsd(spend) : "—", sub: d.records.length ? pace : "No recorded usage",
      ...(d.records.length && before ? insightChange(spend, before, "none") : { note: "No prior period data" }) },
    { label: "Cost per completed item", value: perItem === null || !d.records.length ? "—" : insightUsd(perItem), sub: "Period cost ÷ completed items",
      ...(perItem && perItemBefore ? insightChange(perItem, perItemBefore, "down") : { note: "No prior period data" }) },
  ];
}
function orgStats(d) {
  const days = Math.round((d.end - d.start) / INSIGHT_DAY) + 1;
  const perDay = (d.done.length / days).toFixed(1);
  const completed = d.priorDone.length ? insightChange(d.done.length, d.priorDone.length) : { note: "No prior period data" };
  return [
    { label: "Completed", value: String(d.done.length), sub: `${perDay} closed a day`, ...completed },
    orgCycleStat(d),
    orgRateStat("Approved first try", d, "firstTry", "items"),
    orgRateStat("Autonomous", d, "autonomous", "without your input"),
    ...orgCostStats(d),
  ];
}

/* ---------------------------------------------------------------- insights to act on (doc-51 decision 7) */
function orgWaitInsight(d) {
  const waited = insightSum(d.done, (item) => item.wait);
  if (!waited) return null;
  const [top, topHours] = d.projects.map((id) => [id, insightSum(d.done.filter((item) => item.project === id), (item) => item.wait)]).sort((a, b) => b[1] - a[1])[0];
  return { icon: "clock", href: "#/inbox", title: `Agents waited ${hours(waited)} for you`, text: `Most on ${P[top].name}: ${hours(topHours)}` };
}
function orgSlowInsight(d) {
  const overall = medianOf(d.done, "cycle");
  const slowest = d.projects.map((id) => [id, medianOf(d.done.filter((item) => item.project === id), "cycle")])
    .filter(([, median]) => median !== null).sort((a, b) => b[1] - a[1])[0];
  if (!overall || !slowest || slowest[1] < 1.5 * overall) return null;
  return { icon: "signal", href: `#/projects/${slowest[0]}/insights`, title: `${P[slowest[0]].name} takes ${(slowest[1] / overall).toFixed(1)}× as long`,
    text: `Median cycle ${hours(slowest[1])} vs ${hours(overall)} across projects` };
}
function orgCostInsight(d) {
  const items = insightRows(d.records.filter(({ item, cancelled }) => item && !cancelled), "item");
  const median = insightMedian(items.map(({ total }) => total));
  if (!median || items[0].total < 2.5 * median) return null;
  const record = d.records.find(({ item }) => item === items[0].label);
  return { icon: "currency", href: `#/projects/${record.project}/work?item=${encodeURIComponent(items[0].label)}`,
    title: `${items[0].label} cost ${(items[0].total / median).toFixed(1)}× the median item`, text: `${insightUsd(items[0].total)} vs ${insightUsd(median)} · ${record.projectName}` };
}
function orgActionsHtml(d) {
  const found = [orgWaitInsight(d), orgSlowInsight(d), orgCostInsight(d)].filter(Boolean);
  if (!found.length) return "";
  return `<section class="insight-actions" aria-label="Insights to act on">${found.map(({ icon, href, title, text }) =>
    `<a href="${href}">${ic(icon, "ic16")}<span><strong>${esc(title)}</strong><small>${esc(text)}</small></span>${ic("chev-right", "ic16")}</a>`).join("")}</section>`;
}

/* ---------------------------------------------------------------- delivery and quality */
function orgThroughput(d, chart) {
  const daily = insightDaily(d.start, d.end), buckets = insightSeriesBuckets(d.start, d.end);
  const rows = insightTimeRows(buckets, (bucket) => [d.done.filter((item) => insightInRange(item, bucket.first, bucket.last)).length]);
  const average = insightAverage(daily, buckets, d.completions, (items) => items.length, (value) => value.toFixed(1) + " a day");
  return chart("throughput", "Throughput", daily ? "Tasks and bugs completed each day, with the 7-day average" : "Tasks and bugs completed each week", ["Completed"], rows, {
    kind: "columns", integer: true, wide: true, overlay: average, tones: [average ? "tint" : "accent"], format: String,
    groupLabel: daily ? "Day" : "Week", empty: "No completed items in this period." });
}
function orgWeekly(d, measure) {
  return insightWeeks(d.start, d.end).map((week) => {
    const items = d.done.filter((item) => insightInRange(item, week.first, week.last));
    return { label: week.label, axis: week.axis, tip: week.tip, values: items.length < 5 ? [null, null] : measure(items) };
  });
}
const thinTrend = (rows) => (rows.filter((row) => row.values[0] != null).length < 2 ? "A trend needs at least two weeks with 5 or more completed items." : "");
function orgSpeed(d, chart) {
  const rows = orgWeekly(d, (items) => [medianOf(items, "cycle"), medianOf(items, "lead")]);
  return chart("speed", "Cycle and lead time", "Weekly median · cycle runs from start to done, lead from request to done", ["Cycle time", "Lead time"], rows, {
    kind: "line", format: hours, tick: (value) => value + " h", thin: thinTrend(rows), groupLabel: "Week", empty: "No completed items in this period." });
}
function percentDomain(rows) {
  const low = Math.floor((Math.min(100, ...rows.flatMap((row) => row.values).filter((value) => value != null)) - 5) / 10) * 10;
  return low >= 30 ? Array.from({ length: (100 - low) / 10 + 1 }, (_, index) => low + 10 * index) : [0, 25, 50, 75, 100];
}
function orgQuality(d, chart) {
  const rows = orgWeekly(d, (items) => [share(items, "firstTry"), share(items, "autonomous")]);
  return chart("quality", "Quality and autonomy", "Weekly share of completed items · autonomous means it needed no input from you", ["Approved first try", "Autonomous"], rows, {
    kind: "line", domain: percentDomain(rows), format: percent, tick: (value) => value + "%", thin: thinTrend(rows), groupLabel: "Week", empty: "No completed items in this period." });
}
function orgTiming(d, chart) {
  const rows = d.done.length ? [
    { label: "Waiting on you", values: [insightSum(d.done, (item) => item.wait)] },
    { label: "Blocked", values: [insightSum(d.done, (item) => item.blocked)] },
    { label: "The rest", values: [insightSum(d.done, (item) => item.lead - item.wait - item.blocked)] },
  ] : [];
  return chart("timing", "Where time went", "Total lead time across completed items · working time isn't tracked yet", ["Hours"], rows, {
    kind: "split", tones: ["warn", "bad", "tint"], format: hours, wide: true, empty: "No completed items in this period." });
}

/* ---------------------------------------------------------------- cost */
function orgSpend(d, chart) {
  const daily = insightDaily(d.start, d.end), buckets = insightSeriesBuckets(d.start, d.end);
  const rows = d.records.length ? insightTimeRows(buckets, (bucket) => [insightSum(d.records.filter((item) => insightInRange(item, bucket.first, bucket.last)), insightCost)]) : [];
  const average = insightAverage(daily, buckets, d.usage, (items) => insightSum(items, insightCost), (value) => insightUsd(value) + " a day");
  return chart("spend", "Spend over time", `Estimated cost per ${daily ? "day, with the 7-day average" : "week"}, all projects`, ["Estimated cost"], rows, {
    kind: "columns", overlay: average, tones: [average ? "tint" : "accent"], format: insightUsd, tick: insightTickUsd, groupLabel: daily ? "Day" : "Week", empty: "No recorded usage." });
}
function orgBreakdown(d, chart, by, params) {
  const field = ORG_GROUP_FIELD[by] || by;
  const rows = insightRows(d.records, field).map(({ label, total }) => {
    const next = new URLSearchParams(params);
    next.set("group", label);
    return { label, values: [total], href: "#/insights?" + next.toString(), drill: true };
  });
  return chart("breakdown", "Spending breakdown", "Open a group to see its usage records", ["Estimated cost"], rows, {
    format: insightUsd, valueText: insightShareText(rows, insightUsd), control: insightSelect("insight-breakdown", "Group spend by", ORG_GROUPS, by), empty: "No recorded usage." });
}

/* ---------------------------------------------------------------- compare projects or teams */
function orgCompareRow(name, done, records, priorRecords, investment) {
  const spend = insightSum(records, insightCost), before = insightSum(priorRecords, insightCost);
  const cycle = medianOf(done, "cycle"), first = share(done, "firstTry"), auto = share(done, "autonomous");
  const change = records.length && before ? insightChange(spend, before, "none").delta : "—";
  return `<tr><th scope="row">${name}</th><td>${done.length}</td><td>${cycle === null ? "—" : hours(cycle)}</td>
    <td>${first === null ? "—" : percent(first)}</td><td>${auto === null ? "—" : percent(auto)}</td>
    <td>${records.length ? insightUsd(spend) : "—"}</td><td>${change}</td><td>${investment ? insightUsd(investment) : "—"}</td><td>—</td></tr>`;
}
function orgCompareRows(d, mode) {
  const pick = (test) => [d.done.filter(test), d.records.filter(test), d.priorRecords.filter(test), insightSum(d.usage.filter(test), insightCost)];
  if (mode === "teams") {
    return Object.keys(TEAMS).map((id) => [`<a class="insight-link" href="#/teams/${id}">${esc(TEAMS[id].name)}</a>`, ...pick((item) => item.team === id || item.teamId === id)])
      .filter(([, done, records]) => done.length || records.length);
  }
  return d.projects.map((id) => [`<span class="org-compare-name"><a class="insight-link" href="#/projects/${id}/insights">${esc(P[id].name)}</a>${projPill(id)}</span>`, ...pick((item) => item.project === id)]);
}
function orgCompare(d) {
  const mode = S.insightsCompare === "teams" ? "teams" : "projects";
  const rows = orgCompareRows(d, mode);
  const toggle = `<div class="work-layout-toggle" role="group" aria-label="Compare by">${[["projects", "Projects", "folder"], ["teams", "Teams", "user-group"]].map(([value, label, icon]) =>
    `<button class="queue-tab" type="button" data-act="set" data-k="insightsCompare" data-v="${value}" aria-pressed="${mode === value}" aria-controls="org-compare-table">${ic(icon, "ic16")}${label}</button>`).join("")}</div>`;
  const head = ["Completed", "Median cycle", "First try", "Autonomous", "Period cost", "Change", "Investment to date", "Working time"];
  return `<section class="org-compare" aria-labelledby="org-compare-title">
    ${sectionHead({ id: "org-compare-title", title: "Compare", count: plural(rows.length, mode === "teams" ? "team" : "project", mode === "teams" ? "teams" : "projects"), right: toggle })}
    <div class="insight-card"><div class="insight-table-scroll" id="org-compare-table" tabindex="0" role="region" aria-label="${mode === "teams" ? "Teams" : "Projects"} compared"><table>
      <caption class="sr-only">${mode === "teams" ? "Teams" : "Projects"} compared for this period</caption>
      <thead><tr><th scope="col">${mode === "teams" ? "Team" : "Project"}</th>${head.map((label) => `<th scope="col">${label}</th>`).join("")}</tr></thead>
      <tbody>${rows.map((row) => orgCompareRow(...row)).join("")}</tbody>
      <tfoot>${orgCompareRow("Organization", d.done, d.records, d.priorRecords, insightSum(d.usage, insightCost))}</tfoot>
    </table></div><p class="insight-thin">Change compares period cost with the previous period of the same length. Working time isn't tracked yet.</p></div>
  </section>`;
}

/* ---------------------------------------------------------------- page */
function orgSectionsHtml(d, params, table) {
  const chart = (key, title, note, series, rows, display) => insightChart(key, title, note, series, rows, { table, ...display });
  const by = own(ORG_GROUPS, params.get("by")) ? params.get("by") : "project";
  const field = ORG_GROUP_FIELD[by] || by;
  const selected = d.records.some((record) => record[field] === params.get("group")) ? params.get("group") : "";
  return `${sectionHead({ id: "org-delivery-title", title: "Delivery and quality", count: plural(d.done.length, "completed item", "completed items") })}
    <div class="insights-grid">${orgThroughput(d, chart)}${orgSpeed(d, chart)}${orgQuality(d, chart)}${orgTiming(d, chart)}</div>
    ${sectionHead({ id: "org-cost-title", title: "Cost", count: d.records.length ? `${insightUsd(insightSum(d.records, insightCost))} estimated` : "" })}
    <div class="insights-grid">${orgSpend(d, chart)}${orgBreakdown(d, chart, by, params)}</div>
    ${orgCompare(d)}
    ${d.records.length ? insightTable(selected ? d.records.filter((record) => record[field] === selected) : d.records, selected, true) : ""}`;
}

function pageInsights() {
  const params = qs(), d = orgData(params), table = params.get("display") === "table";
  const note = `Demo data · ${insightDayLabel(d.start)}–${insightDayLabel(d.end)} · ${plural(d.projects.length, "project", "projects")} · API cost estimates in USD`;
  return `<div class="org-insights-page">
    ${hdr("Insights", note, insightControls({ period: d.period, table, records: d.records, filename: "agentic-org-usage.csv" }))}
    <section class="org-insights" aria-label="Organization insights">
      ${insightCustomRange(d.period, d.start, d.end)}
      ${insightKpis(orgStats(d), "Organization at a glance")}
      ${orgActionsHtml(d)}
      ${orgSectionsHtml(d, params, table)}
    </section>
  </div>`;
}

export { pageInsights };
