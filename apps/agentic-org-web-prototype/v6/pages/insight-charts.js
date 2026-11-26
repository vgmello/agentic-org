/* Insight charts: small HTML + SVG chart primitives shared by project, team and organization Insights.
   - Marks take their colour from data-tone (accent, tint, ink, ok, warn, bad); text always uses text tokens.
   - Columns and lines share one frame: a y axis with clean ticks, hairline gridlines and thinned x labels.
   - Every column or point is a focus stop (one tab stop per chart, arrow keys inside) with the same readout
     on hover and focus; the readout is also in the accessible text, and each chart has a table twin. */
import { esc } from "../ui.js";

const sum = (values) => values.reduce((total, value) => total + (value ?? 0), 0);

/* Clean axis ticks from 0 to a rounded top: 0 / 5 / 10 / 15 / 20, never 0 / 3.7 / 7.4. */
function niceTicks(max, integer = false) {
  if (!(max > 0)) return [0, 1];
  const raw = max / 4, magnitude = 10 ** Math.floor(Math.log10(raw));
  const steps = (integer ? [1, 2, 5, 10] : [1, 2, 2.5, 5, 10]).map((step) => step * magnitude);
  const step = Math.max(integer ? 1 : 0, steps.find((value) => value >= raw - 1e-9));
  const top = Math.ceil(max / step - 1e-9) * step;
  return Array.from({ length: Math.round(top / step) + 1 }, (_, index) => Number((index * step).toPrecision(6)));
}

/* Which x labels show. Nested strides counted back from the latest point (always labelled): about eight
   labels on a wide chart, four on a narrow one, two on the narrowest; CSS picks the level by chart width. */
function axisLevel(index, count) {
  const fromEnd = count - 1 - index, step = Math.ceil(count / 8);
  if (fromEnd % (4 * step) === 0) return "few";
  if (fromEnd % (2 * step) === 0) return "some";
  return fromEnd % step === 0 ? "many" : "none";
}
function edge(index, count) {
  const position = (index + .5) / count;
  return position < .25 ? "start" : position > .75 ? "end" : "";
}
function scale(ticks) {
  const low = ticks[0], span = ticks.at(-1) - low || 1;
  return (value) => Math.min(1, Math.max(0, (value - low) / span));
}

function frame(ticks, tick, plot) {
  const at = scale(ticks), width = Math.max(...ticks.map((value) => tick(value).length));
  return `<div class="ichart-frame">
      <ul class="ichart-y" aria-hidden="true" style="--y-w:${width}">${ticks.map((value) => `<li style="--at:${at(value).toFixed(4)}">${esc(tick(value))}</li>`).join("")}</ul>
      <div class="ichart-plot"><ul class="ichart-grid" aria-hidden="true">${ticks.map((value) => `<li style="--at:${at(value).toFixed(4)}"></li>`).join("")}</ul>${plot}</div>
    </div>`;
}

/* One readout line per series: the value leads, the series name follows. */
function readout(lines) {
  return lines.map(([tone, value, name]) => `<span class="ichart-tip-row"><i data-tone="${tone}"></i><b>${esc(value)}</b>${name ? ` ${esc(name)}` : ""}</span>`).join("");
}
function hit(row, index, count, marks, lines) {
  const heading = row.tip || row.label;
  const spoken = `${heading}: ${lines.map(([, value, name]) => (name ? `${name} ${value}` : value)).join(", ")}`;
  return `<li class="ichart-hit" tabindex="${index === count - 1 ? 0 : -1}" data-axis="${axisLevel(index, count)}" data-edge="${edge(index, count)}">
      ${marks}<span class="ichart-x" aria-hidden="true">${esc(row.axis || row.label)}</span>
      <span class="ichart-tip" aria-hidden="true"><span class="ichart-tip-h">${esc(heading)}</span>${readout(lines)}</span>
      <span class="sr-only">${esc(spoken)}</span></li>`;
}
function hits(label, count, items) {
  return `<ol class="ichart-hits" role="list" style="--n:${count}" aria-label="${esc(label)}. ${count} points, arrow keys move between them.">${items.join("")}</ol>`;
}

