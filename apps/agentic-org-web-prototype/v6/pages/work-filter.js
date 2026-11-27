/* Work's multi-select Filter: one menu per Work surface, in two steps. The first lists the facets (Status, Project,
   Team, Assignee, Priority) with how many values each has selected; choosing one shows that facet's values as
   checkboxes, with counts, and Back. Several values in a facet match any of them; facets combine.

   A facet is { label, state, value(w), name(v), options() }: `state` is the S key holding the selected values (an array;
   the legacy "all" reads as none), `value` the item's value, `name` the value's label and `options` every value in
   menu order. Values with no matching item are left out unless selected. The menu opens in the top layer, so the
   board's sticky lane headers and cards never paint over it. */
import { openPopoverMenu } from "../shell/popover-menu.js";
import { S } from "../state.js";
import { esc, ic } from "../ui.js";

const menus = new Map();

function selectedValues(state) {
  const v = S[state];
  if (Array.isArray(v)) return v;
  return v && v !== "all" ? [String(v)] : [];
}
/* The one selected value, or "" when none or several are selected (defaults for New task use it) */
function singleValue(state) {
  const sel = selectedValues(state);
  return sel.length === 1 ? sel[0] : "";
}
const facetMatches = (facet, w) => {
  const sel = selectedValues(facet.state);
  return !sel.length || sel.includes(String(facet.value(w)));
};
const matchesFacets = (facets, w, except = null) => facets.every((facet) => facet === except || facetMatches(facet, w));
const hasFacetFilters = (facets) => facets.some((facet) => selectedValues(facet.state).length);
const selectedCount = (facets) => facets.reduce((n, facet) => n + selectedValues(facet.state).length, 0);
/* [["Status", "New, Blocked"], …] for the trigger's name and the board's summary */
const facetValues = (facets) => facets.map((facet) => [facet.label, selectedValues(facet.state).map((v) => facet.name(v) ?? v).join(", ")]);
function clearFacets(facets) {
  for (const facet of facets) S[facet.state] = [];
}
function toggleFacetValue(state, value) {
  const sel = selectedValues(state);
  S[state] = sel.includes(value) ? sel.filter((v) => v !== value) : [...sel, value];
}

/* "Filter" with the single selected value as secondary text, or a count when several values are selected */
function filterTrigger(id, facets) {
  const active = facetValues(facets).filter(([, v]) => v), count = selectedCount(facets);
  const one = count === 1 ? active[0] : null;
  const label = active.length ? `Filter work, ${active.map(([name, v]) => `${name} ${v}`).join("; ")}` : "Filter work";
  const sub = one ? `<span class="menu-textbtn-sub" aria-hidden="true">${esc(one[1])}</span>${ic("chev-down", "ic14 menu-textbtn-chev")}` : "";
  const badge = count > 1 ? `<span class="menu-textbtn-count" aria-hidden="true">${count}</span>` : "";
  return `<button class="btn ghost menu-textbtn" aria-label="${esc(label)}${count > 1 ? `, ${count} selected` : ""}"${count ? " data-active" : ""} type="button" data-act="menu" data-id="${id}" aria-haspopup="menu" aria-expanded="false" aria-controls="menu-${id}">${ic("filter", "ic16")}<span class="menu-textbtn-label">Filter</span>${sub}${badge}</button>`;
}

function facetCounts(facet, facets, items) {
  const counts = new Map();
  for (const w of items) {
    if (!matchesFacets(facets, w, facet)) continue;
    const v = String(facet.value(w));
    counts.set(v, (counts.get(v) ?? 0) + 1);
  }
  return counts;
}
function facetListHtml({ facets }) {
  const rows = facets.map((facet, i) => {
    const n = selectedValues(facet.state).length;
    const label = `${facet.label}${n ? `, ${n} selected` : ""}`;
    return `<button role="menuitem" type="button" class="filter-facet" data-act="work-filter-facet" data-v="${i}" aria-label="${esc(label)}"><span class="filter-facet-name">${esc(facet.label)}</span>${n ? `<span class="filter-facet-n" aria-hidden="true">${n}</span>` : ""}${ic("chev-right", "ic16 filter-facet-chev")}</button>`;
  }).join("");
  const clear = hasFacetFilters(facets) ? `<hr><button role="menuitem" type="button" data-act="work-clear-filters">${ic("x", "ic20")}Clear filters</button>` : "";
  return `<p class="menu-h" aria-hidden="true">Filter by</p>${rows}${clear}`;
}
function facetValuesHtml({ facets, items, archived }, index) {
  const facet = facets[index], sel = selectedValues(facet.state), counts = facetCounts(facet, facets, items);
  /* A value whose only matches are archived stays, at 0: choosing it offers to show those matches */
  const hidden = facetCounts(facet, facets, archived);
  const values = facet.options().map(String).filter((v) => counts.get(v) || hidden.get(v) || sel.includes(v));
  const rows = values.map((v) => `<button role="menuitemcheckbox" type="button" aria-checked="${sel.includes(v)}" data-act="work-filter-toggle" data-k="${esc(facet.state)}" data-v="${esc(v)}" data-facet="${index}"><span class="filter-check" aria-hidden="true">${ic("check", "ic14")}</span><span class="filter-value-name">${esc(facet.name(v) ?? v)}</span><span class="muted tnum">${counts.get(v) ?? 0}</span></button>`).join("");
  const empty = values.length ? "" : `<p class="filter-empty">No ${esc(facet.label.toLowerCase())} values here</p>`;
  return `<button role="menuitem" type="button" class="filter-back" data-act="work-filter-facet" data-v="" aria-label="Back to all filters">${ic("chev-left", "ic16")}<span>${esc(facet.label)}</span></button><hr>
    <div role="group" aria-label="${esc(facet.label)}">${rows}</div>${empty}`;
}
/* One flat list, like Display: a heading per facet with its values as check items; several can be ticked (owner) */
function facetSectionHtml({ facets, items, archived }, index) {
  const facet = facets[index], sel = selectedValues(facet.state), counts = facetCounts(facet, facets, items);
  const hidden = facetCounts(facet, facets, archived);
  const values = facet.options().map(String).filter((v) => counts.get(v) || hidden.get(v) || sel.includes(v));
  if (!values.length) return "";
  const rows = values.map((v) => `<button role="menuitemcheckbox" type="button" aria-checked="${sel.includes(v)}" data-act="work-filter-toggle" data-k="${esc(facet.state)}" data-v="${esc(v)}" data-facet="${index}"><span class="filter-check" aria-hidden="true">${ic("check", "ic14")}</span><span class="filter-value-name">${esc(facet.name(v) ?? v)}</span><span class="muted tnum">${counts.get(v) ?? 0}</span></button>`).join("");
  return `<div role="group" aria-label="${esc(facet.label)}"><p class="menu-h" aria-hidden="true">${esc(facet.label)}</p>${rows}</div>`;
}
function flatPanelHtml(config) {
  const sections = config.facets.map((_f, i) => facetSectionHtml(config, i)).filter(Boolean).join("<hr>");
  const clear = hasFacetFilters(config.facets) ? `<hr><button role="menuitem" type="button" data-act="work-clear-filters">${ic("x", "ic20")}Clear filters</button>` : "";
  return sections + clear;
}
function panelHtml(id) {
  return flatPanelHtml(menus.get(id));
}

