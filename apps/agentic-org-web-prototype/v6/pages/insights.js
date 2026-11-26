import { M, P, TEAMS, W } from "../data.js";
import { ACTIVITY_STAGES, RELEASE_GATES, assemblyMerged, assemblyPct, assemblyProgressBar, assemblySnapshot, releaseGateState, releaseGates } from "./activity.js";
import { chartBody } from "./insight-charts.js";
import { own, render } from "../shell/router.js";
import { qs } from "../state.js";
import { esc, ic, plural, sectionHead } from "../ui.js";

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
const INSIGHT_CAPS = { "agency-v1": 16800, "office-federation": 9700 };
const INSIGHT_TOKEN_KEYS = ["Input", "Output", "Cache read", "Cache write"];
// Prototype ledger rows capture team, consumer and work classification at usage time.
// Costs are US cents. Across projects, November to date is $142.00 and the last 7 days $72.33, so the
// organization figures match Home (spend $142, pacing to about $204 by month end).
const INSIGHT_USAGE = {
  "agency-v1": [
    [0, "platform", "kai", "codex", "Task", "Build (R&D)", "T-A121", 1201, [320, 112, 1090, 47]],
    [1, "app", "jules", "codex", "Bug", "Run", "T-A88", 727, [194, 73, 630, 34]],
    [3, "quality", "tomas", "opus", "Task", "Build (R&D)", "T-A140", 970, [221, 68, 481, 23]],
    [5, "app", "jules", "codex", "Task", "Build (R&D)", "T-A124", 1479, [418, 153, 1490, 124]],
    [7, "app", "avery", "codex", "Bug", "Run", "T-A86", 412, [124, 48, 275, 22]],
    [10, "leadership", "ada", "opus", "Task", "Build (R&D)", "T-A112", 689, [185, 65, 750, 25]],
    [12, "quality", "noor", "sonnet", "Task", "Run", "T-A144", 340, [141, 40, 290, 20]],
    [14, "app", "avery", "codex", "Bug", "Build (R&D)", "T-A70", 617, [175, 38, 504, 18]],
    [16, "app", "avery", "codex", "Task", "Build (R&D)", "T-A999", 235, [60, 24, 260, 10], true],
    [17, "app", "jules", "codex", "Task", "Build (R&D)", "T-A124", 844, [244, 94, 800, 40]],
    [20, "leadership", "tess", "", "", "", "", 143, [63, 19, 120, 0]],
    [22, "platform", "kai", "codex", "Task", "Build (R&D)", "T-A121", 781, [204, 66, 810, 34]],
    [25, "platform", "kai", "codex", "Task", "Build (R&D)", "T-A121", 820, [221, 74, 779, 41]],
    [27, "quality", "tomas", "opus", "Task", "Build (R&D)", "T-A140", 640, [173, 58, 608, 32]],
    [29, "platform", "kai", "codex", "Task", "Build (R&D)", "T-A121", 731, [203, 72, 720, 42]],
    [30, "app", "jules", "codex", "Task", "Build (R&D)", "T-A124", 910, [246, 82, 864, 46]],
    [33, "leadership", "ada", "opus", "Task", "Build (R&D)", "T-A112", 560, [151, 50, 532, 28]],
    [35, "quality", "tomas", "opus", "Task", "Run", "T-A140", 470, [110, 55, 350, 14]],
    [37, "app", "avery", "codex", "Bug", "Run", "T-A70", 430, [116, 39, 408, 22]],
    [40, "quality", "noor", "sonnet", "Task", "Run", "T-A144", 380, [103, 34, 361, 19]],
    [42, "app", "avery", "codex", "Bug", "Run", "T-A70", 517, [175, 48, 420, 21]],
    [44, "platform", "kai", "codex", "Task", "Build (R&D)", "T-A121", 720, [194, 65, 684, 36]],
    [47, "quality", "tomas", "opus", "Task", "Build (R&D)", "T-A140", 510, [138, 46, 484, 26]],
    [48, "platform", "kai", "codex", "Task", "Build (R&D)", "T-A121", 382, [108, 42, 220, 15]],
    [52, "leadership", "ada", "opus", "Task", "Build (R&D)", "T-A112", 640, [173, 58, 608, 32]],
    [57, "leadership", "ada", "opus", "Task", "Build (R&D)", "T-A112", 584, [105, 66, 310, 17]],
    [63, "platform", "kai", "codex", "Task", "Build (R&D)", "T-A121", 590, [159, 53, 560, 30]],
    [70, "app", "avery", "codex", "Bug", "Run", "T-A70", 350, [94, 32, 332, 18]],
  ],
  "office-federation": [
    [0, "platform", "sage", "codex", "Task", "Build (R&D)", "T-A118", 987, [204, 86, 780, 91]],
    [3, "platform", "quinn", "codex", "Task", "Build (R&D)", "T-A135", 655, [175, 41, 648, 21]],
    [7, "platform", "rowan", "opus", "Task", "Build (R&D)", "T-A138", 757, [212, 66, 670, 45]],
    [11, "platform", "sage", "codex", "Bug", "Run", "T-A81", 311, [81, 35, 238, 18]],
    [18, "platform", "quinn", "codex", "", "", "", 176, [77, 28, 180, 13]],
    [24, "platform", "sage", "codex", "Task", "Build (R&D)", "T-A118", 610, [165, 55, 580, 30]],
    [26, "platform", "sage", "codex", "Task", "Build (R&D)", "T-A118", 592, [152, 67, 570, 32]],
    [30, "platform", "quinn", "codex", "Task", "Build (R&D)", "T-A135", 540, [146, 49, 513, 27]],
    [33, "platform", "rowan", "opus", "Task", "Build (R&D)", "T-A138", 760, [205, 68, 722, 38]],
    [39, "platform", "rowan", "opus", "Task", "Build (R&D)", "T-A138", 626, [169, 43, 575, 34]],
    [41, "platform", "sage", "codex", "Bug", "Run", "T-A81", 290, [78, 26, 276, 14]],
    [47, "platform", "quinn", "codex", "Task", "Build (R&D)", "T-A135", 380, [103, 34, 361, 19]],
    [55, "platform", "rowan", "opus", "Task", "Build (R&D)", "T-A138", 520, [140, 47, 494, 26]],
    [62, "platform", "quinn", "codex", "Task", "Build (R&D)", "T-A135", 353, [91, 31, 231, 23]],
  ],
  "look-feel": [
    [1, "product", "eden", "opus", "Task", "Build (R&D)", "T-A130", 504, [134, 46, 450, 19]],
    [3, "product", "emery", "opus", "Task", "Build (R&D)", "T-A131", 710, [179, 61, 602, 24]],
    [8, "app", "avery", "codex", "Bug", "Run", "T-A76", 382, [103, 31, 315, 18]],
    [13, "app", "jules", "codex", "Bug", "Run", "T-A84", 487, [116, 43, 356, 20]],
    [17, "app", "avery", "codex", "Task", "Build (R&D)", "T-A133", 365, [93, 30, 246, 15]],
    [22, "product", "eden", "opus", "Task", "Build (R&D)", "T-A130", 428, [108, 34, 281, 19]],
    [26, "product", "eden", "opus", "Task", "Build (R&D)", "T-A130", 470, [127, 42, 446, 24]],
    [29, "app", "avery", "codex", "Bug", "Run", "T-A76", 260, [70, 23, 247, 13]],
    [31, "app", "avery", "codex", "Bug", "Run", "T-A76", 181, [58, 20, 150, 8]],
    [34, "product", "emery", "opus", "Task", "Build (R&D)", "T-A131", 520, [140, 47, 494, 26]],
    [38, "app", "jules", "codex", "Bug", "Run", "T-A84", 300, [81, 27, 285, 15]],
    [43, "product", "eden", "opus", "Task", "Build (R&D)", "T-A130", 410, [111, 37, 390, 20]],
    [45, "product", "emery", "opus", "Task", "Build (R&D)", "T-A131", 458, [124, 38, 318, 23]],
    [50, "product", "emery", "opus", "Task", "Build (R&D)", "T-A131", 380, [103, 34, 361, 19]],
  ],
};
// Completion samples are separate from usage events. A task can incur multiple charges.
// [days ago, type, lead h, cycle h, waiting on you h, blocked h, team, member, outcome flags].
// Across projects the last 7 days hold 19 completions, 16 approved first try (84%) and 15 without input (79%).
// Each completion has one owning team and one contributor.
const INSIGHT_COMPLETIONS = {
  "agency-v1": [
    [0, "Task", 24, 14, 3, 2, "platform", "kai"], [0, "Task", 36, 19, 3, 6, "platform", "kai", "RI"], [0, "Bug", 18, 10, 5, 0, "platform", "rowan"],
    [1, "Bug", 13, 9, 2, 0, "app", "jules"], [2, "Task", 62, 39, 8, 9, "leadership", "ada", "RI"], [5, "Bug", 26, 18, 5, 3, "quality", "noor"],
    [5, "Task", 31, 14, 7, 0, "leadership", "ada"], [6, "Bug", 24, 12, 2, 0, "quality", "tomas"], [6, "Bug", 19, 6, 3, 0, "quality", "noor"],
    [7, "Task", 46, 32, 7, 4, "leadership", "maya", "RI"], [7, "Task", 34, 20, 4, 0, "platform", "sage", "RI"], [8, "Bug", 18, 12, 1, 2, "app", "avery"],
    [8, "Bug", 18, 11, 2, 0, "leadership", "maya"], [11, "Task", 41, 24, 6, 4, "platform", "rowan", "RI"], [13, "Bug", 33, 18, 5, 3, "quality", "tomas"],
    [13, "Task", 27, 18, 3, 0, "app", "jules"], [15, "Bug", 17, 11, 3, 0, "app", "jules"], [15, "Bug", 24, 15, 3, 3, "quality", "tomas"],
    [16, "Task", 30, 19, 2, 0, "platform", "kai"], [17, "Task", 28, 17, 1, 0, "app", "avery"], [18, "Task", 58, 40, 8, 7, "leadership", "ada", "RI"],
    [18, "Task", 29, 19, 2, 0, "leadership", "ada"], [21, "Task", 38, 22, 5, 0, "platform", "sage"], [22, "Task", 36, 22, 5, 3, "quality", "tomas"],
    [23, "Bug", 15, 11, 2, 1, "leadership", "maya"], [24, "Bug", 21, 11, 2, 5, "platform", "rowan"], [25, "Task", 28, 16, 3, 0, "leadership", "maya"],
    [26, "Task", 45, 26, 6, 0, "quality", "noor", "I"], [27, "Bug", 35, 18, 7, 6, "app", "jules", "I"], [28, "Task", 38, 25, 4, 4, "platform", "sage"],
    [31, "Bug", 29, 17, 2, 4, "app", "avery"], [32, "Task", 32, 22, 2, 0, "platform", "kai"], [34, "Task", 48, 26, 4, 6, "quality", "tomas", "I"],
    [35, "Task", 57, 29, 3, 7, "quality", "tomas", "R"], [35, "Task", 52, 37, 4, 0, "platform", "rowan", "R"], [36, "Task", 30, 18, 2, 1, "app", "avery"],
    [39, "Bug", 14, 8, 1, 1, "app", "jules"], [40, "Bug", 26, 19, 5, 0, "leadership", "ada", "I"], [41, "Task", 50, 35, 4, 2, "quality", "noor"],
    [43, "Task", 70, 41, 7, 10, "platform", "quinn", "RI"], [46, "Bug", 32, 21, 3, 0, "platform", "sage", "I"], [47, "Task", 48, 28, 2, 7, "leadership", "maya"],
    [49, "Task", 50, 35, 5, 0, "app", "jules", "RI"], [54, "Bug", 22, 17, 2, 0, "quality", "tomas"], [55, "Task", 55, 45, 7, 0, "platform", "kai", "RI"],
    [58, "Task", 33, 22, 2, 3, "leadership", "ada"], [59, "Task", 47, 31, 7, 2, "app", "avery", "RI"], [60, "Bug", 36, 23, 3, 5, "leadership", "ada", "I"],
  ],
  "office-federation": [
    [2, "Bug", 36, 9, 8, 7, "platform", "rowan"], [3, "Task", 51, 37, 8, 12, "platform", "rowan", "I"], [4, "Bug", 25, 12, 3, 4, "platform", "sage"],
    [5, "Task", 72, 57, 11, 14, "platform", "sage", "RI"], [9, "Bug", 26, 21, 2, 6, "platform", "quinn", "R"], [10, "Bug", 34, 15, 6, 0, "platform", "kai", "I"],
    [12, "Task", 43, 30, 5, 7, "platform", "kai", "R"], [12, "Bug", 32, 10, 6, 2, "platform", "quinn", "I"], [16, "Task", 62, 44, 13, 9, "platform", "rowan", "RI"],
    [20, "Task", 46, 31, 11, 3, "platform", "sage", "RI"], [20, "Bug", 38, 11, 10, 8, "platform", "rowan", "I"], [23, "Task", 54, 38, 3, 0, "platform", "sage", "R"],
    [23, "Task", 47, 30, 9, 0, "platform", "kai", "RI"], [28, "Task", 54, 32, 6, 14, "platform", "quinn", "RI"], [31, "Task", 46, 28, 7, 6, "platform", "quinn", "I"],
    [34, "Bug", 18, 10, 2, 3, "platform", "kai"], [38, "Task", 59, 36, 2, 7, "platform", "rowan", "R"], [42, "Task", 69, 50, 8, 11, "platform", "rowan", "RI"],
    [44, "Task", 67, 60, 2, 3, "platform", "sage", "R"], [51, "Task", 83, 62, 8, 0, "platform", "kai", "RI"], [57, "Task", 80, 68, 7, 0, "platform", "quinn", "RI"],
  ],
  "look-feel": [
    [1, "Bug", 12, 7, 0, 2, "product", "eden"], [1, "Task", 25, 13, 6, 0, "product", "emery"], [3, "Task", 28, 12, 6, 0, "product", "eden"],
    [3, "Task", 18, 10, 4, 0, "product", "eden"], [4, "Task", 34, 26, 4, 1, "product", "emery"], [5, "Bug", 8, 4, 1, 1, "app", "jules"],
    [8, "Bug", 19, 14, 3, 3, "product", "eden"], [9, "Task", 19, 10, 6, 0, "app", "avery", "I"], [11, "Task", 30, 21, 1, 3, "product", "emery"],
    [11, "Task", 22, 10, 3, 0, "product", "emery"], [14, "Bug", 30, 7, 6, 4, "product", "eden"], [16, "Bug", 10, 7, 2, 1, "app", "avery"],
    [19, "Task", 35, 16, 6, 0, "app", "jules"], [20, "Bug", 23, 16, 3, 2, "product", "eden"], [22, "Bug", 24, 9, 2, 0, "app", "avery"],
    [24, "Task", 28, 11, 3, 0, "app", "jules"], [27, "Bug", 18, 11, 5, 0, "product", "emery"], [28, "Task", 33, 16, 6, 0, "product", "eden"],
    [30, "Bug", 22, 15, 5, 0, "product", "emery", "I"], [32, "Task", 41, 33, 4, 4, "product", "emery", "RI"], [36, "Bug", 17, 15, 2, 2, "app", "jules"],
    [39, "Bug", 22, 9, 2, 6, "app", "avery"], [43, "Bug", 31, 13, 6, 0, "product", "eden", "I"], [48, "Task", 26, 14, 3, 0, "app", "jules"],
    [50, "Bug", 33, 20, 5, 0, "app", "avery", "I"], [52, "Bug", 34, 21, 6, 0, "product", "emery"], [56, "Bug", 38, 22, 3, 0, "app", "jules"],
  ],
};
// Outcome flags: "R" = revised before approval (not approved on the first try), "I" = needed your input while it ran.
function insightCompletions(id) {
  return (INSIGHT_COMPLETIONS[id] || []).map(([age, kind, lead, cycle, wait, blocked, team, member, outcome = ""]) => ({
    day: INSIGHT_TODAY - age * INSIGHT_DAY, kind, lead, cycle, wait, blocked, team, member, project: id,
    firstTry: !outcome.includes("R"), autonomous: !outcome.includes("I"),
  }));
}
const insightDateText = new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric", timeZone: "UTC" });
const insightWeekdayText = new Intl.DateTimeFormat(undefined, { weekday: "short", month: "short", day: "numeric", timeZone: "UTC" });
const insightDollars = new Intl.NumberFormat(undefined, { style: "currency", currency: "USD" });
const insightCompact = new Intl.NumberFormat(undefined, { notation: "compact", maximumFractionDigits: 1 });
const insightUsd = (cents) => "≈ " + insightDollars.format(cents / 100);
const insightTickUsd = (cents) => "$" + insightCompact.format(cents / 100);
const insightDayLabel = (day) => insightDateText.format(day);
const insightRange = (first, last) => (first === last ? insightDayLabel(first) : insightDateText.formatRange(new Date(first), new Date(last)));
const insightIso = (day) => new Date(day).toISOString().slice(0, 10);
const insightSum = (items, measure) => items.reduce((total, item) => total + measure(item), 0);
const insightCost = (record) => record.cost;
const insightTokens = (record) => insightSum(record.tokens, (value) => value);
/* Durations follow the shared display rule: whole minutes below 1 h, whole hours below 48 h, then days. */
function insightHours(hours) {
  if (hours < 1) return Math.floor(hours * 60) + " min";
  return hours < 48 ? Math.floor(hours) + " h" : Math.floor(hours * 10 / 24) / 10 + " d";
}
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
/* Period, chart/table view and CSV export: the same controls on project and organization Insights. */
function insightControls({ period, table, records, filename }) {
  return `<div class="insights-controls">
      ${insightSelect("insight-period", "Period", INSIGHT_PERIODS, period)}
      ${insightSelect("insight-display", "View", { chart: "Charts", table: "Tables" }, table ? "table" : "chart")}
      ${records.length ? `<a class="btn outline sm" href="data:text/csv;charset=utf-8,${insightsCsv(records)}" download="${filename}">${ic("download", "ic16")}Export CSV</a>` : ""}
    </div>`;
}
function insightCustomRange(period, start, end) {
  if (period !== "custom") return "";
  const bounds = `min="${insightIso(INSIGHT_FIRST_DAY)}" max="${insightIso(INSIGHT_TODAY)}"`;
  return `<form class="insights-controls insight-date-range" data-form="insight-range">
      <label class="insight-select">From <input class="field" type="date" name="from" required ${bounds} value="${insightIso(start)}"></label>
      <label class="insight-select">To <input class="field" type="date" name="to" required ${bounds} value="${insightIso(end)}"></label>
      <button class="btn outline sm" id="insight-apply-period" type="submit">Apply dates</button>
    </form>`;
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

function insightRowLabel(row) {
  return row.href ? `<a class="insight-link" href="${esc(row.href)}"${row.drill ? ' data-act="insight-drill"' : ""}>${esc(row.label)}</a>` : esc(row.label);
}
/* The table twin of a chart; an average line becomes its own column. */
function insightTableView(title, series, rows, { format, groupLabel = "Group", overlay }) {
  const columns = overlay ? [...series, overlay.name] : series;
  const cells = (row, index) => (overlay ? [...row.values, overlay.values[index]] : row.values)
    .map((value, column) => `<td>${value == null ? "—" : esc((column < series.length ? format : overlay.format)(value))}</td>`).join("");
  return `<div class="insight-table-scroll" tabindex="0" role="region" aria-label="${esc(title)} table"><table>
      <caption class="sr-only">${esc(title)}</caption><thead><tr><th scope="col">${esc(groupLabel)}</th>${columns.map((name) => `<th scope="col">${esc(name)}</th>`).join("")}</tr></thead>
      <tbody>${rows.map((row, index) => `<tr><th scope="row">${insightRowLabel(row)}</th>${cells(row, index)}</tr>`).join("")}</tbody>
    </table></div>`;
}

/* A chart card. kind: "columns" (counts per day or week), "line" (a trend), "bars" (compare groups, the
   default) or "split" (one whole in a few parts). The legacy columns/trend flags still select a kind. */
function insightChart(key, title, note, series, rows, options = {}) {
  const { table, control = "", empty = "No data for this period.", format = (value) => String(value), level = "h3" } = options;
  const kind = options.kind || (options.columns ? "columns" : options.trend ? "line" : "bars");
  const blank = !rows.length || rows.every((row) => row.values.every((value) => !value));
  const chartRows = rows.map((row) => (row.href ? { ...row, labelHtml: insightRowLabel(row) } : row));
  const content = blank ? `<p class="insight-empty">${esc(empty)}</p>`
    : table ? insightTableView(title, series, rows, { format, groupLabel: options.groupLabel, overlay: options.overlay })
      : chartBody(kind, series, chartRows, { ...options, format, plotFormat: options.plotFormat || format, label: title });
  return `<section class="insight-card${options.wide ? " is-wide" : ""}" data-chart="${blank ? "empty" : table ? "table" : kind}" aria-labelledby="insight-${key}-title">
    <header class="insight-card-head"><div><${level} id="insight-${key}-title">${title}</${level}><p>${note}</p></div>${control}</header>
    ${content}
  </section>`;
}

/* Time buckets follow the cost-read rule: days through 31 days, Monday weeks after that, partial edges labelled. */
const insightWeekStart = (day) => day - ((new Date(day).getUTCDay() + 6) % 7) * INSIGHT_DAY;
function insightDays(start, end) {
  return Array.from({ length: Math.round((end - start) / INSIGHT_DAY) + 1 }, (_, index) => {
    const day = start + index * INSIGHT_DAY;
    return { first: day, last: day, label: insightDayLabel(day), axis: insightDayLabel(day), tip: insightWeekdayText.format(day) };
  });
}
function insightWeeks(start, end) {
  const weeks = [];
  for (let first = start; first <= end; first = insightWeekStart(first) + 7 * INSIGHT_DAY) {
    const last = Math.min(end, insightWeekStart(first) + 6 * INSIGHT_DAY);
    const range = insightRange(first, last), partial = last - first < 6 * INSIGHT_DAY;
    weeks.push({ first, last, label: range, axis: insightDayLabel(first), tip: `Week of ${range}${partial ? " · partial week" : ""}` });
  }
  return weeks;
}
const insightDaily = (start, end) => (end - start) / INSIGHT_DAY < 31;
const insightSeriesBuckets = (start, end) => (insightDaily(start, end) ? insightDays(start, end) : insightWeeks(start, end));
/* Daily amounts carry their trailing 7-day average as a line; weekly totals are smooth enough without one. */
function insightAverage(daily, buckets, items, measure, format) {
  if (!daily) return null;
  const values = buckets.map(({ first }) => measure(items.filter((item) => insightInRange(item, first - 6 * INSIGHT_DAY, first))) / 7);
  return { name: "7-day average", format, values };
}
const insightTimeRows = (buckets, measure) => buckets.map((bucket) => ({ label: bucket.label, axis: bucket.axis, tip: bucket.tip, values: measure(bucket) }));

/* Kept for Team overview: equal-width buckets from the start date. */
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
/* Change against the previous period of the same length. better: "up", "down" or "none" (no judgement). */
function insightChange(current, previous, better = "up", points = false) {
  if (previous == null || (!points && !previous)) return { note: "Prior period unavailable" };
  const diff = Math.round(points ? current - previous : 100 * (current - previous) / previous);
  const tone = !diff || better === "none" ? "" : (diff > 0) === (better === "up") ? "ok" : "bad";
  return { delta: `${diff > 0 ? "+" : diff < 0 ? "−" : "±"}${Math.abs(diff)}${points ? " pts" : "%"}`, tone, note: "vs prior period" };
}
function insightKpis(stats, label = "") {
  return `<dl class="insight-kpis"${label ? ` aria-label="${esc(label)}"` : ""}>${stats.map(({ label: name, value, sub = "", delta = "", tone = "", note = "" }) => `<div><dt>${esc(name)}</dt><dd>${esc(value)}</dd>
      ${sub ? `<dd class="insight-kpi-sub">${esc(sub)}</dd>` : ""}<dd class="insight-kpi-note">${delta ? `<span class="insight-delta"${tone ? ` data-tone="${tone}"` : ""}>${esc(delta)}</span> ` : ""}${esc(note)}</dd></div>`).join("")}</dl>`;
}
const insightShareText = (rows, format) => {
  const total = insightSum(rows, (row) => row.values[0]);
  return (_row, value) => `${format(value)} · ${total ? Math.round(100 * value / total) : 0}%`;
};

function insightTable(records, selected, showProject = false) {
  const item = (record) => (record.cancelled ? esc(record.item) + " (cancelled)" : record.item ? `<a class="insight-link" href="${insightWorkHref(record.project, record.item)}">${esc(record.item)}</a>` : "Not on a task");
  return `<section class="insight-card insight-records" aria-labelledby="insights-usage-heading">
    <header class="insight-card-head"><div><h3 id="insights-usage-heading" tabindex="-1">Usage records${selected ? " · " + esc(selected) : ""}</h3>
      <p>Captured attribution and estimated API cost</p></div>${selected ? '<button class="btn outline sm" type="button" data-act="insight-clear-drill">Show all records</button>' : ""}</header>
    ${records.length ? `<div class="insight-table-scroll" tabindex="0" role="region" aria-label="Usage record details"><table><caption class="sr-only">Usage records</caption>
      <thead><tr><th scope="col">Date</th><th scope="col">Work item</th>${showProject ? '<th scope="col">Project</th>' : ""}<th scope="col">Consumer</th><th scope="col">Model</th><th scope="col">Tokens</th><th scope="col">Estimated cost</th></tr></thead>
      <tbody>${records.map((record) => `<tr><td>${insightDayLabel(record.day)}</td><th scope="row">${item(record)}</th>${showProject ? `<td>${esc(P[record.project].name)}</td>` : ""}
        <td>${esc(record.consumer)}</td><td>${esc(record.model)}</td><td>${insightTokens(record).toLocaleString()}</td><td>${insightUsd(record.cost)}</td></tr>`).join("")}</tbody></table></div>` : '<p class="insight-empty">No usage records for this selection.</p>'}
  </section>`;
}

function insightsCsv(records) {
  const rows = [["record_id", "date", "project", "release", "milestone", "team", "consumer", "model", "task", "task_type", "work_type", "attribution", "input_tokens", "output_tokens", "cache_read_tokens", "cache_write_tokens", "estimated_cost_usd"],
    ...records.map(({ key, day, project, release, milestone, team, consumer, model, item, kind, type, attribution, tokens, cost }) =>
      [key, insightIso(day), project, release, milestone, team, consumer, model, item, kind, type, attribution, ...tokens, (cost / 100).toFixed(2)])];
  return encodeURIComponent(rows.map((row) => row.map((cell) => '"' + String(cell).replaceAll('"', '""') + '"').join(",")).join("\r\n"));
}

function buildProjectUsage(id, project = P[id]) {
  return (INSIGHT_USAGE[id] || []).map(([age, team, consumer, model, kind, type, item, cost, tokens, cancelled = false], index) => ({
    key: id + "-" + (index + 1), day: INSIGHT_TODAY - age * INSIGHT_DAY, project: id, projectName: project.name,
    team: TEAMS[team].name, teamId: team, consumer: M[consumer].name, consumerId: consumer, model: INSIGHT_MODELS[model] || "Unknown model",
    kind: kind || "Unknown type", type: type || "Unknown work type", item, task: item || "Not on a task", cost, cancelled,
    attribution: cancelled ? "Cancelled work" : item ? "Task-linked" : "Not on a task",
    tokens: tokens.map((value) => value * 1000), release: project.release || "No release", milestone: project.milestoneTitle || "No milestone",
  }));
}

function insightCycleStat(done, prior) {
  const median = insightMedian(done.map(({ cycle }) => cycle)), priorMedian = insightMedian(prior.map(({ cycle }) => cycle));
  if (median === null) return { label: "Median cycle time", value: "—", note: "At least 5 completed items needed" };
  const change = priorMedian === null ? { note: plural(done.length, "completed item", "completed items") } : insightChange(median, priorMedian, "down");
  return { label: "Median cycle time", value: insightHours(median), ...change };
}
function buildInsightStats(usage, records, completions, done, previous) {
  const spend = insightSum(records, insightCost), tokens = insightSum(records, insightTokens);
  const priorUsage = previous(usage);
  return [
    { label: "Investment to date", value: usage.length ? insightUsd(insightSum(usage, insightCost)) : "—", note: "All recorded usage" },
    { label: "Period cost", value: records.length ? insightUsd(spend) : "—", ...(records.length ? insightChange(spend, insightSum(priorUsage, insightCost), "none") : { note: "No recorded usage" }) },
    { label: "Tokens", value: records.length ? insightCompact.format(tokens) : "—", ...(records.length ? insightChange(tokens, insightSum(priorUsage, insightTokens), "none") : { note: "No recorded usage" }) },
    { label: "Completed", value: String(done.length), ...(done.length ? insightChange(done.length, previous(completions).length) : { note: "No recorded completions" }) },
    insightCycleStat(done, previous(completions)),
    { label: "Working time", value: "—", note: "Not tracked yet" },
  ];
}

function insightActionsHtml(id, pending, top, topTitle) {
  if (!pending.length && !topTitle) return "";
  return `<section class="insight-actions" aria-label="Project actions">
      ${pending.length ? `<a href="#/inbox/${pending[0].id}">${ic("flag", "ic16")}<span><strong>${plural(pending.length, "release gate", "release gates")} waiting for you</strong><small>${esc(RELEASE_GATES[pending[0].id])}</small></span>${ic("chev-right", "ic16")}</a>` : ""}
      ${topTitle ? `<a href="${insightWorkHref(id, top.label)}">${ic("signal", "ic16")}<span><strong>Highest task spend this period · ${insightUsd(top.total)}</strong><small>${esc(top.label + " " + topTitle)}</small></span>${ic("chev-right", "ic16")}</a>` : ""}
    </section>`;
}

function insightCostGridHtml({ usage, records, buckets, unit, breakdown, by, investmentBy, investmentRows, build, lifetimeBuild, chart, costOptions }) {
  const spendAverage = insightAverage(unit === "day", buckets, usage, (items) => insightSum(items, insightCost), (value) => insightUsd(value) + " a day");
  const has = records.length > 0, compact = (value) => insightCompact.format(value), count = (value) => value.toLocaleString();
  const tokenMix = has ? INSIGHT_TOKEN_KEYS.map((label, index) => ({ label, values: [insightSum(records, (record) => record.tokens[index])] })) : [];
  const invNote = usage.length ? `Build (R&D) ${has ? insightUsd(build) : "—"} this period · ${insightUsd(lifetimeBuild)} to date` : "Recorded build and run costs";
  return `${sectionHead({ id: "insights-cost-title", title: "Cost and usage", level: "h3" })}<div class="insights-grid">
      ${chart("spend", "Spend over time", `Estimated cost per ${unit}${spendAverage ? ", with the 7-day average" : ""}`, ["Estimated cost"], has ? insightTimeRows(buckets, (bucket) => [bucket.spend]) : [], {
        ...costOptions, kind: "columns", overlay: spendAverage, tones: [spendAverage ? "tint" : "accent"], tick: insightTickUsd, groupLabel: unit === "day" ? "Day" : "Week", empty: "No recorded usage." })}
      ${chart("breakdown", "Spending breakdown", "Open a group to see its usage records", ["Estimated cost"], breakdown, {
        ...costOptions, valueText: insightShareText(breakdown, insightUsd), control: insightSelect("insight-breakdown", "Group spend by", INSIGHT_GROUPS, by), empty: "No recorded usage." })}
      ${chart("tokens-trend", "Tokens over time", `Total tokens per ${unit}`, ["Tokens"], has ? insightTimeRows(buckets, (bucket) => [bucket.tokens]) : [],
        { kind: "columns", format: count, plotFormat: compact, groupLabel: unit === "day" ? "Day" : "Week", empty: "No recorded usage." })}
      ${chart("tokens", "Token mix", "Input, output, and cache usage", ["Tokens"], tokenMix, { format: count, plotFormat: compact, valueText: insightShareText(tokenMix, compact), empty: "No recorded usage." })}
      ${chart("investment", investmentBy === "type" ? "Investment by work type" : "R&D investment", invNote, ["Period", "To date"], investmentRows, {
        ...costOptions, legend: ["This period", "Earlier"], stack: ([period, total]) => [period, total - period],
        valueText: (row) => `${insightUsd(row.values[0])} of ${insightUsd(row.values[1])}`,
        control: insightSelect("insight-investment", "Group investment by", INSIGHT_INVESTMENT, investmentBy) })}
    </div>`;
}

function insightDeliveryGridHtml({ buckets, unit, done, completionTypes, durationRows, chart }) {
  const completedSeries = done.length ? insightTimeRows(buckets, (bucket) => completionTypes.map((kind) => bucket.done.filter((item) => item.kind === kind).length)) : [];
  const timingRows = done.length ? [
    { label: "Waiting on you", values: [insightSum(done, (item) => item.wait)] },
    { label: "Blocked", values: [insightSum(done, (item) => item.blocked)] },
    { label: "The rest", values: [insightSum(done, (item) => item.lead - item.wait - item.blocked)] },
  ] : [];
  return `${sectionHead({ id: "insights-delivery-title", title: "Delivery and speed", level: "h3" })}<div class="insights-grid">
      ${chart("completed", "Completed over time", `Tasks and bugs completed per ${unit}`, completionTypes, completedSeries,
        { kind: "columns", integer: true, groupLabel: unit === "day" ? "Day" : "Week", empty: "No recorded completions." })}
      ${chart("speed", "Lead and cycle time", "Median by task type · at least five completed items", ["Lead", "Cycle", "Waiting on you"], done.length ? durationRows : [], { format: insightHours, empty: "No recorded completions." })}
      ${chart("timing", "Where time went", "Total lead time across completed items", ["Hours"], timingRows, { kind: "split", tones: ["warn", "bad", "tint"], format: insightHours, empty: "No recorded durations." })}
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
  const readiness = delivery.features.map(({ label, entries }) => {
    const merged = assemblyMerged(entries);
    return { label, values: [merged, entries.length - merged] };
  });
  return `${sectionHead({ id: "insights-release-title", title: `Current release · ${esc(project.release)}`, level: "h3" })}
    <div class="insights-grid">
      ${chart("pipeline", "Work by stage", "Current Assembly line", ["Tasks"], delivery.entries.length ? ACTIVITY_STAGES.map(([stage, label]) => ({ label, values: [counts[stage]] })) : [], { empty: "No tasks on this Assembly line." })}
      ${chart("readiness", "Feature readiness", `${assemblyPct(delivery.entries)}% of release tasks merged`, ["Merged", "Remaining"], readiness, {
        tones: ["ok", "tint"], stack: (values) => values, valueText: (row) => `${row.values[0]} of ${row.values[0] + row.values[1]} merged`, empty: "No release features yet." })}
      <section class="insight-card" aria-labelledby="insight-release-title">
        <header class="insight-card-head"><div><h4 id="insight-release-title">Release gates and cap</h4><p>Current release</p></div><a class="insight-link" href="#/projects/${id}/activity">Open Assembly line</a></header>
        ${capLine}
        ${gateList}
      </section>
    </div>`;
}

function projectBreakdown(id, records, by, params) {
  return insightRows(records, by).map(({ label, total }) => {
    const next = new URLSearchParams(params);
    next.set("group", label);
    return { label, values: [total], href: "#/projects/" + id + "/insights?" + next.toString(), drill: true };
  });
}
function projectDurations(done, completionTypes) {
  return completionTypes.map((kind) => {
    const sample = done.filter((item) => item.kind === kind);
    return { label: kind + " · " + plural(sample.length, "item", "items"), values: [insightMedian(sample.map((item) => item.lead)), insightMedian(sample.map((item) => item.cycle)), insightMedian(sample.map((item) => item.wait))] };
  }).filter((row) => row.values.some((value) => value != null));
}
function projectInvestmentRows(usage, investmentBy, start, end) {
  const investmentUsage = investmentBy === "type" ? usage : usage.filter(({ type }) => type === "Build (R&D)");
  return insightRows(investmentUsage, investmentBy).map(({ label, total }) => ({
    label,
    values: [insightSum(investmentUsage.filter((record) => record[investmentBy] === label && insightInRange(record, start, end)), insightCost), total],
  }));
}
function insightBucketTotals(start, end, records, done) {
  return insightSeriesBuckets(start, end).map((bucket) => {
    const spent = records.filter((item) => insightInRange(item, bucket.first, bucket.last));
    return { ...bucket, spend: insightSum(spent, insightCost), tokens: insightSum(spent, insightTokens), done: done.filter((item) => insightInRange(item, bucket.first, bucket.last)) };
  });
}

function projectInsights(id) {
  const project = P[id], params = qs();
  const usage = buildProjectUsage(id, project), completions = insightCompletions(id);
  const { period, start, end } = insightPeriod(usage, completions, params);
  const priorStart = start - (end - start + INSIGHT_DAY);
  const previous = (items) => items.filter((item) => insightInRange(item, priorStart, start - INSIGHT_DAY));
  const records = usage.filter((item) => insightInRange(item, start, end));
  const done = completions.filter((item) => insightInRange(item, start, end));
  const investment = insightSum(usage, insightCost);
  const build = insightSum(records.filter(({ type }) => type === "Build (R&D)"), insightCost);
  const lifetimeBuild = insightSum(usage.filter(({ type }) => type === "Build (R&D)"), insightCost);
  const table = params.get("display") === "table";
  const by = own(INSIGHT_GROUPS, params.get("by")) ? params.get("by") : "team";
  const investmentBy = own(INSIGHT_INVESTMENT, params.get("investmentBy")) ? params.get("investmentBy") : "type";
  const costOptions = { table, format: insightUsd };
  // Cards sit under the h3 section heads of the h2 "Insights" region.
  const chart = (key, title, note, series, rows, display) => insightChart(key, title, note, series, rows, { table, level: "h4", ...display });
  const buckets = insightBucketTotals(start, end, records, done), unit = insightDaily(start, end) ? "day" : "week";
  const breakdown = projectBreakdown(id, records, by, params);
  const selected = breakdown.some(({ label }) => label === params.get("group")) ? params.get("group") : "";
  const completionTypes = ["Task", "Bug"];
  const { projects: [delivery], counts } = assemblySnapshot(true, id);
  const gates = releaseGates([id]), pending = gates.filter(({ handled }) => !handled);
  const top = insightRows(records.filter(({ item, cancelled }) => item && !cancelled), "item")[0];
  const topTitle = W.find(({ id: taskId, proj }) => taskId === top?.label && proj === id)?.title;
  return `<section class="project-insights" aria-labelledby="project-insights-title">
    <header class="insights-header"><div><h2 id="project-insights-title">Insights</h2><p>Demo data · ${insightDayLabel(start)}–${insightDayLabel(end)} · API cost estimates in USD</p></div>
      ${insightControls({ period, table, records, filename: `${id}-usage.csv` })}
    </header>
    ${insightCustomRange(period, start, end)}
    ${insightKpis(buildInsightStats(usage, records, completions, done, previous))}
    ${insightActionsHtml(id, pending, top, topTitle)}
    ${insightCostGridHtml({ usage, records, buckets, unit, breakdown, by, investmentBy, investmentRows: projectInvestmentRows(usage, investmentBy, start, end), build, lifetimeBuild, chart, costOptions })}
    ${insightDeliveryGridHtml({ buckets, unit, done, completionTypes, durationRows: projectDurations(done, completionTypes), chart })}
    ${insightReleaseHtml(id, project, usage, investment, delivery, counts, gates, chart)}
    ${records.length ? insightTable(selected ? records.filter((record) => record[by] === selected) : records, selected) : ""}
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
  INSIGHT_GROUPS,
  INSIGHT_TODAY,
  INSIGHT_USAGE,
  buildProjectUsage,
  insightBuckets,
  insightChange,
  insightChart,
  insightCompletions,
  insightControls,
  insightCost,
  insightCustomRange,
  insightAverage,
  insightDayLabel,
  insightDaily,
  insightDollars,
  insightHours,
  insightInRange,
  insightKpis,
  insightMedian,
  insightPeriod,
  insightRange,
  insightRows,
  insightSelect,
  insightSeriesBuckets,
  insightShareText,
  insightSum,
  insightTable,
  insightTickUsd,
  insightTimeRows,
  insightUsd,
  insightWeeks,
  projectInsights,
  updateProjectInsights,
};
