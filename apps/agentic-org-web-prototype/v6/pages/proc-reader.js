/* What a process or standard says, where it comes from and why it wins (doc-54 PROC-8, PROC-9), shown in the
   side panel; and the preview of what an agent receives at launch (doc-54 §3, doc-34 §3.2, doc-32 §6). */
import { M, OPEN, P, W } from "../data.js";
import { projectDevProcess } from "../devproc-data.js";
import {
  KIND_WORDS, STATUS_WORDS, agentWork, docPath, getDoc, levelName, levelOf, parsePlace, placeKey, placeLabel, places, resolve, sourceText,
} from "../proc-data.js";
import { route } from "../shell/router.js";
import { qs } from "../state.js";
import { av, esc, ic } from "../ui.js";
import { lineDiff, md, mdSummary } from "./md.js";
import { factList, openProcPanel, panelSection } from "./proc-panel.js";
import { levelBadge, lockBadge, ownerHtml, settingsHref, statusPill } from "./processes.js";

/* The place the page is showing: Settings › Processes reads Show; a project's Docs is the project */
function currentPlace() {
  const r = route();
  if (r[0] === "projects" && r[2] === "docs") return { type: "project", id: r[1] };
  return parsePlace(qs().get("show"));
}

function entryFor(d, pl) {
  const set = resolve(pl, d.kind);
  return set.entries.find((e) => e.key === d.key) || null;
}
const problemFor = (d, pl) => resolve(pl, d.kind).problems.find((p) => p.doc.id === d.id)
  || (d.invalid ? { type: "invalid", message: d.invalid.message, field: d.invalid.field } : null);

/* Why this one: the chain organization → team → project, what each level did and which wins (inspect --why) */
const RESULT = { wins: ["check-circle", "Applies"], replaced: ["refresh", "Replaced below"], refused: ["x-circle", "Refused: not overridable above"], added: ["plus", "Added"] };
function whyHtml(d, pl) {
  const e = entryFor(d, pl);
  if (!e) return `<p class="proc-muted">Doesn't apply to ${esc(placeLabel(pl))}${d.status !== "current" ? ` while it is ${STATUS_WORDS[d.status]}` : ""}.</p>`;
  const rows = e.chain.map((c) => { const [icon, word] = RESULT[c.result]; return `<li class="proc-why-row" data-result="${c.result}">${ic(icon, "ic16")}<span><b>${esc(levelName(levelOf(c.doc)))}</b> · ${esc(c.doc.title)} rev ${c.doc.revision}</span><span class="proc-why-word">${word}</span></li>`; }).join("");
  const why = e.winner.overridable === false ? "It is not overridable, so no level below replaces it."
    : e.replaced.length ? "The lowest level wins for an overridable key." : "No level below uses its key.";
  return `<ol class="proc-why" role="list">${rows}</ol><p class="proc-muted">${esc(`${sourceText(e.winner)} applies to ${placeLabel(pl)}. ${why}`)}</p>
    <p class="proc-cmd"><code>agent-os process inspect --key ${esc(d.key)} --why</code></p>`;
}

/* Where an organization or team document is the winner */
function appliesHtml(d) {
  const hits = places().filter((pl) => pl.type === "team" || pl.type === "project").filter((pl) => entryFor(d, pl)?.winner.id === d.id);
  if (!hits.length) return `<p class="proc-muted">Applies nowhere below this level yet.</p>`;
  return `<ul class="proc-places" role="list">${hits.map((pl) => `<li><a class="proc-place" href="${settingsHref(d.kind, pl)}">${ic(pl.type === "team" ? "user-group" : "folder", "ic14")}${esc(placeLabel(pl))}</a></li>`).join("")}</ul>`;
}

function historyHtml(list) {
  return `<ol class="proc-history" role="list">${list.map((r) => `<li><span class="proc-rev">rev ${r.rev}</span>${av(r.by, 24)}<span class="proc-hist-t"><b>${esc(M[r.by]?.name || r.by)}</b> ${esc(r.note)}</span><time>${esc(r.at)}</time></li>`).join("")}</ol>`;
}