/* The Filter control. `items` are the surface's items before any facet (archived visibility and search applied), so
   each value's count says how many items it would match with the other facets as they are; `archived` are the
   matching items archived out of sight. */
function filterMenu(id, facets, items, archived = []) {
  menus.set(id, { facets, items, archived });
  return `<div class="menu-wrap">${filterTrigger(id, facets)}<div class="menu filter-menu top-layer-menu" id="menu-${id}" role="menu" aria-label="Filter work" popover="manual" data-panel="" hidden>${panelHtml(id, "")}</div></div>`;
}

function focusPanel(menu, value) {
  const items = [...menu.querySelectorAll('[role^="menuitem"]')];
  const target = (value != null && items.find((el) => el.dataset.v === value && el.dataset.act === "work-filter-toggle")) || items.find((el) => el.matches("[data-act='work-filter-toggle']")) || items[0];
  target?.focus();
}
const menuTrigger = (menu) => document.querySelector(`[aria-controls="${menu.id}"]`);
/* Fill the menu with a panel; an open menu is placed again, since its height changes */
function fillPanel(menu, panel) {
  menu.dataset.panel = panel;
  menu.innerHTML = panelHtml(menu.id.replace(/^menu-/, ""), panel);
  const trigger = menuTrigger(menu);
  if (!menu.hidden && trigger) openPopoverMenu(menu, trigger);
}
/* Switch the open menu between the facet list and one facet's values */
function showFilterPanel(menu, panel, focusFacet = null) {
  if (!menus.has(menu.id.replace(/^menu-/, ""))) return;
  fillPanel(menu, panel);
  if (panel === "" && focusFacet != null) menu.querySelector(`.filter-facet[data-v="${focusFacet}"]`)?.focus();
  else focusPanel(menu);
}

/* After a value is toggled the page re-renders; reopen the menu on the same facet with focus on the same value */
function reopenFilterMenu(id, panel, value) {
  const menu = document.getElementById("menu-" + id), trigger = menu && menuTrigger(menu);
  if (!trigger) return;
  menu.hidden = false;
  trigger.setAttribute("aria-expanded", "true");
  fillPanel(menu, panel);
  focusPanel(menu, value);
}

/* A trigger opening its menu always starts at the facet list; ArrowRight opens a facet, ArrowLeft goes back */
document.addEventListener("click", (e) => {
  const trigger = e.target.closest?.('[data-act="menu"][aria-controls]');
  const menu = trigger && document.getElementById(trigger.getAttribute("aria-controls"));
  if (menu?.classList.contains("filter-menu") && menu.hidden && menu.dataset.panel !== "") showFilterPanel(menu, "");
}, true);
document.addEventListener("keydown", (e) => {
  const menu = e.target.closest?.(".filter-menu");
  if (!menu || e.altKey || e.ctrlKey || e.metaKey) return;
  if (e.key === "ArrowRight" && e.target.matches(".filter-facet")) { e.preventDefault(); e.target.click(); }
  else if (e.key === "ArrowLeft" && menu.dataset.panel !== "") { e.preventDefault(); showFilterPanel(menu, "", menu.dataset.panel); }
});

export {
  clearFacets,
  facetValues,
  filterMenu,
  hasFacetFilters,
  matchesFacets,
  reopenFilterMenu,
  selectedValues,
  showFilterPanel,
  singleValue,
  toggleFacetValue,
};
