import { ALL_STATUSES, IN, KIND, M, OPEN, P, STATUSES, TEAMS, W, WORK_DISCUSSIONS, needsMe } from "../data.js";
import { detailCommentDrafts, detailEvent, inboxActivityTime, localDetailRecords, workCommentsHtml } from "./inbox.js";
import { closeMenus } from "../shell/actions.js";
import { own, route } from "../shell/router.js";
import { S, qs } from "../state.js";
import { SEP, av, displayCheckbox, esc, ic, iconWithTooltip, kindWord, memberIds, mlink, pill, plink, queueHeadline, searchRow, searchToggle, showMenu, toast } from "../ui.js";

function isArchivedWork(w) {
  return !OPEN(w) && S.archiveAfterDays > 0 && w.closedDaysAgo != null && w.closedDaysAgo >= S.archiveAfterDays;
}
function visibleWorkItems(items) {
  return S.workShowArchived ? items : items.filter((w) => !isArchivedWork(w));
}
function workListDisplayOptions() {
  return `${displayCheckbox("work-subtasks", "Show subtasks", S.workSubtasks)}${displayCheckbox("work-dividers", "Row dividers", S.workDividers)}`;
}
function workDisplayFooter(listLayout) {
  return `<hr><p class="menu-h">Options</p>${listLayout ? workListDisplayOptions() : ""}${displayCheckbox("work-archived", "Show archived", S.workShowArchived)}`;
}
function isWorkDisplayActive(listLayout, group, subgroup, sort, order) {
  return (listLayout && (group !== WORK_DEFAULT_GROUP || subgroup !== "none"))
    || sort !== "default" || order !== "asc"
    || S.workShowArchived || !S.workSubtasks || S.workDividers;
}
function workLayoutToggle(key) {
  return `<div class="work-layout-toggle" role="group" aria-label="Work layout">
    ${[["list", "List"], ["board", "Board"]].map(([layout, label]) => `<button class="queue-tab" type="button"
      data-act="set" data-k="${key}" data-v="${layout}" aria-pressed="${S[key] === layout}" aria-controls="work-results">${ic(layout, "ic16")}${label}</button>`).join("")}
  </div>`;
}
/* Board columns choice lives in Display (next to the list's Group by), not in the toolbar */
const BOARD_COLUMN_OPTION = ["workBoardStages", "Board columns", [["unified", "Stages"], ["detailed", "Status"]]];
const workCalendarLink = () => `<a class="btn ghost menu-textbtn work-cal-btn" href="#/calendar" aria-label="Calendar">${ic("calendar-check", "ic16")}<span>Calendar</span></a>`;
const scopedWorkQueries = new Map();

function scopedWorkType([page, id, tab]) {
  if (page === "projects" && own(P, id) && tab === "work") return "project";
  if (page === "teams" && own(TEAMS, id) && tab === "work") return "team";
  if (page === "members" && own(M, id)) {
    if (M[id].kind === "agent" && (tab === "tasks" || tab === "work")) return "agent";
    if (M[id].kind === "person" && tab === "work") return "person";
  }
  return null;
}

const teamWork = (id) => W.filter(({ who }) => TEAMS[id].members.includes(who));

function scopedWorkView(currentRoute, type) {
  const [, id] = currentRoute;
  const key = type + ":" + id;
  const items = type === "project" ? W.filter((w) => w.proj === id)
    : type === "team" ? teamWork(id)
    : W.filter((w) => w.who === id);
  return { type, id, key, items };
}

const SCOPED_WORK_GROUPS = {
  agent: [["status", "Status"], ["project", "Project"]],
  person: [["status", "Status"], ["project", "Project"]],
  project: [["status", "Status"], ["assignee", "Assignee"]],
  team: [["status", "Status"], ["project", "Project"], ["assignee", "Assignee"]],
};

function scopedWorkPresets(type, id) {
  if (type === "person" && id === "you") return [["all", "All"], ["needs-me", "Needs you"]];
  const mineLabel = isRequestedMineScope(type, id) ? "Requested by me" : "Mine";
  return [["all", "All"], ["needs-me", "Needs you"], ["mine", mineLabel]];
}

function workArchivedChip() {
  if (!S.workShowArchived) return "";
  return `<button class="work-archived-chip" type="button" data-act="work-archived" aria-label="Hide archived tasks" title="Click to hide archived tasks">Showing archived${ic("x", "ic14")}</button>`;
}

function isRequestedMineScope(type, id) {
  return type === "agent" || (type === "person" && id !== "you");
}

function scopedWorkHasFilters(type) {
  return S.scopedWorkStatus !== "all"
    || (type !== "project" && S.scopedWorkProject !== "all")
    || (type === "project" && S.scopedWorkTeam !== "all");
}

function normalizeScopedWorkState(key, value) {
  if (key === "scopedWorkGroup" && (value === "none" || value === S.scopedWorkSubgroup)) {
    S.scopedWorkSubgroup = "none";
  }
}

function scopedWorkBaseItems(type, items, query = "") {
  const { scopedWorkStatus: status, scopedWorkProject: project, scopedWorkTeam: team } = S;
  return visibleWorkItems(items).filter((w) => {
    if (status !== "all" && w.status !== status) return false;
    if (type !== "project" && project !== "all" && w.proj !== project) return false;
    if (type === "project" && team !== "all" && (workTeam(w) ?? "none") !== team) return false;
    return !query || workMatches(w, query);
  });
}

function scopedWorkPresetItems(items, view, type, id) {
  if (view === "needs-me") return items.filter(needsMe);
  if (view === "mine") {
    return isRequestedMineScope(type, id)
      ? items.filter((w) => w.requestedBy === "you")
      : items.filter((w) => w.who === "you");
  }
  return items;
}

function scopedWorkGrouping(type) {
  const allowed = SCOPED_WORK_GROUPS[type] ?? SCOPED_WORK_GROUPS.team;
  const has = (v) => allowed.some(([key]) => key === v);
  const group = S.scopedWorkGroup === "none" || has(S.scopedWorkGroup) ? S.scopedWorkGroup : WORK_DEFAULT_GROUP;
  const subgroup = group !== "none" && S.scopedWorkSubgroup !== group && has(S.scopedWorkSubgroup) ? S.scopedWorkSubgroup : "none";
  return { allowed, group, subgroup };
}

function scopedWorkEmptyText(type, id, view, query) {
  if (query || scopedWorkHasFilters(type)) return "No work matches these filters.";
  if (view === "needs-me") return "Nothing needs you here.";
  if (view === "mine") return isRequestedMineScope(type, id) ? `No tasks you requested for ${M[id].name}.` : "Nothing is assigned to you here.";
  return "No active work here right now.";
}

function scopedWorkMenus(type) {
  const filterGroups = [
    ["scopedWorkStatus", "Status", [["all", "All statuses"], ...ALL_STATUSES.map((s) => [s, s])]],
    ...(type !== "project" ? [["scopedWorkProject", "Project", [["all", "All projects"], ...Object.entries(P).map(([id, p]) => [id, p.name])]]] : []),
    ...(type === "project" ? [["scopedWorkTeam", "Team", [["all", "All teams"], ...Object.entries(TEAMS).map(([id, t]) => [id, t.name]), ["none", "No team"]]]] : []),
  ];
  const nFilters = [S.scopedWorkStatus !== "all", type !== "project" && S.scopedWorkProject !== "all", type === "project" && S.scopedWorkTeam !== "all"].filter(Boolean).length;
  const filter = showMenu("scoped-work-filter", filterGroups, { icon: "filter", label: "Filter work", text: "Filter", count: nFilters, active: scopedWorkHasFilters(type) });
  const listLayout = S.scopedWorkLayout === "list";
  const { allowed, group, subgroup } = scopedWorkGrouping(type);
  const display = showMenu("scoped-work-display", [
    ...(listLayout ? [["scopedWorkGroup", "Group by", [["none", "No grouping"], ...allowed]]] : []),
    ...(listLayout && group !== "none"
      ? [["scopedWorkSubgroup", "Subgroup by", [["none", "No subgroup"], ...allowed.filter(([v]) => v !== group)]]]
      : []),
    ...(listLayout ? [] : [BOARD_COLUMN_OPTION]),
    ["scopedWorkSort", "Sort by", listLayout ? WORK_SORTS : BOARD_SORTS],
    ["scopedWorkOrder", "Direction", [["asc", "Ascending"], ["desc", "Descending"]]],
  ], {
    icon: "adjustments", label: "Work display", text: listLayout ? "Display" : `Display · ${S.workBoardStages === "detailed" ? "Status" : "Stages"}`,
    active: isWorkDisplayActive(listLayout, group, subgroup, S.scopedWorkSort, S.scopedWorkOrder),
    footer: workDisplayFooter(listLayout),
  });
  return `<div class="queue-controls" role="group" aria-label="Work view controls">${scopedWorkSearch().toggle}${filter}${display}</div>`;
}

