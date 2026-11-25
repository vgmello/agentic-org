import { M, P, TEAMS, W } from "../data.js";
import { ACTIVITY_STAGES, RELEASE_GATES, assemblyMerged, assemblyPct, assemblyProgressBar, assemblySnapshot, formatTelemetryHours, releaseGateState, releaseGates } from "./activity.js";
import { own, render } from "../shell/router.js";
import { qs } from "../state.js";
import { esc, ic, plural } from "../ui.js";

const INSIGHT_DAY = 86_400_000;
const INSIGHT_TODAY = Date.UTC(2026, 10, 24);
const INSIGHT_FIRST_DAY = Date.UTC(2026, 8, 1);
const INSIGHT_PERIODS = { week: "This week", month: "This month", last: "Last month", custom: "Custom", all: "All time" };
const INSIGHT_GROUPS = {
  team: "Team", consumer: "Consumer", model: "Model", kind: "Task type", type: "Work type",
  task: "Work item", attribution: "Spend attribution", release: "Release", milestone: "Milestone",
};
const INSIGHT_INVESTMENT = { type: "Work type", release: "Release", milestone: "Milestone", consumer: "Consumer" };
const INSIGHT_MODELS = { opus: "Claude Opus", sonnet: "Claude Sonnet", codex: "Codex" };
const INSIGHT_CAPS = { "agency-v1": 25000, "office-federation": 13500 };
const INSIGHT_TOKEN_KEYS = ["Input", "Output", "Cache read", "Cache write"];
// Prototype ledger rows capture team, consumer and work classification at usage time.
const INSIGHT_USAGE = {
  "agency-v1": [
    [0, "platform", "kai", "codex", "Task", "Build (R&D)", "T-A121", 2860, [320, 112, 1090, 47]],
    [1, "app", "jules", "codex", "Bug", "Run", "T-A88", 1730, [194, 73, 630, 34]],
    [3, "quality", "tomas", "opus", "Task", "Build (R&D)", "T-A140", 2310, [221, 68, 481, 23]],
    [5, "app", "jules", "codex", "Task", "Build (R&D)", "T-A124", 3290, [418, 153, 1490, 124]],
    [7, "app", "avery", "codex", "Bug", "Run", "T-A86", 980, [124, 48, 275, 22]],
    [10, "leadership", "ada", "opus", "Task", "Build (R&D)", "T-A112", 1640, [185, 65, 750, 25]],
    [12, "quality", "noor", "sonnet", "Task", "Run", "T-A144", 810, [141, 40, 290, 20]],
    [14, "app", "avery", "codex", "Bug", "Build (R&D)", "T-A70", 1470, [175, 38, 504, 18]],
    [16, "app", "avery", "codex", "Task", "Build (R&D)", "T-A999", 560, [60, 24, 260, 10], true],
    [17, "app", "jules", "codex", "Task", "Build (R&D)", "T-A124", 2010, [244, 94, 800, 40]],
    [20, "leadership", "tess", "", "", "", "", 340, [63, 19, 120, 0]],
    [22, "platform", "kai", "codex", "Task", "Build (R&D)", "T-A121", 1860, [204, 66, 810, 34]],
    [29, "platform", "kai", "codex", "Task", "Build (R&D)", "T-A121", 1740, [203, 72, 720, 42]],
    [35, "quality", "tomas", "opus", "Task", "Run", "T-A140", 1120, [110, 55, 350, 14]],
    [42, "app", "avery", "codex", "Bug", "Run", "T-A70", 1230, [175, 48, 420, 21]],
    [48, "platform", "kai", "codex", "Task", "Build (R&D)", "T-A121", 910, [108, 42, 220, 15]],
    [57, "leadership", "ada", "opus", "Task", "Build (R&D)", "T-A112", 1390, [105, 66, 310, 17]],
  ],
  "office-federation": [
    [0, "platform", "sage", "codex", "Task", "Build (R&D)", "T-A118", 2350, [204, 86, 780, 91]],
    [3, "platform", "quinn", "codex", "Task", "Build (R&D)", "T-A135", 1560, [175, 41, 648, 21]],
    [7, "platform", "rowan", "opus", "Task", "Build (R&D)", "T-A138", 2070, [212, 66, 670, 45]],
    [11, "platform", "sage", "codex", "Bug", "Run", "T-A81", 740, [81, 35, 238, 18]],
    [18, "platform", "quinn", "codex", "", "", "", 420, [77, 28, 180, 13]],
    [26, "platform", "sage", "codex", "Task", "Build (R&D)", "T-A118", 1410, [152, 67, 570, 32]],
    [39, "platform", "rowan", "opus", "Task", "Build (R&D)", "T-A138", 1490, [169, 43, 575, 34]],
    [62, "platform", "quinn", "codex", "Task", "Build (R&D)", "T-A135", 840, [91, 31, 231, 23]],
  ],
  "look-feel": [
    [1, "product", "eden", "opus", "Task", "Build (R&D)", "T-A130", 1200, [134, 46, 450, 19]],
    [3, "product", "emery", "opus", "Task", "Build (R&D)", "T-A131", 1690, [179, 61, 602, 24]],
    [8, "app", "avery", "codex", "Bug", "Run", "T-A76", 910, [103, 31, 315, 18]],
    [13, "app", "jules", "codex", "Bug", "Run", "T-A84", 1160, [116, 43, 356, 20]],
    [17, "app", "avery", "codex", "Task", "Build (R&D)", "T-A133", 870, [93, 30, 246, 15]],
    [22, "product", "eden", "opus", "Task", "Build (R&D)", "T-A130", 1020, [108, 34, 281, 19]],
    [31, "app", "avery", "codex", "Bug", "Run", "T-A76", 430, [58, 20, 150, 8]],
    [45, "product", "emery", "opus", "Task", "Build (R&D)", "T-A131", 1090, [124, 38, 318, 23]],
  ],
};
// Completion samples are separate from usage events. A task can incur multiple charges.
// Each completion has one owning team and one contributor.
const INSIGHT_COMPLETIONS = {
  "agency-v1": [
    [0, "Task", 24, 14, 3, 2, "platform", "kai"], [1, "Bug", 13, 9, 2, 0, "app", "jules"], [2, "Task", 62, 39, 8, 9, "leadership", "ada"],
    [5, "Bug", 26, 18, 5, 3, "quality", "noor"], [7, "Task", 46, 32, 7, 4, "leadership", "maya"], [8, "Bug", 18, 12, 1, 2, "app", "avery"],
    [11, "Task", 41, 24, 6, 4, "platform", "rowan"], [13, "Bug", 33, 18, 5, 3, "quality", "tomas"], [15, "Bug", 17, 11, 3, 0, "app", "jules"],
    [18, "Task", 58, 40, 8, 7, "leadership", "ada"], [22, "Task", 36, 22, 5, 3, "quality", "tomas"], [23, "Bug", 15, 11, 2, 1, "leadership", "maya"],
    [28, "Task", 38, 25, 4, 4, "platform", "sage"], [31, "Bug", 29, 17, 2, 4, "app", "avery"], [35, "Task", 57, 29, 3, 7, "quality", "tomas"],
    [39, "Bug", 14, 8, 1, 1, "app", "jules"], [43, "Task", 70, 41, 7, 10, "platform", "quinn"], [58, "Task", 33, 22, 2, 3, "leadership", "ada"],
  ],
  "office-federation": [
    [3, "Task", 51, 37, 8, 12, "platform", "rowan"], [5, "Task", 72, 57, 11, 14, "platform", "sage"], [9, "Bug", 26, 21, 2, 6, "platform", "quinn"],
    [12, "Task", 43, 30, 5, 7, "platform", "kai"], [16, "Task", 62, 44, 13, 9, "platform", "rowan"], [20, "Task", 46, 31, 11, 3, "platform", "sage"],
    [28, "Task", 54, 32, 6, 14, "platform", "quinn"], [34, "Bug", 18, 10, 2, 3, "platform", "kai"], [42, "Task", 69, 50, 8, 11, "platform", "rowan"],
  ],
  "look-feel": [
    [1, "Bug", 12, 7, 0, 2, "product", "eden"], [4, "Task", 34, 26, 4, 1, "product", "emery"], [5, "Bug", 8, 4, 1, 1, "app", "jules"],
    [8, "Bug", 19, 14, 3, 3, "product", "eden"], [11, "Task", 30, 21, 1, 3, "product", "emery"], [16, "Bug", 10, 7, 2, 1, "app", "avery"],
    [20, "Bug", 23, 16, 3, 2, "product", "eden"], [32, "Task", 41, 33, 4, 4, "product", "emery"], [36, "Bug", 17, 15, 2, 2, "app", "jules"],
  ],
};
function insightCompletions(id) {
  return (INSIGHT_COMPLETIONS[id] || []).map(([age, kind, lead, cycle, wait, blocked, team, member]) => ({
    day: INSIGHT_TODAY - age * INSIGHT_DAY, kind, lead, cycle, wait, blocked, team, member,
  }));
}
const insightDateText = new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric", timeZone: "UTC" });
const insightDollars = new Intl.NumberFormat(undefined, { style: "currency", currency: "USD" });
const insightCompact = new Intl.NumberFormat(undefined, { notation: "compact", maximumFractionDigits: 1 });
const insightUsd = (cents) => "≈ " + insightDollars.format(cents / 100);
const insightDayLabel = (day) => insightDateText.format(day);
const insightIso = (day) => new Date(day).toISOString().slice(0, 10);
const insightSum = (items, measure) => items.reduce((total, item) => total + measure(item), 0);
const insightTokens = (record) => insightSum(record.tokens, (value) => value);
const insightWorkHref = (id, task) => "#/projects/" + id + "/work?item=" + encodeURIComponent(task);

