import { IN, M, OFFICES, OPEN, P, STATUSES, TEAMS, W } from "../data.js";
import { own, render, route } from "../shell/router.js";
import { S } from "../state.js";
import { av, esc, ic, memberIds, toast } from "../ui.js";
import { persistCreatedWorkTask, workHref, workSelection } from "./work.js";

const TASK_TYPES = [
  ["task", "Task"],
  ["bug", "Bug"],
  ["request", "Request"],
];
const PRIORITIES = ["Urgent", "High", "Normal", "Low"];
const SEVERITIES = ["Critical", "Major", "Minor", "Trivial"];

const AGENT_ROUTING_RULES = [
  { re: /\b(rust|sync|codec|lock|office|cert|sqlite|db|api|backend|snapshot|daemon|cli)\b/i, ids: ["kai", "sage", "rowan"] },
  { re: /\b(ui|svelte|css|theme|design|layout|modal|toolbar|spacing|ux|contrast|pill|button|composer)\b/i, ids: ["eden", "jules", "avery"] },
  { re: /\b(review|audit|qa|test|regression|verify|check)\b/i, ids: ["tomas", "kai"] },
  { re: /\b(plan|spec|roadmap|backlog|priorit|prototype|ia|workflow)\b/i, ids: ["emery", "ada"] },
  { re: /\b(release|notariz|sign-off|deploy|ship|coordinate|apple)\b/i, ids: ["ada", "maya"] },
];

const COMPLEX_INTAKE_RE = /\b(design|architect|policy|feature|workflow|lifecycle|refactor|sync|federation|migration|explore|proposal|strategy|system|redesign|research|refine|multi|permission|security|archive|ideation|draft|inbox|routing)\b/i;

let STC = null;
const tessIntakes = [];

function activeProjects() {
  return Object.entries(P).filter(([, pr]) => !pr.archived);
}

function defaultTeamForProject(projId) {
  return projId && projId !== "none" && P[projId]?.teams?.length ? P[projId].teams[0] : "leadership";
}

function memberAvailability(id) {
  const m = M[id];
  if (!m) return { state: "unassigned", label: "Unassigned", tone: "neutral", ready: false };
  if (m.kind === "person") {
    return { state: "person", label: m.live === "available" ? "Available" : "Offline", tone: "neutral", ready: true };
  }
  if (m.paused) return { state: "paused", label: "Paused", tone: "warn", ready: false };
  if (m.budgetCapped) return { state: "capped", label: "Budget cap", tone: "bad", ready: false };
  if (m.setup?.needs) return { state: "unplaced", label: "Unplaced", tone: "warn", ready: false };
  const office = m.setup?.office ? OFFICES[m.setup.office] : null;
  if (office?.status === "Offline") return { state: "offline-office", label: "Office offline", tone: "bad", ready: false };
  if (m.live === "working" || m.live === "withyou") return { state: "working", label: "Working", tone: "accent", ready: true };
  return { state: "available", label: "Available", tone: "ok", ready: true };
}

function isDispatchReadyAgent(id) {
  const m = M[id];
  return !!(m && m.kind === "agent" && !m.builtin && !m.offboarded && memberAvailability(id).ready);
}

function bestAvailableFallback(projId, excludeId) {
  const projTeams = projId && projId !== "none" ? (P[projId]?.teams || []) : [];
  const candidates = memberIds().filter((id) => id !== excludeId && isDispatchReadyAgent(id));
  return candidates.find((id) => M[id].teams.some((t) => projTeams.includes(t))) || candidates[0] || "ada";
}

function tessInferType(text, pinned) {
  if (pinned) return pinned;
  if (/\b(bug|broken|fix|crash|fail|error|offline|truncat|scroll|stuck|leak|contrast|regression|wrong)\b/i.test(text)) return "bug";
  if (/\b(request|proposal|explore|consider|should we|invite flow|export)\b/i.test(text)) return "request";
  return "task";
}

function tessInferPriority(text, kind, pinned) {
  if (pinned) return pinned;
  if (/\b(urgent|asap|p0|outage|critical|blocker|blocking)\b/i.test(text)) return "Urgent";
  if (/\b(high|important|release|gate|security|lock|sync|notariz)\b/i.test(text) || kind === "bug") return "High";
  if (/\b(low|minor|typo|cleanup|polish|later)\b/i.test(text)) return "Low";
  return "Normal";
}

function tessInferSeverity(text, pinned) {
  if (pinned) return pinned;
  if (/\b(critical|crash|data loss|offline|blocker|security)\b/i.test(text)) return "Critical";
  if (/\b(minor|typo|cosmetic|spacing)\b/i.test(text)) return "Minor";
  if (/\b(trivial)\b/i.test(text)) return "Trivial";
  return "Major";
}

