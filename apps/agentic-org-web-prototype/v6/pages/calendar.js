/* Calendars: one component, three scopes. "me" = meetings you attend (#/calendar), "project" and
   "team" = that project's or team's meetings (Calendar tabs). Views: Day, Week, Month, Agenda.
   A date picker jumps to any day (past or future); clicking a day opens it in Day view.
   State: S.calDate (focused day), S.calView, S.calEvent (selected meeting), S.calPicker. */
import { CAL_TODAY, EVENTS, M, P, TEAMS } from "../data.js";
import { render } from "../shell/router.js";
import { S, qs } from "../state.js";
import { av, esc, ic, kindWord, plural } from "../ui.js";

const DAY = 86_400_000, H0 = 8, H1 = 19, HOUR_PX = 48, NOW = "11:20";
const toTime = (iso) => Date.UTC(+iso.slice(0, 4), +iso.slice(5, 7) - 1, +iso.slice(8, 10));
const TODAY = toTime(CAL_TODAY);
const toIso = (t) => new Date(t).toISOString().slice(0, 10);
const fmt = (opts) => new Intl.DateTimeFormat("en-US", { timeZone: "UTC", ...opts });
const fWeekday = fmt({ weekday: "short" }), fMon = fmt({ month: "short" }), fDay = fmt({ day: "numeric" });
const fLong = fmt({ weekday: "short", month: "short", day: "numeric" }), fMonthYear = fmt({ month: "long", year: "numeric" }), fFull = fmt({ weekday: "long", month: "long", day: "numeric", year: "numeric" });
const minutes = (hhmm) => +hhmm.slice(0, 2) * 60 + +hhmm.slice(3, 5);
const MONTHS = { Jan: 1, Feb: 2, Mar: 3, Apr: 4, May: 5, Jun: 6, Jul: 7, Aug: 8, Sep: 9, Oct: 10, Nov: 11, Dec: 12 };
const PROJECT_TONE = { "agency-v1": "accent", "look-feel": "violet", "office-federation": "warn" };
const VIEWS = [["day", "Day"], ["week", "Week"], ["month", "Month"], ["agenda", "Agenda"]];

/* ---------------------------------------------------------------- dates */
const curDay = () => toTime(S.calDate || CAL_TODAY);
const weekStartOf = (t) => t - ((new Date(t).getUTCDay() + 6) % 7) * DAY;
const monthStartOf = (t) => { const d = new Date(t); return Date.UTC(d.getUTCFullYear(), d.getUTCMonth(), 1); };
const addMonths = (t, n) => { const d = new Date(t); return Date.UTC(d.getUTCFullYear(), d.getUTCMonth() + n, 1); };
function relDay(t) {
  const d = Math.round((t - TODAY) / DAY);
  if (d === 0) return "Today";
  if (d === 1) return "Tomorrow";
  if (d === -1) return "Yesterday";
  return Math.abs(d) <= 6 ? fWeekday.format(t) : fLong.format(t);
}
function eventDate(e) {
  const t = toTime(e.date);
  return { t, mon: fMon.format(t), day: String(new Date(t).getUTCDate()).padStart(2, "0"), when: relDay(t) };
}
/* Days shown by the current view (Agenda: two weeks from the focused week). */
function viewRange(view, t) {
  if (view === "day") return [t];
  if (view === "week") return Array.from({ length: 7 }, (_, i) => weekStartOf(t) + i * DAY);
  if (view === "agenda") return Array.from({ length: 14 }, (_, i) => weekStartOf(t) + i * DAY);
  const first = weekStartOf(monthStartOf(t));
  return Array.from({ length: 42 }, (_, i) => first + i * DAY);
}
function rangeLabel(view, t) {
  if (view === "day") return fFull.format(t);
  if (view === "month") return fMonthYear.format(t);
  const days = viewRange(view, t), a = days[0], b = days[days.length - 1];
  return `${fMon.format(a)} ${fDay.format(a)} – ${fMon.format(b) === fMon.format(a) ? "" : fMon.format(b) + " "}${fDay.format(b)}, ${new Date(b).getUTCFullYear()}`;
}
function stepDate(view, t, n) {
  if (view === "month") return addMonths(t, n);
  return t + n * (view === "day" ? 1 : view === "agenda" ? 14 : 7) * DAY;
}

