/* "Between agents" in the conversation list, built for many agents (10+ agents is up to ~45 pairs):
   the last day's conversations newest first, "Show all N" for the rest, and a filter by agent
   (one agent: every conversation it is in; two: that pair). */
import { CH, M } from "../data.js";
import { esc, ic, plural } from "../ui.js";
import { activeToday, lastActiveLabel, newestFirst } from "./chat-model.js";

const a2a = { all: false, filter: false, agents: [], q: "", open: false, active: 0 };
const MAX_PICKS = 2;

/* Every agent that has a conversation with another agent, by name. */
const pairAgents = () => [...new Set(Object.values(CH).filter((ch) => ch.kind === "a2a").flatMap((ch) => ch.pair))]
  .filter((k) => M[k]).sort((a, b) => M[a].name.localeCompare(M[b].name));

/* Rows shown, and how many older ones wait behind Show all. A filter shows every match, whatever its age.
   The open conversation always shows: when it is older than the window it is listed on its own, labelled
   "Current conversation · outside 24 hours", never under "Last 24 hours". */
function visibleRows(keys, activeId) {
  const sorted = [...keys].sort(newestFirst);
  if (a2a.agents.length) return { rows: sorted.filter((k) => a2a.agents.every((a) => CH[k].pair.includes(a))), hidden: 0, current: "" };
  if (a2a.all) return { rows: sorted, hidden: 0, current: "" };
  const rows = sorted.filter((k) => activeToday(CH[k]));
  const current = sorted.includes(activeId) && !rows.includes(activeId) ? activeId : "";
  return { rows, hidden: sorted.length - rows.length - (current ? 1 : 0), current };
}
/* ---------------------------------------------------------------- the agent picker (a combobox)
   A searchable field for one or two agents: picked agents show as removable names above it, matching agents list
   under it only while searching, and the conversations stay in view below. With one agent picked, only agents it
   has a conversation with are offered. */
