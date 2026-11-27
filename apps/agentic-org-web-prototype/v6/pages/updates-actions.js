/* What Updates does (TASK-250): confirm an update with its effect, run it through its steps (simulated), retry a
   failed one, queue or cancel one for an offline office, check for releases, and the office, release and log panels.
   Rules: an upgrade targets one instance and switches a complete release, keeping the prior one, so a failure goes back
   to it (doc-48 §5, doc-70); an offline office's update is Queued and never reported as completed (doc-48 §4);
   a peer instance is never touched (SRV-16). */
import { M, OFFICES } from "../data.js";
import { DOWNTIME, FAILURES, HISTORY, LATEST, LOGS, PEERS, POLICY, QUEUED, RUNS, STEPS, officeAgents, officeState, releaseOf } from "../updates-data.js";
import { closeSheet, openDialog, setSheetReturn } from "../shell/dialogs.js";
import { render } from "../shell/router.js";
import { esc, ic, pill, plural, toast } from "../ui.js";
import { closeProcPanel, factList, openProcPanel, panelSection, procPanelElement, procPanelOpen } from "./proc-panel.js";
import { cmd, officeKeys, progressHtml, statusChip, statusKey, upgradeCmd, updatable } from "./updates.js";

const cancelBtn = '<button class="btn outline" type="button" data-act="close-sheet">Cancel</button>';
/* The mock's clock: the seed data is written for a morning, so "now" starts at 09:24 and moves with real time */
const LOADED = Date.now();
const clock = () => new Date(new Date(2026, 10, 26, 9, 24).getTime() + (Date.now() - LOADED));
const nowTime = () => clock().toTimeString().slice(0, 5);
const agentNames = (k) => officeAgents(M, k).map((a) => M[a].name);
const opId = () => `0196f9${Math.random().toString(16).slice(2, 6)}-${Math.random().toString(16).slice(2, 6)}`;

/* A polite live region outside #main, so a re-render doesn't drop what it is saying */
function announce(text) {
  let el = document.getElementById("upd-announce");
  if (!el) { el = document.createElement("p"); el.id = "upd-announce"; el.className = "sr-only"; el.setAttribute("role", "status"); document.body.append(el); }
  el.textContent = ""; setTimeout(() => { el.textContent = text; }, 30);
}

/* The confirm: what happens to the agents, how long the office is away, what happens on failure, what isn't touched */
function effectItems(k) {
  const o = OFFICES[k], names = agentNames(k), queued = o.status !== "Online";
  const agents = names.length === 1
    ? `${names[0]}, the agent on ${o.name}, finishes its current step and pauses. New work waits and starts when ${o.name} is back.`
    : names.length
    ? `${names.length} agents on ${o.name} (${names.join(", ")}) finish their current step and pause. New work waits and starts when ${o.name} is back.`
    : `No agents are placed on ${o.name}. Work for it waits until it's back.`;
  const items = [
    ["clock", queued ? `${o.name} is offline, so it can't update now. The update is queued and starts when ${o.name} connects. Until then it's shown as queued, never as done.` : `${o.name} is away for ${DOWNTIME[o.role]} while its instance ${o.instance} restarts on ${LATEST}.`],
    ["pause", agents],
    ["shield-check", `If a step fails, ${o.name} goes back to ${o.version} and keeps running. You can retry.`],
  ];
  if (o.role === "HQ") items.push(["info", "This page loses its connection while HQ restarts and reconnects by itself. Satellite offices keep working on their own meanwhile."]);
  const peers = PEERS[o.host] || [];
  if (peers.length) items.push(["lock", `Not touched: ${peers.map((p) => `instance ${p.name} (${p.version}, ${p.state.toLowerCase()})`).join(", ")} on ${o.host} keeps its release, helpers, data and credentials.`]);
  return items;
}
const effectList = (items) => `<ul class="upd-effects" role="list">${items.map(([icon, text]) => `<li>${ic(icon, "ic16")}<span>${esc(text)}</span></li>`).join("")}</ul>`;
const cliNote = (k) => `<p class="upd-cli">${ic("terminal", "ic16")}<span>Or on ${esc(OFFICES[k].host)}, run ${cmd(upgradeCmd(k))}</span></p>`;

