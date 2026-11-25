import { M, OFFICES, OPEN, P, STATUSES, TEAMS, W } from "../data.js";
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
const ETA_PRESETS = [
  [null, "TBD"],
  [1, "1h"],
  [2, "2h"],
  [4, "4h"],
  [8, "8h"],
  [16, "16h"],
];

const AGENT_ROUTING_RULES = [
  { re: /\b(rust|sync|codec|lock|office|cert|sqlite|db|api|backend|snapshot|daemon|cli)\b/i, ids: ["kai", "sage", "rowan"] },
  { re: /\b(ui|svelte|css|theme|design|layout|modal|toolbar|spacing|ux|contrast|pill|button|composer)\b/i, ids: ["eden", "jules", "avery"] },
  { re: /\b(review|audit|qa|test|regression|verify|check)\b/i, ids: ["tomas", "kai"] },
  { re: /\b(plan|spec|roadmap|backlog|priorit|prototype|ia|workflow)\b/i, ids: ["emery", "ada"] },
  { re: /\b(release|notariz|sign-off|deploy|ship|coordinate|apple)\b/i, ids: ["ada", "maya"] },
];

let STC = null;

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

function dispatchCopyForMember(id, avail, teamId) {
  const m = M[id];
  if (!m) {
    const teamName = TEAMS[teamId]?.name || "Team";
    return { icon: "clipboard", tone: "neutral", text: `Unassigned — saved to ${teamName} queue; no agent will start.`, action: null };
  }
  if (avail.state === "person") {
    const note = m.last || (m.live === "available" ? "Available" : "Offline");
    return { icon: "user", tone: "neutral", text: `Assigned to ${m.name} (${note}).`, action: null };
  }
  if (avail.state === "paused") {
    return { icon: "pause", tone: "warn", text: `${m.name} is paused — task saves and waits until resumed.`, action: { act: "stc-pick-available", label: "Pick available" } };
  }
  if (avail.state === "capped") {
    return { icon: "warn", tone: "bad", text: `${m.name} hit the $210 budget cap — task saves, won't start until unblocked.`, action: { act: "stc-pick-available", label: "Reassign" } };
  }
  if (avail.state === "unplaced") {
    return { icon: "office", tone: "warn", text: `${m.name} isn't placed in an office — task saves and waits until placed.`, action: { act: "stc-pick-available", label: "Pick available" } };
  }
  if (avail.state === "offline-office") {
    const officeName = OFFICES[m.setup.office]?.name || "Office";
    return { icon: "offline", tone: "bad", text: `${officeName} is offline — task saves and queues until reconnected.`, action: { act: "stc-pick-available", label: "Pick available" } };
  }
  if (avail.state === "working") {
    const cur = m.task ? ` on ${m.task}` : "";
    return { icon: "clock", tone: "accent", text: `${m.name} is working${cur} — queues next without interrupting.`, action: null };
  }
  return { icon: "bolt", tone: "ok", text: `${m.name} is available — will wake and start immediately on create.`, action: null };
}

function agentDispatchInfo(assigneeId, collaborators, teamId) {
  const avail = memberAvailability(assigneeId);
  const info = dispatchCopyForMember(assigneeId, avail, teamId);
  const collabs = (collaborators || []).filter((cid) => cid !== assigneeId && M[cid]);
  if (collabs.length) {
    const names = collabs.map((cid) => "@" + M[cid].name.split(" ")[0]).join(", ");
    info.text += ` · ${names} tagged to help.`;
  }
  return info;
}

function isDispatchReadyAgent(id) {
  const m = M[id];
  if (!m || m.kind !== "agent" || m.builtin || m.offboarded) return false;
  return memberAvailability(id).ready;
}

function bestAvailableFallback(projId, excludeId) {
  const projTeams = projId && projId !== "none" ? (P[projId]?.teams || []) : [];
  const candidates = memberIds().filter((id) => id !== excludeId && isDispatchReadyAgent(id));
  const inProj = candidates.find((id) => M[id].teams.some((t) => projTeams.includes(t)));
  return inProj || candidates[0] || "ada";
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
  if (/\b(low|minor|typo|cleanup|polish|nice to have|later)\b/i.test(text)) return "Low";
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
  if (kind === "request") return `${trimmed}. Scope requirements for ${projName}, confirm trade-offs, and propose an implementation path.`;
  return `${trimmed}. Deliver end-to-end in ${projName} with verification checks passing.`;
}

function tessBuildCriteria(text, kind) {
  if (kind === "bug") {
    return [
      "Reproduce the failure path and isolate root cause",
      "Implement fix without regressions in adjacent flows",
      "Add automated test covering the fixed scenario",
    ];
  }
  if (/\b(ui|css|theme|modal|toolbar|layout|button|spacing|design)\b/i.test(text)) {
    return [
      "Matches visual spec across Cozy Glass, Ocean, and Report Day",
      "Keyboard navigation, focus states, and mobile layout verified",
      "Checks and browser smoke suite pass with zero errors",
    ];
  }
  if (kind === "request") {
    return [
      "Document scope, options, and recommended default",
      "Validate contract alignment with affected teams",
      "Confirm acceptance criteria ready for implementation",
    ];
  }
  return [
    "Core behavior implemented against project contract",
    "Edge cases and error states handled cleanly",
    "Automated checks and verification pass",
  ];
}

