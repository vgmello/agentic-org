import { ALL_STATUSES, IN, KIND, M, OPEN, P, STATUSES, TEAMS, W, WORK_DISCUSSIONS, needsMe, ownerAsks } from "../data.js";
import { detailCommentDrafts, detailEvent, inboxActivityTime, localDetailRecords, workCommentsHtml } from "./inbox.js";
import { closeMenus } from "../shell/actions.js";
import { own, route } from "../shell/router.js";
import { S, WORK_ARCHIVED_STORE, qs } from "../state.js";
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
/* The board has one Done history control, a group in its Display menu (Last 7 days / All unarchived /
   Include archived); the list keeps Show archived there. Both read and write the same state. */
function workDisplayFooter(listLayout, calendar = false) {
  const overflow = calendar ? `<a class="work-cal-overflow" role="menuitem" href="#/calendar">${ic("calendar-check", "ic20")}Calendar</a>` : "";
  if (!listLayout) return overflow ? `<hr class="work-cal-overflow">${overflow}` : "";
  return `<hr><p class="menu-h">Options</p>${workListDisplayOptions()}${displayCheckbox("work-archived", "Show archived", S.workShowArchived)}${overflow ? `<hr class="work-cal-overflow">${overflow}` : ""}`;
}
function isWorkDisplayActive(listLayout, group, subgroup, sort, order) {
  return (listLayout && (group !== WORK_DEFAULT_GROUP || subgroup !== "none" || S.workShowArchived || !S.workSubtasks || S.workDividers))
    || (!listLayout && doneHistory() !== "recent")
    || sort !== "default" || order !== "asc";
}
function workLayoutToggle(key) {
  return `<div class="work-layout-toggle" role="group" aria-label="Work layout">
    ${[["list", "List"], ["board", "Board"]].map(([layout, label]) => `<button class="queue-tab" type="button"
      data-act="set" data-k="${key}" data-v="${layout}" aria-pressed="${S[key] === layout}" aria-controls="work-results">${ic(layout, "ic16")}<span class="work-layout-word">${label}</span></button>`).join("")}
  </div>`;
}
/* Board columns choice lives in Display (next to the list's Group by), not in the toolbar */
const BOARD_COLUMN_OPTION = ["workBoardStages", "Board columns", [["unified", "Stages"], ["detailed", "Status"]]];
/* The board's default "Attention first" order is fixed, so Direction only applies to an explicit sort.
   Direction is named for what it does to the chosen sort ("Highest first", "A to Z"). */
const workSortOrder = (layout, sort, order) => (layout === "board" && sort === "default" ? "asc" : order);
const SORT_DIRECTIONS = {
  priority: ["Highest first", "Lowest first"], eta: ["Soonest first", "Latest first"],
  title: ["A to Z", "Z to A"], default: ["As listed", "Reversed"],
};
function workDirectionOption(key, layout, sort) {
  if (layout === "board" && sort === "default") return [];
  const [asc, desc] = SORT_DIRECTIONS[sort] ?? SORT_DIRECTIONS.default;
  return [[key, "Direction", [["asc", asc], ["desc", desc]]]];
}
/* "Display ▾" with the board's column mode as secondary text; the accessible name says the mode too */
function workDisplayTrigger(listLayout) {
  if (listLayout) return { icon: "adjustments", label: "Work display", text: "Display" };
  const mode = S.workBoardStages === "detailed" ? "Status" : "Stages";
  return { icon: "adjustments", label: `Work display, ${mode} columns`, text: "Display", sub: mode };
}
const workCalendarLink = () => `<a class="btn ghost menu-textbtn work-cal-btn" href="#/calendar" aria-label="Calendar" title="Calendar">${ic("calendar-check", "ic16")}<span>Calendar</span></a>`;
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