function openConfirm(k, trigger) {
  const o = OFFICES[k], state = statusKey(k);
  const retry = state === "failed", queue = state === "offline";
  const title = queue ? `Queue the update of ${o.name} to ${LATEST}?` : retry ? `Retry the update of ${o.name} to ${LATEST}?` : `Update ${o.name} to ${LATEST}?`;
  const desc = queue ? `${o.name} runs ${o.version} and was last seen ${o.note.split(" · ")[0].replace(/^Last seen /, "")}.`
    : `${o.name} runs ${o.version}. Its instance downloads and verifies ${LATEST}, drains, then restarts on it.${retry ? " The retry uses the same request, so it can't run twice." : ""}`;
  const label = queue ? "Queue update" : retry ? `Retry update` : `Update ${o.name}`;
  openDialog(trigger, title, desc, effectList(effectItems(k)) + cliNote(k),
    cancelBtn + `<button class="btn primary" type="button" data-upd-act="${queue ? "queue" : "start"}" data-id="${esc(k)}">${esc(label)}</button>`);
  document.querySelector(".sheet")?.classList.add("plan-dialog", "upd-dialog");
}

function openConfirmAll(trigger) {
  const now = updatable(), queued = officeKeys().filter((k) => statusKey(k) === "queued");
  const line = (k) => `<li>${ic("office", "ic16")}<span><b>${esc(OFFICES[k].name)}</b> ${esc(OFFICES[k].version)} → ${esc(LATEST)} · away ${esc(DOWNTIME[OFFICES[k].role])} · ${esc(plural(agentNames(k).length, "agent pauses", "agents pause"))}</span></li>`;
  const queuedNote = queued.length ? `<p class="upd-sub">${esc(queued.map((k) => OFFICES[k].name).join(", "))} ${queued.length > 1 ? "are" : "is"} offline; ${queued.length > 1 ? "their updates stay" : "its update stays"} queued until ${queued.length > 1 ? "they connect" : "it connects"}.</p>` : "";
  openDialog(trigger, `Update ${plural(now.length, "office", "offices")} to ${LATEST}?`, "One office at a time, so the others keep working. Each goes back to its current release if a step fails.",
    `<ul class="upd-effects" role="list">${now.map(line).join("")}</ul>${queuedNote}<p class="upd-sub">Other instances on these machines aren't touched.</p>`,
    cancelBtn + `<button class="btn primary" type="button" data-upd-act="start-all">Update ${plural(now.length, "office", "offices")}</button>`);
  document.querySelector(".sheet")?.classList.add("plan-dialog", "upd-dialog");
}

/* The simulated run: Downloading (with a percentage), Verifying, Restarting, Done */
const pending = [];
function log(k, level, text) { (LOGS[k] ||= []).push([clock().toTimeString().slice(0, 8), level, text]); }
function start(k, by = "you") {
  const o = OFFICES[k]; if (RUNS[k]) return;
  const retry = o.update === "failed";
  RUNS[k] = { step: "download", pct: 0, opId: retry ? "0196f86c-9b02" : opId(), from: o.version };
  LOGS[k] = retry ? LOGS[k] : [];
  log(k, "info", `${retry ? "retry of operation 0196f86c-9b02" : "upgrade requested"} by ${by}`);
  log(k, "info", `downloading ${LATEST} (${o.host.includes("ci") ? "58.7" : "61.2"} MB)`);
  HISTORY.unshift({ at: `Today ${nowTime()}`, office: k, from: o.version, to: LATEST, result: "running", by, via: "Browser", detail: "" });
  announce(`Updating ${o.name}: downloading.`);
  tick(k);
}
function tick(k) {
  const run = RUNS[k]; if (!run) return;
  if (run.step === "download" && run.pct < 100) { run.pct = Math.min(100, run.pct + 20); return schedule(k, 350); }
  const i = STEPS.findIndex(([id]) => id === run.step), next = STEPS[i + 1][0];
  run.step = next; run.pct = 0;
  if (next === "verify") { log(k, "info", `verifying manifest sha256 ${releaseOf(LATEST).sha}… and executable signatures`); announce(`${OFFICES[k].name}: verifying.`); return schedule(k, 1100); }
  if (next === "restart") { log(k, "info", `verified; draining: ${plural(agentNames(k).length, "agent", "agents")} finishing their current step`); log(k, "info", `pin changed ${run.from} → ${LATEST} (prior release kept)`); announce(`${OFFICES[k].name}: restarting.`); return schedule(k, 1600); }
  finish(k);
}
function schedule(k, ms) { patchLive(k); pending.push(setTimeout(() => tick(k), ms)); }