/* Lines are drawn in a 0..count by 0..100 box stretched over the plot; strokes keep their 2px width. */
function runs(values) {
  const out = [];
  values.forEach((value, index) => {
    if (value == null) return;
    const last = out.at(-1);
    if (last && last.at(-1)[0] === index - 1) last.push([index, value]);
    else out.push([[index, value]]);
  });
  return out;
}
function paths(values, at, tone, area) {
  return runs(values).map((run) => {
    const points = run.map(([index, value]) => `${index + .5},${(100 - at(value) * 100).toFixed(2)}`).join(" ");
    const wash = area && run.length > 1 ? `<polygon data-tone="${tone}" points="${run[0][0] + .5},100 ${points} ${run.at(-1)[0] + .5},100"></polygon>` : "";
    return `${wash}<polyline data-tone="${tone}" points="${points}"></polyline>`;
  }).join("");
}
function lineSvg(count, body) {
  return `<svg class="ichart-lines" viewBox="0 0 ${count} 100" preserveAspectRatio="none" aria-hidden="true" focusable="false">${body}</svg>`;
}

/* Columns: one value per bucket, stacked when there are several series, with an optional average line. */
function columnsChart({ label, series, rows, tones, format, tick, integer, overlay }) {
  const overlayPeak = overlay ? Math.max(0, ...overlay.values.filter((value) => value != null)) : 0;
  const ticks = niceTicks(Math.max(overlayPeak, ...rows.map((row) => sum(row.values))), integer);
  const at = scale(ticks), count = rows.length;
  const items = rows.map((row, index) => {
    const marks = row.values.map((value, s) => (value > 0 ? `<i data-tone="${tones[s]}" style="--v:${at(value).toFixed(4)}"></i>` : "")).join("");
    const lines = series.map((name, s) => [tones[s], format(row.values[s] ?? 0), series.length > 1 || overlay ? name : ""]);
    if (overlay && overlay.values[index] != null) lines.push(["ink", overlay.format(overlay.values[index]), overlay.name]);
    return hit(row, index, count, `<span class="ichart-bars" aria-hidden="true">${marks}</span>`, lines);
  });
  const line = overlay ? lineSvg(count, paths(overlay.values, at, "ink", false)) : "";
  return frame(ticks, tick, `${hits(label, count, items)}${line}`);
}

/* Lines: one point per bucket per series; a gap where a bucket has too little data to report. */
function lineChart({ label, series, rows, tones, format, tick, domain, area }) {
  const values = rows.flatMap((row) => row.values).filter((value) => value != null);
  const ticks = domain || niceTicks(Math.max(0, ...values));
  const at = scale(ticks), count = rows.length;
  const items = rows.map((row, index) => {
    const dots = row.values.map((value, s) => (value == null ? "" : `<i class="ichart-dot" data-tone="${tones[s]}" style="--v:${at(value).toFixed(4)}"></i>`)).join("");
    const lines = series.map((name, s) => [tones[s], row.values[s] == null ? "Not enough data" : format(row.values[s]), series.length > 1 ? name : ""]);
    return hit(row, index, count, dots, lines);
  });
  const svg = lineSvg(count, series.map((_, s) => paths(rows.map((row) => row.values[s]), at, tones[s], area && series.length === 1)).join(""));
  return `<div class="ichart-line" data-dots="${count > 12 ? "end" : "all"}">${frame(ticks, tick, `${svg}${hits(label, count, items)}`)}</div>`;
}