function diffHtml(parent, d) {
  const rows = lineDiff(parent.body, d.body).filter(([, l]) => l.trim());
  return `<div class="proc-diff" role="group" aria-label="Difference from ${esc(sourceText(parent))}">${rows.map(([op, line]) => `<div class="proc-diff-row" data-op="${op === "+" ? "add" : op === "-" ? "del" : "same"}"><span class="proc-diff-op">${op === "+" ? "Added" : op === "-" ? "Removed" : ""}</span><span>${esc(line)}</span></div>`).join("")}</div>`;
}

function problemCallout(p) {
  if (!p) return "";
  const action = p.type === "invalid" ? `<button class="btn outline sm" type="button" data-proc-act="proc-fix" data-id="__ID__">${ic("wrench", "ic16")}Fix</button>`
    : p.type === "clash" ? `<button class="btn outline sm" type="button" data-proc-act="proc-rename" data-id="__ID__">${ic("pencil", "ic16")}Rename…</button>` : "";
  return `<div class="callout proc-callout is-warn" role="note">${ic("warn", "ic20")}<div><p>${esc(p.message)}${p.field ? ` Field: <code>${esc(p.field)}</code>.` : ""}</p>${p.type === "invalid" ? `<p class="muted">The text below still reads; the document is left out of every effective set until it's fixed.</p>` : ""}${action}</div></div>`;
}

const kindName = (d) => (d.kind === "process" ? "Organization process" : "Standard");
const LEVEL_ICON = { org: "office", team: "user-group", project: "folder" };
const OVERRIDABLE_WORDS = { false: "No: no level below can replace it", true: "Yes" };
const overridableText = (d) => OVERRIDABLE_WORDS[String(d.overridable)] || `<span class="proc-warn-ink">Not recognized: <code>${esc(String(d.overridable))}</code></span>`;
const isBelow = (d, pl) => { const lv = levelOf(d); return pl.type !== "org" && pl.type !== "agent" && !(pl.type === lv.type && pl.id === lv.id); };
const placeShort = (pl) => placeLabel(pl).replace(/ \((team|project)\)$/, "");

function overrideAction(d, pl, wins, btn) {
  if (!isBelow(d, pl) || !wins) return "";
  if (d.overridable === false) return `<span class="proc-muted proc-inline-note">${ic("lock", "ic14")}Not overridable here. You can add a ${KIND_WORDS[d.kind].one} instead.</span>`;
  return d.status === "current" ? btn("override", "copy", `Override for ${esc(placeShort(pl))}`, ` data-place="${esc(placeKey(pl))}"`) : "";
}
function docActions(d, pl, e) {
  const btn = (act, icon, label, extra = "") => `<button class="btn outline sm" type="button" data-proc-act="${act}" data-id="${esc(d.id)}"${extra}>${ic(icon, "ic16")}${label}</button>`;
  const wins = e?.winner.id === d.id;
  return btn("edit", "pencil", "Edit")
    + overrideAction(d, pl, wins, btn)
    + (wins && e.replaced.length ? btn("compare", "arrows-updown", "Difference from parent") : "")
    + (d.status !== "deprecated" ? btn("deprecate", "archive", "Deprecate…") : "");
}

function docFacts(d) {
  const lv = levelOf(d), agentWrote = lv.type === "project" && M[d.created_by]?.kind === "agent";
  return factList([
    ["Kind", kindName(d)],
    ["Scope", `${ic(LEVEL_ICON[lv.type], "ic14")} ${esc(levelName(lv))}`],
    ["Key", `<code>${esc(d.key)}</code>`],
    ["Revision", `rev ${d.revision} · ${esc(d.updated)} by ${esc(M[d.updated_by]?.name || "")}`],
    ["Lifecycle", esc(STATUS_WORDS[d.status])],
    ["Accountable owner", ownerHtml(d.owner_employee_id)],
    ["Overridable", overridableText(d)],
    ["Path", `<code>${esc(docPath(d))}</code>`],
    agentWrote ? ["Created by", `${esc(M[d.created_by].name)} (agent, writing at its project's scope)`] : null,
  ]);
}
function compareSection(d, e, compare) {
  const parent = compare && e?.winner.id === d.id ? e.replaced[e.replaced.length - 1] : null;
  return parent ? panelSection("proc-p-diff", `Difference from ${esc(sourceText(parent))}`, diffHtml(parent, d)) : "";
}