function scopedWorkSearch() {
  const currentRoute = route(), type = scopedWorkType(currentRoute);
  const value = type ? scopedWorkQueries.get(type + ":" + currentRoute[1]) ?? "" : "";
  const label = "Search work in this view", placeholder = (type === "agent" ? "Search tasks…" : "Search work…") + " (Esc to close)";
  return { toggle: searchToggle("scoped-work-search", label, value), row: searchRow("scoped-work-search", label, value, placeholder, "work-search-row") };
}

function scopedWorkToolbar(type, id, items) {
  const query = canonicalWorkRef(scopedWorkQueries.get(type + ":" + id)).toLowerCase();
  const base = scopedWorkBaseItems(type, items, query);
  const presets = scopedWorkPresets(type, id);
  const activeView = presets.some(([v]) => v === S.scopedWorkView) ? S.scopedWorkView : "all";
  const mineHint = isRequestedMineScope(type, id) ? ` title="Tasks you requested for ${esc(M[id].name)}"` : "";
  return `<div class="work-subbar-header">
    <div class="work-toolbar queue-toolbar">
      <div class="primary-tab-row">
        <div class="queue-tabs" role="group" aria-label="Work views">
          ${presets.map(([value, label]) => `<button class="queue-tab" type="button" data-act="set"
            data-k="scopedWorkView" data-v="${value}" aria-pressed="${activeView === value}" aria-controls="work-results"${value === "mine" ? mineHint : ""}>${label}<span data-scoped-work-count="${value}">${scopedWorkPresetItems(base, value, type, id).length}</span></button>`).join("")}
        </div>
      </div>
      ${workLayoutToggle("scopedWorkLayout")}
      ${scopedWorkMenus(type)}
      ${workArchivedChip()}
      ${type === "project" || type === "team" ? `<button class="btn primary" id="work-new-task" type="button" data-act="work-new-task">${ic("plus", "ic16")}New task</button>` : ""}
    </div>
  </div>`;
}

function refreshScopedWorkCounts({ type, id, key, items }) {
  if (!type) return;
  const query = canonicalWorkRef(scopedWorkQueries.get(key)).toLowerCase();
  const base = scopedWorkBaseItems(type, items, query);
  for (const counter of document.querySelectorAll("[data-scoped-work-count]")) {
    counter.textContent = scopedWorkPresetItems(base, counter.dataset.scopedWorkCount, type, id).length;
  }
}

function scopedWorkResults({ type, id, key, items }) {
  const query = canonicalWorkRef(scopedWorkQueries.get(key)).toLowerCase();
  const presets = scopedWorkPresets(type, id);
  const view = presets.some(([v]) => v === S.scopedWorkView) ? S.scopedWorkView : "all";
  const base = scopedWorkBaseItems(type, items, query);
  const filtered = sortWorkItems(scopedWorkPresetItems(base, view, type, id), S.scopedWorkSort, S.scopedWorkOrder);
  if (!filtered.length) return `<p class="empty-row">${esc(scopedWorkEmptyText(type, id, view, query))}</p>`;
  const hideOwner = type === "agent" || type === "person";
  if (S.scopedWorkLayout === "board") return board(filtered, { hideOwner, hideProject: type === "project", needsYouView: view === "needs-me", sortKey: S.scopedWorkSort, filtered: Boolean(query) || scopedWorkHasFilters(type) });
  const { group, subgroup } = scopedWorkGrouping(type);
  return workList(filtered, {
    groupBy: WORK_GROUP_FIELDS[group],
    subgroupBy: WORK_GROUP_FIELDS[subgroup],
    hideOwner,
  });
}

function scopedWorkWorkspace() {
  const currentRoute = route();
  const view = scopedWorkView(currentRoute, scopedWorkType(currentRoute));
  return workWorkspace(scopedWorkResults(view), view.items, scopedWorkSearch().row, view.type);
}


/* ---------------------------------------------------------------- work */
const WORK_STATUS = {
  "Draft": ["draft", "pencil"],
  "In progress": ["active"], "Rowan is scoping": ["active"],
  "Blocked": ["blocked", "pause"], "Declined": ["blocked", "deny"],
  "Triaged": ["review", "doc-check"], "Scoped": ["review", "doc-check"],
  "Done": ["done", "check-circle"], "Fixed": ["done", "check-circle"],
  "Approved": ["done", "check-circle"], "Closed": ["", "deny"],
};

function workStatus(status, attention = false) {
  const [tone, icon] = WORK_STATUS[status] ?? [""];
  const label = esc(attention ? `${status} · Needs you` : status);
  return `<span class="work-status" data-tone="${tone}"${attention ? " data-attention" : ""} role="img" aria-label="${label}" title="${label}">${icon ? ic(icon, "ic16") : '<span class="work-status-ring" aria-hidden="true"></span>'}</span>`;
}

const WORK_PRIORITY_RANKS = { Low: 1, Minor: 1, Medium: 2, Normal: 2, High: 3, Major: 3, Urgent: 4, Critical: 4 };
const workPriorityRank = (priority) => typeof priority === "string" && own(WORK_PRIORITY_RANKS, priority)
  ? WORK_PRIORITY_RANKS[priority] : undefined;

function workPriority(priority) {
  const rank = workPriorityRank(priority);
  const label = rank ? esc(`${priority} priority`) : "No priority";
  return `<span class="work-priority-mark" data-level="${Math.min(rank ?? 0, 3)}" role="img" aria-label="${label}" title="${label}"><span></span><span></span><span></span></span>`;
}

function workCriteria(criteria, id, placeholder = false) {
  if (!criteria?.length) {
    return placeholder ? `<span class="work-criteria is-empty" data-work-progress="${esc(id)}" title="No acceptance criteria"><span aria-hidden="true">–</span><span class="sr-only">No acceptance criteria</span></span>` : "";
  }
  const done = criteria.filter((item) => item.done).length;
  const label = `${done} of ${criteria.length} acceptance criteria complete`;
  return `<span class="work-criteria${done === criteria.length ? " is-complete" : ""}" data-work-progress="${esc(id)}" title="${label}"><span aria-hidden="true">${done}/${criteria.length}</span><span class="sr-only">${label}</span></span>`;
}

function workEta(w) {
  return w.etaHours != null ? `${w.etaHours}h` : w.kind === "task" && OPEN(w) ? "TBD" : "";
}

function workTitle(w, className) {
  return `<a class="work-item-link ${className}" data-item="${esc(w.id)}" href="${esc(workItemHref(w.id))}" aria-haspopup="dialog">${esc(w.title)}</a>`;
}

function workChild(parent, subtask) {
  return { ...subtask, kind: "task", proj: parent.proj, who: parent.who };
}

function workSubtasks(parent, hideOwner = false) {
  if (!parent.subtasks?.length) return "";
  return `<ul class="work-subtasks" role="list" aria-label="Subtasks">${parent.subtasks.map((subtask) =>
    workRow(workChild(parent, subtask), true, hideOwner)
  ).join("")}</ul>`;
}

function workTeam(w) {
  const memberships = M[w.who]?.teams ?? [];
  return memberships.find((id) => P[w.proj].teams.includes(id)) ?? memberships[0];
}