/* ---------------------------------------------------------------- events */
const eventTone = (e) => (e.proj ? PROJECT_TONE[e.proj] || "accent" : e.team ? "ok" : "gray");
function eventContext(e) {
  if (e.proj && P[e.proj]) return { label: P[e.proj].name, href: `#/projects/${e.proj}` };
  if (e.team && TEAMS[e.team]) return { label: TEAMS[e.team].name, href: `#/teams/${e.team}` };
  return { label: "Personal", href: "" };
}
function scopeEvents(scope, id) {
  if (scope === "project") return EVENTS.filter((e) => e.proj === id);
  if (scope === "team") return EVENTS.filter((e) => e.team === id || (e.proj && P[e.proj]?.teams.includes(id)));
  return EVENTS.filter((e) => e.attendees.includes("you"));
}
function scopeDeadlines(scope, id) {
  const ids = scope === "project" ? [id] : Object.keys(P).filter((p) => !P[p].archived && (scope !== "team" || P[p].teams.includes(id)));
  return ids.filter((p) => P[p].dueMonth).map((p) => ({ proj: p, date: `2026-${String(MONTHS[P[p].dueMonth]).padStart(2, "0")}-${P[p].dueDay}`, label: `${P[p].release || P[p].name} due` }));
}
const sortEvents = (list) => [...list].sort((a, b) => (a.date + a.start).localeCompare(b.date + b.start));
function selectedEvent(list) {
  return list.find((e) => e.id === S.calEvent) || list.find((e) => e.date >= CAL_TODAY) || list[0] || null;
}
/* A link like #/calendar?event=ev1 focuses that meeting's day once. */
function applyLinkedEvent() {
  const id = qs().get("event"), e = id && EVENTS.find((x) => x.id === id);
  if (!e || S.calLinked === id) return;
  S.calLinked = id; S.calEvent = id; S.calDate = e.date;
}

/* ---------------------------------------------------------------- actions */
const CAL_ACTIONS = {
  step: (v) => { S.calDate = toIso(stepDate(S.calView || "week", curDay(), Number(v))); },
  today: () => { S.calDate = CAL_TODAY; S.calPickerMonth = null; },
  view: (v) => { S.calView = v; },
  day: (v) => { S.calDate = v; S.calView = "day"; S.calPicker = false; },
  go: (v) => { S.calDate = v; S.calPicker = false; },
  picker: () => { S.calPicker = !S.calPicker; S.calPickerMonth = toIso(monthStartOf(curDay())); },
  "picker-month": (v) => { S.calPickerMonth = v; },
  select: (v) => { S.calEvent = v; },
};
function calendarAction(kind, v) {
  CAL_ACTIONS[kind]?.(v);
  render(false);
  const again = document.querySelector(`.cal [data-act="cal"][data-id="${CSS.escape(kind)}"]${v != null ? `[data-v="${CSS.escape(String(v))}"]` : ""}`);
  (again || document.querySelector(".cal-range-btn"))?.focus({ preventScroll: true });
}
const calBtn = (kind, v, cls, inner, attrs = "") => `<button type="button" class="${cls}" data-act="cal" data-id="${kind}"${v != null ? ` data-v="${esc(String(v))}"` : ""}${attrs}>${inner}</button>`;