function insightPeriod(records, completions, params) {
  const period = own(INSIGHT_PERIODS, params.get("period")) ? params.get("period") : "month";
  let start = Date.UTC(2026, 10, 1), end = INSIGHT_TODAY;
  if (period === "week") start = Date.UTC(2026, 10, 23);
  if (period === "last") {
    start = Date.UTC(2026, 9, 1);
    end = Date.UTC(2026, 9, 31);
  }
  if (period === "all") start = Math.min(start, ...records.map(({ day }) => day), ...completions.map(({ day }) => day));
  if (period === "custom") {
    const from = params.get("from"), to = params.get("to");
    if (insightValidDate(from) && insightValidDate(to) && from <= to) {
      start = Date.parse(from);
      end = Date.parse(to);
    }
  }
  return { period, start, end };
}

function insightValidDate(value) {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(value)) return false;
  const day = Date.parse(value);
  return day >= INSIGHT_FIRST_DAY && day <= INSIGHT_TODAY && insightIso(day) === value;
}

function insightSelect(id, label, options, value) {
  return `<label class="insight-select"><span>${label}</span><select class="field" id="${id}" data-insight-control aria-label="${label}">
    ${Object.entries(options).map(([key, text]) => `<option value="${key}"${value === key ? " selected" : ""}>${esc(text)}</option>`).join("")}
  </select></label>`;
}
function insightRows(records, by) {
  const groups = new Map();
  for (const record of records) {
    const label = record[by];
    const group = groups.get(label) || { label, total: 0 };
    group.total += record.cost;
    groups.set(label, group);
  }
  return [...groups.values()].sort((a, b) => b.total - a.total);
}