function workRowBadges(w) {
  const eta = workEta(w);
  return `${eta ? ` <span class="work-eta">ETA ${esc(eta)}</span>` : ""}${w.status === "Draft" ? '<span class="work-draft-tag">Draft · Ideation</span>' : ""}${isArchivedWork(w) ? '<span class="work-archived-tag">Archived</span>' : ""}`;
}

function workRow(w, isSubtask = false, hideOwner = false) {
  const sev = ["Critical", "Major"].includes(w.pri)
    ? `<span class="pin ${w.pri === "Critical" ? "c-red" : "c-gray"}">${ic(w.pri === "Critical" ? "warn" : "exclaim", "ic14")}${esc(w.pri)}</span>`
    : esc(w.pri);
  const team = workTeam(w);
  const task = w.kind === "task";
  return `<li class="${isSubtask ? "work-subtask" : "work-item"}"><div class="work-row" data-work-id="${esc(w.id)}"${team ? ` data-team="${esc(team)}"` : ""}>
    <div class="work-summary">
      <span class="work-ident">${workPriority(w.pri)}<span class="work-ref">${esc(w.id)}</span></span>
      <span class="work-heading">
        <span class="work-markers">${workStatus(w.status, showsNeedsYou(w))}${workCriteria(w.criteria, w.id, true)}</span>
        <span>${workTitle(w, "work-title")}${workRowBadges(w)}</span>
      </span>
    </div>
    <div class="work-facts">
      ${!task && sev && !workPriorityRank(w.pri) ? `<span class="work-priority">${sev}</span>` : ""}
      ${plink(w.proj)}
      <span class="work-team">${team ? esc(TEAMS[team].name) : "No team"}</span>
      ${isSubtask || hideOwner ? "" : `<span class="work-owner">${av(w.who, 24)}${mlink(w.who)}</span>`}
    </div></div>${workSubtasks(w, hideOwner)}</li>`;
}
const showsNeedsYou = (w) => needsMe(w) && w.status !== "Scoped";
const WORK_STATUS_ORDER = ["Blocked", "Draft", ...ALL_STATUSES.filter((status) => status !== "Blocked")];
function workGroups(items, field) {
  const groups = new Map();
  for (const item of items) {
    const key = item[field] ?? "none";
    if (!groups.has(key)) groups.set(key, []);
    groups.get(key).push(item);
  }
  const keys = field === "status" ? WORK_STATUS_ORDER.filter((status) => groups.has(status)) : [...groups.keys()];
  return keys.map((key) => {
    const label = field === "status" ? key : field === "proj" ? P[key].name
      : M[key] ? M[key].name + " · " + kindWord(key) : "Unassigned";
    return { label, rows: groups.get(key) };
  });
}
function workList(items, { groupBy, subgroupBy, emptyText = "No work here yet.", hideOwner = false } = {}) {
  if (!items.length) return `<p class="empty-row">${esc(emptyText)}</p>`;
  const list = (rows) => `<ul class="work-list" role="list">${rows.map((w) => workRow(w, false, hideOwner)).join("")}</ul>`;
  if (!groupBy) return `<div class="work-items">${list(items)}</div>`;
  const sections = workGroups(items, groupBy).map(({ label, rows }) => {
    const body = subgroupBy ? `<div class="work-subgroups">${workGroups(rows, subgroupBy).map((subgroup) => `
      <div class="work-subgroup"><h3 class="work-subgroup-title">${esc(subgroup.label)}<span class="cnt">${subgroup.rows.length}</span></h3>
        ${list(subgroup.rows)}</div>`).join("")}</div>` : list(rows);
    return `<div class="work-group"><h2 class="work-group-title">${esc(label)}<span class="cnt">${rows.length}</span></h2>${body}</div>`;
  });
  return `<div class="work-items">${sections.join("")}</div>`;
}
const BOARD_STAGES = ["To do", "In progress", "Blocked", "Done"];
const BOARD_STAGE_OF = { New: "To do", Triaged: "In progress", Scoped: "In progress", "Rowan is scoping": "In progress", Draft: "To do", Fixed: "Done", Closed: "Done", Approved: "Done", Declined: "Done" };
/* Status columns: real statuses in lifecycle order, nothing merged; empty ones are left out */
const BOARD_STATUS_ORDER = ["Draft", "New", "Triaged", "Rowan is scoping", "Scoped", "To do", "In progress", "Blocked", "Fixed", "Done", "Approved", "Closed", "Declined"];
const BOARD_STAGE_STATUS = { "To do": "To do", "In progress": "In progress", Blocked: "Blocked", Done: "Done" };
const DONE_WINDOW_DAYS = 7;
const NEEDS_YOU_SUBS = ["Review requested", "Hand-off"];
const isClosed = (w) => !OPEN(w);