/* ---------------------------------------------------------------- date picker */
function pickerHtml(eventDays) {
  const month = toTime(S.calPickerMonth || toIso(monthStartOf(curDay()))), first = weekStartOf(month), sel = curDay();
  const cells = Array.from({ length: 42 }, (_, i) => first + i * DAY).map((t) => {
    const cls = ["cal-pk-day", t === TODAY && "is-today", t === sel && "is-selected", monthStartOf(t) !== month && "is-out", eventDays.has(toIso(t)) && "has-events"].filter(Boolean).join(" ");
    return calBtn("go", toIso(t), cls, fDay.format(t), ` aria-label="${fFull.format(t)}"${t === sel ? ' aria-current="date"' : ""}`);
  }).join("");
  const heads = ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"].map((d) => `<span>${d}</span>`).join("");
  return `<div class="cal-picker" role="dialog" aria-label="Pick a date">
      <div class="cal-pk-head">${calBtn("picker-month", toIso(addMonths(month, -1)), "btn ghost icon sm", ic("chev-left", "ic16"), ' aria-label="Previous month"')}<b>${fMonthYear.format(month)}</b>${calBtn("picker-month", toIso(addMonths(month, 1)), "btn ghost icon sm", ic("chev-right", "ic16"), ' aria-label="Next month"')}</div>
      <div class="cal-pk-grid">${heads}${cells}</div>
      <div class="cal-pk-foot">${calBtn("go", CAL_TODAY, "link", "Today")}${calBtn("picker", null, "link", "Close")}</div>
    </div>`;
}

/* ---------------------------------------------------------------- toolbar */
function toolbarHtml(view, inRange, narrow, eventDays) {
  const t = curDay(), views = narrow ? VIEWS.filter(([v]) => v === "day" || v === "agenda") : VIEWS;
  const seg = views.map(([v, label]) => calBtn("view", v, "queue-tab", label, ` aria-pressed="${view === v}"`)).join("");
  return `<div class="cal-toolbar">
      <div class="cal-nav" role="group" aria-label="Date">
        ${calBtn("today", null, "btn outline sm", "Today", t === TODAY && view !== "month" ? " disabled" : "")}
        ${calBtn("step", -1, "btn ghost icon sm", ic("chev-left", "ic16"), ` aria-label="Previous ${view === "agenda" ? "two weeks" : view}"`)}
        ${calBtn("step", 1, "btn ghost icon sm", ic("chev-right", "ic16"), ` aria-label="Next ${view === "agenda" ? "two weeks" : view}"`)}
        <div class="cal-range-wrap">${calBtn("picker", null, "cal-range-btn", `<span class="cal-range">${rangeLabel(view, t)}</span>${ic("chev-down", "ic16")}`, ` aria-expanded="${!!S.calPicker}" aria-haspopup="dialog" title="Pick a date"`)}${S.calPicker ? pickerHtml(eventDays) : ""}</div>
        <span class="cal-count">${plural(inRange, "meeting", "meetings")}</span>
      </div>
      <div class="work-layout-toggle" role="group" aria-label="Calendar view">${seg}</div>
    </div>`;
}