function insightTrend(rows, peak, format) {
  const points = rows.map(({ label, values: [value] }, index) => ({
    label, value, x: 100 * (index + .5) / rows.length, y: 98 - 75 * value / peak,
  }));
  const edge = (points, offset) => points.map(({ x, y }) => `${x}% ${y + offset}%`).join(", ");
  const trace = points.length > 1 ? `polygon(${edge(points, -.85)}, ${edge(points.slice().reverse(), .85)})` : "inset(100%)";
  return `<ul class="insight-trend" role="list" style="--insight-points:${points.length};--insight-trace:${trace}">
    ${points.map(({ label, value, x, y }) => `<li style="--insight-x:${x}%;--insight-y:${y / 100}">
      <span class="insight-trend-date">${esc(label)}</span><span class="insight-trend-value">${esc(format(value))}</span>
    </li>`).join("")}
  </ul>`;
}

function insightChart(key, title, note, series, rows, options = {}) {
  const { table, columns, trend, control = "", empty = "No data for this period.", format = (value) => String(value), plotFormat = format } = options;
  const peak = Math.max(1, ...rows.flatMap(({ values }) => values));
  function rowLabel(row) {
    return row.href ? `<a class="insight-link" href="${esc(row.href)}"${row.drill ? ' data-act="insight-drill"' : ""}>${esc(row.label)}</a>` : esc(row.label);
  }
  const tableBody = `<div class="insight-table-scroll" tabindex="0" role="region" aria-label="${esc(title)} table"><table>
      <caption class="sr-only">${esc(title)}</caption><thead><tr><th scope="col">Group</th>${series.map((name) => `<th scope="col">${esc(name)}</th>`).join("")}</tr></thead>
      <tbody>${rows.map((row) => `<tr><th scope="row">${rowLabel(row)}</th>${row.values.map((value) => `<td>${value === null ? "—" : esc(format(value))}</td>`).join("")}</tr>`).join("")}</tbody>
    </table></div>`;
  const plot = `<ul class="insight-plot${columns ? " insight-columns" : ""}" role="list"${columns ? ` style="--insight-points:${rows.length}"` : ""}>
    ${rows.map((row) => `<li><p class="insight-point-label">${rowLabel(row)}</p><div class="insight-series">${row.values.map((value, index) => `<div class="insight-point" data-series="${index}">
      <span class="insight-bar" style="--insight-value:${100 * (value ?? 0) / peak}%" aria-hidden="true"></span>
      <span class="insight-point-value">${series.length > 1 ? `<span class="sr-only">${esc(series[index])} </span>` : ""}${value === null ? "—" : esc(plotFormat(value))}</span>
    </div>`).join("")}</div></li>`).join("")}
  </ul>`;
  return `<section class="insight-card" aria-labelledby="insight-${key}-title">
    <header class="insight-card-head"><div><h3 id="insight-${key}-title">${title}</h3><p>${note}</p></div>${control}</header>
    ${rows.length ? (table ? tableBody : `${series.length > 1 ? `<ul class="insight-legend" aria-hidden="true">${series.map((name, index) => `<li data-series="${index}">${esc(name)}</li>`).join("")}</ul>` : ""}${trend ? `<div class="insight-trend-card">${insightTrend(rows, peak, plotFormat)}${plot}</div>` : plot}`) : `<p class="insight-empty">${empty}</p>`}
  </section>`;
}