/* Priority shown as a word, only when it isn't normal. Bug severity reads like the list (red Critical). */
function boardPriority(w) {
  const p = w.pri;
  /* Requests use pri for request metadata ("Sent Nov 9"), not priority */
  if (!p || w.kind === "request" || !own(WORK_PRIORITY_RANKS, p) || p === "Normal" || p === "Medium") return "";
  if (p === "Critical") return `<span class="bpri is-critical">${ic("warn", "ic14")}Critical</span>`;
  if (p === "Urgent") return `<span class="bpri is-urgent">${ic("exclaim", "ic14")}Urgent</span>`;
  return `<span class="bpri">${workPriority(p)}${esc(p)}</span>`;
}
/* Why a card needs you or is blocked, from its linked Inbox item when there is one. */
function boardReason(w) {
  const asks = IN.filter((i) => !i.handled && i.task === w.id);
  const qualifying = asks.filter((i) => NEEDS_YOU_SUBS.includes(i.sub));
  const ask = qualifying[0] || asks[0];
  if (showsNeedsYou(w)) {
    const what = !w.who ? "Assign" : asks.length > 1 ? `${asks.length} requests` : ask?.sub || (w.waitsOn === "you" ? "Unblock" : "Review");
    return { badge: `Needs you · ${what}`, href: ask ? `#/inbox/${ask.id}` : "" };
  }
  if (w.status === "Blocked") return { line: w.waitsOn && w.waitsOn !== "you" && M[w.waitsOn] ? `Waiting on ${M[w.waitsOn].name}` : "Blocked" };
  return {};
}
function boardMeta(w, hideProject) {
  const type = w.kind === "bug" ? "Bug" : w.kind === "request" ? "Request" : "";
  const parts = [type && `<span class="btype">${type}</span>`, `<span class="mono">${esc(w.id)}</span>`].filter(Boolean);
  const project = hideProject ? "" : `<span class="board-project">${plink(w.proj)}</span>`;
  return `<div class="board-card-meta">${parts.join(SEP)}${project ? SEP + project : ""}</div>`;
}
/* Second row of small facts: priority (only when not normal) and checklist progress */
function boardTags(w) {
  const pri = boardPriority(w);
  /* Under Stages, keep the exact status when it differs from the column (Draft, New, Declined...) */
  const st = S.workBoardStages !== "detailed" && !BOARD_STAGES.includes(w.status) ? `<span class="bstatus">${esc(w.status)}</span>` : "";
  const crit = w.criteria?.length ? `<span class="btag">${ic("check-circle", "ic14")}${workCriteria(w.criteria, w.id)}<span aria-hidden="true">criteria</span></span>` : "";
  return st || pri || crit ? `<div class="board-tags">${st}${pri}${crit}</div>` : "";
}
function boardOwner(w) {
  if (!w.who || !M[w.who]) return `<span class="board-assignee">${av(null, 24)}<span class="board-owner-label">Unassigned</span></span>`;
  return `<span class="board-assignee">${av(w.who, 24)}<span class="board-owner-label">${mlink(w.who)}</span><span class="board-kind">${esc(kindWord(w.who))}</span></span>`;
}
function boardCard(w, { hideOwner, hideProject, selected }) {
  const attention = showsNeedsYou(w), reason = boardReason(w), archived = isArchivedWork(w);
  const badge = reason.badge ? (reason.href ? `<a class="pill t-amber chip board-need" href="${reason.href}" title="Open in Inbox">${ic("bell", "ic14")}${esc(reason.badge)}${ic("chev-right", "ic14")}</a>` : pill(reason.badge, "amber", "bell", true)) : "";
  const status = badge ? `<div class="board-status">${badge}</div>` : reason.line ? `<p class="board-reason">${ic("pause", "ic14")}${esc(reason.line)}</p>` : "";
  const footer = hideOwner && !archived ? "" : `<div class="board-card-footer">${hideOwner ? "" : boardOwner(w)}${archived ? pill("Archived", "gray", null, true) : ""}</div>`;
  return `<li class="bcard" data-work-id="${esc(w.id)}"${attention ? " data-attention" : ""}${selected ? ' aria-current="true"' : ""}>
    <a class="work-item-link t" data-item="${esc(w.id)}" href="${esc(workItemHref(w.id))}" aria-haspopup="dialog" title="${esc(w.title)}">${esc(w.title)}</a>
    ${boardMeta(w, hideProject)}
    ${boardTags(w)}
    ${status}
    ${footer}
  </li>`;
}
const boardRank = (w) => workPriorityRank(w.pri) ?? 2;
function boardColumns(items, needsYouView, sortKey) {
  const status = S.workBoardStages === "detailed";
  const columns = new Map(status ? [] : BOARD_STAGES.map((stage) => [stage, []]));
  for (const item of items) {
    const key = status ? item.status : BOARD_STAGE_OF[item.status] ?? (BOARD_STAGES.includes(item.status) ? item.status : OPEN(item) ? "To do" : "Done");
    if (!columns.has(key)) columns.set(key, []);
    columns.get(key).push(item);
  }
  const order = status ? [...columns.keys()].sort((a, b) => (BOARD_STATUS_ORDER.indexOf(a) + 99) % 99 - (BOARD_STATUS_ORDER.indexOf(b) + 99) % 99) : BOARD_STAGES;
  /* Default sort puts the most urgent first in each column; an explicit Sort by is kept as chosen */
  const byPriority = sortKey === "default";
  return order.filter((key) => !(needsYouView && (key === "Done" || BOARD_STAGE_OF[key] === "Done"))).map((key) => [key,
    byPriority ? columns.get(key).sort((a, b) => boardRank(b) - boardRank(a)) : columns.get(key)]);
}
const isDoneStage = (stage) => stage === "Done" || BOARD_STAGE_OF[stage] === "Done";
/* Done lanes show the last week unless "older" is toggled on */
function doneWindow(stage, tasks) {
  if (!isDoneStage(stage) || S.workBoardAllDone) return { recent: tasks, older: 0 };
  const recent = tasks.filter((w) => !isClosed(w) || (w.closedDaysAgo ?? 0) <= DONE_WINDOW_DAYS);
  return { recent, older: tasks.length - recent.length };
}
function doneToggle(stage, older) {
  if (!isDoneStage(stage) || !(older || S.workBoardAllDone)) return "";
  const label = S.workBoardAllDone ? `Last ${DONE_WINDOW_DAYS} days only` : `+${older} older`;
  return `<button type="button" class="bcol-older" data-act="board-done" aria-pressed="${!!S.workBoardAllDone}" title="${S.workBoardAllDone ? "Show only items completed in the last 7 days" : "Show older completed items"}">${label}</button>`;
}
function boardColumnHead(stage, tasks, recent, older) {
  const shown = older ? `${recent.length}<span class="sr-only"> shown, ${older} older hidden</span>` : String(tasks.length);
  return `<h3 class="bcol-h"><span class="bcol-ic" aria-hidden="true">${workStatus(BOARD_STAGE_STATUS[stage] || stage)}</span><span class="bcol-name">${esc(stage)}</span><span class="cnt">${shown}</span>${doneToggle(stage, older)}</h3>`;
}
function boardColumnHtml(stage, tasks, opts) {
  const { recent, older } = doneWindow(stage, tasks);
  const empty = older ? `No items in the last ${DONE_WINDOW_DAYS} days` : opts.filtered ? "No matches for these filters" : "Nothing here";
  const list = recent.length ? `<ul class="bcol-list" role="list">${recent.map((w) => boardCard(w, { ...opts, selected: w.id === opts.selectedId })).join("")}</ul>` : `<p class="bcol-empty">${empty}</p>`;
  return `<section class="bcol" id="bcol-${esc(stage.replace(/\s+/g, "-").toLowerCase())}" aria-label="${esc(stage)}, ${tasks.length}">${boardColumnHead(stage, tasks, recent, older)}${list}</section>`;
}
/* Phones: a named lane switcher with counts, since only one column fits */
function boardLaneNav(columns) {
  return `<nav class="board-lanes" aria-label="Board columns">${columns.map(([stage, tasks], i) => `<button type="button" data-act="board-lane" data-v="${i}"${i === 0 ? ' aria-current="true"' : ""}>${esc(stage)} <span class="cnt">${doneWindow(stage, tasks).recent.length}</span></button>`).join("")}</nav>`;
}
/* Keep the lane switcher and the edge-fade cue in sync with horizontal scrolling */
function mountBoard(main) {
  const board = main.querySelector(".board"), lanes = main.querySelectorAll(".board-lanes button");
  if (!board) return;
  const sync = () => {
    const max = board.scrollWidth - board.clientWidth;
    board.toggleAttribute("data-more-right", board.scrollLeft < max - 4);
    board.toggleAttribute("data-more-left", board.scrollLeft > 4);
    if (!lanes.length) return;
    const cols = [...board.querySelectorAll(".bcol")], left = board.getBoundingClientRect().left;
    const cur = cols.reduce((best, c, i) => (Math.abs(c.getBoundingClientRect().left - left) < Math.abs(cols[best].getBoundingClientRect().left - left) ? i : best), 0);
    lanes.forEach((b, i) => (i === cur ? b.setAttribute("aria-current", "true") : b.removeAttribute("aria-current")));
  };
  board.addEventListener("scroll", sync, { passive: true });
  sync();
}
function board(items, { hideOwner = false, hideProject = false, needsYouView = false, sortKey = "default", filtered = false } = {}) {
  if (!items.length) return `<p class="empty-row">No work here yet.</p>`;
  const columns = boardColumns(items, needsYouView, sortKey), selectedId = qs().get("item");
  return `${boardLaneNav(columns)}<div class="board" role="region" aria-label="Work board" tabindex="0">
    <div class="board-columns" style="--board-column-count:${columns.length}">${columns.map(([stage, tasks]) => boardColumnHtml(stage, tasks, { hideOwner, hideProject, selectedId, filtered })).join("")}</div>
  </div>`;
}

function workItemHref(item) {
  const currentRoute = route();
  const [page, entity, tab] = currentRoute;
  const scoped = scopedWorkType(currentRoute);
  const path = scoped ? `#/${page}/${encodeURIComponent(entity)}${tab ? "/" + tab : ""}` : "#/work";
  const params = scoped || page === "work" ? qs() : new URLSearchParams();
  if (item) params.set("item", item);
  else params.delete("item");
  return params.size ? `${path}?${params}` : path;
}

function canonicalWorkRef(ref) {
  const value = ref?.trim().toUpperCase() || "";
  return W.find((item) => item.legacyId === value)?.id || value.replace(/^(?:T|BUG|R)-(\d+)$/, "T-A$1");
}

function findWorkItem(ref, items = W) {
  const id = canonicalWorkRef(ref);
  let item = items.find((w) => w.id === id), parent;
  if (id && !item) {
    for (const task of items) {
      const subtask = task.subtasks?.find((child) => child.id === id);
      if (subtask) {
        parent = task;
        item = workChild(parent, subtask);
        break;
      }
    }
  }
  return { item, parent };
}

function workWorkspace(results, items = W, search = "", scope = "") {
  const { item, parent } = findWorkItem(qs().get("item"), items);
  return `${search}<div id="work-results"${scope ? ` data-scope="${esc(scope)}"` : ""}>${results}</div>${item ? workDetail(item, parent) : ""}`;
}

