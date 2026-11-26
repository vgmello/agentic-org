/* Calendars: one component, three scopes. "me" = meetings you attend (#/calendar), "project" and
   "team" = that project's or team's meetings (Calendar tabs). Week grid with a Due row and a
   now-line, or an Agenda list (always used on narrow screens), plus a detail panel. */
import { CAL_TODAY, EVENTS, M, P, TEAMS } from "../data.js";
import { S, qs } from "../state.js";
import { av, esc, ic, kindWord, plural } from "../ui.js";

const DAY = 86_400_000, H0 = 8, H1 = 19, HOUR_PX = 48, NOW = "11:20";
const toTime = (iso) => Date.UTC(+iso.slice(0, 4), +iso.slice(5, 7) - 1, +iso.slice(8, 10));
const TODAY = toTime(CAL_TODAY);
const toIso = (t) => new Date(t).toISOString().slice(0, 10);
const fmt = (opts) => new Intl.DateTimeFormat("en-US", { timeZone: "UTC", ...opts });
const fWeekday = fmt({ weekday: "short" }), fMon = fmt({ month: "short" }), fDay = fmt({ day: "numeric" }), fLong = fmt({ weekday: "short", month: "short", day: "numeric" });
const minutes = (hhmm) => +hhmm.slice(0, 2) * 60 + +hhmm.slice(3, 5);
const MONTHS = { Jan: 1, Feb: 2, Mar: 3, Apr: 4, May: 5, Jun: 6, Jul: 7, Aug: 8, Sep: 9, Oct: 10, Nov: 11, Dec: 12 };
const PROJECT_TONE = { "agency-v1": "accent", "look-feel": "violet", "office-federation": "warn" };

/* Day label relative to today: "Today", "Tomorrow", "Fri", or "Tue, Dec 1" further out. */
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
function weekStart() {
  const dow = (new Date(TODAY).getUTCDay() + 6) % 7;
  return TODAY - dow * DAY + Number(S.calWeek || 0) * 7 * DAY;
}
/* A link like #/calendar?event=ev1 selects that meeting (and jumps to its week) once. */
function applyLinkedEvent() {
  const id = qs().get("event"), e = id && EVENTS.find((x) => x.id === id);
  if (!e || S.calLinked === id) return;
  S.calLinked = id; S.calEvent = id;
  const dow = (new Date(TODAY).getUTCDay() + 6) % 7;
  S.calWeek = String(Math.floor((toTime(e.date) - (TODAY - dow * DAY)) / (7 * DAY)));
}
function selectedEvent(list) {
  return list.find((e) => e.id === S.calEvent) || list.find((e) => e.date >= CAL_TODAY) || list[0] || null;
}

/* ---------------------------------------------------------------- toolbar */
function toolbarHtml(start, inWeek, narrow) {
  const wk = Number(S.calWeek || 0), end = start + 6 * DAY;
  const range = `${fMon.format(start)} ${fDay.format(start)} – ${fMon.format(end) === fMon.format(start) ? "" : fMon.format(end) + " "}${fDay.format(end)}, 2026`;
  const seg = (v, icon, label) => `<button class="queue-tab" type="button" data-act="set" data-k="calView" data-v="${v}" aria-pressed="${(S.calView || "week") === v}">${ic(icon, "ic16")}${label}</button>`;
  return `<div class="cal-toolbar">
      <div class="cal-nav" role="group" aria-label="Week">
        <button class="btn outline sm" type="button" data-act="set" data-k="calWeek" data-v="0"${wk === 0 ? " disabled" : ""}>Today</button>
        <button class="btn ghost icon sm" type="button" data-act="set" data-k="calWeek" data-v="${wk - 1}" aria-label="Previous week">${ic("chev-left", "ic16")}</button>
        <button class="btn ghost icon sm" type="button" data-act="set" data-k="calWeek" data-v="${wk + 1}" aria-label="Next week">${ic("chev-right", "ic16")}</button>
        <h2 class="cal-range">${range}</h2><span class="cal-count">${plural(inWeek, "meeting", "meetings")}</span>
      </div>
      ${narrow ? "" : `<div class="work-layout-toggle" role="group" aria-label="Calendar layout">${seg("week", "board", "Week")}${seg("agenda", "list", "Agenda")}</div>`}
    </div>`;
}