function insightBuckets(start, end, bucketDays) {
  const days = (end - start) / INSIGHT_DAY + 1;
  const width = (bucketDays ?? Math.ceil(days / 6)) * INSIGHT_DAY;
  return Array.from({ length: Math.ceil(days * INSIGHT_DAY / width) }, (_, index) => {
    const first = start + width * index, last = Math.min(end, first + width - INSIGHT_DAY);
    return { first, last, label: first === last ? insightDayLabel(first) : `${insightDayLabel(first)}–${insightDayLabel(last)}` };
  });
}
const insightInRange = ({ day }, start, end) => day >= start && day <= end;
function insightMedian(values) {
  if (values.length < 5) return null;
  return [...values].sort((a, b) => a - b)[Math.ceil(values.length / 2) - 1];
}
function insightDelta(current, previous) {
  if (!previous) return "Prior period unavailable";
  const delta = Math.round(100 * (current - previous) / previous);
  return (delta > 0 ? "+" : "") + delta + "% vs prior period";
}

function insightTable(id, records, selected) {
  return `<section class="insight-card insight-records" aria-labelledby="insights-usage-heading">
    <header class="insight-card-head"><div><h3 id="insights-usage-heading" tabindex="-1">Usage records${selected ? " · " + esc(selected) : ""}</h3>
      <p>Captured attribution and estimated API cost</p></div>${selected ? '<button class="btn outline sm" type="button" data-act="insight-clear-drill">Show all records</button>' : ""}</header>
    ${records.length ? `<div class="insight-table-scroll" tabindex="0" role="region" aria-label="Usage record details"><table><caption class="sr-only">Usage records</caption>
      <thead><tr><th scope="col">Date</th><th scope="col">Work item</th><th scope="col">Consumer</th><th scope="col">Model</th><th scope="col">Tokens</th><th scope="col">Estimated cost</th></tr></thead>
      <tbody>${records.map((record) => `<tr><td>${insightDayLabel(record.day)}</td><th scope="row">${record.cancelled ? esc(record.item) + " (cancelled)" : record.item ? `<a class="insight-link" href="${insightWorkHref(id, record.item)}">${esc(record.item)}</a>` : "Not on a task"}</th>
        <td>${esc(record.consumer)}</td><td>${esc(record.model)}</td><td>${insightTokens(record).toLocaleString()}</td><td>${insightUsd(record.cost)}</td></tr>`).join("")}</tbody></table></div>` : '<p class="insight-empty">No usage records for this selection.</p>'}
  </section>`;
}