function workDetailFields(w, team, eta, priority) {
  const attention = showsNeedsYou(w) ? '<span class="work-detail-attention">· Needs you</span>' : "";
  const severity = w.kind === "bug" && w.pri && !priority ? `<div><dt>Severity</dt><dd>${esc(w.pri)}</dd></div>` : "";
  const teamRow = team ? `<div><dt>Team</dt><dd><a class="alink" href="#/teams/${esc(team)}">${esc(TEAMS[team].name)}</a></dd></div>` : "";
  const etaRow = eta ? `<div><dt>ETA</dt><dd>${esc(eta)}</dd></div>` : "";
  return `<dl class="triage-fields">
          <div><dt>Status</dt><dd>${esc(w.status)}${attention}</dd></div>
          <div><dt>Assignee</dt><dd>${mlink(w.who)}</dd></div>
          <div><dt>Priority</dt><dd>${priority || "No priority"}</dd></div>
          ${severity}
          <div><dt>Project</dt><dd>${plink(w.proj)}</dd></div>
          ${teamRow}
          ${etaRow}
        </dl>`;
}

function workDetailCriteria(w) {
  if (!w.criteria?.length) return "";
  return `<section class="work-detail-section" aria-labelledby="work-detail-criteria">
        <h3 id="work-detail-criteria">Acceptance criteria ${workCriteria(w.criteria, w.id)}</h3>
        <ul class="work-checklist" role="list">${w.criteria.map((criterion, index) => `<li>
          <label><input type="checkbox" data-work-criterion="${esc(w.id)}" data-index="${index}"${criterion.done ? " checked" : ""}><span>${esc(criterion.text)}</span></label>
        </li>`).join("")}</ul>
        <p id="work-criteria-status" class="sr-only" role="status"></p>
      </section>`;
}

function workDetailSubtasks(w) {
  if (!w.subtasks?.length) return "";
  return `<section class="work-detail-section" aria-labelledby="work-detail-subtasks">
        <h3 id="work-detail-subtasks">Subtasks</h3>
        <ul class="work-detail-links" role="list">${w.subtasks.map((subtask) => `<li>
          <a class="alink" href="${esc(workItemHref(subtask.id))}">${workPriority(subtask.pri)}<span class="work-ref">${esc(subtask.id)}</span>${workStatus(subtask.status)}<span>${esc(subtask.title)}</span>${workCriteria(subtask.criteria, subtask.id)}</a>
        </li>`).join("")}</ul>
      </section>`;
}

function workDetailDraftBanner(w) {
  if (w.status === "Draft") {
    const targetStatus = w.kind === "bug" ? STATUSES.bug[0] : STATUSES.task[0];
    return `<div class="work-draft-banner" role="region" aria-label="Draft ideation phase">
      <div class="work-draft-banner-copy">
        <p class="work-draft-eyebrow">${ic("pencil", "ic14")}Draft · Ideation phase</p>
        <p class="work-draft-note">Discuss below while leads &amp; agents keep the description and criteria above up to date. Publishing moves status to ${esc(targetStatus)}, folds ideation comments out of agent context, and begins official execution comments.</p>
      </div>
      <button class="btn primary sm" type="button" data-act="work-publish-draft" data-id="${esc(w.id)}">${ic("check-circle", "ic14")}Publish to ${esc(targetStatus)}</button>
    </div>`;
  }
  if (w.materializedFromIdeation) {
    return `<p class="work-materialized-stamp">${ic("check-circle", "ic14")}Published from Draft · ${w.ideationComments?.length || 0} ideation comments folded out of default agent context.</p>`;
  }
  return "";
}

function workIdeationCommentsList(comments) {
  return `<ol class="work-ideation-list" role="list">${comments.map((c) => `<li>
    <div class="work-ideation-meta">${av(c.who, 20)}<strong>${esc(M[c.who]?.name || c.who)}</strong><span class="work-ideation-role">${esc(M[c.who]?.role || "")}</span>${inboxActivityTime(c.at)}</div>
    <p class="work-ideation-text">${esc(c.text)}</p>
  </li>`).join("")}</ol>`;
}

function workDetailIdeation(w) {
  const comments = w.ideationComments || [];
  if (w.status === "Draft") {
    return `<section class="work-detail-section work-ideation-box" aria-labelledby="work-ideation-title">
      <div class="work-ideation-head">
        <h3 id="work-ideation-title">Ideation discussion <span class="cnt">${comments.length}</span></h3>
        <span class="work-ideation-badge">Folds out on publish</span>
      </div>
      ${workIdeationCommentsList(comments)}
      <form class="triage-comment-form" data-form="work-ideation-comment" data-id="${esc(w.id)}">
        <label class="sr-only" for="work-ideation-input">Add ideation comment</label>
        <textarea id="work-ideation-input" name="comment" rows="2" maxlength="2000" placeholder="Add research, constraints, or @tag an agent to refine…" required></textarea>
        <button class="btn outline icon triage-send" type="submit" aria-label="Add ideation note" title="Add ideation note">${ic("send", "ic16")}</button>
      </form>
    </section>`;
  }
  if (!comments.length) return "";
  const open = !!S.openIdeation[w.id];
  const agentName = w.who && M[w.who]?.kind === "agent" ? M[w.who].name : "Executing agents";
  return `<section class="work-ideation-fold" aria-labelledby="work-ideation-fold-h">
    <div class="work-ideation-fold-bar">
      <div class="work-ideation-fold-copy">
        <h3 id="work-ideation-fold-h">Ideation comments <span class="cnt">${comments.length}</span></h3>
        <span class="work-ideation-ax">${esc(agentName)} sees final spec + execution comments only · Opt-in via <code>agent-os task view ${esc(w.id)} --include-ideation</code></span>
      </div>
      <button class="btn outline sm" type="button" data-act="work-toggle-ideation" data-id="${esc(w.id)}" aria-expanded="${open}">${open ? "Hide ideation" : `Show ideation (${comments.length})`}</button>
    </div>
    ${open ? `<div class="work-ideation-fold-body">${workIdeationCommentsList(comments)}</div>` : ""}
  </section>`;
}

function workDetail(w, parent) {
  const team = workTeam(w), eta = workEta(w);
  const priority = workPriorityRank(w.pri) ? esc(w.pri) : "";
  const expandLabel = "Expand task to full page";
  return `<dialog class="side-panel work-panel" id="work-panel" aria-labelledby="work-detail-title" data-item="${esc(w.id)}">
    <section class="triage-reading work-reading"${team ? ` data-team="${esc(team)}"` : ""}>
      <div class="side-panel-header work-detail-bar">
        <p class="work-detail-id">${workPriority(w.pri)}${esc(w.id)}${workStatus(w.status, showsNeedsYou(w))}<span class="work-kind">${{ task: "Task", bug: "Bug", request: "Request" }[w.kind]}</span></p>
        <div class="work-detail-actions">
          <button class="btn ghost icon sm icon-with-tooltip" type="button" data-act="work-comment-focus" aria-label="Leave a comment">${iconWithTooltip("Leave a comment", "chat")}</button>
          <button class="btn ghost icon sm icon-with-tooltip" type="button" data-act="work-copy-link" data-id="${esc(w.id)}" aria-label="Copy task link">${iconWithTooltip("Copy task link", "copy")}</button>
          <button class="btn ghost icon sm icon-with-tooltip work-panel-expand" type="button" data-act="work-expand-detail" aria-label="${expandLabel}">${iconWithTooltip(expandLabel, "maximize")}</button>
          <button class="btn ghost icon sm icon-with-tooltip" type="button" data-act="work-close-detail" aria-label="Close task">${iconWithTooltip("Close task", "x")}</button>
        </div>
      </div>
    <article class="triage-document">
      ${workDetailDraftBanner(w)}
      <header class="triage-intro">
        ${parent ? `<a class="work-parent alink" href="${esc(workItemHref(parent.id))}">${esc(parent.id)} · ${esc(parent.title)}</a>` : ""}
        <h2 tabindex="-1" id="work-detail-title">${esc(w.title)}</h2>
        ${w.description ? `<p class="triage-description work-description">${esc(w.description)}</p>` : ""}
      </header>
      <section class="triage-context" aria-labelledby="work-detail-context">
        <h3 id="work-detail-context">Work details</h3>
        ${workDetailFields(w, team, eta, priority)}
      </section>
      ${workDetailCriteria(w)}
      ${workDetailSubtasks(w)}
      ${workRelatedInbox(w)}
      ${workDetailIdeation(w)}
      ${w.status === "Draft" ? "" : workDiscussion(w)}
    </article>
    </section>
    <div class="toasts" aria-live="polite" role="status"></div>
  </dialog>`;
}