/* ---------------------------------------------------------------- day / week grid */
function eventBlockHtml(e, sel) {
  const top = ((minutes(e.start) - H0 * 60) / 60) * HOUR_PX, height = Math.max(22, ((minutes(e.end) - minutes(e.start)) / 60) * HOUR_PX - 2);
  const ctx = eventContext(e);
  return `<button type="button" class="cal-ev" data-tone="${eventTone(e)}" data-act="cal" data-id="select" data-v="${esc(e.id)}" aria-pressed="${e === sel}" style="top:${top}px;height:${height}px" aria-label="${esc(`${e.title}, ${e.start} to ${e.end}, ${ctx.label}`)}">
      <span class="cal-ev-t">${esc(e.title)}</span><span class="cal-ev-m">${esc(e.start)} – ${esc(e.end)}${height > 40 ? ` · ${esc(ctx.label)}` : ""}</span></button>`;
}
function dayColumnHtml(t, events, sel) {
  const now = t === TODAY ? `<span class="cal-now" style="top:${((minutes(NOW) - H0 * 60) / 60) * HOUR_PX}px" aria-hidden="true"></span>` : "";
  return `<div class="cal-col${t === TODAY ? " is-today" : ""}">${now}${events.map((e) => eventBlockHtml(e, sel)).join("")}</div>`;
}
function dayHeadHtml(t, single) {
  const cls = `cal-dh${t === TODAY ? " is-today" : ""}${t < TODAY ? " is-past" : ""}${t === curDay() && !single ? " is-focus" : ""}`;
  return calBtn("day", toIso(t), cls, `<span>${single ? fFull.format(t) : fWeekday.format(t)}</span>${single ? "" : `<b>${fDay.format(t)}</b>`}`, ` aria-label="Open ${fFull.format(t)}"`);
}
function timeGridHtml(days, events, deadlines, sel) {
  const single = days.length === 1;
  const due = days.map((t) => `<div class="cal-due-cell">${deadlines.filter((d) => d.date === toIso(t)).map((d) => `<a class="cal-due" href="#/projects/${esc(d.proj)}">${ic("flag", "ic14")}${esc(d.label)}</a>`).join("")}</div>`).join("");
  const hours = Array.from({ length: H1 - H0 }, (_, i) => `<span style="top:${i * HOUR_PX}px">${String(H0 + i).padStart(2, "0")}:00</span>`).join("");
  const cols = days.map((t) => dayColumnHtml(t, events.filter((e) => e.date === toIso(t)), sel)).join("");
  return `<div class="cal-week" style="--cal-days:${days.length};--cal-h:${(H1 - H0) * HOUR_PX}px;--cal-hour:${HOUR_PX}px">
      <div class="cal-grid-head"><span></span>${days.map((t) => dayHeadHtml(t, single)).join("")}</div>
      <div class="cal-grid-due"><span>Due</span>${due}</div>
      <div class="cal-grid-body"><div class="cal-hours">${hours}</div>${cols}</div>
    </div>`;
}

/* ---------------------------------------------------------------- month */
function monthCellHtml(t, month, events, deadlines, sel) {
  const list = events.filter((e) => e.date === toIso(t)), due = deadlines.filter((d) => d.date === toIso(t));
  const chips = [...due.map((d) => `<a class="cal-mchip is-due" href="#/projects/${esc(d.proj)}">${ic("flag", "ic14")}${esc(d.label)}</a>`),
    ...list.map((e) => `<button type="button" class="cal-mchip" data-tone="${eventTone(e)}" data-act="cal" data-id="select" data-v="${esc(e.id)}" aria-pressed="${e === sel}"><span class="cal-dot" aria-hidden="true"></span><span>${esc(e.start)}</span> ${esc(e.title)}</button>`)];
  const more = chips.length > 3 ? calBtn("day", toIso(t), "cal-mmore", `+${chips.length - 3} more`) : "";
  const cls = `cal-mcell${monthStartOf(t) !== month ? " is-out" : ""}${t === TODAY ? " is-today" : ""}${t === curDay() ? " is-focus" : ""}`;
  return `<div class="${cls}">${calBtn("day", toIso(t), "cal-mday", fDay.format(t), ` aria-label="Open ${fFull.format(t)}"`)}${chips.slice(0, 3).join("")}${more}</div>`;
}
function monthHtml(days, events, deadlines, sel) {
  const month = monthStartOf(curDay());
  const heads = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"].map((d) => `<span>${d}</span>`).join("");
  return `<div class="cal-month"><div class="cal-mhead">${heads}</div><div class="cal-mgrid">${days.map((t) => monthCellHtml(t, month, events, deadlines, sel)).join("")}</div></div>`;
}