function insightsCsv(records) {
  const rows = [["record_id", "date", "project", "release", "milestone", "team", "consumer", "model", "task", "task_type", "work_type", "attribution", "input_tokens", "output_tokens", "cache_read_tokens", "cache_write_tokens", "estimated_cost_usd"],
    ...records.map(({ key, day, project, release, milestone, team, consumer, model, item, kind, type, attribution, tokens, cost }) =>
      [key, insightIso(day), project, release, milestone, team, consumer, model, item, kind, type, attribution, ...tokens, (cost / 100).toFixed(2)])];
  return encodeURIComponent(rows.map((row) => row.map((cell) => '"' + String(cell).replaceAll('"', '""') + '"').join(",")).join("\r\n"));
}

function buildProjectUsage(id, project) {
  return (INSIGHT_USAGE[id] || []).map(([age, team, consumer, model, kind, type, item, cost, tokens, cancelled = false], index) => ({
    key: id + "-" + (index + 1), day: INSIGHT_TODAY - age * INSIGHT_DAY, project: id,
    team: TEAMS[team].name, consumer: M[consumer].name, model: INSIGHT_MODELS[model] || "Unknown model",
    kind: kind || "Unknown type", type: type || "Unknown work type", item, task: item || "Not on a task", cost, cancelled,
    attribution: cancelled ? "Cancelled work" : item ? "Task-linked" : "Not on a task",
    tokens: tokens.map((value) => value * 1000), release: project.release || "No release", milestone: project.milestoneTitle || "No milestone",
  }));
}

function buildInsightStats(usage, records, completions, done, previous) {
  const spend = insightSum(records, (record) => record.cost);
  const investment = insightSum(usage, (record) => record.cost);
  const recTokens = insightSum(records, insightTokens);
  const medianCycle = insightMedian(done.map(({ cycle }) => cycle));
  return [
    ["Investment to date", usage.length ? insightUsd(investment) : "—", "All recorded usage"],
    ["Period cost", records.length ? insightUsd(spend) : "—", records.length ? insightDelta(spend, insightSum(previous(usage), (record) => record.cost)) : "No recorded usage"],
    ["Tokens", records.length ? insightCompact.format(recTokens) : "—", records.length ? insightDelta(recTokens, insightSum(previous(usage), insightTokens)) : "No recorded usage"],
    ["Completed", String(done.length), done.length ? insightDelta(done.length, previous(completions).length) : "No recorded completions"],
    ["Median cycle time", medianCycle === null ? "—" : formatTelemetryHours(medianCycle),
      medianCycle === null ? "At least 5 completed items needed" : plural(done.length, "completed item", "completed items")],
    ["Working time", "—", "Not tracked yet"],
  ];
}