/* A non-default history is said in the toolbar, with one click back to the default */
function workArchivedChip(layout) {
  if (layout === "board") {
    const history = doneHistory();
    if (history === "recent") return "";
    const text = history === "archived" ? "Done: with archived" : "Done: all unarchived";
    return `<button class="work-archived-chip" type="button" data-act="board-done" data-v="recent" aria-label="${text}. Show the last ${DONE_WINDOW_DAYS} days only" title="Show the last ${DONE_WINDOW_DAYS} days only">${text}${ic("x", "ic14")}</button>`;
  }
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

function scopedWorkBaseItems(type, items, query = "", archivedOnly = false) {
  const { scopedWorkStatus: status, scopedWorkProject: project, scopedWorkTeam: team } = S;
  return (archivedOnly ? items.filter(isArchivedWork) : visibleWorkItems(items)).filter((w) => {
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

/* Active filters in words: the Filter trigger shows a single one ("Filter  Fixed"), the board's summary shows all */
function filterParts(query, filters) {
  return [...(query ? [`Search “${query}”`] : []), ...filters.filter(([, v]) => v).map(([name, v]) => `${name}: ${v}`)];
}
const teamName = (team) => (team === "none" ? "No team" : TEAMS[team]?.name);
function workFilterValues() {
  return [["Status", S.workStatus !== "all" && S.workStatus], ["Project", S.workProject !== "all" && P[S.workProject]?.name], ["Team", S.workTeam !== "all" && teamName(S.workTeam)]];
}
function scopedWorkFilterValues(type) {
  return [["Status", S.scopedWorkStatus !== "all" && S.scopedWorkStatus],
    ["Project", type !== "project" && S.scopedWorkProject !== "all" && P[S.scopedWorkProject]?.name],
    ["Team", type === "project" && S.scopedWorkTeam !== "all" && teamName(S.scopedWorkTeam)]];
}
function filterTrigger(values) {
  const active = values.filter(([, v]) => v);
  const one = active.length === 1 ? active[0] : null;
  return {
    icon: "filter", text: "Filter", active: active.length > 0,
    label: one ? `Filter work, ${one[0]} ${one[1]}` : "Filter work",
    sub: one ? one[1] : "", count: active.length > 1 ? active.length : 0,
  };
}

/* Zero results from filters or search always offer the way back, and say when hidden history matches */
const clearFiltersButton = () => `<button class="btn outline sm" type="button" data-act="work-clear-filters">${ic("x", "ic14")}Clear filters</button>`;
const matchWord = (n) => (n === 1 ? "match" : "matches");
function revealButtons({ older = 0, archived = 0 } = {}) {
  const button = (v, text) => `<button class="btn outline sm" type="button" data-act="work-reveal" data-v="${v}">${ic("clock", "ic14")}${esc(text)}</button>`;
  return `${older ? button("all", `Show ${older} older ${matchWord(older)}`) : ""}${archived ? button("archived", `Show ${archived} archived ${matchWord(archived)}`) : ""}`;
}
function workEmpty(text, filtered, hidden) {
  const reveal = revealButtons(hidden);
  return `<div class="empty-row work-empty"${filtered ? " data-filtered" : ""}><p>${esc(text)}</p>${reveal || filtered ? `<div class="work-empty-actions">${reveal}${filtered ? clearFiltersButton() : ""}</div>` : ""}</div>`;
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
  const filter = showMenu("scoped-work-filter", filterGroups, filterTrigger(scopedWorkFilterValues(type)));
  const listLayout = S.scopedWorkLayout === "list";
  const { allowed, group, subgroup } = scopedWorkGrouping(type);
  const display = showMenu("scoped-work-display", [
    ...(listLayout ? [["scopedWorkGroup", "Group by", [["none", "No grouping"], ...allowed]]] : []),
    ...(listLayout && group !== "none"
      ? [["scopedWorkSubgroup", "Subgroup by", [["none", "No subgroup"], ...allowed.filter(([v]) => v !== group)]]]
      : []),
    ...(listLayout ? [] : [BOARD_COLUMN_OPTION]),
    ["scopedWorkSort", "Sort by", listLayout ? WORK_SORTS : BOARD_SORTS],
    ...doneHistoryOption(listLayout),
    ...workDirectionOption("scopedWorkOrder", S.scopedWorkLayout, S.scopedWorkSort),
  ], {
    ...workDisplayTrigger(listLayout),
    active: isWorkDisplayActive(listLayout, group, subgroup, S.scopedWorkSort, workSortOrder(S.scopedWorkLayout, S.scopedWorkSort, S.scopedWorkOrder)),
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
            data-k="scopedWorkView" data-v="${value}" aria-pressed="${activeView === value}" aria-controls="work-results"${value === "mine" ? mineHint : ""}>${label}<span data-scoped-work-count="${value}">${visibleCount(scopedWorkPresetItems(base, value, type, id), value, S.scopedWorkLayout)}</span></button>`).join("")}
        </div>
      </div>
      ${workLayoutToggle("scopedWorkLayout")}
      ${scopedWorkMenus(type)}
      ${workArchivedChip(S.scopedWorkLayout)}
      ${type === "project" || type === "team" ? `<button class="btn primary" id="work-new-task" type="button" data-act="work-new-task" aria-label="New task">${ic("plus", "ic16")}New<span class="work-new-word">task</span></button>` : ""}
    </div>
  </div>`;
}

function refreshScopedWorkCounts({ type, id, key, items }) {
  if (!type) return;
  const query = canonicalWorkRef(scopedWorkQueries.get(key)).toLowerCase();
  const base = scopedWorkBaseItems(type, items, query);
  for (const counter of document.querySelectorAll("[data-scoped-work-count]")) {
    const value = counter.dataset.scopedWorkCount;
    counter.textContent = visibleCount(scopedWorkPresetItems(base, value, type, id), value, S.scopedWorkLayout);
  }
}

function scopedWorkResults({ type, id, key, items }) {
  const query = canonicalWorkRef(scopedWorkQueries.get(key)).toLowerCase();
  const presets = scopedWorkPresets(type, id);
  const view = presets.some(([v]) => v === S.scopedWorkView) ? S.scopedWorkView : "all";
  const base = scopedWorkBaseItems(type, items, query);
  const filtered = sortWorkItems(scopedWorkPresetItems(base, view, type, id), S.scopedWorkSort, workSortOrder(S.scopedWorkLayout, S.scopedWorkSort, S.scopedWorkOrder));
  const isFiltered = Boolean(query) || scopedWorkHasFilters(type);
  const archived = S.workShowArchived ? 0 : scopedWorkPresetItems(scopedWorkBaseItems(type, items, query, true), view, type, id).length;
  if (!filtered.length) return workEmpty(scopedWorkEmptyText(type, id, view, query), isFiltered, { archived });
  const hideOwner = type === "agent" || type === "person";
  if (S.scopedWorkLayout === "board") {
    const summary = filterParts(scopedWorkQueries.get(key)?.trim(), scopedWorkFilterValues(type));
    return board(filtered, { hideOwner, hideProject: type === "project", needsYouView: view === "needs-me", sortKey: S.scopedWorkSort, filtered: isFiltered, summary, query, archived, emptyText: scopedWorkEmptyText(type, id, view, query) });
  }
  const { group, subgroup } = scopedWorkGrouping(type);
  return workList(filtered, {
    groupBy: WORK_GROUP_FIELDS[group],
    subgroupBy: WORK_GROUP_FIELDS[subgroup],
    hideOwner, filtered: isFiltered,
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
/* One rule for the Needs you filter, the card's amber signal and its badge */
const showsNeedsYou = needsMe;
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
function workList(items, { groupBy, subgroupBy, emptyText = "No work here yet.", hideOwner = false, filtered = false } = {}) {
  if (!items.length) return `<p class="empty-row">${esc(emptyText)}</p>`;
  const list = (rows) => `<ul class="work-list" role="list">${rows.map((w) => workRow(w, false, hideOwner)).join("")}</ul>`;
  const open = `<div class="work-items"${filtered ? " data-filtered" : ""}>`;
  if (!groupBy) return `${open}${list(items)}</div>`;
  const sections = workGroups(items, groupBy).map(({ label, rows }) => {
    const body = subgroupBy ? `<div class="work-subgroups">${workGroups(rows, subgroupBy).map((subgroup) => `
      <div class="work-subgroup"><h3 class="work-subgroup-title">${esc(subgroup.label)}<span class="cnt">${subgroup.rows.length}</span></h3>
        ${list(subgroup.rows)}</div>`).join("")}</div>` : list(rows);
    return `<div class="work-group"><h2 class="work-group-title">${esc(label)}<span class="cnt">${rows.length}</span></h2>${body}</div>`;
  });
  return `${open}${sections.join("")}</div>`;
}
const BOARD_STAGES = ["To do", "In progress", "Blocked", "Done"];
const BOARD_STAGE_OF = { New: "To do", Triaged: "In progress", Scoped: "In progress", "Rowan is scoping": "In progress", Draft: "To do", Fixed: "Done", Closed: "Done", Approved: "Done", Declined: "Done" };
/* Status columns: real statuses in lifecycle order, nothing merged; empty ones are left out */
const BOARD_STATUS_ORDER = ["Draft", "New", "Triaged", "Rowan is scoping", "Scoped", "To do", "In progress", "Blocked", "Fixed", "Done", "Approved", "Closed", "Declined"];
const BOARD_STAGE_STATUS = { "To do": "To do", "In progress": "In progress", Blocked: "Blocked", Done: "Done" };
const DONE_WINDOW_DAYS = 7;
const isClosed = (w) => !OPEN(w);

/* Priority reads like the List: the same bars before the reference, and always a word beside them. A priority
   someone set reads "High"; a missing one is inferred (needs you, blocked or a Critical/Major bug counts as High,
   anything else as Medium) and says so in text: "High · inferred", or "~High" in narrow lanes, with the full wording
   in its accessible name and tooltip. Bug pri is severity, labelled separately; request pri is request metadata
   ("Sent Nov 9"), so both of those are always inferred. */
const BUG_SEVERITIES = ["Minor", "Normal", "Major", "Critical"];
const bugSeverity = (w) => (w.kind === "bug" && BUG_SEVERITIES.includes(w.pri) ? w.pri : "");
const inferredPriority = (w) => (showsNeedsYou(w) || w.status === "Blocked" || ["Major", "Critical"].includes(bugSeverity(w)) ? "High" : "Medium");
const setPriority = (w) => (w.kind !== "request" && !bugSeverity(w) && workPriorityRank(w.pri) ? w.pri : "");
const priorityBars = (level, inferred) => `<span class="work-priority-mark${inferred ? " is-inferred" : ""}" data-level="${Math.min(level, 3)}" aria-hidden="true"><span></span><span></span><span></span></span>`;
function boardPriorityMark(w) {
  const set = setPriority(w);
  if (set) {
    const label = esc(`${set} priority`);
    return `<span class="bpri" data-priority="${esc(set.toLowerCase())}" role="img" aria-label="${label}" title="${label}">${priorityBars(WORK_PRIORITY_RANKS[set], false)}<span class="bpri-word">${esc(set)}</span></span>`;
  }
  const guess = inferredPriority(w), label = esc(`No priority set, inferred ${guess}`);
  return `<span class="bpri is-inferred" role="img" aria-label="${label}" title="${label}">${priorityBars(WORK_PRIORITY_RANKS[guess], true)}<span class="bpri-word"><span class="bpri-tilde">~</span>${guess}<span class="bpri-inf"> · inferred</span></span></span>`;
}
/* Bug severity is its own fact, never read as priority */
function boardSeverity(w) {
  const sev = bugSeverity(w);
  if (!sev) return "";
  const icon = sev === "Critical" ? "warn" : sev === "Major" ? "exclaim" : "";
  return `<span class="btag bsev" data-severity="${esc(sev.toLowerCase())}">${icon ? ic(icon, "ic14") : ""}${esc(sev)} severity</span>`;
}
/* Why a card needs you or is blocked. The badge counts and links the same open owner asks that put
   the card in Needs you, so handling one request never hides another that is still waiting. */
function boardReason(w) {
  if (showsNeedsYou(w)) {
    const asks = ownerAsks(w);
    const what = asks.length > 1 ? `${asks.length} requests` : !w.who ? "Assign" : asks[0]?.sub || "Unblock";
    return { badge: `Needs you · ${what}`, asks };
  }
  if (w.status === "Blocked") return { line: w.waitsOn && w.waitsOn !== "you" && M[w.waitsOn] ? `Waiting on ${M[w.waitsOn].name}` : "Blocked" };
  return {};
}
/* Expiring asks come first, and the soonest expiry is said on the card, next to the badge */
const expiryRank = (ask) => ask.expiresInHours ?? (ask.expires ? 1e6 : 1e7);
const expiryWhen = (ask) => ask.expires.replace(/^Expires\s+/i, "").replace(/\s*\(.*\)$/, "");
function boardNeedDue(w, asks) {
  const expiring = asks.filter((ask) => ask.expires);
  if (!expiring.length) return { due: "", describedBy: "" };
  const when = expiryWhen(expiring[0]), id = "due-" + w.id;
  const text = asks.length === 1 ? `Expires ${when}` : expiring.length === 1 ? `1 expires ${when}` : `${expiring.length} expire, first ${when}`;
  return { due: `<span class="board-need-due" id="${esc(id)}">${ic("clock", "ic14")}${esc(text)}</span>`, describedBy: ` aria-describedby="${esc(id)}"` };
}
function boardNeedItem(ask) {
  const exp = ask.expires ? `<span class="board-need-exp">${ic("clock", "ic14")}${esc(`Expires ${expiryWhen(ask)}`)}</span>` : "";
  return `<a role="menuitem" tabindex="-1" href="#/inbox/${esc(ask.id)}"><span class="board-need-sub"><span>${esc(ask.sub)}</span>${exp}</span><span class="board-need-what">${esc(ask.what)}</span></a>`;
}
/* One request opens it in Inbox; several open a menu that lists each of them. The menu is a top-layer popover,
   so the board's scroller never clips it. */
function boardNeedBadge(w, { badge, asks }) {
  if (!asks.length) return pill(badge, "amber", "bell", true);
  const ordered = [...asks].sort((a, b) => expiryRank(a) - expiryRank(b));
  const { due, describedBy } = boardNeedDue(w, ordered);
  if (asks.length === 1) return `<a class="pill t-amber chip board-need" href="#/inbox/${esc(asks[0].id)}" title="Open in Inbox"${describedBy}>${ic("bell", "ic14")}${esc(badge)}${ic("chev-right", "ic14")}</a>${due}`;
  const menuId = "need-" + w.id;
  return `<div class="menu-wrap board-need-wrap"><button class="pill t-amber chip board-need" type="button" data-act="menu" data-id="${esc(menuId)}" aria-haspopup="menu" aria-expanded="false" aria-controls="menu-${esc(menuId)}"${describedBy}>${ic("bell", "ic14")}${esc(badge)}${ic("chev-down", "ic14")}</button>
    <div class="menu board-need-menu board-pop-menu" id="menu-${esc(menuId)}" role="menu" aria-label="${esc(`Requests waiting on you for ${w.id}`)}" popover="manual" hidden>${ordered.map(boardNeedItem).join("")}</div></div>${due}`;
}
/* Reference row: priority (bars and word) and the reference, as in the List. The project always takes its own
   line under it, so it is in the same place on every card at every lane width. */
function boardMeta(w, hideProject) {
  const type = w.kind === "bug" ? "Bug" : w.kind === "request" ? "Request" : "";
  const ref = `<span class="board-ref">${boardPriorityMark(w)}${type ? `<span class="btype">${type}</span>` : ""}<span class="mono">${esc(w.id)}</span></span>`;
  const project = hideProject ? "" : `<span class="board-project">${plink(w.proj)}</span>`;
  return `<div class="board-card-meta">${ref}${project}</div>`;
}
/* Small facts, each said once: exact status when it differs from the lane, bug severity, ETA, criteria, subtasks.
   Narrow lanes keep a short word on each count ("2/3 crit", "2 sub"), never a bare number. */
function boardCountTags(w) {
  const crit = w.criteria?.length ? `<span class="btag">${ic("check-circle", "ic14")}${workCriteria(w.criteria, w.id)}<span class="btag-word" aria-hidden="true">criteria</span><span class="btag-short" aria-hidden="true">crit</span></span>` : "";
  const n = w.subtasks?.length ?? 0, word = n === 1 ? "subtask" : "subtasks";
  const subs = n ? `<span class="btag" title="${n} ${word}">${ic("list", "ic14")}<span aria-hidden="true">${n}</span><span class="btag-word" aria-hidden="true">${word}</span><span class="btag-short" aria-hidden="true">sub</span><span class="sr-only">${n} ${word}</span></span>` : "";
  return crit + subs;
}
function boardTags(w, sortKey) {
  const st = S.workBoardStages !== "detailed" && !BOARD_STAGES.includes(w.status) ? `<span class="bstatus">${workStatus(w.status)}${esc(w.status)}</span>` : "";
  const eta = w.etaHours != null || sortKey === "eta" ? workEta(w) : "";
  const etaTag = eta ? `<span class="btag beta">${ic("clock", "ic14")}ETA ${esc(eta)}</span>` : "";
  const tags = `${st}${boardSeverity(w)}${etaTag}${boardCountTags(w)}`;
  return tags ? `<div class="board-tags">${tags}</div>` : "";
}
/* A search that matches a subtask says which one on its parent card, with a link to it */
function boardSubtaskHits(w, query) {
  if (!query) return "";
  const hits = (w.subtasks ?? []).filter((t) => [t.id, t.title, t.description].some((v) => v?.toLowerCase().includes(query)));
  if (!hits.length) return "";
  return `<ul class="board-subhits" role="list" aria-label="${esc(`Matching subtasks of ${w.id}`)}">${hits.map((t) => `<li>${ic("search", "ic14")}<span class="board-subhit-word">Subtask</span><a class="alink" data-item="${esc(t.id)}" href="${esc(workItemHref(t.id))}" aria-haspopup="dialog">${workStatus(t.status)}<span class="mono">${esc(t.id)}</span> ${esc(t.title)}</a></li>`).join("")}</ul>`;
}
function boardOwner(w) {
  if (!w.who || !M[w.who]) return `<span class="board-assignee">${av(null, 24)}<span class="board-owner-label">Unassigned</span></span>`;
  return `<span class="board-assignee">${av(w.who, 24)}<span class="board-owner-label">${mlink(w.who)}</span><span class="board-kind">${esc(kindWord(w.who))}</span></span>`;
}
/* Footer: owner on the start side; team (as the List shows it) and Archived on the end side */
function boardFooter(w, hideOwner) {
  const team = workTeam(w), archived = isArchivedWork(w);
  const end = `${team ? `<span class="work-team board-team">${esc(TEAMS[team].name)}</span>` : ""}${archived ? pill("Archived", "gray", null, true) : ""}`;
  if (hideOwner && !end) return "";
  return `<div class="board-card-footer">${hideOwner ? "" : boardOwner(w)}${end ? `<span class="board-footer-end">${end}</span>` : ""}</div>`;
}
function boardCard(w, { hideOwner, hideProject, selected, sortKey, query }) {
  const attention = showsNeedsYou(w), reason = boardReason(w);
  const badge = reason.badge ? boardNeedBadge(w, reason) : "";
  const status = badge ? `<div class="board-status">${badge}</div>` : reason.line ? `<p class="board-reason">${ic("pause", "ic14")}${esc(reason.line)}</p>` : "";
  const team = workTeam(w);
  return `<li class="bcard" data-work-id="${esc(w.id)}"${team ? ` data-team="${esc(team)}"` : ""}${attention ? " data-attention" : ""}${selected ? ' aria-current="true"' : ""}>
    <a class="work-item-link t" data-item="${esc(w.id)}" href="${esc(workItemHref(w.id))}" aria-haspopup="dialog" title="${esc(w.title)}">${esc(w.title)}</a>
    <div class="board-facts">${boardMeta(w, hideProject)}${boardTags(w, sortKey)}</div>
    ${boardSubtaskHits(w, query)}
    ${status}
    ${boardFooter(w, hideOwner)}
  </li>`;
}
/* Attention first (the board's default): owner asks that expire, soonest deadline first; then other cards that
   need you; then everything else. Priority (set or inferred) breaks ties at every tier. */
const boardRank = (w) => workPriorityRank(setPriority(w)) ?? WORK_PRIORITY_RANKS[inferredPriority(w)];
function attentionKey(w) {
  if (!showsNeedsYou(w)) return [2, 0];
  const soonest = Math.min(...ownerAsks(w).filter((ask) => ask.expires).map(expiryRank));
  return Number.isFinite(soonest) ? [0, soonest] : [1, 0];
}
function byAttention(a, b) {
  const [ta, da] = attentionKey(a), [tb, db] = attentionKey(b);
  return ta - tb || da - db || boardRank(b) - boardRank(a);
}
const boardColumnKey = (item, status = S.workBoardStages === "detailed") => (status ? item.status
  : BOARD_STAGE_OF[item.status] ?? (BOARD_STAGES.includes(item.status) ? item.status : OPEN(item) ? "To do" : "Done"));
function boardColumns(items, needsYouView, sortKey) {
  const status = S.workBoardStages === "detailed";
  const columns = new Map(status ? [] : BOARD_STAGES.map((stage) => [stage, []]));
  for (const item of items) {
    const key = boardColumnKey(item, status);
    if (!columns.has(key)) columns.set(key, []);
    columns.get(key).push(item);
  }
  const order = status ? [...columns.keys()].sort((a, b) => (BOARD_STATUS_ORDER.indexOf(a) + 99) % 99 - (BOARD_STATUS_ORDER.indexOf(b) + 99) % 99) : BOARD_STAGES;
  /* The default order is Attention first; an explicit Sort by is kept as chosen */
  const attention = sortKey === "default";
  return order.filter((key) => !(needsYouView && isDoneStage(key))).map((key) => [key,
    attention ? columns.get(key).sort(byAttention) : columns.get(key)]);
}
const isDoneStage = (stage) => stage === "Done" || BOARD_STAGE_OF[stage] === "Done";
/* Done history: one board-level control (a group in Display) for how far back every Done lane reaches.
   It writes the same state as the list's Show archived. */
const DONE_HISTORY = [["recent", `Last ${DONE_WINDOW_DAYS} days`], ["all", "All unarchived"], ["archived", "Include archived"]];
const doneHistory = () => (S.workShowArchived ? "archived" : S.workBoardAllDone ? "all" : "recent");
function setDoneHistory(value) {
  if (!DONE_HISTORY.some(([v]) => v === value)) return;
  S.workBoardAllDone = value !== "recent";
  S.workShowArchived = value === "archived";
  try { sessionStorage.setItem(WORK_ARCHIVED_STORE, S.workShowArchived ? "1" : "0"); } catch (e) {}
}
/* Display's radio group reads and writes the history through S like any other menu setting */
Object.defineProperty(S, "workDoneHistory", { get: doneHistory, set: setDoneHistory, configurable: true });
function doneHistoryOption(listLayout) {
  if (listLayout) return [];
  return [["workDoneHistory", "Done history", DONE_HISTORY.filter(([v]) => v !== "archived" || S.archiveAfterDays > 0)]];
}
/* Work closed before the Done history window: kept out of Done lanes, and out of the Board's preset counts */
const olderThanDoneWindow = (w) => doneHistory() === "recent" && isClosed(w) && (w.closedDaysAgo ?? 0) > DONE_WINDOW_DAYS;
function doneWindow(stage, tasks) {
  if (!isDoneStage(stage) || doneHistory() !== "recent") return { recent: tasks, older: 0 };
  const recent = tasks.filter((w) => !olderThanDoneWindow(w));
  return { recent, older: tasks.length - recent.length };
}
/* A preset's count is the number of cards it shows: on the Board that leaves out Done history outside the window,
   and the Done lanes that Needs you hides */
function visibleCount(items, view, layout) {
  if (layout !== "board") return items.length;
  return items.filter((w) => !olderThanDoneWindow(w) && !(view === "needs-me" && isDoneStage(boardColumnKey(w)))).length;
}
/* The Done lanes name the history window they show */
const DONE_WINDOW_LABEL = { recent: `Last ${DONE_WINDOW_DAYS} days`, all: "All unarchived", archived: "With archived" };
/* One count convention for a lane wherever it is named: "7 shown · 5 older" while older work is hidden */
function laneCount(stage, tasks) {
  const { recent, older } = doneWindow(stage, tasks);
  return { recent, older, text: older ? `${recent.length} shown · ${older} older` : String(tasks.length) };
}
/* The lane heading is where a column jump lands, so it takes focus (tabindex=-1) */
function boardColumnHead(stage, count) {
  const history = isDoneStage(stage) ? `<span class="bcol-window">${ic("clock", "ic14")}${esc(DONE_WINDOW_LABEL[doneHistory()])}</span>` : "";
  return `<div class="bcol-h"><h3 class="bcol-title" tabindex="-1"><span class="bcol-ic" aria-hidden="true">${workStatus(BOARD_STAGE_STATUS[stage] || stage)}</span><span class="bcol-name">${esc(stage)}</span><span class="cnt">${esc(count.text)}</span></h3>${history}</div>`;
}
function boardColumnHtml(stage, tasks, opts) {
  const count = laneCount(stage, tasks), { recent, older } = count;
  const empty = older ? `None closed in the last ${DONE_WINDOW_DAYS} days` : opts.filtered ? "No matches in this column" : "Nothing here";
  const list = recent.length ? `<ul class="bcol-list" role="list">${recent.map((w) => boardCard(w, { ...opts, selected: w.id === opts.selectedId })).join("")}</ul>`
    : `<div class="bcol-empty"><p>${empty}</p></div>`;
  const history = isDoneStage(stage) ? `, ${DONE_WINDOW_LABEL[doneHistory()]}` : "";
  return `<section class="bcol" id="${esc(laneId(stage))}" aria-label="${esc(`${stage}, ${count.text}${history}`)}">${boardColumnHead(stage, count)}${list}</section>`;
}
const laneId = (stage) => "bcol-" + stage.replace(/\s+/g, "-").toLowerCase();
/* Named column jump: chips when they fit, a picker when they don't (always on phones). Shown whenever lanes overflow. */
function boardLaneNav(columns) {
  const lanes = columns.map(([stage, tasks]) => [stage, laneCount(stage, tasks).text]);
  const chips = lanes.map(([stage, count], i) => `<button type="button" data-act="board-lane" data-v="${i}" aria-controls="${esc(laneId(stage))}"${i === 0 ? ' aria-current="true"' : ""}>${esc(stage)} <span class="cnt">${esc(count)}</span></button>`).join("");
  const options = lanes.map(([stage, count], i) => `<option value="${i}">${esc(stage)} (${esc(count)})</option>`).join("");
  return `<nav class="board-lanes" aria-label="Board columns" data-fit="chips">
    <span class="board-lanes-label" aria-hidden="true">Columns</span>
    <div class="board-lane-chips">${chips}</div>
    <label class="board-lane-pick"><span class="sr-only">Jump to board column</span><select class="field" data-board-lane>${options}</select>${ic("chev-down", "ic16")}</label>
  </nav>`;
}
const boardScrollPad = (board) => parseFloat(getComputedStyle(board).scrollPaddingInlineStart || "0") || 0;
/* The lane whose start is closest to the board's start edge */
function leadLaneIndex(board, cols) {
  const start = board.getBoundingClientRect().left + boardScrollPad(board);
  let lead = 0;
  cols.forEach((col, i) => {
    if (Math.abs(col.getBoundingClientRect().left - start) < Math.abs(cols[lead].getBoundingClientRect().left - start)) lead = i;
  });
  return lead;
}
/* An explicit jump goes to the top of the chosen lane, and that lane stays current while it is in view.
   A committed jump moves focus to the lane's heading, so the next Tab continues inside that lane. */
let jumpTimer = 0;
function jumpToBoardLane(index, root = document, { focus = true, instant = false } = {}) {
  const board = root.querySelector(".board"), col = board?.querySelectorAll(".bcol")[Number(index)];
  if (!col) return;
  const nav = root.querySelector(".board-lanes");
  const left = board.scrollLeft + col.getBoundingClientRect().left - board.getBoundingClientRect().left - boardScrollPad(board);
  const smooth = !instant && !matchMedia("(prefers-reduced-motion: reduce)").matches;
  board.dataset.jump = col.id;
  delete board.dataset.jumpReached;
  clearTimeout(jumpTimer);
  /* A jump the person interrupts before it arrives stops claiming the lane */
  jumpTimer = setTimeout(() => {
    if (board.dataset.jump !== col.id || "jumpReached" in board.dataset) return;
    delete board.dataset.jump;
    syncBoardLanes(board, nav);
  }, 1500);
  revealBoardTop(nav ?? board);
  board.scrollTo({ left, top: 0, behavior: smooth ? "smooth" : "auto" });
  if (focus) col.querySelector(".bcol-title")?.focus({ preventScroll: true });
  syncBoardLanes(board, nav);
}
/* When the page itself scrolls (phones), bring the column jump and lane headers back into view */
function revealBoardTop(el) {
  const content = document.getElementById("content");
  if (!content) return;
  const offset = el.getBoundingClientRect().top - content.getBoundingClientRect().top;
  if (offset < 0) content.scrollTop += offset;
}
/* Chips when every lane name fits on one row; otherwise the picker */
function fitBoardLanes(nav) {
  const chips = nav.querySelector(".board-lane-chips");
  nav.dataset.fit = "chips";
  nav.dataset.fit = chips.scrollWidth > chips.clientWidth + 1 ? "picker" : "chips";
}
/* The jump target is current while it is in view; once it has been reached and scrolled away, the lead lane is */
function currentLaneIndex(board, cols, lead, inView) {
  const target = board.dataset.jump ? cols.findIndex((col) => col.id === board.dataset.jump) : -1;
  if (target < 0) return lead;
  if (inView[target]) { board.dataset.jumpReached = ""; return target; }
  if (!("jumpReached" in board.dataset)) return target;
  delete board.dataset.jump;
  delete board.dataset.jumpReached;
  return lead;
}
function syncBoardLanes(board, nav) {
  const cols = [...board.querySelectorAll(".bcol")];
  if (!nav || !cols.length) return;
  const overflow = board.scrollWidth > board.clientWidth + 1;
  nav.toggleAttribute("data-overflow", overflow);
  if (overflow) fitBoardLanes(nav);
  const view = board.getBoundingClientRect();
  const inView = cols.map((col) => {
    const r = col.getBoundingClientRect();
    return Math.min(r.right, view.right) - Math.max(r.left, view.left) >= r.width * 0.6;
  });
  const current = currentLaneIndex(board, cols, leadLaneIndex(board, cols), inView);
  nav.querySelectorAll(".board-lane-chips button").forEach((chip, i) => {
    chip.toggleAttribute("data-in-view", inView[i]);
    if (i === current) chip.setAttribute("aria-current", "true"); else chip.removeAttribute("aria-current");
  });
  /* Leave an option the person is still browsing alone */
  const select = nav.querySelector("select");
  if (select && !("pending" in select.dataset)) select.value = String(current);
}
/* The picker jumps when a choice is committed, not while options are browsed: a pick from the open list or
   Enter commits; arrow keys and type-ahead on the closed picker only browse. Tabbing away commits without
   taking focus; Escape puts the current lane back. */
function bindLanePicker(select, main) {
  let browsing = false;
  const commit = (focus) => { delete select.dataset.pending; jumpToBoardLane(select.value, main, { focus }); };
  select.addEventListener("keydown", (e) => {
    if (e.key === "Enter" && "pending" in select.dataset) { e.preventDefault(); commit(true); return; }
    if (e.key === "Escape" && "pending" in select.dataset) {
      e.preventDefault(); delete select.dataset.pending; syncBoardLanes(main.querySelector(".board"), main.querySelector(".board-lanes")); return;
    }
    /* A closed picker changes its value during the key's default action; a pick from an open list comes later */
    browsing = e.key !== "Enter" && e.key !== "Tab" && !e.metaKey && !e.ctrlKey;
    setTimeout(() => { browsing = false; });
  });
  select.addEventListener("change", () => {
    if (browsing) { select.dataset.pending = select.value; return; }
    commit(true);
  });
  select.addEventListener("blur", () => { if ("pending" in select.dataset) commit(false); });
}
let boardResize = null;
/* Keep the column jump control in sync with horizontal scrolling and with the board's width */
function mountBoard(main) {
  boardResize?.disconnect();
  boardResize = null;
  const board = main.querySelector(".board"), nav = main.querySelector(".board-lanes");
  if (!board) return;
  const sync = () => syncBoardLanes(board, nav);
  board.addEventListener("scroll", sync, { passive: true });
  const select = nav?.querySelector("select");
  if (select) bindLanePicker(select, main);
  if (typeof ResizeObserver === "function") {
    boardResize = new ResizeObserver(sync);
    boardResize.observe(board);
  }
  sync();
}
/* Which board control to give focus back to: the same control on the same card or lane, by selector */
const CARD_CONTROLS = [
  [".work-item-link", " > .work-item-link"],
  [".board-need, .board-need-menu a", " .board-need"],
  [".board-project a", " .board-project a"],
  [".board-owner-label a", " .board-owner-label a"],
  [".board-subhits a", " .board-subhits a"],
];
function boardControlSelector(el) {
  const card = el?.closest?.(".bcard"), lane = el?.closest?.(".bcol");
  if (!el || !document.getElementById("work-results")?.contains(el)) return "";
  if (card) {
    const hit = CARD_CONTROLS.find(([match]) => el.matches(match));
    return hit ? `.bcard[data-work-id="${CSS.escape(card.dataset.workId)}"]${hit[1]}` : "";
  }
  if (lane && el.matches(".bcol-title")) return `#${CSS.escape(lane.id)} .bcol-title`;
  return el.matches("[data-board-lane]") ? "[data-board-lane]" : "";
}
/* Re-rendering results keeps the owner's place: the lane at the start edge, both scroll axes and focus */
function boardPlace(results, control = document.activeElement) {
  const board = results.querySelector(".board");
  if (!board) return null;
  const cols = [...board.querySelectorAll(".bcol")], lead = cols.length ? cols[leadLaneIndex(board, cols)] : null;
  return {
    mode: board.dataset.mode, lead: lead?.id, leadOffset: lead ? lead.getBoundingClientRect().left - board.getBoundingClientRect().left : 0,
    jump: "jumpReached" in board.dataset ? board.dataset.jump : undefined,
    top: board.scrollTop, page: document.getElementById("content")?.scrollTop ?? 0, focus: boardControlSelector(control),
  };
}
function restoreBoardPlace(results, place, focus = place?.focus) {
  const board = results.querySelector(".board");
  if (!place) return;
  const content = document.getElementById("content");
  if (content) content.scrollTop = place.page;
  if (board) {
    /* Switching Stages and Status is a new set of lanes, so it starts from the first lane */
    const sameMode = place.mode === board.dataset.mode;
    const lead = sameMode && place.lead && board.querySelector("#" + CSS.escape(place.lead));
    if (lead) board.scrollLeft += lead.getBoundingClientRect().left - board.getBoundingClientRect().left - place.leadOffset;
    if (sameMode && place.jump && board.querySelector("#" + CSS.escape(place.jump))) Object.assign(board.dataset, { jump: place.jump, jumpReached: "" });
    board.scrollTop = place.top;
    syncBoardLanes(board, results.querySelector(".board-lanes"));
  }
  if (focus) results.querySelector(focus)?.focus({ preventScroll: true });
}
/* Search or filters that leave every lane in view empty bring the first lane with matches into view (phones show
   one lane at a time). Focus stays where it is, in search. */
function laneInView(board, col) {
  const view = board.getBoundingClientRect(), r = col.getBoundingClientRect();
  return Math.min(r.right, view.right) - Math.max(r.left, view.left) >= r.width * 0.6;
}
function revealMatchingLane(results) {
  const board = results.querySelector(".board");
  if (!board || !("filtered" in board.dataset)) return null;
  const cols = [...board.querySelectorAll(".bcol")];
  if (cols.some((col) => laneInView(board, col) && col.querySelector(".bcard"))) return null;
  const index = cols.findIndex((col) => col.querySelector(".bcard"));
  if (index < 0) return null;
  jumpToBoardLane(index, results, { focus: false, instant: true });
  return cols[index];
}
/* The lane named in an announcement: the one just revealed, or, when lanes overflow the board, the one in view */
function announcedLane(results, revealed) {
  const board = results.querySelector(".board");
  if (revealed || !board || board.scrollWidth <= board.clientWidth + 1) return revealed;
  const cols = [...board.querySelectorAll(".bcol")];
  return cols[leadLaneIndex(board, cols)];
}
/* Says how many cards or rows match and, on a board that shows only some lanes, which lane is in view */
function announceWorkResults(results, revealed) {
  const status = document.getElementById("work-results-status");
  if (!status) return;
  const filtered = Boolean(results.querySelector("[data-filtered]"));
  const n = results.querySelectorAll(".bcard, .work-list > .work-item").length;
  const count = n ? `${n} ${n === 1 ? "match" : "matches"}` : "No matches";
  const name = announcedLane(results, revealed)?.querySelector(".bcol-name")?.textContent;
  status.textContent = filtered ? `${count}.${name ? ` Showing ${name}.` : ""}` : "";
}
function settleWorkResults(results) {
  if (results) announceWorkResults(results, revealMatchingLane(results));
}
function replaceWorkResults(html, focus) {
  const results = document.getElementById("work-results");
  if (!results) return;
  const place = boardPlace(results);
  results.innerHTML = html;
  mountBoard(results);
  restoreBoardPlace(results, place, focus ?? place?.focus);
  settleWorkResults(results);
}
/* Closing a task returns focus to the exact link that opened it: the card's title, or a subtask link on its parent
   card; a subtask opened from inside the panel falls back to its parent card */
function workPanelOpener(root, id) {
  const results = root.querySelector("#work-results");
  if (!results || !id) return null;
  const shown = (el) => el?.getClientRects().length;
  const exact = [...results.querySelectorAll(`[data-item="${CSS.escape(id)}"]`)].find(shown);
  if (exact) return exact;
  const { parent } = findWorkItem(id);
  const fallback = parent && results.querySelector(`.work-item-link[data-item="${CSS.escape(parent.id)}"]`);
  return shown(fallback) ? fallback : null;
}
function refreshScopedWorkResults(view) {
  replaceWorkResults(scopedWorkResults(view));
  refreshScopedWorkCounts(view);
}
/* When every match is hidden history, show the empty state with the way to reveal it, not a board of empty lanes */
function boardHiddenEmpty(columns, { filtered, archived = 0, emptyText }) {
  const counts = columns.map(([stage, tasks]) => laneCount(stage, tasks));
  if (counts.some((count) => count.recent.length)) return "";
  const older = counts.reduce((sum, count) => sum + count.older, 0);
  return workEmpty(emptyText || "No work matches these filters.", filtered, { older, archived });
}
/* Filters and search are summed up once above the board, with the one way back to every card */
function boardFilterBar(columns, summary) {
  const shown = columns.reduce((sum, [stage, tasks]) => sum + doneWindow(stage, tasks).recent.length, 0);
  return `<div class="board-filterbar"><p><strong>${shown} ${shown === 1 ? "card" : "cards"}</strong>${summary.map((part) => `${SEP}<span>${esc(part)}</span>`).join("")}</p>${clearFiltersButton()}</div>`;
}
function board(items, { hideOwner = false, hideProject = false, needsYouView = false, sortKey = "default", filtered = false, summary = [], query = "", archived = 0, emptyText = "" } = {}) {
  if (!items.length) return `<p class="empty-row">No work here yet.</p>`;
  const columns = boardColumns(items, needsYouView, sortKey), selectedId = qs().get("item");
  const hidden = boardHiddenEmpty(columns, { filtered, archived, emptyText });
  if (hidden) return hidden;
  return `${filtered ? boardFilterBar(columns, summary) : ""}${boardLaneNav(columns)}<div class="board" role="region" aria-label="Work board" tabindex="0" data-mode="${S.workBoardStages === "detailed" ? "status" : "stages"}"${filtered ? " data-filtered" : ""}>
    <div class="board-columns" style="--board-column-count:${columns.length}">${columns.map(([stage, tasks]) => boardColumnHtml(stage, tasks, { hideOwner, hideProject, selectedId, filtered, sortKey, query })).join("")}</div>
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

/* A work link opened from a board's Inbox request stays in that board's scope when the item belongs there */
function boardItemHref(origin, item) {
  if (!origin) return workHref({ item });
  const [path, query = ""] = origin.replace(/^#\/?/, "").split("?");
  let parts;
  try { parts = path.split("/").filter(Boolean).map(decodeURIComponent); } catch (e) { return workHref({ item }); }
  const params = new URLSearchParams(query);
  params.set("item", item);
  if (parts[0] === "work" && parts.length === 1) return `#/work?${params}`;
  const type = scopedWorkType(parts);
  if (!type || !scopedWorkView(parts, type).items.some((w) => w.id === item)) return workHref({ item });
  return `#/${path}?${params}`;
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
  return `${search}<p class="sr-only" id="work-results-status" role="status"></p><div id="work-results"${scope ? ` data-scope="${esc(scope)}"` : ""}>${results}</div>${item ? workDetail(item, parent) : ""}`;
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
  const priority = esc(setPriority(w));
  const expandLabel = "Expand task to full page";
  return `<dialog class="side-panel work-panel" id="work-panel" aria-labelledby="work-detail-title" data-item="${esc(w.id)}">
    <section class="triage-reading work-reading"${team ? ` data-team="${esc(team)}"` : ""}>
      <div class="side-panel-header work-detail-bar">
        <p class="work-detail-id">${workPriority(setPriority(w))}${esc(w.id)}${workStatus(w.status, showsNeedsYou(w))}<span class="work-kind">${{ task: "Task", bug: "Bug", request: "Request" }[w.kind]}</span></p>
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
/* On the board the default order is Attention first (expiring owner asks, other owner asks, then priority);
   Priority is the explicit priority-only order */
const BOARD_SORTS = [["default", "Attention first"], ["priority", "Priority"], ["eta", "ETA"], ["title", "Title"]];
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

function workFilteredItems(archivedOnly = false) {
  const query = canonicalWorkRef(S.workQuery).toLowerCase();
  const { workStatus: status, workProject: project, workTeam: team } = S;
  return (archivedOnly ? W.filter(isArchivedWork) : visibleWorkItems(W)).filter((w) => {
    if (status !== "all" && w.status !== status) return false;
    if (project !== "all" && w.proj !== project) return false;
    if (team !== "all" && (workTeam(w) ?? "none") !== team) return false;
    return !query || workMatches(w, query);
  });
}

function sortWorkItems(items, sortKey = S.workSort, order = S.workOrder) {
  const descending = order === "desc";
  /* Ascending priority is highest first, so the default direction puts the most important work on top */
  const read = { title: (w) => w.title, priority: (w) => -workPriorityRank(w.pri) || undefined, eta: (w) => w.etaHours }[sortKey];
  if (!read) return descending ? [...items].reverse() : items;
  return [...items].sort((left, right) => {
    const a = read(left), b = read(right);
    if (a == null || b == null) return Number(a == null) - Number(b == null);
    const cmp = typeof a === "string" ? a.localeCompare(b, undefined, { sensitivity: "base", numeric: true }) : a - b;
    return descending ? -cmp : cmp;
  });
}

function workResults(filtered, { view, group, subgroup }) {
  const items = sortWorkItems(workPresetItems(filtered, view), S.workSort, workSortOrder(S.workLayout, S.workSort, S.workOrder));
  const isFiltered = Boolean(S.workQuery.trim()) || workHasFilters();
  const text = isFiltered ? "No work matches these filters."
    : view === "needs-me" ? "Nothing needs you in Work."
    : view === "mine" ? "Nothing is assigned to you." : "No work items yet.";
  const archived = S.workShowArchived ? 0 : workPresetItems(workFilteredItems(true), view).length;
  if (!items.length) return workEmpty(text, isFiltered, { archived });
  const query = canonicalWorkRef(S.workQuery).toLowerCase();
  const summary = filterParts(S.workQuery.trim(), workFilterValues());
  return S.workLayout === "board" ? board(items, { needsYouView: view === "needs-me", sortKey: S.workSort, filtered: isFiltered, summary, query, archived, emptyText: text }) : workList(items, {
    groupBy: WORK_GROUP_FIELDS[group], subgroupBy: WORK_GROUP_FIELDS[subgroup], filtered: isFiltered,
  });
}

function refreshWorkResults(focus) {
  const filtered = workFilteredItems();
  replaceWorkResults(workResults(filtered, workSelection()), focus);
  for (const counter of document.querySelectorAll("[data-work-count]")) {
    counter.textContent = visibleCount(workPresetItems(filtered, counter.dataset.workCount), counter.dataset.workCount, S.workLayout);
  }
}

function workViewControls(group, subgroup) {
  const filter = showMenu("work-filter", [
    ["workStatus", "Status", [["all", "All statuses"], ...ALL_STATUSES.map((status) => [status, status])]],
    ["workProject", "Project", [["all", "All projects"], ...Object.entries(P).map(([id, project]) => [id, project.name])]],
    ["workTeam", "Team", [["all", "All teams"], ...Object.entries(TEAMS).map(([id, team]) => [id, team.name]), ["none", "No team"]]],
  ], filterTrigger(workFilterValues()));
  const listLayout = S.workLayout === "list";
  const display = showMenu("work-display", [
    ...(listLayout ? [["workGroup", "Group by", [["none", "No grouping"], ...WORK_GROUPS]]] : []),
    ...(listLayout && group !== "none"
      ? [["workSubgroup", "Subgroup by", [["none", "No subgroup"], ...WORK_GROUPS.filter(([value]) => value !== group)]]]
      : []),
    ...(listLayout ? [] : [BOARD_COLUMN_OPTION]),
    ["workSort", "Sort by", listLayout ? WORK_SORTS : BOARD_SORTS],
    ...workDirectionOption("workOrder", S.workLayout, S.workSort),
    ...doneHistoryOption(listLayout),
  ], {
    ...workDisplayTrigger(listLayout),
    active: isWorkDisplayActive(listLayout, group, subgroup, S.workSort, workSortOrder(S.workLayout, S.workSort, S.workOrder)),
    footer: workDisplayFooter(listLayout, true),
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
    ${queueHeadline("Work", `Across all work: ${W.filter(OPEN).length} open · ${W.filter(needsMe).length} need you`)}
    <div class="work-toolbar queue-toolbar">
      <div class="primary-tab-row" data-page-menu>
        <div class="queue-tabs" role="group" aria-label="Work views">
          ${WORK_PRESETS.map(([value, label]) => `<button class="queue-tab" type="button" data-act="work-preset"
            data-v="${value}" aria-pressed="${selection.view === value}" data-menu-label="${esc(label)}">${label}<span data-work-count="${value}">${visibleCount(workPresetItems(filtered, value), value, S.workLayout)}</span></button>`).join("")}
        </div>
      </div>
      ${workLayoutToggle("workLayout")}
      ${workViewControls(selection.group, selection.subgroup)}
      ${workArchivedChip(S.workLayout)}
      <div class="work-tb-end">${workCalendarLink()}<button class="btn primary" id="work-new-task" type="button" data-act="work-new-task" aria-label="New task">${ic("plus", "ic16")}New<span class="work-new-word">task</span></button></div>
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
  boardItemHref,
  settleWorkResults,
  workPanelOpener,
  boardPlace,
  jumpToBoardLane,
  restoreBoardPlace,
  mountBoard,
  refreshScopedWorkResults,
  setDoneHistory,
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
