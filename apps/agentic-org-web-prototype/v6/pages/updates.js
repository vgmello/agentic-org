/* Updates (TASK-250): which release of Agentic Org each office runs, the releases there are, and what was updated.
   An Organization page in the sidebar's More fold (#/updates; #/settings/updates still opens it).
   doc-48 §5 and SRV-15/16, doc-70 "Service and updates", doc-67 "Versions", doc-44 §6.1. Controls carry data-upd-act;
   the actions, dialogs, panels and the simulated progress are in updates-actions.js. */
import { M, OFFICES } from "../data.js";
import { HISTORY, LATEST, QUEUED, RELEASES, RUNS, STEPS, heldBack, isLatest, officeAgents, officeState, releaseOf, FAILURES } from "../updates-data.js";
import { av, esc, hdr, ic, pill, plural } from "../ui.js";

const VIEWS = [["", "Offices"], ["releases", "Releases"], ["history", "History"]];
/* Order as on Offices (doc-44 §6.1): HQ, offices on HQ's machine, then the others A–Z */
const officeKeys = () => Object.keys(OFFICES).sort((a, b) => (OFFICES[a].role === "HQ" ? -1 : OFFICES[b].role === "HQ" ? 1 : OFFICES[a].name.localeCompare(OFFICES[b].name)));
const updatable = () => officeKeys().filter((k) => OFFICES[k].status === "Online" && ["available", "failed"].includes(officeState(k)));
const offline = (k) => OFFICES[k].status !== "Online";
const cmd = (text) => `<code class="upd-cmd">${esc(text)}</code>`;
const upgradeCmd = (k) => `agentic-org upgrade --name ${OFFICES[k].instance}`;

/* Status: a word plus an icon, never colour alone */
const STATUS = {
  current: ["Up to date", "green", "check-circle"],
  available: ["Update available", "sky", "download"],
  updating: ["Updating", "sky", "refresh"],
  failed: ["Update failed", "red", "x-circle"],
  queued: ["Update queued", "amber", "clock"],
  offline: ["Offline", "outline", "offline"],
};
function statusKey(k) {
  const s = officeState(k);
  if (offline(k)) return s === "queued" ? "queued" : "offline";
  return s;
}
const statusChip = (k) => { const [t, tone, icon] = STATUS[statusKey(k)]; return pill(t, tone, icon); };

function stepIndex(k) { return Math.max(0, STEPS.findIndex(([id]) => id === RUNS[k]?.step)); }
/* The row's progress: the step, its number and one bar for the whole update */
function progressHtml(k) {
  const run = RUNS[k]; if (!run) return "";
  const i = stepIndex(k), label = STEPS[i][1], pct = Math.min(100, Math.round((i * 100 + (run.pct || 0)) / 3));
  return `<div class="upd-progress"><div class="progress" role="progressbar" aria-label="Update of ${esc(OFFICES[k].name)}" aria-valuemin="0" aria-valuemax="100" aria-valuenow="${pct}" aria-valuetext="${esc(label)}, step ${i + 1} of 4"><span style="width:${pct}%"></span></div>
    <span class="upd-step-now">${esc(label)}${run.step === "download" ? ` ${run.pct || 0}%` : ""} · step ${i + 1} of 4</span></div>`;
}

/* The one line under a row that explains its state, when it needs explaining */
function stateLine(k) {
  const o = OFFICES[k], s = statusKey(k);
  if (s === "queued") return `<p class="upd-line">${ic("clock", "ic14")}<span>${esc(o.name)} is offline, so it can't update now. The update to ${esc(LATEST)} is queued: it starts when ${esc(o.name)} connects, and nothing is reported done until it is.</span></p>`;
  if (s === "offline") return `<p class="upd-line">${ic("offline", "ic14")}<span>Offline, so it can't update now. Queue the update and it starts when ${esc(o.name)} connects.</span></p>`;
  if (s === "failed") return `<p class="upd-line is-bad">${ic("x-circle", "ic14")}<span>Update to ${esc(LATEST)} failed: ${esc(FAILURES[k]?.reason || "the instance reported an error.")} It went back to ${esc(o.version)} and is running.</span></p>`;
  return "";
}