function insightActionsHtml(id, pending, top, topTitle) {
  if (!pending.length && !topTitle) return "";
  return `<section class="insight-actions" aria-label="Project actions">
      ${pending.length ? `<a href="#/inbox/${pending[0].id}">${ic("flag", "ic16")}<span><strong>${plural(pending.length, "release gate", "release gates")} waiting for you</strong><small>${esc(RELEASE_GATES[pending[0].id])}</small></span>${ic("chev-right", "ic16")}</a>` : ""}
      ${topTitle ? `<a href="${insightWorkHref(id, top.label)}">${ic("signal", "ic16")}<span><strong>Highest task spend this period · ${insightUsd(top.total)}</strong><small>${esc(top.label + " " + topTitle)}</small></span>${ic("chev-right", "ic16")}</a>` : ""}
    </section>`;
}

function insightCostGridHtml({ usage, records, buckets, breakdown, by, investmentBy, investmentRows, build, lifetimeBuild, chart, costOptions }) {
  const tokenSeries = records.length ? buckets.map(({ label, tokens: value }) => ({ label, values: [value] })) : [];
  const tokenMix = records.length ? INSIGHT_TOKEN_KEYS.map((label, index) => ({ label, values: [insightSum(records, (record) => record.tokens[index])] })) : [];
  const invNote = usage.length ? `Build (R&D) ${records.length ? insightUsd(build) : "—"} this period · ${insightUsd(lifetimeBuild)} to date` : "Recorded build and run costs";
  return `<h3 class="insights-section-title">Cost and usage</h3><div class="insights-grid">
      ${chart("spend", "Spend over time", "Estimated cost per interval", ["Estimated cost"], records.length ? buckets.map(({ label, spend: value }) => ({ label, values: [value] })) : [], { ...costOptions, trend: true, empty: "No recorded usage." })}
      ${chart("breakdown", "Spending breakdown", "Open a group to see its usage records", ["Estimated cost"], breakdown, {
        ...costOptions, control: insightSelect("insight-breakdown", "Group spend by", INSIGHT_GROUPS, by), empty: "No recorded usage.",
      })}
      ${chart("tokens-trend", "Tokens over time", "Total tokens per interval", ["Tokens"], tokenSeries,
        { format: (value) => value.toLocaleString(), plotFormat: (value) => insightCompact.format(value), columns: true })}
      ${chart("tokens", "Token mix", "Input, output, and cache usage", ["Tokens"], tokenMix,
        { format: (value) => value.toLocaleString(), plotFormat: (value) => insightCompact.format(value) })}
      ${chart("investment", investmentBy === "type" ? "Investment by work type" : "R&D investment", invNote, ["Period", "To date"], investmentRows, {
        ...costOptions, control: insightSelect("insight-investment", "Group investment by", INSIGHT_INVESTMENT, investmentBy),
      })}
    </div>`;
}

function insightDeliveryGridHtml({ buckets, done, completionTypes, durationRows, chart }) {
  const completedSeries = done.length ? buckets.map(({ label, done: items }) => ({ label, values: completionTypes.map((kind) => items.filter((item) => item.kind === kind).length) })) : [];
  const timingRows = done.length ? [
    { label: "Waiting on you", values: [insightSum(done, (item) => item.wait)] },
    { label: "Blocked", values: [insightSum(done, (item) => item.blocked)] },
    { label: "The rest", values: [insightSum(done, (item) => item.lead - item.wait - item.blocked)] },
  ] : [];
  return `<h3 class="insights-section-title">Delivery and speed</h3><div class="insights-grid">
      ${chart("completed", "Completed over time", "Tasks and bugs completed per interval", completionTypes, completedSeries, { columns: true, empty: "No recorded completions." })}
      ${chart("speed", "Lead and cycle time", "Median by task type · at least five completed items", ["Lead", "Cycle", "Waiting on you"], done.length ? durationRows : [], { format: formatTelemetryHours, empty: "No recorded completions." })}
      ${chart("timing", "Where time went", "Total lead time across completed items", ["Hours"], timingRows, { format: formatTelemetryHours, empty: "No recorded durations." })}
    </div>`;
}