function tessCleanTitle(text) {
  let clean = text.trim().split(/\r?\n/)[0] || "";
  clean = clean.replace(/@[A-Za-z]+(?:\s+[A-Za-z]+)?/g, "").replace(/\s+/g, " ").trim();
  clean = clean.replace(/^(please|can we|could we|we need to|i want to|i need to|let's|lets|help me)\s+/i, "");
  clean = clean.replace(/\s+(?:and\s+)?(?:loop in|ask|tag|cc|assign(?:\s+to)?|with|for)\s*$/i, "");
  clean = clean.replace(/[.!?,;:\s]+$/, "");
  if (!clean) clean = text.trim().slice(0, 72);
  if (clean.length > 78) clean = clean.slice(0, 75).replace(/\s+\S*$/, "") + "…";
  return clean.charAt(0).toUpperCase() + clean.slice(1);
}

function tessBuildDescription(text, kind, projId) {
  const trimmed = text.trim().replace(/\s+/g, " ");
  const projName = projId && projId !== "none" && P[projId] ? P[projId].name : "team backlog";
  if (trimmed.length > 90) return trimmed;
  if (kind === "bug") return `${trimmed}. Investigate root cause in ${projName}, ship a clean fix, and add regression coverage.`;
  if (kind === "request") return `${trimmed}. Scope requirements for ${projName}, confirm trade-offs, and align on an implementation plan.`;
  return `${trimmed}. Deliver end-to-end in ${projName} with verification checks passing.`;
}

function tessBuildCriteria(text, kind) {
  if (kind === "bug") {
    return [
      { text: "Reproduce the failure path and isolate root cause", done: false },
      { text: "Ship fix without regressions in adjacent flows", done: false },
      { text: "Add automated regression test", done: false },
    ];
  }
  if (/\b(ui|css|theme|modal|toolbar|layout|button|spacing|design)\b/i.test(text)) {
    return [
      { text: "Matches visual spec across Cozy Glass, Ocean, and Report Day", done: false },
      { text: "Keyboard navigation, focus states, and mobile layout verified", done: false },
      { text: "Browser smoke suite passes with zero errors", done: false },
    ];
  }
  return [
    { text: "Core behavior implemented against project contract", done: false },
    { text: "Edge cases and error states handled cleanly", done: false },
    { text: "Automated checks and verification pass", done: false },
  ];
}

function tessPickAssignee(text, projId, mentions, pinned) {
  if (pinned === "unassigned") return null;
  if (pinned && M[pinned]) return pinned;
  if (mentions.length) return mentions[0];
  for (const rule of AGENT_ROUTING_RULES) {
    if (!rule.re.test(text)) continue;
    const ready = rule.ids.find(isDispatchReadyAgent);
    if (ready) return ready;
  }
  return bestAvailableFallback(projId, null);
}

function tessPickRefinementLead(text, projId, mentions, pinned) {
  if (pinned && pinned !== "unassigned" && M[pinned]) return pinned;
  if (mentions.length) return mentions[0];
  if (/\b(ui|ux|design|layout|theme|modal|spacing|button)\b/i.test(text) && isDispatchReadyAgent("eden")) return "eden";
  const projLead = projId && projId !== "none" ? P[projId]?.lead : null;
  if (projLead && isDispatchReadyAgent(projLead)) return projLead;
  return "ada";
}

function isComplexIntake(prompt, kind, mentions) {
  const trimmed = prompt.trim();
  if (/\b(quick|typo|rename label|simple)\b/i.test(trimmed) && trimmed.length < 60) return false;
  return trimmed.length >= 48 || kind === "request" || mentions.length >= 2 || COMPLEX_INTAKE_RE.test(trimmed);
}

function seedIdeationComments(leadId, prompt, projId, mentions) {
  const lead = M[leadId] || M.ada;
  const projName = P[projId]?.name || "this project";
  const pmNote = leadId === "emery" ? "" : " I've looped in @Emery (PM) to align scope.";
  const tagged = mentions.filter((id) => id !== leadId && M[id]).map((id) => "@" + M[id].name.split(" ")[0]).join(", ");
  const tagNote = tagged ? ` Also tagging ${tagged} from your note.` : pmNote;
  const now = Date.now();
  return [
    {
      id: "ic-1",
      who: "tess",
      at: now - 1000,
      text: `Routed intake to @${lead.name.split(" ")[0]} (${lead.role}) for ${projName} refinement. Original request: “${prompt.trim()}”.`,
    },
    {
      id: "ic-2",
      who: leadId,
      at: now,
      text: `Taking this in Draft to refine before execution.${tagNote} Review the initial criteria below or add constraints here — once we click Materialize & Publish, this ideation thread folds out of executing agent context.`,
    },
  ];
}

function addDraftInboxNotice(created, leadId) {
  const inboxId = "in-draft-" + created.id.toLowerCase();
  IN.unshift({
    id: inboxId,
    group: "Today",
    kind: "question",
    sub: "Draft refinement",
    from: leadId,
    proj: created.proj,
    task: created.id,
    time: "Just now",
    what: `Draft ready to refine: ${created.title}`,
    why: `Tess routed your request to ${M[leadId]?.name || "the project lead"} in Draft mode. Discuss details or click Materialize & Publish on ${created.id} when ready.`,
    rec: `Open ${created.id} to review criteria, add ideation notes, or click Materialize & Publish to move it into To do.`,
    recTone: "ok",
    src: { href: `#/work?item=${created.id}`, label: `Open draft ${created.id}` },
    handled: false,
  });
  return inboxId;
}

function renderTessIntakeTray() {
  let tray = document.getElementById("tess-intake-tray");
  if (!tessIntakes.length) { tray?.remove(); return; }
  if (!tray) {
    tray = document.createElement("aside");
    tray.id = "tess-intake-tray";
    tray.className = "tess-intake-tray";
    tray.setAttribute("aria-label", "Tess background task intake");
    document.body.appendChild(tray);
    tray.addEventListener("click", handleTrayClick);
  }
  const latest = tessIntakes[0];
  const activeCount = tessIntakes.filter((i) => i.state === "processing").length;
  if (activeCount > 0) {
    const label = activeCount > 1 ? `Tess refining ${activeCount} tasks…` : `Tess refining: “${latest.shortTitle}”`;
    tray.innerHTML = `<div class="tess-pill is-busy">
      <span class="tess-pill-dot" aria-hidden="true"></span>
      ${ic("sparkle", "ic14")}
      <span class="tess-pill-txt">${esc(label)}</span>
    </div>`;
    return;
  }
  const routeTxt = latest.mode === "draft"
    ? `Draft ${latest.taskId} with ${M[latest.leadId]?.name || "lead"}`
    : `Filed ${latest.taskId}${latest.who && M[latest.who] ? " · " + M[latest.who].name : ""}`;
  tray.innerHTML = `<div class="tess-pill is-done" data-mode="${latest.mode}">
    ${ic(latest.mode === "draft" ? "pencil" : "check-circle", "ic14")}
    <span class="tess-pill-txt">${esc(routeTxt)}</span>
    <a class="tess-pill-link" href="${esc(latest.href)}" data-tray-act="open">${latest.mode === "draft" ? "Open draft" : "Open"}</a>
    <button class="tess-pill-x" type="button" data-tray-act="dismiss" aria-label="Dismiss">${ic("x", "ic14")}</button>
  </div>`;
}

function handleTrayClick(e) {
  const act = e.target.closest("[data-tray-act]")?.dataset.trayAct;
  if (act === "dismiss") {
    tessIntakes.length = 0;
    renderTessIntakeTray();
  } else if (act === "open") {
    setTimeout(() => { tessIntakes.length = 0; renderTessIntakeTray(); }, 150);
  }
}

function finalizeAsyncIntake(intake) {
  const kind = tessInferType(intake.prompt, intake.pinnedType);
  const pri = tessInferPriority(intake.prompt, kind, intake.pinnedPriority);
  const severity = kind === "bug" ? tessInferSeverity(intake.prompt, intake.pinnedSeverity) : null;
  const proj = intake.proj === "none" ? activeProjects()[0][0] : intake.proj;
  const title = tessCleanTitle(intake.prompt);
  const complex = isComplexIntake(intake.prompt, kind, intake.mentions);

  if (complex) {
    const leadId = tessPickRefinementLead(intake.prompt, proj, intake.mentions, intake.pinnedAssignee);
    const created = persistCreatedWorkTask({
      title,
      description: tessBuildDescription(intake.prompt, kind, proj),
      kind,
      proj,
      who: leadId,
      requestedBy: "you",
      status: "Draft",
      pri: kind === "bug" && severity ? severity : pri,
      etaHours: null,
      criteria: tessBuildCriteria(intake.prompt, kind),
      ideationComments: seedIdeationComments(leadId, intake.prompt, proj, intake.mentions),
      draftPrompt: intake.prompt.trim(),
    });
    addDraftInboxNotice(created, leadId);
    Object.assign(intake, { state: "done", mode: "draft", taskId: created.id, leadId, href: `#/work?item=${created.id}` });
    toast(`Draft ${created.id} opened with ${M[leadId]?.name || "lead"} · Ready to refine`, "sparkle");
  } else {
    const who = tessPickAssignee(intake.prompt, proj, intake.mentions, intake.pinnedAssignee);
    const status = kind === "bug" ? STATUSES.bug[0] : STATUSES.task[0];
    const created = persistCreatedWorkTask({
      title,
      description: tessBuildDescription(intake.prompt, kind, proj),
      kind,
      proj,
      who,
      requestedBy: "you",
      status,
      pri: kind === "bug" && severity ? severity : pri,
      etaHours: 2,
      criteria: tessBuildCriteria(intake.prompt, kind),
    });
    Object.assign(intake, { state: "done", mode: "fast", taskId: created.id, who, href: `#/work?item=${created.id}` });
    toast(`Tess filed ${created.id}${who && M[who] ? ` · Assigned to ${M[who].name}` : ""}`, "check-circle");
  }
  render(false);
  renderTessIntakeTray();
}

function enqueueTessIntake(state) {
  const shortTitle = tessCleanTitle(state.prompt).slice(0, 34);
  const intake = {
    id: "intake-" + Date.now(),
    state: "processing",
    shortTitle,
    prompt: state.prompt,
    proj: state.proj,
    team: state.team,
    mentions: [...state.mentions],
    pinnedAssignee: state.pinnedAssignee,
    pinnedType: state.pinnedType,
    pinnedPriority: state.pinnedPriority,
    pinnedSeverity: state.pinnedSeverity,
  };
  tessIntakes.unshift(intake);
  renderTessIntakeTray();
  toast("Sent to Tess — refining in background", "sparkle");
  const delay = isComplexIntake(intake.prompt, intake.pinnedType || "task", intake.mentions) ? 1400 : 900;
  setTimeout(() => finalizeAsyncIntake(intake), delay);
}

function openUnassignedTasks() {
  return W.filter((w) => OPEN(w) && !w.who);
}

function renderScopeHeader(state) {
  const projLabel = state.proj === "none" ? "No project" : (P[state.proj]?.name || "Project");
  const teamLabel = TEAMS[state.team]?.name || "Team";
  const teamChip = state.proj === "none" ? `
    <div class="stc-menu-anchor">
      <button class="stc-scope-btn" type="button" data-stc="menu" data-menu="team" aria-expanded="${state.openMenu === "team"}">
        ${ic("user-group", "ic14")}<span>${esc(teamLabel)}</span>${ic("chev-down", "ic14")}
      </button>
      ${state.openMenu === "team" ? renderPropertyMenu(state, "team") : ""}
    </div>` : "";
  const unassigned = state.context.agentId ? openUnassignedTasks() : [];
  const assignSwitch = state.context.agentId && unassigned.length ? `
    <button class="stc-head-link" type="button" data-stc="toggle-assign-existing">
      ${state.step === "assign-existing" ? "← New intake" : `Assign existing (${unassigned.length})`}
    </button>` : "";
  const badge = state.context.agentId && M[state.context.agentId]
    ? `<span class="stc-head-badge">For ${esc(M[state.context.agentId].name)}</span>`
    : `<span class="stc-head-badge">${ic("sparkle", "ic14")}Tess intake</span>`;
  return `<div class="stc-head">
    <div class="stc-head-left">
      <div class="stc-menu-anchor">
        <button class="stc-scope-btn" type="button" data-stc="menu" data-menu="proj" aria-expanded="${state.openMenu === "proj"}">
          ${ic("folder", "ic14")}<span>${esc(projLabel)}</span>${ic("chev-down", "ic14")}
        </button>
        ${state.openMenu === "proj" ? renderPropertyMenu(state, "proj") : ""}
      </div>
      ${teamChip}
      ${assignSwitch}
    </div>
    <div class="stc-head-right">
      ${badge}
      <button class="btn ghost icon stc-close" type="button" data-stc="request-close" aria-label="Close" title="Close (Esc)">${ic("x", "ic16")}</button>
    </div>
  </div>`;
}

function renderMentionStrip(state) {
  if (!state.mentions.length) return "";
  return `<div class="stc-mentions" aria-label="Tagged members">${state.mentions.map((id) => {
    const m = M[id];
    if (!m) return "";
    return `<span class="stc-mention-pill">${av(id, 16)}<span>@${esc(m.name.split(" ")[0])}</span><span class="stc-mention-role">${esc(m.role)}</span><button type="button" data-stc="rm-mention" data-id="${esc(id)}" aria-label="Remove ${esc(m.name)}">${ic("x", "ic14")}</button></span>`;
  }).join("")}</div>`;
}

function renderCaptureOverrides(state) {
  const whoLabel = state.pinnedAssignee === "unassigned" ? "Unassigned"
    : state.pinnedAssignee && M[state.pinnedAssignee] ? M[state.pinnedAssignee].name : "Auto (Tess)";
  const typeLabel = state.pinnedType ? TASK_TYPES.find(([k]) => k === state.pinnedType)?.[1] : "Auto";
  const priLabel = state.pinnedPriority || "Auto";
  const chips = [
    { key: "assignee", prefix: "Assignee", val: whoLabel, pinned: !!state.pinnedAssignee, avatar: state.pinnedAssignee && M[state.pinnedAssignee] ? state.pinnedAssignee : null },
    { key: "type", prefix: "Type", val: typeLabel, pinned: !!state.pinnedType },
    { key: "priority", prefix: "Priority", val: priLabel, pinned: !!state.pinnedPriority },
    ...(state.pinnedType === "bug" ? [{ key: "severity", prefix: "Severity", val: state.pinnedSeverity || "Auto", pinned: !!state.pinnedSeverity }] : []),
  ];
  return `<div class="stc-overrides" role="group" aria-label="Optional task overrides">${chips.map((c) => `
    <div class="stc-menu-anchor">
      <button class="stc-chip${c.pinned ? " is-pinned" : ""}" type="button" data-stc="menu" data-menu="${c.key}" aria-expanded="${state.openMenu === c.key}">
        ${c.avatar ? av(c.avatar, 16) : ""}<span class="stc-chip-k">${c.prefix}:</span><span class="stc-chip-v">${esc(c.val)}</span>${ic("chev-down", "ic14")}
      </button>
      ${state.openMenu === c.key ? renderPropertyMenu(state, c.key) : ""}
    </div>`).join("")}</div>`;
}

function renderRoutingPreviewLine(state) {
  const trimmed = state.prompt.trim();
  if (!trimmed) {
    return `<p class="stc-route-preview">${ic("sparkle", "ic14")}<span>Simple asks file immediately; complex work opens a <strong>Draft</strong> with your project lead for ideation.</span></p>`;
  }
  const kind = tessInferType(trimmed, state.pinnedType);
  const complex = isComplexIntake(trimmed, kind, state.mentions);
  if (complex) {
    const leadId = tessPickRefinementLead(trimmed, state.proj, state.mentions, state.pinnedAssignee);
    return `<p class="stc-route-preview is-draft">${ic("pencil", "ic14")}<span>Complex scope detected — Tess will open a <strong>Draft</strong> with <strong>${esc(M[leadId]?.name || "the lead")}</strong> in the background and notify Inbox.</span></p>`;
  }
  const who = tessPickAssignee(trimmed, state.proj, state.mentions, state.pinnedAssignee);
  const whoTxt = who && M[who] ? ` and assign to <strong>${esc(M[who].name)}</strong>` : "";
  return `<p class="stc-route-preview is-fast">${ic("bolt", "ic14")}<span>Straightforward ask — Tess will file this directly in the background${whoTxt}.</span></p>`;
}

function renderCaptureStep(state) {
  const errHtml = state.error ? `<p class="stc-inline-err" role="alert">${esc(state.error)}</p>` : "";
  return `<div class="stc-body stc-capture">
    <div class="stc-prompt-wrap">
      <label class="sr-only" for="stc-prompt">Describe what you want done</label>
      <textarea class="stc-prompt" id="stc-prompt" rows="4" maxlength="4000" placeholder="Describe what needs to happen… Type @ to tag an agent">${esc(state.prompt)}</textarea>
      ${renderMentionStrip(state)}
    </div>
    ${errHtml}
    ${renderCaptureOverrides(state)}
    ${renderRoutingPreviewLine(state)}
  </div>`;
}

function renderAssignExistingStep(state) {
  const agent = M[state.context.agentId];
  const tasks = openUnassignedTasks();
  return `<div class="stc-body stc-existing">
    <p class="stc-existing-lead">Assign an open unassigned work item directly to <strong>${esc(agent?.name || "agent")}</strong>:</p>
    ${tasks.length ? `<ul class="stc-existing-list" role="list">${tasks.map((w) => `<li>
      <button class="stc-existing-item" type="button" data-stc="assign-existing-pick" data-id="${esc(w.id)}">
        <span class="mono">${esc(w.id)}</span>
        <span class="stc-existing-title">${esc(w.title)}</span>
        <span class="muted">${esc(P[w.proj]?.name || "")}</span>
      </button>
    </li>`).join("")}</ul>` : `<p class="empty-row">No unassigned open tasks right now.</p>`}
  </div>`;
}

function renderAssigneeMenuOptions(state) {
  const current = state.pinnedAssignee;
  const autoOpt = `<button role="menuitemradio" aria-checked="${current === null}" type="button" data-stc="pick" data-field="assignee" data-v="auto">${ic("sparkle", "ic14")}<span>Auto (Tess routes)</span></button>`;
  const unassignOpt = `<button role="menuitemradio" aria-checked="${current === "unassigned"}" type="button" data-stc="pick" data-field="assignee" data-v="unassigned">${ic("user", "ic14")}<span>Unassigned</span></button>`;
  const agents = memberIds().filter((id) => M[id].kind === "agent" && !M[id].builtin);
  const people = memberIds().filter((id) => M[id].kind === "person");
  const row = (id) => {
    const m = M[id], avail = memberAvailability(id);
    return `<button role="menuitemradio" aria-checked="${current === id}" type="button" data-stc="pick" data-field="assignee" data-v="${esc(id)}">
      ${av(id, 20)}<span class="stc-m-name">${esc(m.name)}</span><span class="stc-m-state" data-tone="${avail.tone}">${esc(avail.label)}</span>
    </button>`;
  };
  return `${autoOpt}${unassignOpt}<p class="menu-h">Agents</p>${agents.map(row).join("")}<p class="menu-h">People</p>${people.map(row).join("")}`;
}

const PROPERTY_MENU_RENDERERS = {
  proj: (state) => activeProjects().map(([id, pr]) =>
    `<button role="menuitemradio" aria-checked="${state.proj === id}" type="button" data-stc="pick" data-field="proj" data-v="${esc(id)}">${ic("folder", "ic14")}<span>${esc(pr.name)}</span></button>`
  ).join("") + `<hr><button role="menuitemradio" aria-checked="${state.proj === "none"}" type="button" data-stc="pick" data-field="proj" data-v="none">${ic("user-group", "ic14")}<span>No project (Team work)</span></button>`,
  team: (state) => Object.entries(TEAMS).map(([id, t]) =>
    `<button role="menuitemradio" aria-checked="${state.team === id}" type="button" data-stc="pick" data-field="team" data-v="${esc(id)}"><span>${esc(t.name)}</span></button>`
  ).join(""),
  assignee: renderAssigneeMenuOptions,
  type: (state) => `<button role="menuitemradio" aria-checked="${!state.pinnedType}" type="button" data-stc="pick" data-field="type" data-v="auto">${ic("sparkle", "ic14")}<span>Auto (Tess)</span></button>`
    + TASK_TYPES.map(([k, l]) => `<button role="menuitemradio" aria-checked="${state.pinnedType === k}" type="button" data-stc="pick" data-field="type" data-v="${k}"><span>${l}</span></button>`).join(""),
  priority: (state) => `<button role="menuitemradio" aria-checked="${!state.pinnedPriority}" type="button" data-stc="pick" data-field="priority" data-v="auto">${ic("sparkle", "ic14")}<span>Auto (Tess)</span></button>`
    + PRIORITIES.map((p) => `<button role="menuitemradio" aria-checked="${state.pinnedPriority === p}" type="button" data-stc="pick" data-field="priority" data-v="${p}"><span>${p}</span></button>`).join(""),
  severity: (state) => `<button role="menuitemradio" aria-checked="${!state.pinnedSeverity}" type="button" data-stc="pick" data-field="severity" data-v="auto">${ic("sparkle", "ic14")}<span>Auto (Tess)</span></button>`
    + SEVERITIES.map((s) => `<button role="menuitemradio" aria-checked="${state.pinnedSeverity === s}" type="button" data-stc="pick" data-field="severity" data-v="${s}"><span>${s}</span></button>`).join(""),
};

function renderPropertyMenu(state, key) {
  const fn = PROPERTY_MENU_RENDERERS[key];
  return fn ? `<div class="stc-menu" role="menu">${fn(state)}</div>` : "";
}

function mentionCandidates(query) {
  const q = query.toLowerCase();
  return memberIds()
    .filter((id) => !M[id].builtin && (!q || M[id].name.toLowerCase().includes(q) || M[id].role.toLowerCase().includes(q) || id.includes(q)))
    .slice(0, 6);
}

function renderMentionPopover(state) {
  if (!state.mention.open) return "";
  const list = mentionCandidates(state.mention.query);
  if (!list.length) return "";
  const idx = Math.min(state.mention.activeIdx, list.length - 1);
  return `<div class="stc-popover" id="stc-mention-popover" role="listbox" aria-label="Tag agent or person">${list.map((id, i) => {
    const m = M[id], avail = memberAvailability(id);
    return `<button class="stc-mention-opt${i === idx ? " is-active" : ""}" type="button" role="option" aria-selected="${i === idx}" data-stc="mention-pick" data-id="${esc(id)}">
      ${av(id, 24)}
      <span class="stc-m-col"><span class="stc-m-name">${esc(m.name)}</span><span class="stc-m-role">${esc(m.role)}</span></span>
      <span class="stc-m-state" data-tone="${avail.tone}">${esc(avail.label)}</span>
    </button>`;
  }).join("")}</div>`;
}

function renderComposerFooter(state) {
  if (state.confirmDiscard) {
    return `<div class="stc-foot is-discard">
      <span class="stc-discard-txt">Discard unsaved task?</span>
      <div class="stc-foot-actions">
        <button class="btn outline" type="button" data-stc="keep-editing" id="stc-keep-editing">Keep editing</button>
        <button class="btn danger" type="button" data-stc="confirm-discard">Discard</button>
      </div>
    </div>`;
  }
  if (state.step === "assign-existing") {
    return `<div class="stc-foot"><span class="stc-hint">Pick a task above to assign immediately</span><div class="stc-foot-actions"><button class="btn outline" type="button" data-stc="toggle-assign-existing">New intake instead</button></div></div>`;
  }
  const offline = S.homeState === "offline";
  return `<div class="stc-foot">
    <label class="stc-more-toggle"><button class="switch" type="button" role="switch" aria-checked="${state.createMore}" data-stc="toggle-more"></button><span>Create more</span></label>
    <div class="stc-foot-actions">
      <button class="btn ${offline ? "primary" : "outline"}" type="button" data-stc="create-raw" title="Create immediately without Tess (⌘⇧↵)">Create as-is</button>
      ${offline ? "" : `<button class="btn primary" type="button" data-stc="send-to-tess" id="stc-send-btn">${ic("sparkle", "ic16")}Send to Tess<kbd>⌘↵</kbd></button>`}
    </div>
  </div>`;
}

function paintComposer(focusSelector) {
  if (!STC?.dialog) return;
  const bodyHtml = STC.step === "assign-existing" ? renderAssignExistingStep(STC) : renderCaptureStep(STC);
  STC.dialog.innerHTML = `${renderScopeHeader(STC)}${bodyHtml}${renderComposerFooter(STC)}`;
  updateMentionPopoverOnly();
  if (focusSelector) {
    const el = STC.dialog.querySelector(focusSelector);
    if (el) {
      el.focus({ preventScroll: true });
      if ("selectionStart" in el && el.value) el.setSelectionRange(el.value.length, el.value.length);
    }
  }
}

function updateRoutingPreviewOnly() {
  const el = STC?.dialog?.querySelector(".stc-route-preview");
  if (el) el.outerHTML = renderRoutingPreviewLine(STC);
}

function requestComposerClose() {
  if (!STC) return;
  if (STC.openMenu) { STC.openMenu = null; paintComposer(); return; }
  if (STC.mention.open) { STC.mention.open = false; updateMentionPopoverOnly(); return; }
  if (STC.confirmDiscard) { STC.confirmDiscard = false; paintComposer(); return; }
  if (STC.prompt.trim()) { STC.confirmDiscard = true; paintComposer("#stc-keep-editing"); return; }
  closeComposerNow();
}

function closeComposerNow() {
  if (!STC) return;
  const dlg = STC.dialog, trigger = STC.context.trigger;
  STC = null;
  dlg?.close();
  trigger?.focus?.({ preventScroll: true });
}

function sendPromptToTess() {
  if (!STC) return;
  if (!STC.prompt.trim()) {
    STC.error = "Describe what you want done first.";
    paintComposer("#stc-prompt");
    return;
  }
  enqueueTessIntake(STC);
  if (STC.createMore) {
    Object.assign(STC, { prompt: "", mentions: [], error: "" });
    paintComposer("#stc-prompt");
    return;
  }
  closeComposerNow();
}

function commitRawTaskNow() {
  if (!STC) return;
  if (!STC.prompt.trim()) {
    STC.error = "Describe what you want done first.";
    paintComposer("#stc-prompt");
    return;
  }
  const lines = STC.prompt.trim().split(/\r?\n/);
  const kind = STC.pinnedType || "task";
  const proj = STC.proj === "none" ? activeProjects()[0][0] : STC.proj;
  const who = STC.pinnedAssignee === "unassigned" ? null : (STC.pinnedAssignee || STC.mentions[0] || null);
  const created = persistCreatedWorkTask({
    title: lines[0].trim().slice(0, 160),
    description: lines.slice(1).join("\n").trim(),
    kind,
    proj,
    who,
    requestedBy: "you",
    status: kind === "bug" ? STATUSES.bug[0] : STATUSES.task[0],
    pri: kind === "bug" && STC.pinnedSeverity ? STC.pinnedSeverity : (STC.pinnedPriority || "Normal"),
    etaHours: null,
  });
  toast(`Created ${created.id}`, "check-circle");
  if (STC.createMore) {
    Object.assign(STC, { prompt: "", mentions: [], error: "" });
    render(false);
    paintComposer("#stc-prompt");
    return;
  }
  const [page] = route();
  closeComposerNow();
  if (page === "work") {
    Object.assign(S, { workQuery: "", workStatus: "all", workProject: "all", workTeam: "all" });
    location.hash = workHref({ ...workSelection(), view: "all", item: created.id });
  } else {
    render(false);
  }
}

const PROPERTY_PICKERS = {
  proj: (val) => { STC.proj = val; if (val !== "none") STC.team = defaultTeamForProject(val); },
  team: (val) => { STC.team = val; },
  assignee: (val) => { STC.pinnedAssignee = val === "auto" ? null : val; },
  type: (val) => { STC.pinnedType = val === "auto" ? null : val; },
  priority: (val) => { STC.pinnedPriority = val === "auto" ? null : val; },
  severity: (val) => { STC.pinnedSeverity = val === "auto" ? null : val; },
};

function insertMentionIntoPrompt(id) {
  if (!STC || !M[id]) return;
  const first = M[id].name.split(" ")[0];
  STC.prompt = `${STC.prompt.slice(0, STC.mention.start)}@${first} ${STC.prompt.slice(STC.mention.end)}`;
  if (!STC.mentions.includes(id)) STC.mentions.push(id);
  if (!STC.pinnedAssignee) STC.pinnedAssignee = id;
  STC.mention.open = false;
  paintComposer("#stc-prompt");
}

function detectMentionTrigger(el) {
  const pos = el.selectionStart ?? el.value.length;
  const match = el.value.slice(0, pos).match(/(^|\s)@([a-zA-Z]*)$/);
  if (!match) {
    if (STC.mention.open) { STC.mention.open = false; updateMentionPopoverOnly(); }
    return;
  }
  STC.mention = { open: true, start: pos - match[2].length - 1, end: pos, query: match[2], activeIdx: 0 };
  updateMentionPopoverOnly();
}

function updateMentionPopoverOnly() {
  if (!STC?.dialog) return;
  STC.dialog.querySelector("#stc-mention-popover")?.remove();
  const html = renderMentionPopover(STC);
  if (!html) return;
  const host = STC.dialog.querySelector(".stc-prompt-wrap") || STC.dialog;
  host.insertAdjacentHTML("beforeend", html);
}

const STC_CLICK_ACTIONS = {
  "request-close": () => requestComposerClose(),
  "keep-editing": () => { STC.confirmDiscard = false; paintComposer(); },
  "confirm-discard": () => closeComposerNow(),
  menu: (btn) => { STC.openMenu = STC.openMenu === btn.dataset.menu ? null : btn.dataset.menu; paintComposer(); },
  pick: (btn) => { PROPERTY_PICKERS[btn.dataset.field]?.(btn.dataset.v); STC.openMenu = null; paintComposer(); },
  "mention-pick": (btn) => insertMentionIntoPrompt(btn.dataset.id),
  "rm-mention": (btn) => {
    const rid = btn.dataset.id;
    STC.mentions = STC.mentions.filter((x) => x !== rid);
    if (STC.pinnedAssignee === rid) STC.pinnedAssignee = STC.mentions[0] || null;
    paintComposer("#stc-prompt");
  },
  "send-to-tess": () => sendPromptToTess(),
  "create-raw": () => commitRawTaskNow(),
  "toggle-more": () => { STC.createMore = !STC.createMore; paintComposer(); },
  "toggle-assign-existing": () => { STC.step = STC.step === "assign-existing" ? "capture" : "assign-existing"; paintComposer(); },
  "assign-existing-pick": (btn) => {
    const item = W.find((w) => w.id === btn.dataset.id);
    if (!item || !STC.context.agentId) return;
    item.who = STC.context.agentId;
    toast(`Assigned ${item.id} to ${M[item.who].name}`, "check-circle");
    closeComposerNow();
    render(false);
  },
};

function handleComposerClick(e) {
  if (!STC) return;
  if (e.target === STC.dialog) { requestComposerClose(); return; }
  const btn = e.target.closest("[data-stc]");
  if (!btn) {
    if (STC.mention.open && !e.target.closest("#stc-mention-popover, #stc-prompt")) {
      STC.mention.open = false; updateMentionPopoverOnly();
    }
    if (STC.openMenu && !e.target.closest(".stc-menu-anchor")) { STC.openMenu = null; paintComposer(); }
    return;
  }
  STC_CLICK_ACTIONS[btn.dataset.stc]?.(btn);
}

function handleComposerInput(e) {
  if (!STC || e.target.id !== "stc-prompt") return;
  STC.prompt = e.target.value;
  if (STC.error) { STC.error = ""; STC.dialog.querySelector(".stc-inline-err")?.remove(); }
  detectMentionTrigger(e.target);
  updateRoutingPreviewOnly();
}

function handleMentionKeydown(e) {
  const list = mentionCandidates(STC.mention.query);
  if (!list.length) return false;
  if (e.key === "ArrowDown") { e.preventDefault(); STC.mention.activeIdx = (STC.mention.activeIdx + 1) % list.length; updateMentionPopoverOnly(); return true; }
  if (e.key === "ArrowUp") { e.preventDefault(); STC.mention.activeIdx = (STC.mention.activeIdx - 1 + list.length) % list.length; updateMentionPopoverOnly(); return true; }
  if (e.key === "Enter" || e.key === "Tab") { e.preventDefault(); insertMentionIntoPrompt(list[Math.min(STC.mention.activeIdx, list.length - 1)]); return true; }
  if (e.key === "Escape") { e.preventDefault(); STC.mention.open = false; updateMentionPopoverOnly(); return true; }
  return false;
}

function handleComposerKeydown(e) {
  if (!STC) return;
  if (STC.mention.open && handleMentionKeydown(e)) return;
  if (e.key === "Escape") { e.preventDefault(); requestComposerClose(); return; }
  if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
    e.preventDefault();
    if (e.shiftKey || S.homeState === "offline") commitRawTaskNow();
    else sendPromptToTess();
  }
}

function isActiveProject(id) {
  return !!id && own(P, id) && !P[id].archived;
}

function inferProjectFromRoute(page, id, projects) {
  if (page === "projects" && isActiveProject(id)) return { proj: id, team: defaultTeamForProject(id) };
  if (page === "teams" && own(TEAMS, id)) {
    const teamProj = projects.find(([, pr]) => pr.teams.includes(id));
    return { proj: teamProj ? teamProj[0] : "none", team: id };
  }
  const proj = isActiveProject(S.workProject) ? S.workProject : projects[0][0];
  return { proj, team: defaultTeamForProject(proj) };
}

function inferOpeningContext(opts = {}) {
  const [page, id] = route();
  const projects = activeProjects();
  let { proj, team } = inferProjectFromRoute(page, id, projects);
  let agentId = opts.agentId && own(M, opts.agentId) ? opts.agentId : null;
  if (!agentId && page === "members" && own(M, id) && id !== "you") agentId = id;
  if (agentId && isActiveProject(M[agentId]?.proj)) {
    proj = M[agentId].proj;
    team = defaultTeamForProject(proj);
  }
  return { proj, team, agentId };
}

function openSmartTaskComposer(trigger, opts = {}) {
  document.getElementById("work-task-dialog")?.remove();
  const { proj, team, agentId } = inferOpeningContext(opts);
  const dialog = document.createElement("dialog");
  dialog.className = "work-task-dialog stc-dialog";
  dialog.id = "work-task-dialog";
  dialog.setAttribute("aria-label", "New task");
  document.getElementById("overlay-root").appendChild(dialog);
  STC = {
    dialog,
    step: "capture",
    proj,
    team,
    prompt: "",
    pinnedAssignee: agentId || null,
    pinnedType: null,
    pinnedPriority: null,
    pinnedSeverity: null,
    mentions: [],
    createMore: false,
    confirmDiscard: false,
    openMenu: null,
    error: "",
    mention: { open: false, start: 0, end: 0, query: "", activeIdx: 0 },
    context: { agentId, trigger },
  };
  dialog.addEventListener("click", handleComposerClick);
  dialog.addEventListener("input", handleComposerInput);
  dialog.addEventListener("keydown", handleComposerKeydown);
  dialog.addEventListener("cancel", (e) => { e.preventDefault(); requestComposerClose(); });
  dialog.addEventListener("close", () => dialog.remove(), { once: true });
  paintComposer();
  dialog.showModal();
  dialog.querySelector("#stc-prompt")?.focus({ preventScroll: true });
}

export { openSmartTaskComposer };