const MAX_OPTIONS = 6;
function pickable() {
  const q = a2a.q.trim().toLowerCase();
  const partners = a2a.agents.length ? new Set(Object.values(CH).filter((ch) => ch.kind === "a2a" && ch.pair.includes(a2a.agents[0])).flatMap((ch) => ch.pair)) : null;
  return pairAgents().filter((k) => !a2a.agents.includes(k) && (!partners || partners.has(k)))
    .filter((k) => !q || M[k].name.toLowerCase().split(/\s+/).some((w) => w.startsWith(q)) || k.startsWith(q)).slice(0, MAX_OPTIONS);
}
const optionsOpen = () => a2a.open && a2a.agents.length < MAX_PICKS;
function optionsHtml() {
  if (!optionsOpen()) return "";
  const opts = pickable();
  if (!opts.length) return `<li class="chat-a2a-none" role="presentation">No agent matches “${esc(a2a.q.trim())}”.</li>`;
  return opts.map((k, i) => `<li role="option" id="chat-a2a-opt-${i}" data-act="chat-a2a-agent" data-id="${esc(k)}" aria-selected="${i === a2a.active}">${esc(M[k].name)}</li>`).join("");
}
function tokenHtml(k) {
  return `<li class="chat-a2a-token">${esc(M[k].name)}<button class="chat-a2a-rm" id="chat-a2a-rm-${esc(k)}" type="button" data-act="chat-a2a-agent" data-id="${esc(k)}" aria-label="Remove ${esc(M[k].name)}" title="Remove">${ic("x", "ic14")}</button></li>`;
}
function comboField() {
  if (a2a.agents.length >= MAX_PICKS) return `<p class="chat-a2a-hint">Two agents picked. Remove one to pick another.</p>`;
  const open = optionsOpen() && pickable().length > 0;
  const active = open ? ` aria-activedescendant="chat-a2a-opt-${a2a.active}"` : "";
  const placeholder = a2a.agents.length ? "Add a second agent" : "Find an agent";
  return `<input class="field" id="chat-a2a-q" type="text" role="combobox" placeholder="${placeholder}" value="${esc(a2a.q)}" autocomplete="off"
      aria-labelledby="chat-a2a-filter-label" aria-autocomplete="list" aria-expanded="${open}" aria-controls="chat-a2a-options"${active}>
    <ul class="chat-a2a-options" id="chat-a2a-options" role="listbox" aria-label="Agents">${optionsHtml()}</ul>`;
}
function filterPanel() {
  if (!a2a.filter) return "";
  const clear = a2a.agents.length ? `<button class="btn ghost sm chat-a2a-clear" id="chat-a2a-clear" type="button" data-act="chat-a2a-clear">Clear</button>` : "";
  const picked = a2a.agents.length ? `<ul class="chat-a2a-picked" role="list" aria-label="Picked agents">${a2a.agents.map(tokenHtml).join("")}</ul>` : "";
  return `<div class="chat-a2a-filter" id="chat-a2a-filter" role="group" aria-labelledby="chat-a2a-filter-label">
      <p class="chat-a2a-hint" id="chat-a2a-filter-label">One agent for all its conversations, two for their pair</p>
      ${picked}<div class="chat-a2a-combo">${comboField()}</div>${clear}
    </div>`;
}
function filterButton() {
  const n = a2a.agents.length, label = n ? `Filter by agent, ${a2a.agents.map((k) => M[k].name).join(" and ")}` : "Filter by agent";
  return `<button class="btn ghost sm chat-a2a-filter-btn" id="chat-a2a-filter-btn" type="button" data-act="chat-a2a-filter" aria-expanded="${a2a.filter}" aria-controls="chat-a2a-filter" aria-label="${esc(label)}" title="Filter by agent">${ic("filter", "ic16")}<span>Filter</span>${n ? `<span class="chat-a2a-count" aria-hidden="true">${n}</span>` : ""}</button>`;
}
/* Which conversations the rows are: the default window is named, so "Show all" reads as widening it. */
function windowLabel() {
  if (a2a.agents.length) return "";
  return `<p class="chat-a2a-window">${a2a.all ? "All conversations" : "Last 24 hours"}</p>`;
}
function showAllButton(total, hidden) {
  if (a2a.agents.length || (!hidden && !a2a.all) || total < 2) return "";
  const label = a2a.all ? "Show recent only" : `Show all ${total}`;
  const more = a2a.all ? "" : `<span class="sr-only">, ${plural(hidden, "conversation", "conversations")} older than a day</span>`;
  return `<button class="btn ghost sm chat-a2a-all" id="chat-a2a-all" type="button" data-act="chat-a2a-all" aria-expanded="${a2a.all}">${ic(a2a.all ? "chev-right" : "chev-down", "ic16")}<span>${label}</span>${more}</button>`;
}
function emptyLine() {
  if (a2a.agents.length) return `<p class="chat-find-empty">No conversations between ${esc(a2a.agents.map((k) => M[k].name).join(" and "))}.</p>`;
  return `<p class="chat-find-empty">None in the last 24 hours.</p>`;
}
const pickedNames = () => a2a.agents.map((k) => M[k].name).join(" and ");
/* With the picker closed the filter stays readable: the picked names and a Clear, under the heading. */
function pickedSummary() {
  if (a2a.filter || !a2a.agents.length) return "";
  return `<p class="chat-a2a-active"><span>${ic("filter", "ic14")}<span><span class="sr-only">Filtered to </span>${esc(pickedNames())}</span></span>
    <button class="btn ghost sm chat-a2a-clear" id="chat-a2a-clear" type="button" data-act="chat-a2a-clear" aria-label="Clear the agent filter">Clear</button></p>`;
}
/* The open pair, when older than the window, leads the section under its own label, so it stays in view. */
function currentOutside(current, rowHtml) {
  if (!current) return "";
  return `<p class="chat-a2a-window chat-a2a-older">Older than 24h</p><ul>${rowHtml(current, lastActiveLabel(CH[current]))}</ul>`;
}
/* Widened (Show all) or filtered rows say when each pair last spoke; the last day's rows need no date. */
const rowWhen = (k) => (a2a.all || a2a.agents.length ? lastActiveLabel(CH[k]) : "");
function statusText(n) {
  if (!a2a.agents.length) return "";
  return `${plural(n, "conversation", "conversations")} with ${a2a.agents.map((k) => M[k].name).join(" and ")}`;
}
/* The section: heading with its filter, the rows, then Show all. `rowHtml(key)` renders one list row. */
function a2aSectionHtml(keys, activeId, rowHtml) {
  const { rows, hidden, current } = visibleRows(keys, activeId);
  const list = rows.length ? `<ul>${rows.map((k) => rowHtml(k, rowWhen(k))).join("")}</ul>` : emptyLine();
  return `<div class="chat-sec-head" id="chat-a2a-head"><h2>Between agents</h2>${filterButton()}</div>${pickedSummary()}${currentOutside(current, rowHtml)}${windowLabel()}${filterPanel()}
    <p class="sr-only" role="status">${statusText(rows.length)}</p>
    ${list}${showAllButton(keys.length, hidden)}`;
}