function finish(k) {
  const o = OFFICES[k], run = RUNS[k];
  log(k, "info", "readiness: office ready, connected to HQ"); log(k, "info", "completed");
  o.version = LATEST; o.update = "current"; o.checked = `Today ${nowTime()}`;
  delete FAILURES[k]; delete RUNS[k];
  const h = HISTORY.find((x) => x.office === k && x.result === "running");
  if (h) Object.assign(h, { result: "completed", detail: `Away ${o.role === "HQ" ? "1 min 58 s" : "54 s"}.` }); /* mock: the real figure comes from the instance */
  rerender(k);
  announce(`${o.name} is up to date on ${LATEST}.`);
  toast(`${o.name} updated to ${LATEST}.`, "check-circle");
  const nextUp = pending.queue?.shift();
  if (nextUp) { start(nextUp); rerender(nextUp); }
}

/* In place while it runs: the row's chip and bar, and the panel's state, without moving focus */
function patchLive(k) {
  const st = document.getElementById(`upd-st-${k}`), pg = document.getElementById(`upd-pg-${k}`);
  if (st) st.innerHTML = statusChip(k);
  if (pg) pg.innerHTML = progressHtml(k);
  const panel = procPanelElement();
  if (procPanelOpen() && panel.dataset.upd === k) {
    const box = panel.querySelector("#upd-panel-state"), hadFocus = box?.contains(document.activeElement);
    if (box) box.innerHTML = panelState(k);
    if (hadFocus) panel.querySelector("#proc-panel-title")?.focus({ preventScroll: true });
    const logBox = panel.querySelector("#upd-panel-log"); if (logBox) logBox.innerHTML = logLines(k);
  }
}
/* A state change re-renders the page; focus goes back to the same control, or to the office's name */
function rerender(k) {
  const a = document.activeElement, act = a?.dataset?.updAct, id = a?.dataset?.id, inMain = document.getElementById("main")?.contains(a);
  render(false);
  if (inMain) (document.querySelector(`#main [data-upd-act="${act}"][data-id="${CSS.escape(id || "")}"]`) || document.querySelector(`#main [data-upd-act="open"][data-id="${CSS.escape(k)}"]`))?.focus({ preventScroll: true });
  patchLive(k);
  const panel = procPanelElement();
  if (procPanelOpen() && panel.dataset.upd === k) panel.querySelector("#upd-panel-facts")?.replaceChildren(...factsNode(k));
}