/* ---------------------------------------------------------------- week grid */
function eventBlockHtml(e, sel) {
  const top = ((minutes(e.start) - H0 * 60) / 60) * HOUR_PX, height = Math.max(22, ((minutes(e.end) - minutes(e.start)) / 60) * HOUR_PX - 2);
  const ctx = eventContext(e);
  return `<button type="button" class="cal-ev" data-tone="${eventTone(e)}" data-act="set" data-k="calEvent" data-v="${esc(e.id)}" aria-pressed="${e === sel}" style="top:${top}px;height:${height}px" aria-label="${esc(`${e.title}, ${e.start} to ${e.end}, ${ctx.label}`)}">
      <span class="cal-ev-t">${esc(e.title)}</span><span class="cal-ev-m">${esc(e.start)} – ${esc(e.end)}</span></button>`;
}
function dayColumnHtml(t, events, sel) {
  const now = t === TODAY ? `<span class="cal-now" style="top:${((minutes(NOW) - H0 * 60) / 60) * HOUR_PX}px" aria-hidden="true"></span>` : "";
  return `<div class="cal-col${t === TODAY ? " is-today" : ""}">${now}${events.map((e) => eventBlockHtml(e, sel)).join("")}</div>`;
}
function weekGridHtml(start, events, deadlines, sel) {
  const days = Array.from({ length: 7 }, (_, i) => start + i * DAY);
  const head = days.map((t) => `<div class="cal-dh${t === TODAY ? " is-today" : ""}${t < TODAY ? " is-past" : ""}"><span>${fWeekday.format(t)}</span><b>${fDay.format(t)}</b></div>`).join("");
  const due = days.map((t) => `<div class="cal-due-cell">${deadlines.filter((d) => d.date === toIso(t)).map((d) => `<a class="cal-due" href="#/projects/${esc(d.proj)}">${ic("flag", "ic14")}${esc(d.label)}</a>`).join("")}</div>`).join("");
  const hours = Array.from({ length: H1 - H0 }, (_, i) => `<span style="top:${i * HOUR_PX}px">${String(H0 + i).padStart(2, "0")}:00</span>`).join("");
  const cols = days.map((t) => dayColumnHtml(t, events.filter((e) => e.date === toIso(t)), sel)).join("");
  return `<div class="cal-week" style="--cal-h:${(H1 - H0) * HOUR_PX}px;--cal-hour:${HOUR_PX}px">
      <div class="cal-grid-head"><span></span>${head}</div>
      <div class="cal-grid-due"><span>Due</span>${due}</div>
      <div class="cal-grid-body"><div class="cal-hours">${hours}</div>${cols}</div>
    </div>`;
}

/* ---------------------------------------------------------------- agenda */
function agendaRowHtml(e, sel) {
  const ctx = eventContext(e);
  return `<li><button type="button" class="cal-ag-row" data-act="set" data-k="calEvent" data-v="${esc(e.id)}" aria-pressed="${e === sel}">
      <span class="cal-ag-time">${esc(e.start)}<small>${esc(e.end)}</small></span>
      <span class="cal-dot" data-tone="${eventTone(e)}" aria-hidden="true"></span>
      <span class="cal-ag-main"><b>${esc(e.title)}</b><small>${esc(ctx.label)} · ${plural(e.attendees.length, "attendee", "attendees")}</small></span>
      <span class="avstack">${e.attendees.slice(0, 4).map((a) => av(a, 24)).join("")}</span></button></li>`;
}
function agendaHtml(start, events, deadlines, sel) {
  const days = Array.from({ length: 14 }, (_, i) => start + i * DAY);
  const groups = days.map((t) => {
    const list = events.filter((e) => e.date === toIso(t)), due = deadlines.filter((d) => d.date === toIso(t));
    if (!list.length && !due.length) return "";
    const dues = due.map((d) => `<li class="cal-ag-due">${ic("flag", "ic14")}<a href="#/projects/${esc(d.proj)}">${esc(d.label)}</a></li>`).join("");
    const rel = Math.abs(Math.round((t - TODAY) / DAY)) <= 1 ? `${relDay(t)}<span>${fLong.format(t)}</span>` : fLong.format(t);
    return `<section class="cal-ag-day${t === TODAY ? " is-today" : ""}"><h3>${rel}</h3><ul role="list">${dues}${list.map((e) => agendaRowHtml(e, sel)).join("")}</ul></section>`;
  }).join("");
  return `<div class="cal-agenda">${groups || `<p class="empty-row">No meetings in the next two weeks.</p>`}</div>`;
}

/* ---------------------------------------------------------------- detail */
function detailHtml(e) {
  if (!e) return `<aside class="cal-detail"><p class="muted">No meetings yet.</p></aside>`;
  const ctx = eventContext(e), d = eventDate(e);
  const who = e.attendees.map((a) => `<li>${av(a, 24)}<span><b>${esc(M[a].name)}</b><small>${esc(M[a].owner ? "Owner" : `${M[a].role} · ${kindWord(a)}`)}</small></span></li>`).join("");
  const open = ctx.href ? `<a class="btn outline sm" href="${ctx.href}">Open ${e.proj ? "project" : "team"}${ic("chev-right", "ic16")}</a>` : "";
  return `<aside class="cal-detail" aria-labelledby="cal-d-t">
      <p class="cal-d-when">${esc(d.when)} · ${fLong.format(d.t)} · ${esc(e.start)} – ${esc(e.end)}</p>
      <h3 id="cal-d-t">${esc(e.title)}</h3>
      <p class="cal-d-ctx"><span class="cal-dot" data-tone="${eventTone(e)}" aria-hidden="true"></span>${ctx.href ? `<a href="${ctx.href}">${esc(ctx.label)}</a>` : esc(ctx.label)}</p>
      <h4>Attendees <span>${e.attendees.length}</span></h4>
      <ul class="cal-d-who" role="list">${who}</ul>
      <div class="cal-d-acts">${open}</div>
    </aside>`;
}

function calendarHtml(scope, id) {
  applyLinkedEvent();
  const narrow = typeof window !== "undefined" && window.matchMedia("(max-width: 720px)").matches;
  const start = weekStart(), events = sortEvents(scopeEvents(scope, id)), deadlines = scopeDeadlines(scope, id);
  const inWeek = events.filter((e) => toTime(e.date) >= start && toTime(e.date) < start + 7 * DAY);
  const sel = selectedEvent(inWeek.length ? inWeek : events);
  const view = narrow || S.calView === "agenda" ? agendaHtml(start, events, deadlines, sel) : weekGridHtml(start, inWeek, deadlines, sel);
  return `<div class="cal" data-scope="${scope}">${toolbarHtml(start, inWeek.length, narrow)}<div class="cal-split">${view}${detailHtml(sel)}</div></div>`;
}

export { calendarHtml, eventContext, eventDate, eventTone, sortEvents };
