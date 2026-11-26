/* "Between agents" in the conversation list, built for many agents (10+ agents is up to ~45 pairs):
   the last day's conversations newest first, "Show all N" for the rest, and a filter by agent
   (one agent: every conversation it is in; two: that pair). */
import { CH, M } from "../data.js";
import { esc, ic, plural } from "../ui.js";
import { activeToday, newestFirst } from "./chat-model.js";

const a2a = { all: false, filter: false, agents: [] };
const MAX_PICKS = 2;

/* Every agent that has a conversation with another agent, by name. */
const pairAgents = () => [...new Set(Object.values(CH).filter((ch) => ch.kind === "a2a").flatMap((ch) => ch.pair))]
  .filter((k) => M[k]).sort((a, b) => M[a].name.localeCompare(M[b].name));

/* Rows shown, and how many older ones wait behind Show all. A filter shows every match, whatever its age;
   the open conversation always shows. */
function visibleRows(keys, activeId) {
  const sorted = [...keys].sort(newestFirst);
  if (a2a.agents.length) return { rows: sorted.filter((k) => a2a.agents.every((a) => CH[k].pair.includes(a))), hidden: 0 };
  if (a2a.all) return { rows: sorted, hidden: 0 };
  const rows = sorted.filter((k) => activeToday(CH[k]) || k === activeId);
  return { rows, hidden: sorted.length - rows.length };
}
function chipHtml(k) {
  const on = a2a.agents.includes(k), full = !on && a2a.agents.length >= MAX_PICKS;
  return `<li><button class="chat-a2a-chip" id="chat-a2a-ag-${esc(k)}" type="button" data-act="chat-a2a-agent" data-id="${esc(k)}" aria-pressed="${on}"${full ? " disabled" : ""}>${on ? ic("check", "ic14") : ""}${esc(M[k].name)}</button></li>`;
}
function filterPanel() {
  if (!a2a.filter) return "";
  const clear = a2a.agents.length ? `<button class="btn ghost sm chat-a2a-clear" id="chat-a2a-clear" type="button" data-act="chat-a2a-clear">Clear</button>` : "";
  return `<div class="chat-a2a-filter" id="chat-a2a-filter" role="group" aria-labelledby="chat-a2a-filter-label">
      <p class="chat-a2a-hint" id="chat-a2a-filter-label">Pick one agent for all its conversations, or two for their pair.</p>
      <ul class="chat-a2a-chips">${pairAgents().map(chipHtml).join("")}</ul>${clear}
    </div>`;
}
function filterButton() {
  const n = a2a.agents.length, label = n ? `Filter by agent, ${a2a.agents.map((k) => M[k].name).join(" and ")}` : "Filter by agent";
  return `<button class="btn ghost sm chat-a2a-filter-btn" id="chat-a2a-filter-btn" type="button" data-act="chat-a2a-filter" aria-expanded="${a2a.filter}" aria-controls="chat-a2a-filter" aria-label="${esc(label)}" title="Filter by agent">${ic("filter", "ic16")}<span>Filter</span>${n ? `<span class="chat-a2a-count" aria-hidden="true">${n}</span>` : ""}</button>`;
}
function showAllButton(total, hidden) {
  if (a2a.agents.length || (!hidden && !a2a.all) || total < 2) return "";
  const label = a2a.all ? "Show recent only" : `Show all ${total}`;
  const more = a2a.all ? "" : `<span class="sr-only">, ${plural(hidden, "conversation", "conversations")} older than a day</span>`;
  return `<button class="btn ghost sm chat-a2a-all" id="chat-a2a-all" type="button" data-act="chat-a2a-all" aria-expanded="${a2a.all}">${ic(a2a.all ? "chev-right" : "chev-down", "ic16")}<span>${label}</span>${more}</button>`;
}
function emptyLine() {
  if (a2a.agents.length) return `<p class="chat-find-empty">No conversations between ${esc(a2a.agents.map((k) => M[k].name).join(" and "))}.</p>`;
  return `<p class="chat-find-empty">None in the last day.</p>`;
}
function statusText(n) {
  if (!a2a.agents.length) return "";
  return `${plural(n, "conversation", "conversations")} with ${a2a.agents.map((k) => M[k].name).join(" and ")}`;
}
/* The section: heading with its filter, the rows, then Show all. `rowHtml(key)` renders one list row. */
function a2aSectionHtml(keys, activeId, rowHtml) {
  const { rows, hidden } = visibleRows(keys, activeId);
  const list = rows.length ? `<ul>${rows.map(rowHtml).join("")}</ul>` : emptyLine();
  return `<div class="chat-sec-head"><h2>Between agents</h2>${filterButton()}</div>${filterPanel()}
    <p class="sr-only" role="status">${statusText(rows.length)}</p>
    ${list}${showAllButton(keys.length, hidden)}`;
}

/* Clicks: each returns the id of the control that keeps focus after the list re-renders. */
function toggleAgent(k) {
  a2a.agents = a2a.agents.includes(k) ? a2a.agents.filter((x) => x !== k) : [...a2a.agents, k].slice(-MAX_PICKS);
  return `chat-a2a-ag-${k}`;
}
const A2A_CLICKS = {
  "chat-a2a-all": () => { a2a.all = !a2a.all; return "chat-a2a-all"; },
  "chat-a2a-filter": () => { a2a.filter = !a2a.filter; return "chat-a2a-filter-btn"; },
  "chat-a2a-agent": (id) => toggleAgent(id),
  "chat-a2a-clear": () => { a2a.agents = []; return "chat-a2a-filter-btn"; },
};

export { A2A_CLICKS, a2aSectionHtml };