const versionHtml = (k) => {
  const o = OFFICES[k];
  return isLatest(o.version) || RUNS[k] ? `<span>Runs ${esc(o.version)}</span>` : `<span>Runs ${esc(o.version)} ${ic("chev-right", "ic14")} ${esc(LATEST)}</span>`;
};
function rowAction(k) {
  const s = statusKey(k), o = OFFICES[k], name = `<span class="sr-only"> ${esc(o.name)}</span>`;
  const btn = (act, icon, label, cls = "outline") => `<button class="btn ${cls} sm proc-row-btn" type="button" data-upd-act="${act}" data-id="${esc(k)}">${ic(icon, "ic16")}${label}${name}</button>`;
  if (s === "available") return btn("confirm", "download", "Update…", "primary");
  if (s === "failed") return btn("logs", "terminal", "View logs", "ghost") + btn("confirm", "refresh", "Retry…");
  if (s === "offline") return btn("confirm", "clock", "Queue update…");
  return "";
}
function officeRow(k) {
  const o = OFFICES[k], n = officeAgents(M, k).length;
  const hq = o.role === "HQ" && o.name !== "HQ" ? `<span class="proc-badge is-version">${ic("office", "ic14")}HQ</span>` : "";
  return `<li class="linked plan-row proc-row upd-row" id="upd-row-${esc(k)}" data-state="${statusKey(k)}">
    <span class="proc-row-ic" aria-hidden="true">${ic(offline(k) ? "offline" : "office", "ic20")}</span>
    <div class="grow"><p class="rt"><button class="stretch proc-open" type="button" data-upd-act="open" data-id="${esc(k)}">${esc(o.name)}</button>${hq}</p>
      <div class="meta">${versionHtml(k)}<span>${esc(o.host)} · instance ${esc(o.instance)}</span><span>${offline(k) ? `Offline · ${esc(o.note.split(" · ")[0].toLowerCase())}` : "Online"}</span><span>Checked ${esc(o.checked.toLowerCase())}</span><span>${plural(n, "agent", "agents")}</span></div>
      <div class="upd-live" id="upd-pg-${esc(k)}">${progressHtml(k)}</div>${stateLine(k)}</div>
    <div class="proc-badges" id="upd-st-${esc(k)}">${statusChip(k)}</div>
    <div class="dev-row-actions upd-row-actions">${rowAction(k)}</div>
    ${ic("chev-right", "ic20 chev")}</li>`;
}

/* This instance and the organization level (doc-67: the app shows which offices hold a new level back) */
function summaryHtml() {
  const hq = Object.keys(OFFICES).find((k) => OFFICES[k].role === "HQ"), o = OFFICES[hq];
  const latest = isLatest(o.version) ? `<span class="proc-badge is-ok">${ic("check-circle", "ic14")}Latest</span>` : `<span class="proc-badge is-warn">${ic("download", "ic14")}${esc(LATEST)} available</span>`;
  return `<section class="upd-summary" aria-labelledby="upd-this">
      <div class="upd-this"><h2 id="upd-this">This instance</h2><p class="upd-big">Agentic Org ${esc(o.version)} ${latest}</p>
        <p class="upd-sub">Instance ${cmd(o.instance)} on ${esc(o.machine)}, running ${esc(o.name)} · checked for releases ${esc(o.checked.toLowerCase())}</p></div>
      ${heldBackHtml()}
    </section>`;
}
function heldBackHtml() {
  const held = heldBack(); if (!held.length) return "";
  const names = held.map((k) => `<a class="alink" href="#upd-row-${esc(k)}" data-upd-act="jump" data-id="${esc(k)}">${esc(OFFICES[k].name)}</a>`);
  const list = names.length > 1 ? `${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}` : names[0];
  const verb = held.length > 1 ? "run older versions" : "runs an older version";
  return `<div class="callout proc-callout upd-held" role="note">${ic("info", "ic20")}<div><p>${list} ${verb} of Agentic Org, so new features wait until ${held.length > 1 ? "they update" : "it updates"}.</p>
    <p class="upd-sub">${esc(LATEST)} raises the organization level to ${releaseOf(LATEST).level}. HQ raises it once every office seen in the last 30 days supports it; offices can update in any order.</p></div></div>`;
}

function releaseBadges(r) {
  return (isLatest(r.version) ? `<span class="proc-badge is-ok">${ic("star", "ic14")}Latest</span>` : "")
    + (r.kind === "security" ? `<span class="proc-badge is-warn">${ic("shield-alert", "ic14")}Security fix · recommended</span>` : "")
    + (r.kind === "level" ? `<span class="proc-badge is-version">${ic("arrows-updown", "ic14")}Raises organization level</span>` : "")
    + (!r.kind ? `<span class="proc-badge">Optional</span>` : "");
}
function releaseRow(r) {
  const on = officeKeys().filter((k) => OFFICES[k].version === r.version).map((k) => OFFICES[k].name);
  return `<li class="linked plan-row proc-row upd-rel">
    <span class="proc-row-ic" aria-hidden="true">${ic("download", "ic20")}</span>
    <div class="grow"><p class="rt"><button class="stretch proc-open" type="button" data-upd-act="release" data-id="${esc(r.version)}">Agentic Org ${esc(r.version)}</button></p>
      <p class="dev-summary">${esc(r.summary)}</p>
      <div class="meta"><span>Released ${esc(r.date)}</span><span>${on.length ? `Running on ${esc(on.join(", "))}` : "No office runs it"}</span><code class="proc-key">${esc(r.id)}</code></div></div>
    <div class="proc-badges">${releaseBadges(r)}</div>${ic("chev-right", "ic20 chev")}</li>`;
}