/* Office panel ---------------------------------------------------------------------------------------- */
function stepsHtml(k) {
  const state = officeState(k), run = RUNS[k], failAt = FAILURES[k]?.step;
  if (state !== "updating" && state !== "failed") return "";
  const cur = run ? STEPS.findIndex(([id]) => id === run.step) : STEPS.findIndex(([id]) => id === failAt);
  return `<ol class="upd-steps" role="list" aria-label="Update steps">${STEPS.map(([, label], i) => {
    const st = state === "failed" ? (i < cur ? "done" : i === cur ? "failed" : "todo") : i < cur ? "done" : i === cur ? "now" : "todo";
    const icon = { done: "check-circle", now: "refresh", failed: "x-circle", todo: "clock" }[st];
    const word = { done: "done", now: "in progress", failed: "failed", todo: "not started" }[st];
    return `<li data-st="${st}">${ic(icon, "ic16")}<span>${esc(label)}</span><span class="sr-only">, ${word}</span></li>`;
  }).join("")}</ol>`;
}
function panelActions(k) {
  const s = statusKey(k), b = (act, icon, label, cls = "outline") => `<button class="btn ${cls} sm" type="button" data-upd-act="${act}" data-id="${esc(k)}">${ic(icon, "ic16")}${label}</button>`;
  const main = s === "available" ? b("confirm", "download", "Update…", "primary") : s === "failed" ? b("confirm", "refresh", "Retry…", "primary") : s === "offline" ? b("confirm", "clock", "Queue update…") : s === "queued" ? b("unqueue", "x", "Cancel queued update") : "";
  return `<div class="proc-panel-actions">${main}${b("logs", "terminal", "View logs")}</div>`;
}
function panelCallout(k) {
  const o = OFFICES[k], s = statusKey(k);
  const text = {
    current: `${o.name} runs the latest release.`,
    available: `${LATEST} is downloaded and verified in the background. Updating restarts ${o.name} for ${DOWNTIME[o.role]}.`,
    updating: `Updating to ${LATEST}. Agents on ${o.name} pause until it's back.`,
    failed: `Update to ${LATEST} failed at ${STEPS.find(([id]) => id === FAILURES[k]?.step)?.[1] || "a step"}: ${FAILURES[k]?.reason || ""} ${o.name} went back to ${o.version} and is running.`,
    queued: `${o.name} is offline, so it can't update now. Queued ${QUEUED[k]?.at.toLowerCase() || "earlier"}: it starts when ${o.name} connects. Nothing is reported done until ${o.name} confirms it.`,
    offline: `${o.name} is offline, so it can't update now. Queue the update, or run the command on ${o.host} when it's back.`,
  }[s];
  const icon = { current: "check-circle", available: "download", updating: "refresh", failed: "x-circle", queued: "clock", offline: "offline" }[s];
  return `<div class="callout proc-callout upd-callout" data-state="${s}" role="note">${ic(icon, "ic20")}<div><p>${esc(text)}</p></div></div>`;
}
const panelState = (k) => `<div class="proc-badges">${statusChip(k)}</div>${panelCallout(k)}${stepsHtml(k)}${panelActions(k)}`;
function factsRows(k) {
  const o = OFFICES[k], rel = releaseOf(o.version), names = agentNames(k);
  return [["Role", o.role === "HQ" ? "HQ" : "Office"], ["Machine", esc(o.machine)], ["Instance", cmd(o.instance)], ["Runs", `${esc(o.version)}${rel ? ` · <code>${esc(rel.id)}</code>` : ""}`],
    ["Connection", esc(o.status === "Online" ? "Online" : `Offline · ${o.note.split(" · ")[0]}`)], ["Checked for releases", esc(o.checked)], ["Agents here", names.length ? esc(names.join(", ")) : "None"]];
}
function factsNode(k) { const t = document.createElement("template"); t.innerHTML = factList(factsRows(k)); return [t.content.firstElementChild]; }
const peersHtml = (k) => {
  const peers = PEERS[OFFICES[k].host] || [];
  return peers.length ? `<ul class="dev-list" role="list">${peers.map((p) => `<li><b>${esc(p.name)}</b><span class="proc-muted">${esc(p.version)} · ${esc(p.state)} · ${esc(p.note)}</span></li>`).join("")}</ul><p class="proc-lead">Updating ${esc(OFFICES[k].name)} never replaces their helpers, removes the release they use, deletes their data or sweeps their credentials.</p>`
    : `<p class="proc-lead">No other instance on ${esc(OFFICES[k].host)}.</p>`;
};
function openOffice(k, from) {
  const o = OFFICES[k], recent = HISTORY.filter((h) => h.office === k).slice(0, 3);
  const hist = recent.length ? `<ul class="dev-list" role="list">${recent.map((h) => `<li><b>${esc(h.from)} → ${esc(h.to)}</b><span class="proc-muted">${esc(h.at)} · ${esc(M[h.by]?.name || h.by)} · ${esc(h.result[0].toUpperCase() + h.result.slice(1))}</span></li>`).join("")}</ul>` : `<p class="proc-lead">No updates yet.</p>`;
  const cli = `<ul class="proc-instr-list is-compact" role="list"><li>${cmd(upgradeCmd(k))}<p class="proc-muted">Updates this instance from a terminal on ${esc(o.host)}.</p></li><li>${cmd(`agentic-org service status --name ${o.instance}`)}<p class="proc-muted">Shows the release it runs and whether its service is running.</p></li><li>${cmd(`agentic-org logs --name ${o.instance}`)}<p class="proc-muted">The instance's log, with secrets redacted.</p></li></ul>`;
  const body = `<div class="upd-panel-state" id="upd-panel-state">${panelState(k)}</div><div id="upd-panel-facts">${factList(factsRows(k))}</div><p class="proc-lead">${esc(POLICY)}</p>`
    + panelSection("upd-p-peers", `Other instances on ${esc(o.host)}`, peersHtml(k)) + panelSection("upd-p-cli", "On the machine", cli) + panelSection("upd-p-hist", "Recent updates", hist);
  openProcPanel({ eyebrow: `${ic("office", "ic16")}${o.role === "HQ" ? "HQ office" : "Office"} · instance ${esc(o.instance)}`, title: o.name }, body, from);
  procPanelElement().dataset.upd = k;
}