function tessPickAssignee(text, projId, mentions, pinned) {
  if (pinned === "unassigned") {
    return { who: null, collaborators: mentions.slice(), rationale: "" };
  }
  if (pinned && M[pinned]) {
    const collabs = mentions.filter((id) => id !== pinned);
    return { who: pinned, collaborators: collabs, rationale: "" };
  }
  if (mentions.length) {
    const primary = mentions[0];
    const collabs = mentions.slice(1);
    return {
      who: primary,
      collaborators: collabs,
      rationale: `Tess: Assigned to @${M[primary].name.split(" ")[0]} from your prompt${collabs.length ? " and looped in tagged collaborators" : ""}.`,
    };
  }
  for (const rule of AGENT_ROUTING_RULES) {
    if (!rule.re.test(text)) continue;
    const ready = rule.ids.find(isDispatchReadyAgent);
    if (ready) {
      const m = M[ready];
      const avail = memberAvailability(ready);
      const statusTxt = avail.state === "available" ? "available now" : "active on related work";
      return { who: ready, collaborators: [], rationale: `Tess: ${m.name} (${m.role}) fits this scope and is ${statusTxt}.` };
    }
  }
  const fallback = bestAvailableFallback(projId, null);
  const m = M[fallback];
  return { who: fallback, collaborators: [], rationale: `Tess: ${m.name} (${m.role}) is the best available lead for this scope.` };
}

function tessEstimateEta(text, kind) {
  if (/\b(typo|copy|label|rename|tweak|minor)\b/i.test(text)) return 1;
  if (/\b(architecture|refactor|migration|federation|sync|policy|redesign)\b/i.test(text)) return 6;
  return kind === "bug" ? 3 : 4;
}

function buildTessDraft(state) {
  const kind = tessInferType(state.prompt, state.pinnedType);
  const pri = tessInferPriority(state.prompt, kind, state.pinnedPriority);
  const severity = kind === "bug" ? tessInferSeverity(state.prompt, state.pinnedSeverity) : null;
  const pick = tessPickAssignee(state.prompt, state.proj, state.mentions, state.pinnedAssignee);
  return {
    title: tessCleanTitle(state.prompt),
    description: tessBuildDescription(state.prompt, kind, state.proj),
    kind,
    pri,
    severity,
    who: pick.who,
    collaborators: pick.collaborators,
    etaHours: tessEstimateEta(state.prompt, kind),
    criteria: tessBuildCriteria(state.prompt, kind),
    rationale: pick.rationale,
    userEditedAssignee: !!state.pinnedAssignee,
  };
}

function buildRawDraft(state) {
  const lines = state.prompt.trim().split(/\r?\n/);
  const title = (lines[0] || "").trim().slice(0, 160);
  const description = lines.slice(1).join("\n").trim();
  const kind = state.pinnedType || "task";
  const who = state.pinnedAssignee === "unassigned" ? null : (state.pinnedAssignee || state.mentions[0] || null);
  const collaborators = state.mentions.filter((id) => id !== who);
  return {
    title,
    description,
    kind,
    pri: state.pinnedPriority || "Normal",
    severity: kind === "bug" ? (state.pinnedSeverity || "Major") : null,
    who,
    collaborators,
    etaHours: null,
    criteria: [],
    rationale: "",
    userEditedAssignee: true,
  };
}

function parseMentionedIds(text) {
  const found = [];
  for (const id of memberIds()) {
    const first = M[id].name.split(" ")[0];
    const re = new RegExp(`@${first}\\b`, "i");
    if (re.test(text) && !found.includes(id)) found.push(id);
  }
  return found;
}

function applyRefinePriorityAndType(draft, cmd, mutated) {
  if (/\b(urgent|p0)\b/i.test(cmd)) { draft.pri = "Urgent"; mutated.add("pri"); }
  else if (/\bhigh\b/i.test(cmd)) { draft.pri = "High"; mutated.add("pri"); }
  else if (/\b(normal|medium)\b/i.test(cmd)) { draft.pri = "Normal"; mutated.add("pri"); }
  else if (/\blow\b/i.test(cmd)) { draft.pri = "Low"; mutated.add("pri"); }

  if (/\bbug\b/i.test(cmd)) { draft.kind = "bug"; draft.severity = draft.severity || "Major"; mutated.add("kind"); }
  else if (/\brequest\b/i.test(cmd)) { draft.kind = "request"; draft.severity = null; mutated.add("kind"); }
  else if (/\btask\b/i.test(cmd) && /\bmake it a task\b/i.test(cmd)) { draft.kind = "task"; draft.severity = null; mutated.add("kind"); }

  if (/\bcritical\b/i.test(cmd)) { draft.kind = "bug"; draft.severity = "Critical"; mutated.add("severity"); mutated.add("kind"); }
  const etaMatch = cmd.match(/\b(\d+(?:\.\d+)?)\s*h(?:ours?)?\b/i);
  if (etaMatch) { draft.etaHours = Number(etaMatch[1]); mutated.add("eta"); }
}