const RESULT = { completed: ["Completed", "green", "check-circle"], failed: ["Failed", "red", "x-circle"], queued: ["Queued", "amber", "clock"], cancelled: ["Cancelled", "outline", "x"], running: ["Running", "sky", "refresh"] };
const who = (h) => (M[h.by] ? `<span class="proc-owner">${av(h.by, 24)}${esc(M[h.by].name)}</span>` : `<span>${esc(h.by)}</span>`);
function historyRow(h) {
  const [t, tone, icon] = RESULT[h.result];
  return `<li class="plan-row proc-row upd-hist">
    <span class="proc-row-ic" aria-hidden="true">${ic(icon, "ic20")}</span>
    <div class="grow"><p class="rt"><span class="proc-title">${esc(OFFICES[h.office]?.name || h.office)}: ${esc(h.from)} ${ic("chev-right", "ic14")} ${esc(h.to)}</span></p>
      <div class="meta"><span>${esc(h.at)}</span>${who(h)}<span>${esc(h.via)}</span></div>
      ${h.detail ? `<p class="upd-sub">${esc(h.detail)}</p>` : ""}</div>
    <div class="proc-badges">${pill(t, tone, icon)}</div></li>`;
}

function toolbar(view) {
  const counts = { "": Object.keys(OFFICES).length, releases: RELEASES.length, history: HISTORY.length };
  const tabs = VIEWS.map(([v, l]) => `<a class="queue-tab" href="#/updates${v ? `/${v}` : ""}"${v === view ? ' aria-current="page"' : ""}>${l}<span>${counts[v]}</span></a>`).join("");
  const n = updatable().length;
  const all = n ? `<button class="btn primary" type="button" data-upd-act="confirm-all">${ic("download", "ic16")}Update all offices<span class="upd-n">${n}</span></button>` : "";
  return `<div class="work-subbar-header proc-subbar"><div class="work-toolbar queue-toolbar proc-toolbar upd-toolbar">
      <div class="primary-tab-row"><nav class="queue-tabs" aria-label="Updates views">${tabs}</nav></div>
      <div class="queue-controls" role="group" aria-label="Release checks"><button class="btn ghost" type="button" data-upd-act="check">${ic("refresh", "ic16")}Check now</button></div>
      <div class="proc-toolbar-end">${all}</div>
    </div></div>`;
}

function bodyFor(view) {
  if (view === "releases") return `<p class="desc proc-note upd-note">Each release is complete: the server, office worker, browser app and agent-os switch together. Every instance keeps the release it used before, so a failed update goes back to it.</p><ul class="rows plan-rows" role="list">${RELEASES.map(releaseRow).join("")}</ul>`;
  if (view === "history") return `<ul class="rows plan-rows" role="list">${HISTORY.map(historyRow).join("")}</ul><p class="upd-foot">Updates run from a terminal on the machine are listed when its office reports them.</p>`;
  const queued = Object.keys(QUEUED).length;
  return summaryHtml() + `<ul class="rows plan-rows" role="list" aria-label="Offices">${officeKeys().map(officeRow).join("")}</ul>
    <p class="upd-foot">Shown from HQ. Statuses include last-seen information.${queued ? " An offline office's update stays queued until it connects." : ""} On the machine itself: ${cmd("agentic-org upgrade --name default")}.</p>`;
}

function pageUpdates(sub) {
  const view = VIEWS.some(([v]) => v === sub) ? sub : "";
  const n = updatable().length;
  const header = hdr("Updates", n ? `${plural(n, "office", "offices")} can update to ${LATEST}` : `Agentic Org ${LATEST} is the latest release`);
  const intro = `<p class="desc proc-note">Each office runs on an instance of Agentic Org. Updating an office switches its instance to a new release; other instances on the same machine keep theirs.</p>`;
  return `<div class="settings org-standalone upd-page">${header}<div class="settings-body">${intro}${toolbar(view)}<div class="plan-results proc-results upd-results" id="upd-results">${bodyFor(view)}</div></div></div>`;
}

export { STATUS, cmd, officeKeys, pageUpdates, progressHtml, statusChip, statusKey, upgradeCmd, updatable };