function insightReleaseHtml(id, project, usage, investment, delivery, counts, gates, chart) {
  if (!project.release) return "";
  const cap = INSIGHT_CAPS[id];
  const capLine = cap
    ? `<p class="insight-cap">${insightUsd(investment)} spent of ${insightDollars.format(cap / 100)} cap
          ${assemblyProgressBar(Math.min(cap, investment), cap, "Release spend against cap")}</p>`
    : `<p class="insight-cap">${usage.length ? insightUsd(investment) + " spent · " : ""}No release cap set</p>`;
  const gateList = gates.length
    ? `<ul class="insight-gates" role="list">${gates.map((item) => `<li><a class="insight-link" href="#/inbox/${item.id}">${esc(RELEASE_GATES[item.id])}</a><span data-gate-state="${releaseGateState(item)}">${item.handled ? esc(item.outcome) : "Pending"}</span></li>`).join("")}</ul>`
    : '<p class="insight-empty">No release gates.</p>';
  return `<h3 class="insights-section-title">Current release · ${esc(project.release)}</h3>
    <div class="insights-grid">
      ${chart("pipeline", "Work by stage", "Current Assembly line", ["Tasks"], delivery.entries.length ? ACTIVITY_STAGES.map(([stage, label]) => ({ label, values: [counts[stage]] })) : [], { empty: "No tasks on this Assembly line." })}
      ${chart("readiness", "Feature readiness", `${assemblyPct(delivery.entries)}% of release tasks merged`, ["Merged", "Remaining"], delivery.features.map(({ label, entries }) => {
        const merged = assemblyMerged(entries);
        return { label, values: [merged, entries.length - merged] };
      }), { empty: "No release features yet." })}
      <section class="insight-card" aria-labelledby="insight-release-title">
        <header class="insight-card-head"><div><h3 id="insight-release-title">Release gates and cap</h3><p>Current release</p></div><a class="insight-link" href="#/projects/${id}/activity">Open Assembly line</a></header>
        ${capLine}
        ${gateList}
      </section>
    </div>`;
}