function applyRefinePeopleAndCriteria(draft, cmd, mutated) {
  const mentioned = parseMentionedIds(cmd);
  if (mentioned.length) {
    if (/\b(tag|review|loop|help|cc|with|collaborat)\b/i.test(cmd)) {
      for (const cid of mentioned) {
        if (cid !== draft.who && !draft.collaborators.includes(cid)) draft.collaborators.push(cid);
      }
      mutated.add("collab");
    } else {
      draft.who = mentioned[0];
      draft.rationale = `Tess: Reassigned to ${M[draft.who].name} per your refinement.`;
      mutated.add("who");
    }
  }
  if (/\bunassign/i.test(cmd)) { draft.who = null; draft.rationale = ""; mutated.add("who"); }
  if (/\b(tighten|sharpen|criteria|checklist)\b/i.test(cmd)) {
    draft.criteria = [...draft.criteria.slice(0, 2), "Verify edge cases and zero regressions in automated checks", "Confirm measurable outcome before marking Done"];
    mutated.add("criteria");
  } else if (/\b(add|include)\b/i.test(cmd) && !mentioned.length) {
    const cleaned = cmd.replace(/^(add|include)\s+(a\s+)?(criterion|check|step)?\s*(for|to|about)?\s*/i, "").trim();
    if (cleaned) { draft.criteria.push(cleaned.charAt(0).toUpperCase() + cleaned.slice(1)); mutated.add("criteria"); }
  }
  if (/\b(shorter|concise|brief)\b/i.test(cmd)) {
    draft.description = draft.description.split(".")[0].trim() + ".";
    mutated.add("desc");
  }
}

function applyTessRefinement(state, instruction) {
  const cmd = instruction.trim();
  if (!cmd || !state.draft) return false;
  state.prevDraft = structuredClone(state.draft);
  const mutated = new Set();
  applyRefinePriorityAndType(state.draft, cmd, mutated);
  applyRefinePeopleAndCriteria(state.draft, cmd, mutated);
  if (!mutated.size) {
    state.draft.description = `${state.draft.description.replace(/\s+$/, "")}\n\nRefinement: ${cmd}`;
    mutated.add("desc");
  }
  state.pulseFields = mutated;
  return true;
}