/* Logs and release panels ----------------------------------------------------------------------------- */
const logLines = (k) => (LOGS[k] || []).map(([t, lv, text]) => `<li data-lv="${lv}"><time>${esc(t)}</time><span class="upd-lv">${esc(lv)}</span><span>${esc(text)}</span></li>`).join("");
function openLogs(k, from) {
  const o = OFFICES[k];
  const body = `<p class="proc-lead">The latest update on ${esc(o.name)}, from instance ${cmd(o.instance)} on ${esc(o.host)}. Secrets are redacted before they're logged.</p>
    <ol class="upd-log" id="upd-panel-log" role="list" aria-label="Update log">${logLines(k)}</ol>
    <div class="proc-panel-actions"><button class="btn outline sm" type="button" data-upd-act="copy-log" data-id="${esc(k)}">${ic("copy", "ic16")}Copy log</button><button class="btn ghost sm" type="button" data-upd-act="open" data-id="${esc(k)}">${ic("office", "ic16")}Back to ${esc(o.name)}</button></div>`
    + panelSection("upd-l-cli", "Full log on the machine", `<p>${cmd(`agentic-org logs --name ${o.instance}`)}</p><p class="proc-muted">Run on ${esc(o.host)}. ${cmd(`agentic-org doctor --name ${o.instance}`)} checks the instance and says what to repair.</p>`);
  openProcPanel({ eyebrow: `${ic("terminal", "ic16")}Update log · ${esc(o.name)}`, title: `${o.name}: ${statusKey(k) === "failed" ? "failed update" : "latest update"}` }, body, from, true);
  procPanelElement().dataset.upd = k;
}
function openRelease(v, from) {
  const r = releaseOf(v); if (!r) return;
  const on = officeKeys().filter((k) => OFFICES[k].version === v).map((k) => OFFICES[k].name);
  const facts = factList([["Released", esc(r.date)], ["Release ID", `<code>${esc(r.id)}</code>`], ["Manifest", `<code>sha256 ${esc(r.sha)}…</code> · signature verified`], ["Organization level", String(r.level)], ["Running on", on.length ? esc(on.join(", ")) : "No office"]]);
  const notes = `<ul class="proc-instr-list" role="list">${r.notes.map((n) => `<li><p>${esc(n)}</p></li>`).join("")}</ul>`;
  const kind = r.kind === "security" ? pill("Security fix · recommended", "amber", "shield-alert") : r.kind === "level" ? pill("Raises organization level", "sky", "arrows-updown") : pill("Optional", "gray");
  openProcPanel({ eyebrow: `${ic("download", "ic16")}Release`, title: `Agentic Org ${r.version}`, badges: kind }, `<p class="proc-lead">${esc(r.summary)}</p>${facts}${panelSection("upd-r-notes", "What's new", notes)}`, from);
}