function showSidePanel(panel, animate = true) {
  panel.classList.toggle("is-opening", animate);
  panel.showModal();
  panel.querySelector("[tabindex='-1']")?.focus({ preventScroll: true });
}

function closeWorkPanel() {
  location.hash = workItemHref();
}

function setWorkPanelExpanded(panel, expanded) {
  panel.toggleAttribute("data-expanded", expanded);
  const button = panel.querySelector(".work-panel-expand");
  const label = expanded ? "Restore task panel" : "Expand task to full page";
  button.setAttribute("aria-label", label);
  button.innerHTML = iconWithTooltip(label, expanded ? "minimize" : "maximize");
}

function workRelatedInbox(w) {
  const items = IN.filter((item) => item.task === w.id);
  if (!items.length) return "";
  return `<section class="work-detail-section" aria-labelledby="work-detail-inbox">
    <h3 id="work-detail-inbox">Related inbox items</h3>
    <ul class="work-detail-links" role="list">${items.map((item) => `<li>
      <a class="alink" href="#/inbox/${esc(item.id)}">${ic(KIND[item.kind].icon, "ic16")}
        <span>${esc(item.what)}</span>
        <span class="work-related-state">${item.handled ? "Handled" : esc(KIND[item.kind].label)}</span>
      </a>
    </li>`).join("")}</ul>
  </section>`;
}

const workCommentPrefix = (id) => "agentic-org.v6.work-comment." + id + ".";
const workCriteriaPrefix = (id) => "agentic-org.v6.work-criterion." + id + ".";

function workCriterionChanges(item) {
  return localDetailRecords(workCriteriaPrefix(item.id))
    .filter(({ index, done }) => Number.isInteger(index) && item.criteria?.[index] && typeof done === "boolean");
}

function changeWorkCriterion(input) {
  const { item } = findWorkItem(input.dataset.workCriterion);
  const index = Number(input.dataset.index), criterion = item?.criteria?.[index];
  if (!criterion) return;
  try {
    localStorage.setItem(workCriteriaPrefix(item.id) + crypto.randomUUID(), JSON.stringify({ index, done: input.checked, at: Date.now() }));
  } catch (e) {
    input.checked = criterion.done;
    toast("Could not save the checklist change on this device.");
    return;
  }
  criterion.done = input.checked;
  for (const progress of document.querySelectorAll(`[data-work-progress="${CSS.escape(item.id)}"]`)) {
    progress.outerHTML = workCriteria(item.criteria, item.id);
  }
  document.getElementById("work-history").innerHTML = workHistory(item);
  document.getElementById("work-criteria-status").textContent = `${item.criteria.filter((entry) => entry.done).length} of ${item.criteria.length} complete.`;
}

function workHistory(w) {
  const recent = workCriterionChanges(w).reverse().map(({ index, done, at }) =>
    detailEvent(done ? "check-circle" : "refresh", `<strong>You</strong> ${done ? "completed" : "reopened"} “${esc(w.criteria[index].text)}”`, inboxActivityTime(at)));
  const events = (WORK_DISCUSSIONS[w.id]?.events || []).map(([icon, text, time]) =>
    detailEvent(icon, esc(text), `<span class="triage-event-time">${esc(time)}</span>`));
  if (Number.isFinite(w.createdAt)) events.push(detailEvent("plus", "<strong>You</strong> created this task", inboxActivityTime(w.createdAt)));
  return recent.length || events.length
    ? `<h4 id="work-history-title">Task history</h4><ol class="triage-activity-log" role="list" aria-labelledby="work-history-title">${recent.concat(events).join("")}</ol>`
    : "";
}

function workDiscussion(w) {
  const prefix = workCommentPrefix(w.id);
  const hasIdeation = !!(w.ideationComments?.length);
  return `<section class="triage-activity work-discussion" aria-labelledby="work-activity-title">
    <div class="work-ideation-head">
      <h3 id="work-activity-title">${hasIdeation ? "Execution comments &amp; activity" : "Activity"}</h3>
      ${hasIdeation ? '<span class="work-exec-badge">Visible to agent by default</span>' : ""}
    </div>
    <form class="triage-comment-form" data-form="work-comment" data-id="${esc(w.id)}">
      <label class="sr-only" for="work-comment">Comment on this task</label>
      <textarea id="work-comment" name="comment" rows="2" maxlength="2000" placeholder="Leave a comment…" aria-keyshortcuts="Meta+Enter Control+Enter" required>
${esc(detailCommentDrafts.get(prefix) || "")}</textarea>
      <button class="btn outline icon triage-send" type="submit" aria-label="Send comment" title="Send comment">${ic("send", "ic16")}</button>
    </form>
    <p id="work-comment-status" class="sr-only" role="status"></p>
    <div id="work-comments">${workCommentsHtml(w.id)}</div>
    <div class="triage-history" id="work-history">${workHistory(w)}</div>
  </section>`;
}

function fitTextarea(field) {
  if (!field || CSS.supports("field-sizing", "content")) return;
  field.style.height = "auto";
  field.style.height = `${field.scrollHeight}px`;
}

const WORK_PRESETS = [["all", "All"], ["needs-me", "Needs you"], ["mine", "Mine"]];
const WORK_DEFAULT_GROUP = "status";
const WORK_GROUPS = [["project", "Project"], ["status", "Status"], ["assignee", "Assignee"]];
const WORK_GROUP_FIELDS = { project: "proj", status: "status", assignee: "who" };
const WORK_SORTS = [["default", "Original order"], ["title", "Title"], ["priority", "Priority"], ["eta", "ETA"]];
/* On the board the default order is priority first, so say so */
const BOARD_SORTS = [["default", "Priority first"], ...WORK_SORTS.slice(1)];
const TASK_PRIORITIES = ["Low", "Medium", "High"];
const WORK_TASK_STORE = "agentic-org.v6.task.";
Object.assign(S, { workQuery: "", workStatus: "all", workProject: "all", workTeam: "all", workSort: "default", workOrder: "asc" });

function workSelection() {
  const params = qs();
  const view = params.get("view");
  const primary = params.get("group") ?? view?.match(/^by-(.+)$/)?.[1];
  const subgroup = params.get("subgroup");
  const group = primary === "none" || own(WORK_GROUP_FIELDS, primary) ? primary : WORK_DEFAULT_GROUP;
  return {
    view: WORK_PRESETS.some(([value]) => value === view) ? view : "all",
    group,
    subgroup: group !== "none" && subgroup !== group && own(WORK_GROUP_FIELDS, subgroup) ? subgroup : "none",
  };
}

function workHref({ view = "all", group = WORK_DEFAULT_GROUP, subgroup = "none", item }) {
  const params = new URLSearchParams();
  if (view !== "all") params.set("view", view);
  if (group !== WORK_DEFAULT_GROUP) params.set("group", group);
  if (group !== "none" && subgroup !== "none" && subgroup !== group) params.set("subgroup", subgroup);
  if (item) params.set("item", item);
  return params.size ? "#/work?" + params : "#/work";
}

function selectWorkView(selection, focusSelector) {
  closeMenus();
  const href = workHref(selection);
  if (href === location.hash) {
    document.querySelector(focusSelector)?.focus();
    return;
  }
  S.focusAfter = focusSelector;
  location.hash = href;
}

function workPresetItems(items, view) {
  if (view === "needs-me") return items.filter(needsMe);
  if (view === "mine") return items.filter((w) => w.who === "you");
  return items;
}

function workHasFilters() {
  return S.workStatus !== "all" || S.workProject !== "all" || S.workTeam !== "all";
}

function workMatches(w, query) {
  return [w.id, w.title, w.description, P[w.proj].name, M[w.who]?.name,
    ...(w.subtasks ?? []).flatMap((task) => [task.id, task.title, task.description]),
  ].some((value) => value?.toLowerCase().includes(query));
}