function similarOpenBug(title, kind) {
  if (kind !== "bug" || title.trim().length < 3) return null;
  const words = title.toLowerCase().split(/\W+/).filter((w) => w.length >= 4);
  if (!words.length) return null;
  return W.find((w) => w.kind === "bug" && OPEN(w) && words.some((word) => w.title.toLowerCase().includes(word))) || null;
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
      ${state.step === "assign-existing" ? "← Create new task" : `Assign existing (${unassigned.length})`}
    </button>` : "";
  const badge = state.step === "draft" ? `<span class="stc-head-badge">${ic("sparkle", "ic14")}Drafted by Tess</span>`
    : state.context.agentId && M[state.context.agentId] ? `<span class="stc-head-badge">For ${esc(M[state.context.agentId].name)}</span>` : "";
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
    : state.pinnedAssignee && M[state.pinnedAssignee] ? M[state.pinnedAssignee].name : "Auto";
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

function renderCaptureStep(state) {
  const pinnedId = state.pinnedAssignee && M[state.pinnedAssignee] ? state.pinnedAssignee : null;
  const pinnedAvail = pinnedId ? memberAvailability(pinnedId) : null;
  const headsup = pinnedAvail && !pinnedAvail.ready ? (() => {
    const info = agentDispatchInfo(pinnedId, [], state.team);
    return `<div class="stc-dispatch" data-tone="${info.tone}">${ic(info.icon, "ic16")}<span>${esc(info.text)}</span></div>`;
  })() : "";
  const errHtml = state.error ? `<p class="stc-inline-err" role="alert">${esc(state.error)}</p>` : "";
  return `<div class="stc-body stc-capture">
    <div class="stc-prompt-wrap">
      <label class="sr-only" for="stc-prompt">Describe the task</label>
      <textarea class="stc-prompt" id="stc-prompt" rows="4" maxlength="4000" placeholder="Describe what needs to happen… Type @ to tag an agent">${esc(state.prompt)}</textarea>
      ${renderMentionStrip(state)}
    </div>
    ${errHtml}
    ${renderCaptureOverrides(state)}
    ${headsup}
  </div>`;
}

function renderDraftingStep() {
  return `<div class="stc-body stc-drafting" aria-busy="true" aria-live="polite">
    <div class="stc-drafting-status">${ic("sparkle", "ic16")}<span>Tess is structuring your task…</span></div>
    <div class="stc-skel stc-skel-title"></div>
    <div class="stc-skel stc-skel-line"></div>
    <div class="stc-skel stc-skel-line short"></div>
    <div class="stc-skel-row"><span class="stc-skel stc-skel-chip"></span><span class="stc-skel stc-skel-chip"></span><span class="stc-skel stc-skel-chip"></span></div>
  </div>`;
}

function renderDraftCriteria(state) {
  const items = state.draft.criteria || [];
  const pulse = state.pulseFields.has("criteria") ? " is-pulsing" : "";
  return `<div class="stc-criteria${pulse}">
    <div class="stc-section-h"><span>Acceptance criteria</span><span class="cnt">${items.length}</span></div>
    <ul class="stc-crit-list" role="list">
      ${items.map((text, idx) => `<li class="stc-crit-row">
        <span class="stc-crit-box" aria-hidden="true"></span>
        <input class="stc-crit-input" type="text" value="${esc(text)}" data-stc-crit="${idx}" aria-label="Acceptance criterion ${idx + 1}">
        <button class="stc-crit-rm" type="button" data-stc="rm-crit" data-idx="${idx}" aria-label="Remove criterion">${ic("x", "ic14")}</button>
      </li>`).join("")}
      <li class="stc-crit-row is-add">
        ${ic("plus", "ic14")}
        <input class="stc-crit-input stc-crit-add" id="stc-crit-add" type="text" placeholder="Add acceptance criterion… (Enter)" aria-label="Add acceptance criterion">
      </li>
    </ul>
  </div>`;
}

function renderDraftChip(state, { menu, cls = "", pulseKey, iconHtml = "", prefix = "", value, chevron = true }) {
  const pulse = pulseKey && state.pulseFields.has(pulseKey) ? " is-pulsing" : "";
  return `<div class="stc-menu-anchor">
    <button class="stc-chip${cls}${pulse}" type="button" data-stc="menu" data-menu="${menu}" aria-expanded="${state.openMenu === menu}">
      ${iconHtml}${prefix ? `<span class="stc-chip-k">${prefix}:</span>` : ""}<span class="stc-chip-v">${esc(value)}</span>${chevron ? ic("chev-down", "ic14") : ""}
    </button>
    ${state.openMenu === menu ? renderPropertyMenu(state, menu) : ""}
  </div>`;
}

function renderDraftPropertyChips(state) {
  const d = state.draft;
  const whoName = d.who && M[d.who] ? M[d.who].name : "Unassigned";
  const typeLabel = TASK_TYPES.find(([k]) => k === d.kind)?.[1] || "Task";
  const etaLabel = d.etaHours != null ? `~${d.etaHours}h` : "TBD";
  const collabs = d.collaborators.filter((id) => id !== d.who && M[id]);
  const specs = [
    { menu: "assignee", cls: " is-primary-chip", pulseKey: "who", iconHtml: d.who && M[d.who] ? av(d.who, 18) : ic("user", "ic14"), value: whoName },
    { menu: "type", pulseKey: "kind", value: typeLabel },
    ...(d.kind === "bug" ? [{ menu: "severity", pulseKey: "severity", prefix: "Severity", value: d.severity || "Major" }] : []),
    { menu: "priority", pulseKey: "pri", prefix: "Priority", value: d.pri },
    { menu: "eta", pulseKey: "eta", iconHtml: ic("clock", "ic14"), value: etaLabel },
    { menu: "collab", cls: collabs.length ? " is-pinned" : "", pulseKey: "collab", iconHtml: ic("user-plus", "ic14"), value: collabs.length ? collabs.map((c) => "@" + M[c].name.split(" ")[0]).join(", ") : "Tag agent", chevron: false },
  ];
  return `<div class="stc-props-block">
    <div class="stc-overrides" role="group" aria-label="Task properties">${specs.map((s) => renderDraftChip(state, s)).join("")}</div>
    ${d.rationale ? `<p class="stc-rationale">${ic("sparkle", "ic14")}<span>${esc(d.rationale)}</span></p>` : ""}
  </div>`;
}

function renderDraftStep(state) {
  const d = state.draft;
  const dup = similarOpenBug(d.title, d.kind);
  const dupHtml = dup ? `<p class="stc-dup-hint">${ic("info", "ic14")}<span>Similar open bug: <strong>${esc(dup.id)}</strong> ${esc(dup.title)}</span></p>` : "";
  const dispatch = agentDispatchInfo(d.who, d.collaborators, state.team);
  const dispatchBtn = dispatch.action ? `<button class="stc-dispatch-act" type="button" data-stc="${dispatch.action.act}">${esc(dispatch.action.label)}</button>` : "";
  const quickRefines = [
    d.pri !== "High" && d.pri !== "Urgent" ? "Make High priority" : "Tighten criteria",
    !d.collaborators.includes("tomas") && d.who !== "tomas" ? "Tag @Tomas to review" : "Make concise",
  ];
  return `<div class="stc-body stc-draft">
    <div class="stc-title-row">
      <label class="sr-only" for="stc-title">Task title</label>
      <input class="stc-title${state.pulseFields.has("title") ? " is-pulsing" : ""}" id="stc-title" type="text" maxlength="160" value="${esc(d.title)}" placeholder="Task title">
      <button class="stc-edit-prompt-btn" type="button" data-stc="back-capture">${ic("chev-left", "ic14")}Prompt</button>
    </div>
    ${dupHtml}
    <div class="stc-desc-wrap">
      <label class="sr-only" for="stc-desc">Description</label>
      <textarea class="stc-desc${state.pulseFields.has("desc") ? " is-pulsing" : ""}" id="stc-desc" rows="3" maxlength="4000" placeholder="Add context or constraints…">${esc(d.description)}</textarea>
    </div>
    ${renderDraftCriteria(state)}
    ${renderDraftPropertyChips(state)}
    <div class="stc-dispatch" data-tone="${dispatch.tone}">
      ${ic(dispatch.icon, "ic16")}
      <span class="stc-dispatch-txt">${esc(dispatch.text)}</span>
      ${dispatchBtn}
    </div>
    <div class="stc-refine">
      <div class="stc-refine-row">
        ${ic("sparkle", "ic16")}
        <input class="stc-refine-input" id="stc-refine-input" type="text" value="${esc(state.refineText)}" placeholder="Refine with Tess… e.g. Make High priority, tag @Tomas to review">
        ${state.prevDraft ? `<button class="stc-undo-btn" type="button" data-stc="undo-refine">Undo</button>` : ""}
        <button class="btn ghost icon stc-refine-go" type="button" data-stc="apply-refine" aria-label="Apply refinement" title="Apply (Enter)">${ic("send", "ic14")}</button>
      </div>
      <div class="stc-quick-refines">${quickRefines.map((q) => `<button class="stc-qchip" type="button" data-stc="quick-refine" data-v="${esc(q)}">${esc(q)}</button>`).join("")}</div>
    </div>
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

function renderAssigneeMenuOptions(state, isCollab) {
  const current = isCollab ? null : (state.step === "draft" ? state.draft.who : state.pinnedAssignee);
  const autoOpt = !isCollab && state.step === "capture"
    ? `<button role="menuitemradio" aria-checked="${current === null}" type="button" data-stc="pick" data-field="assignee" data-v="auto">${ic("sparkle", "ic14")}<span>Auto (Tess)</span></button>` : "";
  const unassignOpt = !isCollab
    ? `<button role="menuitemradio" aria-checked="${current === "unassigned" || (state.step === "draft" && current === null)}" type="button" data-stc="pick" data-field="assignee" data-v="unassigned">${ic("user", "ic14")}<span>Unassigned</span></button>` : "";
  const agents = memberIds().filter((id) => M[id].kind === "agent" && !M[id].builtin);
  const people = memberIds().filter((id) => M[id].kind === "person");
  const row = (id) => {
    const m = M[id], avail = memberAvailability(id);
    const checked = isCollab ? state.draft.collaborators.includes(id) : current === id;
    return `<button role="${isCollab ? "menuitemcheckbox" : "menuitemradio"}" aria-checked="${checked}" type="button" data-stc="pick" data-field="${isCollab ? "collab" : "assignee"}" data-v="${esc(id)}">
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
  assignee: (state) => renderAssigneeMenuOptions(state, false),
  collab: (state) => renderAssigneeMenuOptions(state, true),
  type: (state) => {
    const cur = state.step === "draft" ? state.draft.kind : state.pinnedType;
    const auto = state.step === "capture" ? `<button role="menuitemradio" aria-checked="${!cur}" type="button" data-stc="pick" data-field="type" data-v="auto">${ic("sparkle", "ic14")}<span>Auto (Tess)</span></button>` : "";
    return auto + TASK_TYPES.map(([k, l]) => `<button role="menuitemradio" aria-checked="${cur === k}" type="button" data-stc="pick" data-field="type" data-v="${k}"><span>${l}</span></button>`).join("");
  },
  priority: (state) => {
    const cur = state.step === "draft" ? state.draft.pri : state.pinnedPriority;
    const auto = state.step === "capture" ? `<button role="menuitemradio" aria-checked="${!cur}" type="button" data-stc="pick" data-field="priority" data-v="auto">${ic("sparkle", "ic14")}<span>Auto (Tess)</span></button>` : "";
    return auto + PRIORITIES.map((p) => `<button role="menuitemradio" aria-checked="${cur === p}" type="button" data-stc="pick" data-field="priority" data-v="${p}"><span>${p}</span></button>`).join("");
  },
  severity: (state) => {
    const cur = state.step === "draft" ? state.draft.severity : state.pinnedSeverity;
    const auto = state.step === "capture" ? `<button role="menuitemradio" aria-checked="${!cur}" type="button" data-stc="pick" data-field="severity" data-v="auto">${ic("sparkle", "ic14")}<span>Auto (Tess)</span></button>` : "";
    return auto + SEVERITIES.map((s) => `<button role="menuitemradio" aria-checked="${cur === s}" type="button" data-stc="pick" data-field="severity" data-v="${s}"><span>${s}</span></button>`).join("");
  },
  eta: (state) => ETA_PRESETS.map(([hrs, lbl]) =>
    `<button role="menuitemradio" aria-checked="${state.draft.etaHours === hrs}" type="button" data-stc="pick" data-field="eta" data-v="${hrs ?? ""}"><span>${lbl}</span></button>`
  ).join(""),
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
    return `<div class="stc-foot"><span class="stc-hint">Pick a task above to assign immediately</span><div class="stc-foot-actions"><button class="btn outline" type="button" data-stc="toggle-assign-existing">Create new instead</button></div></div>`;
  }
  if (state.step === "drafting") {
    return `<div class="stc-foot"><span class="stc-hint">Structuring title, checklist &amp; assignee</span><div class="stc-foot-actions"><button class="btn outline" type="button" data-stc="create-raw">Skip · Create as-is</button></div></div>`;
  }
  if (state.step === "draft") {
    const ctaLabel = state.draft.kind === "bug" ? "Create bug" : state.draft.kind === "request" ? "Create request" : "Create task";
    return `<div class="stc-foot">
      <label class="stc-more-toggle"><button class="switch" type="button" role="switch" aria-checked="${state.createMore}" data-stc="toggle-more"></button><span>Create more</span></label>
      <div class="stc-foot-actions">
        <button class="btn ghost" type="button" data-stc="back-capture">Back</button>
        <button class="btn primary" type="button" data-stc="commit-draft" id="stc-commit-btn">${ctaLabel}<kbd>⌘↵</kbd></button>
      </div>
    </div>`;
  }
  const offline = S.homeState === "offline";
  return `<div class="stc-foot">
    <span class="stc-hint">${offline ? "Tess unavailable offline" : "@ tag agent · ⌘↵ draft with Tess"}</span>
    <div class="stc-foot-actions">
      <button class="btn ${offline ? "primary" : "outline"}" type="button" data-stc="create-raw" title="Create immediately without Tess (⌘⇧↵)">Create as-is</button>
      ${offline ? "" : `<button class="btn primary" type="button" data-stc="start-draft" id="stc-draft-btn">${ic("sparkle", "ic16")}Draft with Tess<kbd>⌘↵</kbd></button>`}
    </div>
  </div>`;
}

function paintComposer(focusSelector) {
  if (!STC?.dialog) return;
  const bodyHtml = STC.step === "capture" ? renderCaptureStep(STC)
    : STC.step === "drafting" ? renderDraftingStep()
    : STC.step === "assign-existing" ? renderAssignExistingStep(STC)
    : renderDraftStep(STC);
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

function isComposerDirty(state) {
  return !!(state && (state.prompt.trim() || state.step === "draft"));
}

function requestComposerClose() {
  if (!STC) return;
  if (STC.openMenu) { STC.openMenu = null; paintComposer(); return; }
  if (STC.mention.open) { STC.mention.open = false; paintComposer("#stc-prompt"); return; }
  if (STC.confirmDiscard) { STC.confirmDiscard = false; paintComposer(); return; }
  if (isComposerDirty(STC)) { STC.confirmDiscard = true; paintComposer("#stc-keep-editing"); return; }
  closeComposerNow();
}

function closeComposerNow() {
  if (!STC) return;
  if (STC.draftTimer) clearTimeout(STC.draftTimer);
  const dlg = STC.dialog, trigger = STC.context.trigger;
  STC = null;
  dlg?.close();
  trigger?.focus?.({ preventScroll: true });
}

function startTessDrafting() {
  if (!STC) return;
  if (!STC.prompt.trim()) {
    STC.error = "Describe what you want done first.";
    paintComposer("#stc-prompt");
    return;
  }
  STC.error = "";
  STC.openMenu = null;
  STC.mention.open = false;
  STC.step = "drafting";
  paintComposer();
  STC.draftTimer = setTimeout(() => {
    if (!STC || STC.step !== "drafting") return;
    STC.draft = buildTessDraft(STC);
    STC.prevDraft = null;
    STC.pulseFields = new Set();
    STC.step = "draft";
    paintComposer("#stc-commit-btn");
  }, 320);
}

function createdToastMessage(created) {
  if (!created.who || !M[created.who]) return `Created ${created.id}`;
  const avail = memberAvailability(created.who);
  const started = avail.state === "available" && M[created.who].kind === "agent";
  return `Created ${created.id} · ${started ? `${M[created.who].name} started` : `Assigned to ${M[created.who].name}`}`;
}

function resetComposerForNextTask() {
  Object.assign(STC, { prompt: "", mentions: [], draft: null, prevDraft: null, refineText: "", step: "capture" });
  render(false);
  paintComposer("#stc-prompt");
}

function commitTaskObject(draft) {
  const proj = STC.proj === "none" ? activeProjects()[0][0] : STC.proj;
  const status = draft.kind === "bug" ? STATUSES.bug[0] : STATUSES.task[0];
  const criteria = (draft.criteria || []).map((t) => t.trim()).filter(Boolean).map((text) => ({ text, done: false }));
  const pri = draft.kind === "bug" && draft.severity ? draft.severity : (draft.pri || "Normal");
  const created = persistCreatedWorkTask({
    title: draft.title.trim(), description: draft.description.trim(), kind: draft.kind,
    proj, who: draft.who || null, requestedBy: "you", status, pri, etaHours: draft.etaHours,
    ...(criteria.length ? { criteria } : {}),
  });
  toast(createdToastMessage(created), "check-circle");
  if (STC.createMore) { resetComposerForNextTask(); return; }
  const [page] = route();
  closeComposerNow();
  if (page === "work") {
    Object.assign(S, { workQuery: "", workStatus: "all", workProject: "all", workTeam: "all" });
    location.hash = workHref({ ...workSelection(), view: "all", item: created.id });
  } else {
    render(false);
  }
}

function commitFromCaptureRaw() {
  if (!STC) return;
  if (!STC.prompt.trim()) {
    STC.error = "Describe what you want done first.";
    paintComposer("#stc-prompt");
    return;
  }
  commitTaskObject(buildRawDraft(STC));
}

function commitFromDraft() {
  if (!STC?.draft) return;
  if (!STC.draft.title.trim()) {
    toast("Add a task title before creating.");
    STC.dialog.querySelector("#stc-title")?.focus();
    return;
  }
  commitTaskObject(STC.draft);
}

const PROPERTY_PICKERS = {
  proj: (val) => { STC.proj = val; if (val !== "none") STC.team = defaultTeamForProject(val); },
  team: (val) => { STC.team = val; },
  assignee: (val) => {
    const next = val === "auto" ? null : val;
    if (STC.step === "draft") {
      STC.draft.who = next === "unassigned" ? null : next;
      STC.draft.rationale = "";
      STC.draft.userEditedAssignee = true;
    } else {
      STC.pinnedAssignee = next;
    }
  },
  collab: (val) => {
    const idx = STC.draft.collaborators.indexOf(val);
    if (idx >= 0) STC.draft.collaborators.splice(idx, 1);
    else if (val !== STC.draft.who) STC.draft.collaborators.push(val);
  },
  type: (val) => {
    const next = val === "auto" ? null : val;
    if (STC.step === "draft") { STC.draft.kind = next; STC.draft.severity = next === "bug" ? (STC.draft.severity || "Major") : null; }
    else { STC.pinnedType = next; }
  },
  priority: (val) => { if (STC.step === "draft") STC.draft.pri = val; else STC.pinnedPriority = val === "auto" ? null : val; },
  severity: (val) => { if (STC.step === "draft") STC.draft.severity = val; else STC.pinnedSeverity = val === "auto" ? null : val; },
  eta: (val) => { STC.draft.etaHours = val === "" ? null : Number(val); },
};

function applyPropertyPick(field, val) {
  PROPERTY_PICKERS[field]?.(val);
  if (field !== "collab") STC.openMenu = null;
  paintComposer();
}

function insertMentionIntoTarget(id) {
  if (!STC || !M[id]) return;
  const first = M[id].name.split(" ")[0];
  const isRefine = STC.mention.target === "refine";
  const source = isRefine ? STC.refineText : STC.prompt;
  const before = source.slice(0, STC.mention.start);
  const after = source.slice(STC.mention.end);
  const next = `${before}@${first} ${after}`;
  if (isRefine) {
    STC.refineText = next;
  } else {
    STC.prompt = next;
    if (!STC.mentions.includes(id)) STC.mentions.push(id);
    if (!STC.pinnedAssignee) STC.pinnedAssignee = id;
  }
  STC.mention.open = false;
  paintComposer(isRefine ? "#stc-refine-input" : "#stc-prompt");
}

function detectMentionTrigger(el, target) {
  const pos = el.selectionStart ?? el.value.length;
  const slice = el.value.slice(0, pos);
  const match = slice.match(/(^|\s)@([a-zA-Z]*)$/);
  if (!match) {
    if (STC.mention.open) { STC.mention.open = false; updateMentionPopoverOnly(); }
    return;
  }
  STC.mention = { open: true, start: pos - match[2].length - 1, end: pos, query: match[2], activeIdx: 0, target };
  updateMentionPopoverOnly();
}

function updateMentionPopoverOnly() {
  if (!STC?.dialog) return;
  STC.dialog.querySelector("#stc-mention-popover")?.remove();
  const html = renderMentionPopover(STC);
  if (!html) return;
  const host = STC.dialog.querySelector(STC.mention.target === "refine" ? ".stc-refine-row" : ".stc-prompt-wrap") || STC.dialog;
  host.insertAdjacentHTML("beforeend", html);
}

const STC_CLICK_ACTIONS = {
  "request-close": () => requestComposerClose(),
  "keep-editing": () => { STC.confirmDiscard = false; paintComposer(); },
  "confirm-discard": () => closeComposerNow(),
  menu: (btn) => { STC.openMenu = STC.openMenu === btn.dataset.menu ? null : btn.dataset.menu; paintComposer(); },
  pick: (btn) => applyPropertyPick(btn.dataset.field, btn.dataset.v),
  "mention-pick": (btn) => insertMentionIntoTarget(btn.dataset.id),
  "rm-mention": (btn) => {
    const rid = btn.dataset.id;
    STC.mentions = STC.mentions.filter((x) => x !== rid);
    if (STC.pinnedAssignee === rid) STC.pinnedAssignee = STC.mentions[0] || null;
    paintComposer("#stc-prompt");
  },
  "start-draft": () => startTessDrafting(),
  "create-raw": () => commitFromCaptureRaw(),
  "commit-draft": () => commitFromDraft(),
  "back-capture": () => { STC.step = "capture"; STC.openMenu = null; paintComposer("#stc-prompt"); },
  "toggle-more": () => { STC.createMore = !STC.createMore; paintComposer(); },
  "rm-crit": (btn) => { STC.draft.criteria.splice(Number(btn.dataset.idx), 1); paintComposer(); },
  "apply-refine": () => { if (applyTessRefinement(STC, STC.refineText)) { STC.refineText = ""; paintComposer("#stc-refine-input"); } },
  "quick-refine": (btn) => { if (applyTessRefinement(STC, btn.dataset.v)) paintComposer("#stc-refine-input"); },
  "undo-refine": () => { if (!STC.prevDraft) return; STC.draft = STC.prevDraft; STC.prevDraft = null; STC.pulseFields = new Set(); paintComposer("#stc-refine-input"); },
  "stc-pick-available": () => {
    const cur = STC.step === "draft" ? STC.draft.who : STC.pinnedAssignee;
    applyPropertyPick("assignee", bestAvailableFallback(STC.proj, cur));
  },
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
    if (STC.mention.open && !e.target.closest("#stc-mention-popover, #stc-prompt, #stc-refine-input")) {
      STC.mention.open = false; updateMentionPopoverOnly();
    }
    if (STC.openMenu && !e.target.closest(".stc-menu-anchor")) { STC.openMenu = null; paintComposer(); }
    return;
  }
  STC_CLICK_ACTIONS[btn.dataset.stc]?.(btn);
}

function handleComposerInput(e) {
  if (!STC) return;
  const el = e.target;
  if (el.id === "stc-prompt") {
    STC.prompt = el.value;
    if (STC.error) { STC.error = ""; STC.dialog.querySelector(".stc-inline-err")?.remove(); }
    detectMentionTrigger(el, "prompt");
  } else if (el.id === "stc-title" && STC.draft) {
    STC.draft.title = el.value;
  } else if (el.id === "stc-desc" && STC.draft) {
    STC.draft.description = el.value;
  } else if (el.dataset.stcCrit != null && STC.draft) {
    STC.draft.criteria[Number(el.dataset.stcCrit)] = el.value;
  } else if (el.id === "stc-refine-input") {
    STC.refineText = el.value;
    detectMentionTrigger(el, "refine");
  }
}

function handleMentionKeydown(e) {
  const list = mentionCandidates(STC.mention.query);
  if (!list.length) return false;
  if (e.key === "ArrowDown") { e.preventDefault(); STC.mention.activeIdx = (STC.mention.activeIdx + 1) % list.length; updateMentionPopoverOnly(); return true; }
  if (e.key === "ArrowUp") { e.preventDefault(); STC.mention.activeIdx = (STC.mention.activeIdx - 1 + list.length) % list.length; updateMentionPopoverOnly(); return true; }
  if (e.key === "Enter" || e.key === "Tab") { e.preventDefault(); insertMentionIntoTarget(list[Math.min(STC.mention.activeIdx, list.length - 1)]); return true; }
  if (e.key === "Escape") { e.preventDefault(); STC.mention.open = false; updateMentionPopoverOnly(); return true; }
  return false;
}

function handleComposerSubmitKey(e) {
  e.preventDefault();
  if (STC.step === "capture") { if (e.shiftKey) commitFromCaptureRaw(); else startTessDrafting(); }
  else if (STC.step === "draft") commitFromDraft();
}

function handleComposerEnterKey(e) {
  if (e.target.id === "stc-refine-input") {
    e.preventDefault();
    if (applyTessRefinement(STC, STC.refineText)) { STC.refineText = ""; paintComposer("#stc-refine-input"); }
  } else if (e.target.id === "stc-crit-add") {
    e.preventDefault();
    const val = e.target.value.trim();
    if (val && STC.draft) { STC.draft.criteria.push(val); paintComposer("#stc-crit-add"); }
  }
}

function handleComposerKeydown(e) {
  if (!STC) return;
  if (STC.mention.open && handleMentionKeydown(e)) return;
  if (e.key === "Escape") { e.preventDefault(); requestComposerClose(); return; }
  if ((e.metaKey || e.ctrlKey) && e.key === "Enter") { handleComposerSubmitKey(e); return; }
  if (e.key === "Enter") handleComposerEnterKey(e);
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
    draft: null,
    prevDraft: null,
    pulseFields: new Set(),
    refineText: "",
    createMore: false,
    confirmDiscard: false,
    openMenu: null,
    error: "",
    mention: { open: false, start: 0, end: 0, query: "", activeIdx: 0, target: "prompt" },
    context: { agentId, trigger },
    draftTimer: null,
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