/* Clicks: each returns the id of the control that keeps focus after the list re-renders. Picking or removing an
   agent keeps focus in the picker (on the last name's Remove once two are picked). */
function toggleAgent(k) {
  a2a.agents = a2a.agents.includes(k) ? a2a.agents.filter((x) => x !== k) : [...a2a.agents, k].slice(-MAX_PICKS);
  Object.assign(a2a, { q: "", open: false, active: 0 });
  return a2a.agents.length >= MAX_PICKS ? `chat-a2a-rm-${a2a.agents.at(-1)}` : "chat-a2a-q";
}
function toggleFilter() {
  a2a.filter = !a2a.filter;
  Object.assign(a2a, { q: "", open: false, active: 0 });
  return a2a.filter && a2a.agents.length < MAX_PICKS ? "chat-a2a-q" : "chat-a2a-filter-btn";
}
const A2A_CLICKS = {
  "chat-a2a-all": () => { a2a.all = !a2a.all; return "chat-a2a-all"; },
  "chat-a2a-filter": () => toggleFilter(),
  "chat-a2a-agent": (id) => toggleAgent(id),
  "chat-a2a-clear": () => { Object.assign(a2a, { agents: [], q: "", open: false }); return a2a.filter ? "chat-a2a-q" : "chat-a2a-filter-btn"; },
};

/* ---------------------------------------------------------------- picker typing and keys (the field keeps focus) */
function paintOptions(field) {
  const list = document.getElementById("chat-a2a-options");
  if (!list) return;
  list.innerHTML = optionsHtml();
  const open = optionsOpen() && pickable().length > 0;
  field.setAttribute("aria-expanded", String(open));
  if (open) field.setAttribute("aria-activedescendant", `chat-a2a-opt-${a2a.active}`);
  else field.removeAttribute("aria-activedescendant");
}
function onA2AInput(field) {
  Object.assign(a2a, { q: field.value, open: Boolean(field.value.trim()), active: 0 });
  paintOptions(field);
}
const MOVES = { ArrowDown: 1, ArrowUp: -1 };
function moveActive(field, step) {
  const n = pickable().length;
  if (!a2a.open) Object.assign(a2a, { open: true, active: 0 });
  else if (n) a2a.active = (a2a.active + step + n) % n;
  paintOptions(field);
}
/* Returns the agent to pick (Enter on an option), "remove" (Backspace in an empty field), "close" (Escape with
   nothing open), or "" when the key was handled here or is not the picker's. */
function onA2AKeydown(e) {
  const field = e.target;
  if (MOVES[e.key]) { e.preventDefault(); moveActive(field, MOVES[e.key]); return ""; }
  if (e.key === "Enter") { e.preventDefault(); return optionsOpen() ? pickable()[a2a.active] || "" : ""; }
  if (e.key === "Escape" && (a2a.open || a2a.q)) { e.preventDefault(); e.stopPropagation(); Object.assign(a2a, { q: "", open: false }); field.value = ""; paintOptions(field); return ""; }
  if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); return "close"; }
  if (e.key === "Backspace" && !field.value && a2a.agents.length) return "remove";
  return "";
}
const lastPicked = () => a2a.agents.at(-1);

export { A2A_CLICKS, a2aSectionHtml, lastPicked, onA2AInput, onA2AKeydown };