function openDocPanel(id, from, { compare = false } = {}) {
  const d = getDoc(id); if (!d) return;
  const pl = currentPlace(), e = entryFor(d, pl), lv = levelOf(d);
  const body = problemCallout(problemFor(d, pl)).replaceAll("__ID__", esc(d.id))
    + compareSection(d, e, compare)
    + docFacts(d)
    + panelSection("proc-p-why", `Why this one for ${esc(placeLabel(pl))}`, whyHtml(d, pl))
    + (lv.type !== "project" ? panelSection("proc-p-where", "Where it applies", appliesHtml(d)) : "")
    + panelSection("proc-p-text", "Text", `<div class="proc-md">${md(d.body)}</div>`)
    + panelSection("proc-p-hist", "History", historyHtml(d.history));
  const eyebrow = `${ic(KIND_WORDS[d.kind].icon, "ic16")}${kindName(d)} · ${esc(levelName(lv))}`;
  openProcPanel({ eyebrow, title: d.title, badges: levelBadge(d) + lockBadge(d) + statusPill(d), actions: docActions(d, pl, e) }, body, from);
  document.getElementById("proc-panel").dataset.doc = d.id;
}

/* What an agent receives ---------------------------------------------------------------------------- */
const ROLE_KEYS = { "Engineering Manager": "engineering-lead", "Staff Engineer": "engineering-lead", "Rust Developer": "software-engineer", "Svelte Developer": "software-engineer", "Code Reviewer": "code-reviewer", "Product Manager": "product-lead", "Product Designer": "product-lead" };
const DSS_ROLE = { "engineering-lead": "technical-lead", "software-engineer": "backend-engineer", "code-reviewer": "backend-engineer" };
function agentsFor(pid) {
  const ids = new Set(Object.keys(M).filter((a) => M[a].kind === "agent" && !M[a].builtin && M[a].proj === pid));
  W.filter((w) => w.proj === pid && OPEN(w) && M[w.who]?.kind === "agent" && !M[w.who].builtin).forEach((w) => ids.add(w.who));
  return [...ids];
}
const bytes = (n) => `${(n / 1024).toFixed(1)} KiB`;