function workFilteredItems() {
  const query = canonicalWorkRef(S.workQuery).toLowerCase();
  const { workStatus: status, workProject: project, workTeam: team } = S;
  return visibleWorkItems(W).filter((w) => {
    if (status !== "all" && w.status !== status) return false;
    if (project !== "all" && w.proj !== project) return false;
    if (team !== "all" && (workTeam(w) ?? "none") !== team) return false;
    return !query || workMatches(w, query);
  });
}

function sortWorkItems(items, sortKey = S.workSort, order = S.workOrder) {
  const descending = order === "desc";
  const read = { title: (w) => w.title, priority: (w) => workPriorityRank(w.pri), eta: (w) => w.etaHours }[sortKey];
  if (!read) return descending ? [...items].reverse() : items;
  return [...items].sort((left, right) => {
    const a = read(left), b = read(right);
    if (a == null || b == null) return Number(a == null) - Number(b == null);
    const cmp = typeof a === "string" ? a.localeCompare(b, undefined, { sensitivity: "base", numeric: true }) : a - b;
    return descending ? -cmp : cmp;
  });
}

function workResults(filtered, { view, group, subgroup }) {
  const items = sortWorkItems(workPresetItems(filtered, view));
  if (!items.length) {
    const text = S.workQuery.trim() || workHasFilters() ? "No work matches these filters."
      : view === "needs-me" ? "Nothing needs you in Work."
      : view === "mine" ? "Nothing is assigned to you." : "No work items yet.";
    return `<p class="empty-row">${text}</p>`;
  }
  return S.workLayout === "board" ? board(items, { needsYouView: view === "needs-me", sortKey: S.workSort, filtered: Boolean(S.workQuery.trim()) || workHasFilters() }) : workList(items, {
    groupBy: WORK_GROUP_FIELDS[group], subgroupBy: WORK_GROUP_FIELDS[subgroup],
  });
}

function refreshWorkResults() {
  const filtered = workFilteredItems();
  document.getElementById("work-results").innerHTML = workResults(filtered, workSelection());
  for (const counter of document.querySelectorAll("[data-work-count]")) {
    counter.textContent = workPresetItems(filtered, counter.dataset.workCount).length;
  }
}

function workViewControls(group, subgroup) {
  const filter = showMenu("work-filter", [
    ["workStatus", "Status", [["all", "All statuses"], ...ALL_STATUSES.map((status) => [status, status])]],
    ["workProject", "Project", [["all", "All projects"], ...Object.entries(P).map(([id, project]) => [id, project.name])]],
    ["workTeam", "Team", [["all", "All teams"], ...Object.entries(TEAMS).map(([id, team]) => [id, team.name]), ["none", "No team"]]],
  ], { icon: "filter", label: "Filter work", text: "Filter", count: [S.workStatus, S.workProject, S.workTeam].filter((v) => v !== "all").length, active: workHasFilters() });
  const listLayout = S.workLayout === "list";
  const display = showMenu("work-display", [
    ...(listLayout ? [["workGroup", "Group by", [["none", "No grouping"], ...WORK_GROUPS]]] : []),
    ...(listLayout && group !== "none"
      ? [["workSubgroup", "Subgroup by", [["none", "No subgroup"], ...WORK_GROUPS.filter(([value]) => value !== group)]]]
      : []),
    ...(listLayout ? [] : [BOARD_COLUMN_OPTION]),
    ["workSort", "Sort by", listLayout ? WORK_SORTS : BOARD_SORTS],
    ["workOrder", "Direction", [["asc", "Ascending"], ["desc", "Descending"]]],
  ], {
    icon: "adjustments", label: "Work display", text: listLayout ? "Display" : `Display · ${S.workBoardStages === "detailed" ? "Status" : "Stages"}`,
    active: isWorkDisplayActive(listLayout, group, subgroup, S.workSort, S.workOrder),
    footer: workDisplayFooter(listLayout),
  });
  return `<div class="queue-controls" role="group" aria-label="Work view controls">${searchToggle("work-search", "Search work", S.workQuery)}${filter}${display}</div>`;
}

function statusOptions(statuses, selected) {
  return statuses.map((status) => `<option${status === selected ? " selected" : ""}>${esc(status)}</option>`).join("");
}

function pageWork() {
  const selection = workSelection(), filtered = workFilteredItems();
  S.workGroup = selection.group;
  S.workSubgroup = selection.subgroup;
  return `<header class="work-header queue-header">
    ${queueHeadline("Work", `${W.filter(OPEN).length} open · ${W.filter(needsMe).length} need you`)}
    <div class="work-toolbar queue-toolbar">
      <div class="primary-tab-row" data-page-menu>
        <div class="queue-tabs" role="group" aria-label="Work views">
          ${WORK_PRESETS.map(([value, label]) => `<button class="queue-tab" type="button" data-act="work-preset"
            data-v="${value}" aria-pressed="${selection.view === value}" data-menu-label="${esc(label)}">${label}<span data-work-count="${value}">${workPresetItems(filtered, value).length}</span></button>`).join("")}
        </div>
      </div>
      ${workLayoutToggle("workLayout")}
      ${workViewControls(selection.group, selection.subgroup)}
      ${workArchivedChip()}
      <div class="work-tb-end">${workCalendarLink()}<button class="btn primary" id="work-new-task" type="button" data-act="work-new-task">${ic("plus", "ic16")}New task</button></div>
    </div>
  </header>${workWorkspace(workResults(filtered, selection), W, searchRow("work-search", "Search work", S.workQuery, "Search work… (Esc to close)", "work-search-row"))}`;
}

function openWorkTask(trigger) {
  const status = STATUSES.task.includes(S.workStatus) ? S.workStatus : STATUSES.task[0];
  const projects = Object.entries(P).filter(([, project]) => !project.archived);
  const form = `<dialog class="work-task-dialog" id="work-task-dialog" aria-labelledby="work-task-title">
    <h2 id="work-task-title">New task</h2>
    <form class="work-task-form" data-form="work-create">
      <label>Title<input class="field" name="title" required maxlength="160" autofocus></label>
      <label>Description <textarea class="field" name="description" rows="4" maxlength="4000"></textarea></label>
      <label>Project<select class="field" name="project">${projects.map(([id, project]) => `<option value="${esc(id)}"${id === S.workProject ? " selected" : ""}>${esc(project.name)}</option>`).join("")}</select></label>
      <div class="work-task-fields">
        <label>Assignee<select class="field" name="assignee">
          ${memberIds().map((id) => `<option value="${esc(id)}"${id === "you" ? " selected" : ""}>${esc(M[id].name)}</option>`).join("")}
          <option value="">Unassigned</option>
        </select></label>
        <label>Status<select class="field" name="status">${statusOptions(STATUSES.task, status)}</select></label>
        <label>Priority<select class="field" name="priority"><option value="">No priority</option>${statusOptions(TASK_PRIORITIES, "Medium")}</select></label>
        <label>ETA (hours)<input class="field" name="eta" type="number" min="0" step="any" placeholder="Optional"></label>
      </div>
      <div class="actions">
        <button class="btn outline" type="button" data-act="work-cancel-task">Cancel</button>
        <button class="btn primary" type="submit">Create task</button>
      </div>
    </form>
  </dialog>`;
  const overlay = document.getElementById("overlay-root");
  overlay.insertAdjacentHTML("beforeend", form);
  const dialog = document.getElementById("work-task-dialog");
  dialog.addEventListener("close", () => dialog.remove(), { once: true });
  trigger.focus({ preventScroll: true });
  dialog.showModal();
}

const HQ_WORK_REF = /^T-A([1-9]\d*)$/;
const OLD_LOCAL_WORK_REF = /^T-[0-9A-F]{8}$/;
const workRefNumber = (ref) => Number(ref?.match(HQ_WORK_REF)?.[1]);

function isValidSavedTaskMeta({ pri, etaHours, createdAt }) {
  if (pri != null && pri !== "" && !workPriorityRank(pri)) return false;
  if (etaHours != null && (!Number.isFinite(etaHours) || etaHours < 0)) return false;
  return Number.isFinite(createdAt) && Number.isFinite(new Date(createdAt).getTime());
}