/* ---------------------------------------------------------------- agenda */
function agendaRowHtml(e, sel) {
  const ctx = eventContext(e);
  return `<li><button type="button" class="cal-ag-row" data-act="cal" data-id="select" data-v="${esc(e.id)}" aria-pressed="${e === sel}">
      <span class="cal-ag-time">${esc(e.start)}<small>${esc(e.end)}</small></span>
      <span class="cal-dot" data-tone="${eventTone(e)}" aria-hidden="true"></span>
      <span class="cal-ag-main"><b>${esc(e.title)}</b><small>${esc(ctx.label)} · ${plural(e.attendees.length, "attendee", "attendees")}</small></span>
      <span class="avstack">${e.attendees.slice(0, 4).map((a) => av(a, 24)).join("")}</span></button></li>`;
}
function agendaHtml(days, events, deadlines, sel) {
  const groups = days.map((t) => {
    const list = events.filter((e) => e.date === toIso(t)), due = deadlines.filter((d) => d.date === toIso(t));
    if (!list.length && !due.length) return "";
    const head = Math.abs(Math.round((t - TODAY) / DAY)) <= 1 ? `${relDay(t)}<span>${fLong.format(t)}</span>` : fLong.format(t);
    const dues = due.map((d) => `<li class="cal-ag-due">${ic("flag", "ic14")}<a href="#/projects/${esc(d.proj)}">${esc(d.label)}</a></li>`).join("");
    return `<section class="cal-ag-day${t === TODAY ? " is-today" : ""}"><h3>${calBtn("day", toIso(t), "cal-ag-dbtn", head)}</h3><ul role="list">${dues}${list.map((e) => agendaRowHtml(e, sel)).join("")}</ul></section>`;
  }).join("");
  return `<div class="cal-agenda">${groups || `<p class="empty-row">No meetings in these two weeks.</p>`}</div>`;
}

/* ---------------------------------------------------------------- detail */
function detailHtml(e, view) {
  if (!e) return `<aside class="cal-detail"><p class="cal-d-empty">No meetings ${view === "day" ? "on this day" : "in this range"}.</p></aside>`;
  const ctx = eventContext(e), d = eventDate(e);
  const who = e.attendees.map((a) => `<li>${av(a, 24)}<span><b>${esc(M[a].name)}</b><small>${esc(M[a].owner ? "Owner" : `${M[a].role} · ${kindWord(a)}`)}</small></span></li>`).join("");
  const open = ctx.href ? `<a class="btn outline sm" href="${ctx.href}">Open ${e.proj ? "project" : "team"}${ic("chev-right", "ic16")}</a>` : "";
  return `<aside class="cal-detail" aria-labelledby="cal-d-t">
      <p class="cal-d-when">${calBtn("day", e.date, "cal-d-day", `${esc(d.when)} · ${fLong.format(d.t)}`)} · ${esc(e.start)} – ${esc(e.end)}</p>
      <h3 id="cal-d-t">${esc(e.title)}</h3>
      <p class="cal-d-ctx"><span class="cal-dot" data-tone="${eventTone(e)}" aria-hidden="true"></span>${ctx.href ? `<a href="${ctx.href}">${esc(ctx.label)}</a>` : esc(ctx.label)}</p>
      <h4>Attendees <span>${e.attendees.length}</span></h4>
      <ul class="cal-d-who" role="list">${who}</ul>
      <div class="cal-d-acts">${open}</div>
    </aside>`;
}

const VIEW_RENDER = { day: timeGridHtml, week: timeGridHtml, month: monthHtml, agenda: agendaHtml };
function calendarHtml(scope, id) {
  applyLinkedEvent();
  const narrow = typeof window !== "undefined" && window.matchMedia("(max-width: 720px)").matches;
  const view = narrow && (S.calView || "week") !== "day" ? "agenda" : S.calView || "week";
  const days = viewRange(view, curDay()), first = toIso(days[0]), last = toIso(days[days.length - 1]);
  const events = sortEvents(scopeEvents(scope, id)), deadlines = scopeDeadlines(scope, id);
  const inRange = events.filter((e) => e.date >= first && e.date <= last);
  const sel = selectedEvent(inRange);
  const eventDays = new Set(events.map((e) => e.date));
  return `<div class="cal" data-scope="${scope}" data-view="${view}">${toolbarHtml(view, inRange.length, narrow, eventDays)}
      <div class="cal-split">${VIEW_RENDER[view](days, inRange, deadlines, sel)}${detailHtml(sel, view)}</div></div>`;
}

export { calendarAction, calendarHtml, eventContext, eventDate, eventTone, sortEvents };