/* Clicks --------------------------------------------------------------------------------------------- */
function fromPanel(t, k) {
  if (!document.getElementById("proc-panel")?.contains(t)) return t;
  closeProcPanel(false);
  return document.querySelector(`#main [data-upd-act="open"][data-id="${CSS.escape(k)}"]`) || document.querySelector("#main h1");
}
function done(msg, k) { setSheetReturn(null); closeSheet(); rerender(k); if (msg) toast(msg, "check-circle"); }
const CLICK = {
  open: (k, t) => openOffice(k, document.getElementById("main").contains(t) ? t : null),
  release: (v, t) => openRelease(v, t),
  logs: (k, t) => openLogs(k, document.getElementById("main").contains(t) ? t : null),
  confirm: (k, t) => openConfirm(k, fromPanel(t, k)),
  "confirm-all": (_k, t) => openConfirmAll(t),
  start: (k) => { setSheetReturn(null); closeSheet(); start(k); rerender(k); document.querySelector(`#main [data-upd-act="open"][data-id="${CSS.escape(k)}"]`)?.focus(); },
  "start-all": () => { const list = updatable(); setSheetReturn(null); closeSheet(); pending.queue = list.slice(1); start(list[0]); rerender(list[0]); document.querySelector(`#main [data-upd-act="open"][data-id="${CSS.escape(list[0])}"]`)?.focus(); },
  queue: (k) => { OFFICES[k].update = "queued"; QUEUED[k] = { at: `Today ${nowTime()}`, by: "you" }; HISTORY.unshift({ at: `Today ${nowTime()}`, office: k, from: OFFICES[k].version, to: LATEST, result: "queued", by: "you", via: "Browser", detail: `${OFFICES[k].name} is offline. It updates when it connects.` }); done(`Update queued. ${OFFICES[k].name} updates when it connects.`, k); },
  unqueue: (k) => { OFFICES[k].update = "available"; delete QUEUED[k]; const h = HISTORY.find((x) => x.office === k && x.result === "queued"); if (h) Object.assign(h, { result: "cancelled", detail: `Cancelled before ${OFFICES[k].name} connected.` }); rerender(k); toast(`Queued update of ${OFFICES[k].name} cancelled.`, "check-circle"); },
  check: () => { const t = nowTime(); Object.values(OFFICES).forEach((o) => { if (o.status === "Online") o.checked = `Today ${t}`; }); render(false); document.querySelector('#main [data-upd-act="check"]')?.focus(); toast(`Checked at ${t}. ${LATEST} is the latest release.`, "check-circle"); },
  jump: (k, t, e) => { e.preventDefault(); const row = document.getElementById(`upd-row-${k}`); row?.scrollIntoView({ block: "center" }); row?.querySelector('[data-upd-act="open"]')?.focus({ preventScroll: true }); },
  "copy-log": (k) => { const text = (LOGS[k] || []).map((l) => l.join(" ")).join("\n"); navigator.clipboard?.writeText(text).catch(() => {}); toast("Log copied.", "check-circle"); },
};
document.addEventListener("click", (e) => {
  const t = e.target.closest?.("[data-upd-act]");
  if (!t || !Object.hasOwn(CLICK, t.dataset.updAct)) return;
  CLICK[t.dataset.updAct](t.dataset.id, t, e);
});