function isValidSavedTaskFields(key, data) {
  const { id, title, proj, who, status } = data;
  const allowedStatuses = new Set(["Draft", ...ALL_STATUSES]);
  if (typeof id !== "string" || key !== WORK_TASK_STORE + id) return false;
  if (typeof title !== "string" || !title.trim() || !own(P, proj)) return false;
  if ((who !== null && !own(M, who)) || !allowedStatuses.has(status)) return false;
  return isValidSavedTaskMeta(data);
}

function resolveSavedWorkIds(id, ref) {
  const legacy = OLD_LOCAL_WORK_REF.test(id);
  const number = workRefNumber(typeof ref === "string" ? ref : id);
  const validRef = Number.isSafeInteger(number) && (ref == null ? !legacy : legacy || id === ref);
  if (!validRef && !legacy) return null;
  return {
    id: validRef ? ref ?? id : id,
    legacyId: legacy && (!validRef || ref !== id) ? id : undefined,
  };
}

function parseSavedWorkTask(key) {
  try {
    const data = JSON.parse(localStorage.getItem(key));
    if (!data || !isValidSavedTaskFields(key, data)) return null;
    const ids = resolveSavedWorkIds(data.id, data.ref);
    if (!ids) return null;
    const { title, description, proj, who, status, pri, etaHours, createdAt, kind, criteria, ideationComments, materializedFromIdeation, requestedBy } = data;
    return {
      ...ids, title,
      description: typeof description === "string" ? description.slice(0, 4000) : "",
      proj, who, status, pri, etaHours, createdAt,
      kind: kind || "task",
      ...(Array.isArray(criteria) ? { criteria } : {}),
      ...(Array.isArray(ideationComments) ? { ideationComments } : {}),
      ...(materializedFromIdeation ? { materializedFromIdeation: true } : {}),
      ...(requestedBy ? { requestedBy } : {}),
    };
  } catch (e) {
    return null;
  }
}

function savedWorkTasks() {
  const tasks = [];
  try {
    for (const key of Object.keys(localStorage)) {
      if (!key.startsWith(WORK_TASK_STORE)) continue;
      const parsed = parseSavedWorkTask(key);
      if (parsed) tasks.push(parsed);
    }
  } catch (e) {}
  return tasks.sort((a, b) => a.createdAt - b.createdAt || a.id.localeCompare(b.id));
}

function workItems() {
  return W.flatMap((task) => [task, ...(task.subtasks ?? []).map((child) => workChild(task, child))]);
}

function lastWorkNumber(saved) {
  return [...workItems(), ...saved].reduce((last, { id, legacyId }) =>
    Math.max(last, (id === legacyId ? 0 : workRefNumber(id)) || 0), 0);
}

function saveWorkTask(task) {
  const id = task.legacyId || task.id;
  try {
    localStorage.setItem(WORK_TASK_STORE + id, JSON.stringify({ ...task, id, ref: task.id }));
    return true;
  } catch (e) {
    return false;
  }
}

function loadWorkTasks() {
  const saved = savedWorkTasks();
  let last = lastWorkNumber(saved);
  for (const task of saved) {
    if (task.id !== task.legacyId) continue;
    task.id = "T-A" + ++last;
    saveWorkTask(task);
  }
  W.unshift(...saved.reverse());
  for (const task of workItems()) {
    if (!task.criteria?.length) continue;
    for (const { index, done } of workCriterionChanges(task)) task.criteria[index].done = done;
  }
}

function persistCreatedWorkTask(fields) {
  const task = {
    id: "T-A" + (lastWorkNumber(savedWorkTasks()) + 1),
    createdAt: Date.now(),
    kind: "task",
    ...fields,
  };
  saveWorkTask(task);
  W.unshift(task);
  return task;
}

function publishDraftTask(id) {
  const w = W.find((item) => item.id === id);
  if (!w || w.status !== "Draft") return;
  w.status = w.kind === "bug" ? STATUSES.bug[0] : STATUSES.task[0];
  w.materializedFromIdeation = true;
  w.publishedAt = Date.now();
  S.openIdeation[w.id] = false;
  for (const inboxItem of IN) {
    if (inboxItem.task === w.id && inboxItem.sub === "Draft refinement") {
      inboxItem.handled = true;
      inboxItem.outcome = `Published to ${w.status}`;
    }
  }
  saveWorkTask(w);
  WORK_DISCUSSIONS[w.id] = WORK_DISCUSSIONS[w.id] || { events: [] };
  WORK_DISCUSSIONS[w.id].events.unshift(["check-circle", `Published from Draft to ${w.status} (${(w.ideationComments || []).length} ideation comments folded)`, "Just now"]);
  toast(`Published ${w.id} to ${w.status} · New comments are now official execution comments`, "check-circle");
}

function addIdeationComment(form) {
  const id = form.dataset.id, w = W.find((item) => item.id === id);
  const ta = form.elements.comment, text = ta?.value?.trim();
  if (!w || !text) return;
  w.ideationComments = w.ideationComments || [];
  w.ideationComments.push({ id: "ic-" + Date.now(), who: "you", at: Date.now(), text });
  const lead = w.who && M[w.who] ? w.who : "ada";
  const prefix = w.description.includes("\n\nAgreed during ideation:") ? "\n" : "\n\nAgreed during ideation:\n";
  w.description = `${w.description.replace(/\s+$/, "")}${prefix}• ${text}`;
  w.ideationComments.push({
    id: "ic-ack-" + Date.now(),
    who: lead,
    at: Date.now() + 50,
    text: `Agreed — I updated the task description above with that decision. Ready to publish whenever you are.`,
  });
  saveWorkTask(w);
  ta.value = "";
  toast(`${M[lead]?.name || "Lead"} updated the Draft description`, "check-circle");
}

async function createWorkTask(form) {
  const data = new FormData(form);
  const title = data.get("title").trim();
  if (!title) {
    form.elements.title.setCustomValidity("Enter a task title.");
    form.elements.title.reportValidity();
    return;
  }
  const dialog = form.closest("dialog");
  const submit = form.querySelector('[type="submit"]');
  submit.disabled = true;
  const create = () => {
    if (!dialog.open || !dialog.isConnected) return;
    const task = {
      id: "T-A" + (lastWorkNumber(savedWorkTasks()) + 1),
      title, description: data.get("description").trim(), kind: "task", createdAt: Date.now(),
      proj: data.get("project"), who: data.get("assignee") || null,
      status: data.get("status"), pri: data.get("priority"),
      etaHours: data.get("eta") === "" ? null : Number(data.get("eta")),
    };
    const saved = saveWorkTask(task);
    W.unshift(task);
    dialog.close();
    Object.assign(S, { workQuery: "", workStatus: "all", workProject: "all", workTeam: "all" });
    const selection = workSelection();
    const view = workPresetItems([task], selection.view).length ? selection.view : "all";
    location.hash = workHref({ ...selection, view, item: task.id });
    toast(saved ? `Created ${task.id}` : `Created ${task.id} for this session`);
  };
  try {
    if (navigator.locks) await navigator.locks.request(WORK_TASK_STORE, create);
    else create();
  } catch (e) {
    toast("Could not create the task. Try again.");
  } finally {
    submit.disabled = false;
  }
}

export {
  mountBoard,
  addIdeationComment,
  canonicalWorkRef,
  changeWorkCriterion,
  closeWorkPanel,
  createWorkTask,
  findWorkItem,
  fitTextarea,
  isArchivedWork,
  loadWorkTasks,
  normalizeScopedWorkState,
  openWorkTask,
  pageWork,
  persistCreatedWorkTask,
  publishDraftTask,
  refreshScopedWorkCounts,
  refreshWorkResults,
  scopedWorkQueries,
  scopedWorkResults,
  scopedWorkToolbar,
  scopedWorkType,
  scopedWorkView,
  scopedWorkWorkspace,
  selectWorkView,
  setWorkPanelExpanded,
  showSidePanel,
  teamWork,
  visibleWorkItems,
  workCommentPrefix,
  workGroups,
  workHref,
  workItemHref,
  workItems,
  workList,
  workSelection,
};