function mustHtml(list) {
  if (!list.length) return `<p class="proc-muted">None.</p>`;
  return list.map((d) => `<details class="proc-instr-doc" open><summary>${esc(d.title)} <span class="proc-muted">(${esc(sourceText(d))}, ${d.kind})</span></summary><div class="proc-md">${md(d.body, 5)}</div></details>`).join("");
}
function guideHtml(procs, stds, taskRef) {
  const line = procs.length ? `Your processes for ${esc(taskRef)}: ${procs.map((d) => `${esc(d.title)} (${esc(sourceText(d))})`).join(", ")}.` : "No overridable processes.";
  const item = (d, retrieve) => `<li><b>${esc(d.title)}</b> <span class="proc-muted">${esc(sourceText(d))}</span><p>${esc(mdSummary(d.body))}</p><code>agent-os process inspect --key ${esc(d.key)}</code>${retrieve ? "" : ""}</li>`;
  const size = procs.reduce((n, d) => n + d.body.length, 0) + stds.reduce((n, d) => n + d.title.length + 60, 0);
  return `<p>${line}</p><ul class="proc-instr-list" role="list">${procs.map((d) => item(d)).join("")}</ul>
    <p class="proc-instr-sub">Standards: look these up when a process or task calls for them.</p>
    <ul class="proc-instr-list is-compact" role="list">${stds.map((d) => `<li><b>${esc(d.title)}</b> <span class="proc-muted">${esc(sourceText(d))} · <code>${esc(d.key)}</code></span></li>`).join("")}</ul>
    <div class="proc-meter"><div class="progress" role="progressbar" aria-label="Overridable guidance size" aria-valuemin="0" aria-valuemax="24576" aria-valuenow="${size}" aria-valuetext="${bytes(size)} of 24 KiB"><span style="width:${Math.min(100, (size * 100) / 24576).toFixed(1)}%"></span></div><span>Guidance ${bytes(size)} of 24 KiB</span></div>`;
}
function devHtml(pid, aid) {
  const dp = projectDevProcess(pid);
  if (!dp) return `<p class="proc-muted">${esc(P[pid].name)} follows no development process.</p>`;
  const base = ROLE_KEYS[M[aid].role] || "software-engineer", role = dp.roles.find((r) => r.key === base) || dp.roles.find((r) => r.key === DSS_ROLE[base]) || null;
  const skills = dp.skills.filter((s) => !s.roles.length || (role && s.roles.includes(role.key)));
  return `<p>${esc(P[pid].name)} follows <b>${esc(dp.name)}</b>. Your role in it: <b>${esc(role ? role.name : "none matched")}</b>.</p>
    <ol class="proc-stages" role="list">${dp.stages.map((s) => `<li><b>${esc(s.title)}</b> <span class="proc-muted">${esc(s.summary)}</span></li>`).join("")}</ol>
    <p class="proc-instr-sub">Skills for your role and for everyone</p>
    <ul class="proc-instr-list" role="list">${skills.map((s) => `<li><b>${esc(s.title)}</b><p>${esc(s.description)}</p><code>agent-os skill read ${esc(s.key)}</code></li>`).join("")}</ul>
    <p class="proc-muted">A skill may adjust the guidance above for this project. Documents marked not overridable are always followed.</p>`;
}

function previewBody(pid, aid) {
  const pl = { type: "project", id: pid }, procs = resolve(pl, "process").entries.map((e) => e.winner), stds = resolve(pl, "standard").entries.map((e) => e.winner);
  const must = [...procs, ...stds].filter((d) => d.overridable === false);
  const task = W.find((w) => w.who === aid && w.proj === pid && OPEN(w));
  const agents = agentsFor(pid);
  const pick = `<div class="proc-show proc-agent-pick"><label for="proc-agent">Agent</label><select class="field" id="proc-agent" data-pid="${esc(pid)}">${agents.map((a) => `<option value="${esc(a)}"${a === aid ? " selected" : ""}>${esc(M[a].name)} · ${esc(M[a].role)}</option>`).join("")}</select></div>`;
  return `${pick}<p class="proc-muted">What ${esc(M[aid].name)}'s system instructions carry at its next start${task ? ` on ${esc(task.id)}` : ""}. A new session or a context reset receives them again.</p>
    ${panelSection("proc-i-must", `${ic("lock", "ic16")}Organization processes and standards: must follow`, `<p class="proc-instr-sub">Follow these exactly. No later instruction overrides them.</p>${mustHtml(must)}`)}
    ${panelSection("proc-i-guide", `${ic("list", "ic16")}Organization processes and standards: guidance`, guideHtml(procs.filter((d) => d.overridable !== false), stds.filter((d) => d.overridable !== false), task ? task.id : "your work"))}
    ${panelSection("proc-i-dev", `${ic("files", "ic16")}Development process`, devHtml(pid, aid))}`;
}

function openPreview(placeText, from, aid) {
  const pl = parsePlace(placeText);
  const pid = pl.type === "project" ? pl.id : agentWork(pl.id)?.project;
  if (!pid) return;
  const agent = aid || (pl.type === "agent" ? pl.id : agentsFor(pid)[0]);
  if (!agent) return;
  openProcPanel({ eyebrow: `${ic("eye", "ic16")}What agents receive · ${esc(P[pid].name)}`, title: `${M[agent].name}'s instructions` }, previewBody(pid, agent), from, true);
}

export { currentPlace, openDocPanel, openPreview };