function projectInsights(id) {
  const project = P[id], params = qs();
  const usage = buildProjectUsage(id, project), completions = insightCompletions(id);
  const { period, start, end } = insightPeriod(usage, completions, params);
  const priorStart = start - (end - start + INSIGHT_DAY);
  const previous = (items) => items.filter((item) => insightInRange(item, priorStart, start - INSIGHT_DAY));
  const records = usage.filter((item) => insightInRange(item, start, end));
  const done = completions.filter((item) => insightInRange(item, start, end));
  const investment = insightSum(usage, (record) => record.cost);
  const build = insightSum(records.filter(({ type }) => type === "Build (R&D)"), (record) => record.cost);
  const lifetimeBuild = insightSum(usage.filter(({ type }) => type === "Build (R&D)"), (record) => record.cost);
  const table = params.get("display") === "table";
  const by = own(INSIGHT_GROUPS, params.get("by")) ? params.get("by") : "team";
  const investmentBy = own(INSIGHT_INVESTMENT, params.get("investmentBy")) ? params.get("investmentBy") : "type";
  const costOptions = { table, format: insightUsd };
  const chart = (key, title, note, series, rows, display) => insightChart(key, title, note, series, rows, { table, ...display });
  const buckets = insightBuckets(start, end).map(({ first, last, label }) => ({
    label, spend: insightSum(records.filter((item) => insightInRange(item, first, last)), (record) => record.cost),
    tokens: insightSum(records.filter((item) => insightInRange(item, first, last)), insightTokens),
    done: done.filter((item) => insightInRange(item, first, last)),
  }));
  const groups = insightRows(records, by);
  const selected = groups.some(({ label }) => label === params.get("group")) ? params.get("group") : "";
  const breakdown = groups.map(({ label, total }) => {
    const next = new URLSearchParams(params);
    next.set("group", label);
    return { label, values: [total], href: "#/projects/" + id + "/insights?" + next.toString(), drill: true };
  });
  const stats = buildInsightStats(usage, records, completions, done, previous);
  const completionTypes = ["Task", "Bug"];
  const durationRows = completionTypes.map((kind) => {
    const sample = done.filter((item) => item.kind === kind);
    return { label: kind + " · " + plural(sample.length, "item", "items"), values: [insightMedian(sample.map((item) => item.lead)), insightMedian(sample.map((item) => item.cycle)), insightMedian(sample.map((item) => item.wait))] };
  });
  const investmentUsage = investmentBy === "type" ? usage : usage.filter(({ type }) => type === "Build (R&D)");
  const investmentRows = insightRows(investmentUsage, investmentBy).map(({ label, total }) => ({
    label,
    values: [insightSum(investmentUsage.filter((record) => record[investmentBy] === label && insightInRange(record, start, end)), (record) => record.cost), total],
  }));
  const { projects: [delivery], counts } = assemblySnapshot(true, id);
  const gates = releaseGates([id]), pending = gates.filter(({ handled }) => !handled);
  const top = insightRows(records.filter(({ item, cancelled }) => item && !cancelled), "item")[0];
  const topTitle = W.find(({ id: taskId, proj }) => taskId === top?.label && proj === id)?.title;
  const customRange = period === "custom" ? `<form class="insights-controls insight-date-range" data-form="insight-range">
      <label class="insight-select">From <input class="field" type="date" name="from" required min="${insightIso(INSIGHT_FIRST_DAY)}" max="${insightIso(INSIGHT_TODAY)}" value="${insightIso(start)}"></label>
      <label class="insight-select">To <input class="field" type="date" name="to" required min="${insightIso(INSIGHT_FIRST_DAY)}" max="${insightIso(INSIGHT_TODAY)}" value="${insightIso(end)}"></label>
      <button class="btn outline sm" id="insight-apply-period" type="submit">Apply dates</button>
    </form>` : "";
  return `<section class="project-insights" aria-labelledby="project-insights-title">
    <header class="insights-header"><div><h2 id="project-insights-title">Insights</h2><p>Demo data · ${insightDayLabel(start)}–${insightDayLabel(end)} · API cost estimates in USD</p></div>
      <div class="insights-controls">
        ${insightSelect("insight-period", "Period", INSIGHT_PERIODS, period)}
        ${insightSelect("insight-display", "View", { chart: "Charts", table: "Tables" }, table ? "table" : "chart")}
        ${records.length ? `<a class="btn outline sm" href="data:text/csv;charset=utf-8,${insightsCsv(records)}" download="${id}-usage.csv">${ic("download", "ic16")}Export CSV</a>` : ""}
      </div>
    </header>
    ${customRange}
    <dl class="insight-kpis">${stats.map(([label, value, context]) => `<div><dt>${label}</dt><dd>${esc(value)}</dd><dd class="insight-kpi-note">${esc(context)}</dd></div>`).join("")}</dl>
    ${insightActionsHtml(id, pending, top, topTitle)}
    ${insightCostGridHtml({ usage, records, buckets, breakdown, by, investmentBy, investmentRows, build, lifetimeBuild, chart, costOptions })}
    ${insightDeliveryGridHtml({ buckets, done, completionTypes, durationRows, chart })}
    ${insightReleaseHtml(id, project, usage, investment, delivery, counts, gates, chart)}
    ${records.length ? insightTable(id, selected ? records.filter((record) => record[by] === selected) : records, selected) : ""}
  </section>`;
}

function updateProjectInsights(params, focus) {
  const content = document.getElementById("content");
  const scroll = content.scrollTop;
  const hash = location.hash.split("?")[0];
  history.replaceState(null, "", hash + (params.size ? "?" + params : ""));
  render(false);
  content.scrollTop = scroll;
  const target = document.getElementById(focus);
  target?.focus({ preventScroll: true });
  if (focus === "insights-usage-heading") target?.scrollIntoView({ block: "nearest" });
}

export {
  INSIGHT_COMPLETIONS,
  INSIGHT_DAY,
  INSIGHT_TODAY,
  insightBuckets,
  insightChart,
  insightCompletions,
  insightDayLabel,
  insightInRange,
  insightMedian,
  projectInsights,
  updateProjectInsights,
};