/* Horizontal bars with the value at the tip. Several series draw one thin bar each; a stack splits one bar. */
function barLine(value, peak, tone, text, name) {
  return `<span class="ibars-line"><span class="ibars-bar" style="--v:${(value / peak).toFixed(4)}" aria-hidden="true">${value > 0 ? `<i data-tone="${tone}"></i>` : ""}</span>
      <span class="ibars-value">${name ? `<span class="sr-only">${esc(name)} </span>` : ""}${esc(text)}</span></span>`;
}
function stackLine(parts, peak, tones, text) {
  const marks = parts.map((value, s) => (value > 0 ? `<i data-tone="${tones[s]}" style="flex-grow:${value}"></i>` : "")).join("");
  return `<span class="ibars-line"><span class="ibars-bar" style="--v:${(sum(parts) / peak).toFixed(4)}" aria-hidden="true">${marks}</span>
      <span class="ibars-value">${esc(text)}</span></span>`;
}
function barTexts(rows, { stack, valueText, plotFormat }) {
  return rows.map((row) => (stack ? [valueText(row)]
    : row.values.map((value) => (value == null ? "—" : valueText ? valueText(row, value) : plotFormat(value)))));
}
function barsChart({ series, rows, tones, plotFormat, stack, valueText }) {
  const parts = (row) => (stack ? stack(row.values) : row.values.map((value) => value ?? 0));
  const peak = Math.max(1e-9, ...rows.map((row) => (stack ? sum(parts(row)) : Math.max(...parts(row)))));
  const texts = barTexts(rows, { stack, valueText, plotFormat });
  const width = Math.max(...texts.flat().map((text) => text.length)) + 1;
  const lines = (row, index) => (stack ? stackLine(parts(row), peak, tones, texts[index][0])
    : parts(row).map((value, s) => barLine(value, peak, tones[s], texts[index][s], series.length > 1 ? series[s] : "")).join(""));
  return `<ol class="ibars" role="list" style="--value-w:${width}ch">${rows.map((row, index) => `<li>
      <span class="ibars-label">${row.labelHtml || esc(row.label)}</span><span class="ibars-lines">${lines(row, index)}</span>
    </li>`).join("")}</ol>`;
}

/* One whole split into a few parts: a thin segmented bar above a list that carries every figure. */
function splitChart({ rows, tones, format }) {
  const total = sum(rows.map((row) => row.values[0]));
  const share = (value) => (total ? Math.round(100 * value / total) : 0);
  const bar = rows.map((row, s) => (row.values[0] > 0 ? `<i data-tone="${tones[s]}" style="flex-grow:${row.values[0]}"></i>` : "")).join("");
  return `<div class="isplit"><div class="isplit-bar" aria-hidden="true">${bar}</div>
    <ul class="isplit-list" role="list">${rows.map((row, s) => `<li><span class="isplit-key"><i data-tone="${tones[s]}" aria-hidden="true"></i>${row.labelHtml || esc(row.label)}</span>
      <span class="isplit-value">${esc(format(row.values[0]))}</span><span class="isplit-share">${share(row.values[0])}%</span></li>`).join("")}</ul></div>`;
}

function legend(series, tones, shape, overlay) {
  const keys = series.map((name, s) => `<li><i data-tone="${tones[s]}" data-shape="${shape}"></i>${esc(name)}</li>`);
  if (overlay) keys.push(`<li><i data-tone="ink" data-shape="line"></i>${esc(overlay.name)}</li>`);
  return `<ul class="ichart-legend" aria-hidden="true">${keys.join("")}</ul>`;
}

const RENDER = { columns: columnsChart, line: lineChart, bars: barsChart, split: splitChart };
const DEFAULT_TONES = ["accent", "tint", "warn"];

/* The chart body for a card: a legend for two or more series, the chart, and a thin-data note when needed. */
function chartBody(kind, series, rows, options) {
  const tones = options.tones || DEFAULT_TONES;
  const keyed = kind !== "split" && (series.length > 1 || options.overlay);
  const body = RENDER[kind]({ ...options, series, rows, tones, tick: options.tick || options.plotFormat });
  const thin = options.thin ? `<p class="insight-thin">${esc(options.thin)}</p>` : "";
  return `${keyed ? legend(options.legend || series, tones, kind === "line" ? "line" : "box", options.overlay) : ""}<div class="ichart" data-kind="${kind}">${body}</div>${thin}`;
}

/* Arrow keys move between the points of one chart; Home and End jump to its ends. */
document.addEventListener("keydown", (event) => {
  const current = event.target.closest?.(".ichart-hit");
  if (!current || !["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
  const all = [...current.parentElement.children], index = all.indexOf(current);
  const step = { ArrowLeft: index - 1, ArrowRight: index + 1, Home: 0, End: all.length - 1 }[event.key];
  const next = all[Math.min(all.length - 1, Math.max(0, step))];
  event.preventDefault();
  current.tabIndex = -1;
  next.tabIndex = 0;
  next.focus();
});

export { chartBody, niceTicks };
