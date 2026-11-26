import { A, IN, KIND, M, OFFICES, OPEN, P, W } from "../data.js";
import { S } from "../state.js";
import { ATYPE, av, avStack, effectiveHealth, esc, ic, leadOf, leadTxt, memberIds, mlink, pill, plink, plural, presence, projMembers, projPill, skel, spotQueue, unhandled } from "../ui.js";

/* ---------------------------------------------------------------- pages */
function v4PrimaryBtn(it, act, v, icon, label, tone = "win") {
  const ico = icon ? `<span class="h4-cta-ico">${ic(icon, "ic16")}</span>` : "";
  return `<button class="btn primary sm h4-cta" data-cta-tone="${tone}" type="button" data-act="${act}" data-id="${it.id}"${v ? ` data-v="${esc(v)}"` : ""}>
      <span class="h4-cta-fill" aria-hidden="true"></span>
      <span class="h4-cta-sheen" aria-hidden="true"></span>
      <span class="h4-cta-inner">
        <span class="h4-cta-lbl">${ico}<span>${esc(label)}</span></span>
      </span>
    </button>`;
}

function v4ActionBtn(it, style, act, v, icon, label, tone) {
  if (style === "primary") return v4PrimaryBtn(it, act, v, icon, label, tone);
  return `<button class="btn ${style} sm" type="button" data-act="${act}" data-id="${it.id}"${v ? ` data-v="${esc(v)}"` : ""}>${icon ? ic(icon, "ic16") : ""}${esc(label)}</button>`;
}

function v4ReplyBox(it) {
  const changes = S.replyMode === "changes", deny = S.replyMode === "deny", who = esc(M[it.from].name);
  const lbl = deny ? `Optional reason for ${who}…` : changes ? `What should ${who} change?` : `Reply to ${who}`;
  const submitInner = deny
    ? `${ic("deny", "ic16")}Confirm deny`
    : `<span class="h4-cta-fill" aria-hidden="true"></span><span class="h4-cta-sheen" aria-hidden="true"></span><span class="h4-cta-inner"><span class="h4-cta-lbl"><span class="h4-cta-ico">${ic("send", "ic16")}</span><span>${changes ? "Send changes" : "Send reply"}</span></span></span>`;
  return `<div class="reply"><label class="sr-only" for="r-${it.id}">${lbl}</label><textarea id="r-${it.id}" rows="2" placeholder="${lbl}"></textarea>
      <div class="actions"><button class="btn ${deny ? "outline" : "primary h4-cta"} sm" ${deny ? "" : 'data-cta-tone="reply"'} type="button" data-act="send-reply" data-id="${it.id}">${submitInner}</button><button class="btn ghost sm" type="button" data-act="cancel-reply" data-id="${it.id}">Cancel</button></div></div>`;
}

function v4DenySlot(it, label, outcome, icon = "deny") {
  if (S.confirmDeny !== it.id) {
    return `<button class="btn outline sm hb-deny-btn" type="button" data-act="deny-arm" data-id="${it.id}">${ic(icon, "ic16")}${esc(label)}</button>`;
  }
  const who = esc(M[it.from] ? M[it.from].name : "agent");
  return `<div class="hb-deny-guard" role="group" aria-label="Confirm ${esc(label.toLowerCase())}">
        <div class="hb-deny-guard-top">
          <span class="hb-deny-guard-lbl">${ic(icon, "ic14")}<span>Confirm ${esc(label.toLowerCase())}?</span></span>
          <div class="hb-deny-guard-acts">
            <button class="btn sm hb-deny-confirm" type="button" data-act="deny-confirm" data-id="${it.id}" data-v="${esc(outcome)}">Confirm</button>
            <button class="btn ghost sm" type="button" data-act="deny-keep" data-id="${it.id}">Keep</button>
          </div>
        </div>
        <label class="sr-only" for="deny-msg-${it.id}">Optional message for ${who}</label>
        <input class="field hb-deny-input" id="deny-msg-${it.id}" data-deny-id="${it.id}" data-deny-outcome="${esc(outcome)}" type="text" placeholder="Optional reason for ${who}…" autocomplete="off">
      </div>`;
}

function v4ProblemButtons(it) {
  const b = [];
  if (it.resume) b.push(v4ActionBtn(it, "primary", "handle", "Goal resumed", "play", "Resume", "win"));
  if (it.assign) b.push(v4ActionBtn(it, "primary", "assign", "", "user-plus", "Assign…", "win"));
  const guardedHtml = it.dismiss ? v4DenySlot(it, "Dismiss", "Dismissed", "x") : "";
  return { b, guardedHtml };
}

function v4CustomButtons(it) {
  const b = [];
  let guardedHtml = "";
  for (let i = 0; i < it.acts.length; i++) {
    const [label, outcome, icon, mode] = it.acts[i];
    if (mode === "deny") guardedHtml = v4DenySlot(it, label, outcome, icon || "x");
    else b.push(mode === "changes" ? v4ActionBtn(it, i ? "outline" : "primary", "reply", "changes", icon, label, "reply") : v4ActionBtn(it, i ? "outline" : "primary", "handle", outcome, icon, label, "win"));
  }
  return { b, guardedHtml };
}

function v4BuildButtons(it) {
  if (it.acts) return v4CustomButtons(it);
  if (it.kind === "approval") return { b: [v4ActionBtn(it, "primary", "handle", "Approved", "check", it.cta || "Approve", "win")], guardedHtml: v4DenySlot(it, "Deny", "Denied", "deny") };
  if (it.kind === "question") return { b: [v4ActionBtn(it, "primary", "reply", "", "reply", "Reply", "reply")], guardedHtml: "" };
  if (it.kind === "decision") return { b: it.opts.map((o, i) => v4ActionBtn(it, i ? "outline" : "primary", "handle", "Chose: " + o, i ? null : "check", o, "win")), guardedHtml: "" };
  return it.kind === "problem" ? v4ProblemButtons(it) : { b: [], guardedHtml: "" };
}

function itemActionsV4(it, opts = {}) {
  if (it.handled) return `<p class="h4-handled-win" id="handled-${esc(it.id)}">${ic("check-circle", "ic16")}<span>${esc(it.outcome)}</span></p>`;
  if (S.replying === it.id) return v4ReplyBox(it);
  const { b, guardedHtml } = v4BuildButtons(it);
  if (guardedHtml) {
    if (S.confirmDeny !== it.id) b.push(`<span class="hb-act-sep" aria-hidden="true"></span>`);
    b.push(guardedHtml);
  }
  if (!opts.hideDetails) b.push(`<a class="btn ghost sm" href="#/inbox/${it.id}">Details${ic("chev-right", "ic14")}</a>`);
  return `<div class="actions">${b.join("")}</div>`;
}
function needsCardV4(it) {
  const isUrgent = !!(it.expires && !it.handled);
  const isWinBurst = !!it.justHandled;
  if (it.justHandled) it.justHandled = false;
  const k = KIND[it.kind] || { icon: "bell" };
  const ctxKey = "ctx-" + it.id;
  const ctxOpen = !!S.v4Open[ctxKey];
  return `<article class="h4-decision${isUrgent ? " urgent" : ""}${it.handled ? " done" : ""}${isWinBurst ? " win-burst" : ""}" aria-label="${esc(it.sub)} from ${esc(M[it.from].name)}">
    <div class="h4-d-top">
      <div class="h4-d-who">${av(it.from, 24)}<span class="who-name">${mlink(it.from)}</span><span class="h4-kind-tag">${ic(k.icon, "ic14")}${esc(it.sub)}</span><span class="h4-d-time">${esc(it.time)}</span></div>
      ${it.handled ? pill("Handled", "gray", "check-circle") : isUrgent ? `<span class="h4-exp-pill">${ic("clock", "ic14")}${esc(it.expires)}</span>` : ""}
    </div>
    <div class="h4-d-body">
      <h3>${esc(it.what)}</h3>
    </div>
    ${it.rec && !it.handled ? `<div class="h4-rec">${ic("info", "ic16")}<span>${esc(it.rec)}</span></div>` : ""}
    ${ctxOpen ? `<div class="h4-ctx-body" id="panel-${ctxKey}">${esc(it.why)}</div>` : ""}
    <div class="h4-d-foot">
      <div class="h4-origin">
        <button class="h4-ctx-toggle" type="button" data-act="v4-acc" data-id="${ctxKey}" aria-expanded="${ctxOpen}" aria-controls="panel-${ctxKey}">${ic("chev-right", "ic14 h4-chev")}<span>${ctxOpen ? "Hide context" : "Why & context"}</span></button>
        <span class="muted">·</span>
        ${plink(it.proj)}${it.task ? `<span class="h4-task-chip">${esc(it.task)}</span>` : ""}
      </div>
      ${itemActionsV4(it)}
    </div>
  </article>`;
}
function inputAccordionV4(it) {
  const isWinBurst = !!it.justHandled;
  if (it.justHandled) it.justHandled = false;
  const k = KIND[it.kind] || { icon: "bell" };
  const key = "in-" + it.id;
  const open = !!(S.v4Open[key] || S.replying === it.id || it.handledNow);
  return `<article class="h4-acc-item${it.handled ? " done" : ""}${isWinBurst ? " win-burst" : ""}" aria-label="${esc(it.sub)} from ${esc(M[it.from].name)}">
    <button class="h4-acc-trigger" type="button" data-act="v4-acc" data-id="${key}" aria-expanded="${open}" aria-controls="panel-${key}">
      ${ic("chev-right", "ic16 h4-chev")}
      ${av(it.from, 24)}
      <span class="h4-acc-title">${esc(it.what)}</span>
      <span class="h4-acc-meta">
        <span class="h4-kind-tag">${ic(k.icon, "ic14")}${esc(it.sub)}</span>
        <span class="hide-sm">${esc(P[it.proj].name)}</span>
        <span class="h4-d-time">${esc(it.time)}</span>
        ${it.handled ? pill("Handled", "gray", "check-circle") : ""}
      </span>
    </button>
    ${open ? `<div class="h4-acc-body" id="panel-${key}">
      <p class="h4-d-why">${esc(it.why)}</p>
      ${it.rec && !it.handled ? `<div class="h4-rec">${ic("info", "ic16")}<span>${esc(it.rec)}</span></div>` : ""}
      <div class="h4-d-foot">
        <div class="h4-origin">${mlink(it.from)}<span class="muted">·</span>${plink(it.proj)}${it.task ? `<span class="h4-task-chip">${esc(it.task)}</span>` : ""}</div>
        ${itemActionsV4(it)}
      </div>
    </div>` : ""}
  </article>`;
}
function projCardV4(p) {
  const pr = P[p], ws = W.filter((w) => w.proj === p), total = ws.length, done = ws.filter((w) => !OPEN(w)).length;
  const pct = total ? Math.round(done * 100 / total) : 0;
  const openN = ws.filter(OPEN).length, blocked = ws.filter((w) => w.status === "Blocked").length;
  const healthTone = pr.health === "On track" ? "ok" : pr.health === "At risk" ? "warn" : pr.health === "Off track" ? "bad" : "";
  const healthBadge = pr.health === "At risk" ? pill("At risk", "amber", "warn") : projPill(p);
  const key = "proj-" + p;
  const open = !!S.v4Open[key];
  return `<article class="h4-proj">
    <div class="h4-proj-top">
      <div class="h4-proj-id"><span class="h4-proj-mark" aria-hidden="true">${esc(pr.name[0])}</span><h3 class="h4-proj-name"><a href="#/projects/${p}">${esc(pr.name)}</a></h3></div>
      ${healthBadge}
    </div>
    <div class="h4-proj-prog">
      <div class="h4-proj-prog-hd"><span>${done} of ${total} done${blocked ? ` · <span class="is-blocked">${blocked} blocked</span>` : ""}</span><strong>${pct}%</strong></div>
      <div class="h4-bar ${healthTone}" role="progressbar" aria-label="${esc(pr.name)} progress" aria-valuemin="0" aria-valuemax="${total}" aria-valuenow="${done}" aria-valuetext="${done} of ${total} work items done"><span style="width:${pct}%"></span></div>
    </div>
    <button class="h4-proj-acc-btn" type="button" data-act="v4-acc" data-id="${key}" aria-expanded="${open}" aria-controls="panel-${key}">
      <span>${open ? "Hide details" : `Details · ${openN} open${pr.due ? " · Due " + esc(pr.due) : ""}`}</span>
      ${ic("chev-right", "ic14 h4-chev")}
    </button>
    ${open ? `<div class="h4-proj-details" id="panel-${key}">
      <p class="h4-proj-goal">${esc(pr.risk || pr.goal)}</p>
      <div class="h4-proj-stats">
        <span><b>${openN}</b> open</span>
        <span${blocked ? ' class="is-blocked"' : ""}><b>${blocked}</b> blocked</span>
        ${pr.due ? `<span>Due ${esc(pr.due)}</span>` : ""}
      </div>
      <div class="h4-proj-foot">${avStack(projMembers(p), 4)}${leadOf(p) ? `<span>${esc(leadTxt(p))}</span>` : ""}</div>
    </div>` : ""}
  </article>`;
}
function feedV4(items) {
  if (!items.length) return `<p class="h4-rail-empty">No activity yet. Hires, work and decisions show up here.</p>`;
  return `<ul class="h4-timeline" role="list">${items.map((a) => {
    const t = ATYPE[a.type], open = S.openRuns[a.id];
    return `<li class="h4-tl-item">
      <div class="h4-tl-av">${av(a.who, 24)}</div>
      <div class="h4-tl-main">
        <div class="h4-tl-hd"><span class="actor">${mlink(a.who)}</span><span class="h4-tl-kind">${ic(t.icon, "ic14")}${t.label}</span><time>${esc(a.time)}</time></div>
        <p class="h4-tl-tx">${esc(a.text)} · ${plink(a.proj)}</p>
        ${a.summary ? `<p class="h4-tl-sum">${esc(a.summary)}</p>` : ""}
        ${a.type === "run" ? `<div class="runbtn"><button class="btn ghost sm" type="button" data-act="run" data-id="${a.id}" aria-expanded="${!!open}" aria-controls="log-${a.id}">${ic(open ? "chev-down" : "chev-right", "ic16")}${open ? "Hide run log" : "Show run log"}</button></div>
        <ol class="runlog" id="log-${a.id}" ${open ? "" : "hidden"}>${a.log.map(([tm, tx]) => `<li><time>${esc(tm)}</time><span>${esc(tx)}</span></li>`).join("")}</ol>` : ""}
      </div>
    </li>`;
  }).join("")}</ul>`;
}
const spotLinkHtml = (id, label) => `<a href="#/inbox/${id}" data-act="spot-jump" data-id="${id}">${label}</a>`;

function answerBlockerQuestion(t, blocked, in1) {
  const v1Only = /v1/.test(t);
  const scoped = v1Only ? blocked.filter((b) => b.proj === "agency-v1") : blocked;
  const describe = (b) => b.id === "T-A118"
    ? `${spotLinkHtml("in2", "file-access choice")} for <a href="#/members/sage">Sage</a> <em>(Office federation)</em>`
    : b.id === "T-A124"
      ? `${spotLinkHtml("in5", "local test access")} for <a href="#/members/jules">Jules</a> (security checks)`
      : `<a href="#/projects/${b.proj}/work">${esc(b.title)}</a>`;
  const syncPending = Boolean(in1 && !in1.handled);
  const syncLi = syncPending
    ? `<li><strong>Release sign-off:</strong> <a href="#/members/kai">Kai</a>'s v1.0 sync update passed all 412 checks and awaits your ${spotLinkHtml("in1", "approval")}.</li>`
    : `<li><strong>Release sign-off:</strong> <a href="#/members/kai">Kai</a>'s v1.0 sync update is approved.</li>`;
  const pendingN = scoped.length + (syncPending ? 1 : 0);
  return `<strong>${plural(pendingN, "decision is", "decisions are")} holding up ${v1Only ? "Agentic Org v1.0" : "active work"}:</strong><ul class="cmdk-ai-list">${scoped.map((b) => `<li><strong>Access request:</strong> ${describe(b)}.</li>`).join("")}${syncLi}</ul>`;
}

function answerHorizonQuestion(in2, in17, v1Pending) {
  const lockTxt = in2 && !in2.handled ? `${spotLinkHtml("in2", "file-access choice")} pending` : "file-access choice decided";
  const polishTxt = in17 && !in17.handled ? `pending your ${spotLinkHtml("in17", "final design polish sign-off")}` : "final polish approved";
  const v1Txt = v1Pending.length ? `waiting on ${plural(v1Pending.length, "sign-off", "sign-offs")} (${spotLinkHtml(v1Pending[0].id, "review now")})` : "all sign-offs cleared";
  return `Upcoming checkpoints over the next 14 days:<ul class="cmdk-ai-list"><li><strong>Nov 22 · Office sync test (v0.9):</strong> Build box office offline + ${lockTxt}.</li><li><strong>Nov 27 · Final screen polish (v1.1):</strong> 1 day ahead, ${polishTxt}.</li><li><strong>Dec 2 · Release freeze (v1.0):</strong> ${v1Txt}.</li></ul>`;
}

const ORG_QA_HANDLERS = [
  [/kai|sync update|should i approve|publish/, ({ in1 }) => in1 && !in1.handled
    ? `<strong>Yes — Ada recommends approving Kai's v1.0 sync update:</strong><ul class="cmdk-ai-list"><li><strong>Low risk:</strong> easy to roll back; all 412 automated checks passed and Tomas approved with zero open issues.</li><li><strong>High impact:</strong> unblocks Agentic Org v1.0 launch (window closes in 4 hours). ${spotLinkHtml("in1", "Jump to decision")}.</li></ul>`
    : `<strong>Already approved:</strong> Kai's v1.0 sync update is cleared for release.`],
  [/horizon|schedule|slip|pace|milestone|standard|promote|14 day|release/, ({ in2, in17, v1Pending }) => answerHorizonQuestion(in2, in17, v1Pending)],
  [/block|slow|v1|ship|merge/, ({ t, blocked, in1 }) => answerBlockerQuestion(t, blocked, in1)],
  [/cycle|first-pass|accept|rework|hitl|human|quality|autonom|metric|kpi|turnaround|revision|deliver/, () => `Last 7 days delivery quality: <strong>Approved first try 84%</strong> (+6%), <strong>Avg. turnaround 18m</strong> (-14% faster), <strong>19 tasks delivered</strong> (+4 vs prior week), and <strong>78% autonomous</strong> (22% needed your input). Monthly spend sits at <strong>$142 / $210</strong>.`],
  [/budget|spend|cost|\$|dollar|office|fleet|offline|jules|noor|health|pool|headroom|idle|spin|capacity/, () => `<strong>Monthly spend is $142 of $210 (68%, ~$204 forecast)</strong> — averaging <strong>$7.45 per completed task</strong> (-11%), with <strong>74% AI capacity available</strong>. <a href="#/members/jules">Jules</a> is paused at its monthly cap (<a href="#/settings/agents/jules">raise cap</a>). Team: <strong>11 agents</strong> (7 active · 3 idle: <a href="#/members/jules">Jules</a>, <a href="#/members/avery">Avery</a>, <a href="#/members/noor">Noor</a> · 1 offline: <a href="#/members/quinn">Quinn</a> on Build box office).`],
  [/run|overnight|happen|catch|digest|summary|activity|away/, ({ finishedRuns }) => `In the last 4 hours: <strong>${plural(finishedRuns.length, "autonomous session", "autonomous sessions")}</strong> completed. <a href="#/members/ada">Ada</a> &amp; <a href="#/members/maya">Maya</a> drafted v1.0 release notes &amp; onboarding; <a href="#/members/avery">Avery</a> shipped 2 UI polish tasks; <a href="#/members/sage">Sage</a> prepared the reconnect fix. <a href="#/activity">Open activity log</a>.`],
  [/federation|sage|lock|satellite|access/, ({ in2 }) => `<a href="#/projects/office-federation">Office federation</a> is <strong>Off track (20%)</strong>: the Build box office went offline after sleep (reconnect fix is ready to <a href="#/inbox/in11">verify</a>)${in2 && !in2.handled ? `, and <a href="#/members/sage">Sage</a> is waiting on your ${spotLinkHtml("in2", "file-access choice")} (Sage &amp; Rowan recommend timed access)` : `, and your file-access choice is locked in`}.`],
  [/need|urgent|priority|first|inbox|ada|hire/, ({ open }) => { const urg = open.filter((i) => i.expires); return `<strong>${open.length} items need you</strong> (${urg.length} time-sensitive). ${urg.length ? `Start with ${urg.map((u) => spotLinkHtml(u.id, u.what)).join(" and ")}.` : "No urgent deadlines remaining."} <a href="#/members/ada">Ada</a> also requested hiring a QA specialist to speed up reviews.`; }],
];

function answerOrgQuestion(rawQ) {
  const q = String(rawQ || "").trim(), t = q.toLowerCase();
  if (!q) return null;
  if (S.homeState === "error") {
    return { q, html: `<strong>HQ office is currently offline.</strong> Live org metrics and agent data will return automatically as soon as your office reconnects.` };
  }
  if (S.homeState === "empty") {
    const html = /budget|spend|capacity|cost|limit|money/.test(t)
      ? `You have <strong>$0 spent</strong> and <strong>no monthly cap set yet</strong>, with <strong>100% AI capacity ready</strong> and <strong>HQ office online</strong>. Set your monthly spend limit anytime in <a href="#/settings">Settings</a>.`
      : `Launch your organization in 2 steps:<ul class="cmdk-ai-list"><li><strong>Create your first project</strong> in <a href="#/projects">Projects</a> to set a goal and target date.</li><li><strong>Hire your first autonomous agent</strong> in <a href="#/teams">Teams</a> so approvals and progress appear right on Home.</li></ul>`;
    return { q, html };
  }
  const open = unhandled(), blocked = W.filter((w) => w.status === "Blocked");
  const finishedRuns = A.filter((a) => a.type === "run" && /finished/.test(a.summary || ""));
  const byId = (id) => IN.find((i) => i.id === id);
  const in1 = byId("in1"), in5 = byId("in5"), in2 = byId("in2"), in17 = byId("in17");
  const ctx = { t, open, blocked, finishedRuns, in1, in2, in17, v1Pending: [in1, in5].filter((i) => i && !i.handled) };
  const match = ORG_QA_HANDLERS.find(([rx]) => rx.test(t));
  const html = match ? match[1](ctx) : `Right now <strong>${open.length} decisions</strong> wait in <a href="#/inbox">Inbox</a>, <strong>${blocked.length} work items</strong> are waiting on you, <strong>1 of 2 offices</strong> is online, and <a href="#/members/kai">Kai</a> is working with you. Try asking about <em>v1.0 blockers</em>, <em>upcoming milestones</em>, or <em>budget &amp; capacity</em>.`;
  return { q, html };
}

const homeSkelCards = (n, label) => `<div class="hb-skel-grid" aria-busy="true"><span class="sr-only">${esc(label)}</span>${Array.from({ length: n }, () => `<div class="hb-skel-card" aria-hidden="true"><span class="skel-b" style="width:38%;height:11px"></span><span class="skel-b" style="width:62%;height:22px"></span><span class="skel-b" style="width:100%;height:6px"></span><span class="skel-b" style="width:74%;height:11px"></span></div>`).join("")}</div>`;

function homeHeadline(st, openUrgentCount, openCount) {
  const hour = new Date().getHours();
  const greet = hour < 12 ? "Good morning." : hour < 18 ? "Good afternoon." : "Good evening.";
  if (st === "loading") return `${greet} Preparing your briefing…`;
  if (st === "error") return greet;
  if (st === "empty") return `${greet} <em class="calm">Welcome</em> to Agentic Org.`;
  if (openUrgentCount) return `${greet} <em>${plural(openUrgentCount, "time-sensitive decision", "time-sensitive decisions")}</em> ${openUrgentCount === 1 ? "needs" : "need"} you.`;
  if (openCount) return `${greet} <em class="calm">${plural(openCount, "item", "items")}</em> waiting when you're ready.`;
  return `${greet} <em class="ok">All clear</em> across your organization.`;
}

function financialsPillarHtml(emptyMode) {
  return `<article class="hb-pillar" aria-labelledby="hb-pil-fin">
        <div class="hb-pillar-hd">
          <a class="hb-pillar-name" id="hb-pil-fin" href="#/settings">${ic("currency", "ic14 c-amber")}<span>Financials</span></a>
          <a class="hb-pillar-pill ${emptyMode ? "ok" : "warn"}" href="${emptyMode ? "#/settings" : "#/settings/agents/jules"}">${emptyMode ? "No spend yet" : "Jules at cap"}</a>
        </div>
        <a class="hb-pillar-hero" href="#/settings">
          <div class="hb-pillar-valrow">
            <div class="hb-pillar-primary"><span class="hb-pillar-num">${emptyMode ? "$0" : "$142"}</span><span class="hb-pillar-unit">${emptyMode ? "spent" : "/ $210 monthly"}</span></div>
            <span class="hb-pillar-right" ${emptyMode ? 'style="color:var(--tone-accent-ink)"' : ""}>${emptyMode ? "Set monthly cap" : "68% · ~$204 est."}</span>
          </div>
          <div class="hb-pillar-bar" aria-hidden="true"><span class="seg-spend" style="width:${emptyMode ? 0 : 68}%"></span></div>
        </a>
        <div class="hb-pillar-sub">
          <a class="hb-sub-item" href="#/activity" title="Average AI spend per completed task (last 7 days)"><span class="hb-sub-lbl">Cost per task · 7d</span><span class="hb-sub-val">${emptyMode ? "—" : '$7.45<span class="hb-sub-meta ok">-11%</span>'}</span></a>
          <a class="hb-sub-item" href="#/settings" title="Remaining parallel capacity across your connected AI subscriptions"><span class="hb-sub-lbl">AI capacity</span><span class="hb-sub-val">${emptyMode ? '100%<span class="hb-sub-meta ok">Ready</span>' : '74%<span class="hb-sub-meta ok">available</span>'}</span></a>
        </div>
      </article>`;
}

function summarizeHomeOffices(emptyMode) {
  const officeList = Object.values(OFFICES);
  const onlineN = emptyMode ? 1 : officeList.filter((o) => o.status === "Online").length;
  const offlineN = emptyMode ? 0 : officeList.filter((o) => o.status !== "Online").length;
  const offlineNames = officeList.filter((o) => o.status !== "Online").map((o) => o.name).join(", ");
  return {
    onlineN, offlineN,
    pillCls: offlineN ? "bad" : "ok",
    pillTitle: offlineN ? `Offline: ${esc(offlineNames)}` : "All connected offices are online",
    pillText: offlineN ? `${plural(offlineN, "office", "offices")} offline` : (emptyMode ? "HQ online" : "All offices online"),
    rowTitle: offlineN ? `${onlineN} online · ${offlineN} offline (${esc(offlineNames)})` : `${plural(onlineN, "office", "offices")} online`,
  };
}

function teamPillarHtml(emptyMode, fleet) {
  const off = summarizeHomeOffices(emptyMode);
  const capVal = emptyMode
    ? '0 agents<span class="hb-sub-meta">· 1 person</span>'
    : `${fleet.liveWorkingN} active<span class="hb-sub-meta warn">· ${fleet.idleSpecsN} idle</span>${fleet.offlineSpecsN ? `<span class="hb-sub-meta bad">· ${fleet.offlineSpecsN} off</span>` : ""}`;
  const offlineTag = off.offlineN ? `<span class="hb-sub-sep" aria-hidden="true">·</span><span class="hb-sub-bad">${off.offlineN} offline</span>` : "";
  return `<article class="hb-pillar" aria-labelledby="hb-pil-team">
        <div class="hb-pillar-hd">
          <a class="hb-pillar-name" id="hb-pil-team" href="#/teams/members">${ic("user-group", "ic14 c-indigo")}<span>Team</span></a>
          <a class="hb-pillar-pill ${off.pillCls}" href="#/settings/offices" title="${off.pillTitle}">${off.pillText}</a>
        </div>
        <a class="hb-pillar-hero" href="#/teams/members" title="Share of work completed autonomously without needing your input (last 7 days)">
          <div class="hb-pillar-valrow">
            <div class="hb-pillar-primary"><span class="hb-pillar-num">${emptyMode ? "—" : "78%"}</span><span class="hb-pillar-unit">autonomous</span></div>
            <span class="hb-pillar-right">${emptyMode ? "No activity yet" : "22% needed you · 7d"}</span>
          </div>
          <div class="hb-pillar-bar" aria-hidden="true"><span class="seg-auto" style="width:${emptyMode ? 0 : 78}%"></span><span class="seg-human" style="width:${emptyMode ? 0 : 22}%"></span></div>
        </a>
        <div class="hb-pillar-sub is-team">
          <a class="hb-sub-item" href="#/teams/members" title="All ${fleet.totalAgents} agents: ${fleet.liveWorkingN} active, ${fleet.idleSpecsN} idle (Jules at budget cap, Avery paused, Noor unplaced), ${fleet.offlineSpecsN} waiting on an offline office (Quinn)">
            <span class="hb-sub-lbl">${emptyMode ? "Team capacity" : `${fleet.leadsN} leads · ${fleet.specsN} specialists`}</span>
            <span class="hb-sub-val">${capVal}</span>
          </a>
          <a class="hb-sub-item" href="#/settings/offices" title="${off.rowTitle}">
            <span class="hb-sub-lbl">Offices</span>
            <span class="hb-sub-val is-offices"><span class="hb-sub-ok">${off.onlineN} online</span>${offlineTag}</span>
          </a>
        </div>
      </article>`;
}

function deliveryBadgeMeta(emptyMode, offTrackProjs, atRiskProjs) {
  if (emptyMode) return { cls: "", text: "No deliveries yet" };
  const needsAttn = offTrackProjs + atRiskProjs;
  if (!needsAttn) return { cls: "ok", text: "All on track" };
  if (offTrackProjs && !atRiskProjs) return { cls: "bad", text: `${plural(offTrackProjs, "project", "projects")} off track` };
  return { cls: "warn", text: `${needsAttn} need attention` };
}

function deliveryPillarHtml(emptyMode, offTrackProjs, atRiskProjs) {
  const badge = deliveryBadgeMeta(emptyMode, offTrackProjs, atRiskProjs);
  return `<article class="hb-pillar" aria-labelledby="hb-pil-proj">
        <div class="hb-pillar-hd">
          <a class="hb-pillar-name" id="hb-pil-proj" href="#/work">${ic("check-circle", "ic14 c-green")}<span>Delivery</span></a>
          <a class="hb-pillar-pill ${badge.cls}" href="#/projects">${badge.text}</a>
        </div>
        <a class="hb-pillar-hero" href="#/work" title="Percentage of completed tasks approved on the first try without revisions (last 7 days)">
          <div class="hb-pillar-valrow">
            <div class="hb-pillar-primary"><span class="hb-pillar-num">${emptyMode ? "—" : "84%"}</span><span class="hb-pillar-unit">approved first try</span></div>
            <span class="hb-pillar-right ok">${emptyMode ? "" : "+6% · 7d"}</span>
          </div>
          <div class="hb-pillar-bar" aria-hidden="true"><span class="seg-pass" style="width:${emptyMode ? 0 : 84}%"></span><span class="seg-rework" style="width:${emptyMode ? 0 : 16}%"></span></div>
        </a>
        <div class="hb-pillar-sub">
          <a class="hb-sub-item" href="#/activity" title="Average time from task start to approval (last 7 days)"><span class="hb-sub-lbl">Avg. turnaround · 7d</span><span class="hb-sub-val">${emptyMode ? "—" : '18m<span class="hb-sub-meta ok">-14%</span>'}</span></a>
          <a class="hb-sub-item" href="#/work" title="Tasks completed and accepted in the last 7 days (+4 vs. prior week)"><span class="hb-sub-lbl">Delivered · 7d</span><span class="hb-sub-val">${emptyMode ? "—" : '19 tasks<span class="hb-sub-meta ok">+4</span>'}</span></a>
        </div>
      </article>`;
}

function renderHomeMetrics(st, fleet, offTrackProjs, atRiskProjs) {
  if (st === "loading") return homeSkelCards(3, "Loading executive overview…");
  if (st === "error") return "";
  const emptyMode = st === "empty";
  return `<section class="hb-pillars" aria-label="Executive overview: Financials, Team, and Delivery">${financialsPillarHtml(emptyMode)}${teamPillarHtml(emptyMode, fleet)}${deliveryPillarHtml(emptyMode, offTrackProjs, atRiskProjs)}</section>`;
}

function spotlightErrorHtml() {
  return `<section class="hb-spot is-error is-centered" role="alert" aria-label="Connection status">
        <div class="hb-cleared">
          <span class="hb-cleared-ring is-error">${ic("offline", "ic20")}</span>
          <h2>Couldn't reach HQ office</h2>
          <p>Your files and work are safe. We'll reconnect automatically, or you can retry now.</p>
          <span class="hb-err-meta"><span class="h4-live-dot is-error" style="width:7px;height:7px" aria-hidden="true"></span><span>Reconnecting to This Mac · Last connected 2m ago</span></span>
          <div class="hb-cleared-acts">
            <button class="btn primary sm h4-cta" data-cta-tone="reply" type="button" data-act="set" data-k="homeState" data-v="filled">
              <span class="h4-cta-fill" aria-hidden="true"></span><span class="h4-cta-sheen" aria-hidden="true"></span>
              <span class="h4-cta-inner"><span class="h4-cta-lbl"><span class="h4-cta-ico">${ic("refresh", "ic16")}</span><span>Try again</span></span></span>
            </button>
            <a class="btn ghost sm" href="#/settings/offices">Check offices${ic("chev-right", "ic14")}</a>
          </div>
        </div>
      </section>`;
}

function spotlightClearedHtml(st, queue, open, allQueueCleared) {
  const isFreshEmpty = st === "empty";
  const heading = allQueueCleared ? "Priority spotlight cleared" : isFreshEmpty ? "Launch your organization" : "Nothing needs your decision";
  const body = allQueueCleared
    ? (open.length ? `You cleared all ${queue.length} spotlight items. ${plural(open.length, "more item", "more items")} still waiting in Inbox.` : "Everything waiting on you is handled. Your agents have what they need to keep moving.")
    : isFreshEmpty ? "Create your first project or hire an autonomous agent. Approvals, updates, and executive metrics will appear here automatically."
      : "When an agent needs an approval, decision, or answer, it appears right here.";
  const emptyCtas = isFreshEmpty ? `<button class="btn primary sm h4-cta" data-cta-tone="win" type="button" data-act="proto" data-v="Create project"><span class="h4-cta-fill" aria-hidden="true"></span><span class="h4-cta-sheen" aria-hidden="true"></span><span class="h4-cta-inner"><span class="h4-cta-lbl"><span class="h4-cta-ico">${ic("plus", "ic16")}</span><span>Create first project</span></span></span></button>
            <button class="btn outline sm" type="button" data-act="proto" data-v="Hire an agent">${ic("user-plus", "ic16")}Hire first agent</button>` : "";
  const inboxCta = !isFreshEmpty && open.length ? `<a class="btn primary sm h4-cta" href="#/inbox"><span class="h4-cta-fill" aria-hidden="true"></span><span class="h4-cta-sheen" aria-hidden="true"></span><span class="h4-cta-inner"><span class="h4-cta-lbl"><span>Open Inbox (${open.length})</span></span></span></a>` : "";
  const reviewBtn = allQueueCleared ? `<button class="btn ghost sm" type="button" data-act="v4-acc" data-id="spot-review">Review handled</button>` : "";
  return `<section class="hb-spot is-done${isFreshEmpty ? " is-empty-hero" : ""}" aria-label="${isFreshEmpty ? "Get started" : "Decision spotlight"}">
        <div class="hb-cleared">
          <span class="hb-cleared-ring">${ic(isFreshEmpty ? "sparkle" : "check-circle", "ic20")}</span>
          <h2>${heading}</h2><p>${body}</p>
          <div class="hb-cleared-acts">${emptyCtas}${inboxCta}${reviewBtn}</div>
        </div>
      </section>`;
}

function spotlightUpNextHtml(queue, it) {
  if (queue.length < 2) return "";
  let nextIdx = -1;
  for (let step = 1; step < queue.length; step++) {
    const cand = (S.spotIdx + step) % queue.length;
    if (!queue[cand].handled && queue[cand].id !== it.id) { nextIdx = cand; break; }
  }
  if (nextIdx < 0) return "";
  const nxt = queue[nextIdx];
  const expTag = nxt.expires && !nxt.handled
    ? `<span class="hb-spot-next-exp">${ic("clock", "ic14")}${esc(nxt.expires.replace(/\s*\(.*\)/, ""))}</span>`
    : nxt.from === "tess" ? `<span class="hb-spot-next-ai">${ic("sparkle", "ic14")}Tess</span>` : "";
  return `<button type="button" class="hb-spot-next" data-act="spot-go" data-v="${nextIdx}">
          <span class="hb-spot-next-l"><span class="hb-spot-next-lbl">Up next</span><span class="hb-spot-next-txt">${esc(nxt.what)}</span></span>
          <span class="hb-spot-next-r">${expTag}${ic("chev-right", "ic14")}</span>
        </button>`;
}

function spotlightBodyHtml(it, ctxKey, ctxOpen) {
  const rec = it.rec && !it.handled ? `<div class="hb-spot-rec">${ic("info", "ic16")}<span>${esc(it.rec)}</span></div>` : "";
  const impact = it.impact?.length && !it.handled ? `<div class="hb-spot-meta" role="group" aria-label="Decision risk and impact">${it.impact.map(([ico, tone, title, sub]) => `<div class="hb-spot-cell"><span class="hb-spot-well${tone ? " " + tone : ""}">${ic(ico, "ic16")}</span><div class="hb-spot-cell-txt"><span class="hb-spot-cell-val">${esc(title)}</span><span class="hb-spot-cell-sub">${esc(sub)}</span></div></div>`).join("")}</div>` : "";
  const askBtn = !it.handled ? `<button class="hb-spot-ai-ask" type="button" data-act="brief-ai-ask" data-v="Explain the risk, impact, and recommendation for: ${esc(it.what)}">${ic("sparkle", "ic14")}<span>Ask about this</span></button>` : "";
  const audit = it.audit?.length ? `<div class="hb-ctx-checks" role="list" aria-label="Audit checks and reviewer notes">${it.audit.map((a) => `<span class="hb-ctx-check" role="listitem">${ic("check-circle", "ic14")}<span>${esc(a)}</span></span>`).join("")}</div>` : "";
  const ctxPanel = ctxOpen ? `<div class="h4-ctx-body" id="panel-${ctxKey}"><p>${esc(it.why)}</p>${audit}</div>` : "";
  return `<div class="hb-spot-main">
        <div class="hb-spot-who">${av(it.from, 24)}<span class="who">${mlink(it.from)}</span><span>·</span><span>${esc(it.sub)}</span><span>·</span><span>${esc(it.time)}</span><span>·</span>${plink(it.proj)}</div>
        <h2 class="hb-spot-title">${esc(it.what)}</h2>${rec}${impact}
        <div class="hb-spot-ctx-row">
          <button class="h4-ctx-toggle" type="button" data-act="v4-acc" data-id="${ctxKey}" aria-expanded="${ctxOpen}" aria-controls="panel-${ctxKey}">${ic("chev-right", "ic14 h4-chev")}<span>${ctxOpen ? "Hide background context" : "Read background context"}</span></button>
          ${askBtn}
        </div>${ctxPanel}
      </div>`;
}

function spotlightDotsHtml(queue) {
  return queue.map((q, idx) => {
    const cls = ["hb-dot-btn", idx === S.spotIdx ? "is-cur" : "", q.expires && !q.handled ? "is-urgent" : "", q.handled ? "is-cleared" : ""].filter(Boolean).join(" ");
    return `<button type="button" class="${cls}" data-act="spot-go" data-v="${idx}" aria-label="Decision ${idx + 1} of ${queue.length}: ${esc(q.what)}" ${idx === S.spotIdx ? 'aria-current="true"' : ""}></button>`;
  }).join("");
}

function spotlightFootHtml(it, open, allQueueCleared, remainingInInbox) {
  const detailsLink = !it.handled ? `<a class="btn ghost sm" href="#/inbox/${it.id}">Details${ic("chev-right", "ic14")}</a>` : "";
  const doneBtn = allQueueCleared && S.v4Open["spot-review"] ? `<button type="button" class="hb-skip-btn" data-act="v4-acc" data-id="spot-review">Done reviewing</button>` : "";
  const inboxLink = remainingInInbox ? `<a class="hb-skip-btn" href="#/inbox">All ${open.length} in Inbox${ic("chev-right", "ic14")}</a>` : "";
  return `<div class="hb-spot-foot">${itemActionsV4(it, { hideDetails: true })}<div class="hb-spot-foot-meta">${detailsLink}${doneBtn}${inboxLink}</div></div>`;
}

function spotlightHeaderBarHtml(it, queue, isUrgent) {
  const k = KIND[it.kind] || { icon: "bell" };
  const badgeText = it.handled ? "Handled" : isUrgent ? esc(it.expires) : esc(it.sub);
  const disableNav = queue.length < 2 ? "disabled" : "";
  return `<div class="hb-spot-bar">
        <span class="hb-spot-badge${isUrgent ? " urgent" : ""}">${ic(isUrgent ? "clock" : k.icon, "ic14")}${badgeText}</span>
        <div class="hb-spot-nav">
          <div class="hb-dots" role="group" aria-label="Priority queue">${spotlightDotsHtml(queue)}</div>
          <span class="hb-step-ctr">Priority ${S.spotIdx + 1} of ${queue.length}</span>
          <div class="hb-nav-arrows">
            <button type="button" class="hb-arrow-btn" data-act="spot-step" data-v="-1" aria-label="Previous decision" ${disableNav}>${ic("chev-left", "ic14")}</button>
            <button type="button" class="hb-arrow-btn" data-act="spot-step" data-v="1" aria-label="Next decision" ${disableNav}>${ic("chev-right", "ic14")}</button>
          </div>
        </div>
      </div>`;
}

function renderHomeSpotlight({ st, queue, open, curItem, allQueueCleared, remainingInInbox }) {
  if (st === "loading") return `<div class="hb-spot">${skel(3, "Loading spotlight decision…")}</div>`;
  if (st === "error") return spotlightErrorHtml();
  if (!queue.length || (allQueueCleared && !S.v4Open["spot-review"])) return spotlightClearedHtml(st, queue, open, allQueueCleared);
  const it = curItem, isUrgent = Boolean(it.expires && !it.handled);
  const ctxKey = "spot-ctx-" + it.id, ctxOpen = !!S.v4Open[ctxKey];
  return `<section class="hb-spot${isUrgent ? " is-urgent" : ""}${it.handled ? " is-done" : ""}" id="hb-spotlight" aria-label="Decision spotlight">
      <div class="hb-rep-only hb-rep-sec-hd"><h2 class="hb-rep-sec-title">Decision &amp; Approval Queue</h2></div>
      ${spotlightHeaderBarHtml(it, queue, isUrgent)}
      ${spotlightBodyHtml(it, ctxKey, ctxOpen)}
      ${spotlightFootHtml(it, open, allQueueCleared, remainingInInbox)}
      ${spotlightUpNextHtml(queue, it)}
    </section>`;
}

function briefSectionsHtml(lens, recentDoneN, tessApplied, liveWorkingN) {
  if (lens === "week") {
    return `<div class="hb-cos-sec"><span class="hb-cos-k is-ok">Velocity &amp; Delivery</span><p class="hb-cos-p">The team shipped <strong>19 tasks over the last 7 days</strong> (+4 WoW) with median turnaround improving <strong>14% to 18m</strong>. <strong>Look &amp; feel refresh</strong> is running 1 day ahead of its Nov 28 milestone.</p></div>
        <div class="hb-cos-sec"><span class="hb-cos-k">Autonomy &amp; Budget</span><p class="hb-cos-p"><strong>78% of work ran autonomously</strong> across 4 leads and 7 specialists. Month-to-date spend is <strong>$142 of $210</strong> (68%), pacing to <strong>~$204</strong> — inside budget, though Jules hit its individual cap.</p></div>
        <div class="hb-cos-sec"><span class="hb-cos-k is-ok">Quality Bar</span><p class="hb-cos-p"><strong>84% of deliverables were approved on the first try</strong> (+6% WoW). All <strong>412 automated workspace checks</strong> are green on the v1.0 release candidate.</p></div>`;
  }
  if (lens === "board") {
    return `<div class="hb-cos-sec"><span class="hb-cos-k is-ok">Executive Summary</span><p class="hb-cos-p">Delivery throughput rose <strong>+27% week over week</strong> (19 tasks shipped) at <strong>84% first-pass approval</strong>, with <strong>78%</strong> autonomous execution and spend tracking <strong>$6 under the $210 monthly budget</strong>.</p></div>
          <div class="hb-cos-sec"><span class="hb-cos-k">v1.0 Launch Readiness</span><p class="hb-cos-p"><strong>Agentic Org v1.0</strong> has passed 412 automated checks with release notes and onboarding complete; <strong>${recentDoneN ? "priority release sign-offs are advancing" : "Kai’s sync sign-off is the primary remaining gate"}</strong> for the Dec 5 target.</p></div>
          <div class="hb-cos-sec"><span class="hb-cos-k is-warn">Operational Watchpoints</span><p class="hb-cos-p">The Build box satellite office is offline (+5d slip on Office federation v0.9), with a verified reconnect fix staged. ${tessApplied ? "Jules’s stalled queue has been rerouted to Maya &amp; Rowan at $0 extra spend." : "A zero-cost reassignment for Jules’s 3 stalled tasks is queued for owner approval."}</p></div>`;
  }
  const doneLead = recentDoneN ? `You’ve already cleared <strong>${recentDoneN} ${recentDoneN === 1 ? "item" : "items"}</strong> this session. ` : "";
  const tessNote = tessApplied ? " and applied the reroute for Jules’s 3 tasks to Maya &amp; Rowan at $0 added spend." : " and added my suggested <strong>zero-cost reroute for Jules’s 3 stalled tasks</strong> right behind it — clearing both keeps v1.0 on schedule for Dec 5.";
  return `<div class="hb-cos-sec"><span class="hb-cos-k is-ok">Live Status &amp; Highlights</span><p class="hb-cos-p"><strong>${liveWorkingN} agents are working live</strong> across 3 projects — Kai is in session with you and Maya is auditing the v1.0 launch checklist. In the last 4 hours, Ada &amp; Maya drafted the v1.0 release notes, Avery shipped 2 UI polish updates, and all 412 CI checks cleared green.</p></div>
          <div class="hb-cos-sec"><span class="hb-cos-k is-warn">Problems Needing Attention</span><p class="hb-cos-p"><strong>Build box office</strong> has been offline for 2h, putting Office federation <strong>+5d behind</strong> (Sage’s reconnect fix is staged for your review). Separately, <strong>Jules reached its $210 monthly cap</strong>, leaving 3 tasks stalled in the queue.</p></div>
          <div class="hb-cos-sec"><span class="hb-cos-k">What I’ve Queued For You</span><p class="hb-cos-p">${doneLead}I prioritized <strong>Kai’s v1.0 sync sign-off</strong> first on the left${tessNote}</p></div>`;
}

function renderHomeCatchup(st, liveWorkingN) {
  if (st === "error" || st === "empty") return "";
  const recentDoneN = IN.filter((i) => i.handled && i.handledNow).length;
  const tessApplied = Boolean(IN.find((i) => i.id === "in18")?.handled);
  const lens = S.briefLens || "today";
  const memoBody = st === "loading" ? skel(4, "Tess is preparing your executive brief…") : `
      <div class="hb-cos-memo${S.briefExpanded ? " is-expanded" : ""}">
        <div class="hb-cos-scroll" id="hb-cos-scroll" tabindex="0" aria-label="Executive brief content">${briefSectionsHtml(lens, recentDoneN, tessApplied, liveWorkingN)}</div>
        <button type="button" class="hb-cos-expand" data-act="brief-expand" aria-expanded="${!!S.briefExpanded}" aria-controls="hb-cos-scroll">
          <span>${S.briefExpanded ? "Show less" : "Show full brief"}</span>${ic("chev-down", "ic14")}
        </button>
        <div class="hb-cos-sig">
          <a class="hb-cos-sig-who" href="#/members/tess" title="Open Tess's profile">
            <span class="hb-cos-mark" aria-hidden="true">${ic("sparkle", "ic14")}</span>
            <div class="hb-cos-sig-txt"><span class="hb-cos-sig-name">Tess</span><span class="hb-cos-sig-role">Chief of Staff</span></div>
          </a>
          <div class="hb-cos-sig-r">
            ${lens !== "board" ? `<span class="hb-cos-sig-meta">Updated 2m ago</span>` : ""}
            <button type="button" class="hb-cos-copy${S.briefCopied ? " is-copied" : ""}" data-act="ai-copy-brief" aria-live="polite">
              ${ic(S.briefCopied ? "check" : "clipboard", "ic14")}<span>${S.briefCopied ? "Copied" : "Copy brief"}</span>
            </button>
          </div>
        </div>
      </div>`;
  return `<aside class="hb-catchup" aria-label="Executive brief from Tess, Chief of Staff">
      <div class="hb-cu-hd">
        <div class="hb-cu-title">${ic("sparkle", "ic16")}<span class="lbl-std">Executive Brief</span><span class="lbl-rep">Executive Brief</span></div>
        <div class="hb-brief-lenses" role="group" aria-label="Brief timeframe">
          <button type="button" data-act="brief-lens" data-v="today" aria-pressed="${lens === "today"}">Today</button>
          <button type="button" data-act="brief-lens" data-v="week" aria-pressed="${lens === "week"}">7-Day</button>
          <button type="button" data-act="brief-lens" data-v="board" aria-pressed="${lens === "board"}">Board</button>
        </div>
      </div>
      ${memoBody}
    </aside>`;
}

function resolveAgencyV1Blocker(base) {
  const pending = ["in1", "in5"].map((id) => IN.find((i) => i.id === id)).filter((i) => i && !i.handled);
  if (pending.length === 2) return { ...base, text: "2 sign-offs waiting: v1.0 release & test access", label: "Review (2)", spotId: pending[0].id, href: `#/inbox/${pending[0].id}` };
  if (pending.length === 1) return { ...base, text: pending[0].id === "in5" ? "1 sign-off waiting: local test access for Jules" : "1 sign-off waiting: v1.0 release publish", label: "Review (1)", spotId: pending[0].id, href: `#/inbox/${pending[0].id}` };
  return { ...base, text: "", clearNote: "All v1.0 sign-offs approved · ready for launch" };
}

function resolveProjectBlocker(p, pr) {
  const base = {
    text: pr.blocker || "", label: pr.unblockLabel || "Unblock", href: pr.unblockHref || `#/projects/${p}`,
    spotId: pr.unblockSpotId || "", clearNote: pr.clearNote || "All checks passing · on schedule",
    tone: pr.paceTone === "warn" ? "warn" : "", ico: "warn", icoCls: pr.paceTone === "warn" ? "c-amber" : "c-red",
  };
  if (p === "agency-v1") return resolveAgencyV1Blocker(base);
  if (p === "office-federation" && IN.find((i) => i.id === "in2")?.handled) {
    return { ...base, text: "Build box office offline · file-access choice decided", label: "Check office", spotId: "", href: "#/settings/offices" };
  }
  if (p === "look-feel" && !IN.find((i) => i.id === "in17")?.handled) {
    return { ...base, text: "Final design polish ready for your sign-off", tone: "sky", ico: "flag", icoCls: "c-indigo", label: "Sign off", spotId: "in17", href: "#/inbox/in17" };
  }
  return base;
}

function renderProjBlockerHtml(bl, queue) {
  if (!bl.text) return `<div class="hb-blocker ok"><span class="hb-blocker-txt">${ic("check-circle", "ic14 c-green")}<span>${esc(bl.clearNote)}</span></span></div>`;
  const canJump = Boolean(bl.spotId && queue.some((q) => q.id === bl.spotId && !q.handled));
  const unblockEl = canJump
    ? `<button type="button" class="hb-unblock-btn" data-act="spot-jump" data-id="${bl.spotId}">${esc(bl.label)}${ic("chev-right", "ic14")}</button>`
    : `<a class="hb-unblock-btn" href="${bl.href}">${esc(bl.label)}${ic("chev-right", "ic14")}</a>`;
  return `<div class="hb-blocker ${bl.tone}"><span class="hb-blocker-txt">${ic(bl.ico, "ic14 " + bl.icoCls)}<span>${esc(bl.text)}</span></span>${unblockEl}</div>`;
}

function renderProjBentoCard(p, queue) {
  const pr = P[p], ws = W.filter((w) => w.proj === p), total = ws.length, done = ws.filter((w) => !OPEN(w)).length;
  const pct = total ? Math.round(done * 100 / total) : 0, curHealth = effectiveHealth(p);
  const healthTone = { "On track": "ok", "At risk": "warn", "Off track": "bad" }[curHealth] || "";
  const healthBadge = curHealth === "At risk" ? pill("At risk", "amber", "warn") : curHealth === "On track" ? pill("On track", "green", "check-circle") : projPill(p);
  const paceLabel = curHealth === "On track" && pr.health !== "On track" ? "On pace" : pr.pace;
  const paceTone = curHealth === "On track" ? "ok" : (pr.paceTone || "");
  const metaParts = `${done} of ${total} done${pr.due ? ` · Due ${esc(pr.due)}` : ""}${paceLabel ? ` · <span class="hb-pace ${paceTone}">${esc(paceLabel)}</span>` : ""}`;
  const relTag = pr.release && !pr.name.includes(pr.release) ? `<span class="hb-rel-tag">${esc(pr.release)}</span>` : "";
  return `<article class="hb-card" data-card-href="#/projects/${p}">
      <div class="hb-card-top"><div class="hb-card-id"><a class="hb-card-name" href="#/projects/${p}">${esc(pr.name)}</a>${relTag}</div>${healthBadge}</div>
      <div class="hb-card-prog">
        <div class="hb-card-prog-row"><span>${metaParts}</span><strong>${pct}%</strong></div>
        <div class="h4-bar ${healthTone}" role="progressbar" aria-label="${esc(pr.name)} progress" aria-valuemin="0" aria-valuemax="${total}" aria-valuenow="${done}"><span style="width:${pct}%"></span></div>
      </div>
      ${renderProjBlockerHtml(resolveProjectBlocker(p, pr), queue)}
      <div class="hb-card-foot">${avStack(projMembers(p), 4)}<a class="hb-card-lead" href="#/projects/${p}">${leadOf(p) ? esc(leadTxt(p)) : "Open project"}${ic("chev-right", "ic14")}</a></div>
    </article>`;
}

function renderTaskTimeSection(st, openCount, idleSpecsN) {
  if (st === "loading") return skel(1, "Analyzing where task time goes…");
  if (st === "error" || st === "empty") return "";
  const agentH = 10.0, ciH = 1.9, queuedH = 18.0, humanH = +(Math.max(0.8, openCount * 0.525)).toFixed(1);
  const totalHrs = +(agentH + ciH + humanH + queuedH).toFixed(1);
  const agentPct = Math.round(agentH * 100 / totalHrs), ciPct = Math.round(ciH * 100 / totalHrs), humanPct = Math.round(humanH * 100 / totalHrs);
  const queuedPct = Math.max(1, 100 - agentPct - ciPct - humanPct);
  const queuedHighCls = queuedPct > 30 ? " is-high" : "";
  const constraint = queuedPct >= 40
    ? `${queuedPct}% of open task time is waiting for an agent. Reconnecting the Build box office and activating ${idleSpecsN} idle specialists will speed up delivery.`
    : humanPct >= 25 ? "Your decisions are the main wait right now. Clearing the Spotlight queue will unblock downstream work."
      : "Work is flowing smoothly across agents, automated checks, and your approvals.";
  const actions = queuedPct >= 40
    ? `<a class="hb-inline-act" href="#/settings/offices">Check Build box office${ic("chev-right", "ic14")}</a><a class="hb-inline-act" href="#/teams/members">Review ${idleSpecsN} idle agents${ic("chev-right", "ic14")}</a>`
    : humanPct >= 25 ? `<a class="hb-inline-act" href="#/inbox">Review pending decisions${ic("chev-right", "ic14")}</a>`
      : `<a class="hb-inline-act" href="#/activity">Open activity log${ic("chev-right", "ic14")}</a>`;
  return `<section class="hb-time" aria-labelledby="hb-time-h">
        <div class="hb-time-hd"><h2 class="hb-time-title" id="hb-time-h"><span class="lbl-std">Where task time goes</span><span class="lbl-rep">Capacity &amp; Bottleneck Analysis</span></h2><span class="hb-time-unit">${totalHrs.toFixed(1)}h total across ${W.filter(OPEN).length} open tasks</span></div>
        <div class="hb-time-bar" role="group" aria-label="Task time breakdown across open tasks">
          <a class="hb-tseg agents" href="#/teams/members" style="width:${agentPct}%" aria-label="Agents working: ${agentH.toFixed(1)}h (${agentPct}%)" title="Agents working: ${agentH.toFixed(1)}h (${agentPct}%)"></a>
          <a class="hb-tseg ci" href="#/activity" style="width:${ciPct}%" aria-label="Automated checks: ${ciH.toFixed(1)}h (${ciPct}%)" title="Automated checks: ${ciH.toFixed(1)}h (${ciPct}%)"></a>
          <a class="hb-tseg humans" href="#/inbox" style="width:${humanPct}%" aria-label="Waiting on you: ${humanH.toFixed(1)}h (${humanPct}%)" title="Waiting on you: ${humanH.toFixed(1)}h (${humanPct}%)"></a>
          <a class="hb-tseg queued${queuedHighCls}" href="#/work" style="width:${queuedPct}%" aria-label="Waiting for an agent: ${queuedH.toFixed(1)}h (${queuedPct}%)" title="Waiting for an agent: ${queuedH.toFixed(1)}h (${queuedPct}%)"></a>
        </div>
        <div class="hb-time-legend">
          <a class="hb-tleg agents" href="#/teams/members"><span class="hb-tleg-dot" aria-hidden="true"></span><strong>${agentPct}%</strong><span class="hrs">${agentH.toFixed(1)}h</span><span>Agents working</span></a>
          <a class="hb-tleg ci" href="#/activity"><span class="hb-tleg-dot" aria-hidden="true"></span><strong>${ciPct}%</strong><span class="hrs">${ciH.toFixed(1)}h</span><span>Automated checks</span></a>
          <a class="hb-tleg humans" href="#/inbox"><span class="hb-tleg-dot" aria-hidden="true"></span><strong>${humanPct}%</strong><span class="hrs">${humanH.toFixed(1)}h</span><span>Waiting on you</span></a>
          <a class="hb-tleg queued${queuedHighCls}" href="#/work"><span class="hb-tleg-dot" aria-hidden="true"></span><strong>${queuedPct}%</strong><span class="hrs">${queuedH.toFixed(1)}h</span><span>Waiting for an agent</span></a>
        </div>
        <div class="hb-time-foot"><p class="hb-time-note">${esc(constraint)}</p><div class="hb-time-acts">${actions}</div></div>
      </section>`;
}

function summarizeHomeFleet(st) {
  const allAgents = Object.values(M).filter((m) => m.kind === "agent");
  const liveWorking = st === "empty" ? [] : Object.keys(M).filter((k) => M[k].kind === "agent" && ["working", "withyou"].includes(M[k].live) && !M[k].paused && !M[k].budgetCapped);
  return {
    totalAgents: allAgents.length,
    leadsN: allAgents.filter((m) => m.tier === "lead").length,
    specsN: allAgents.filter((m) => m.tier === "specialist").length,
    idleSpecsN: allAgents.filter((m) => m.tier === "specialist" && (m.paused || m.budgetCapped || (m.live === "asleep" && !m.proj))).length,
    offlineSpecsN: allAgents.filter((m) => m.setup?.office && OFFICES[m.setup.office]?.status === "Offline").length,
    liveWorkingN: liveWorking.length,
  };
}

function homeProjectsSectionHtml(st, projs, queue, needsAttnProjs) {
  if (st === "error" || st === "empty") return "";
  const countTxt = st === "loading" ? "" : `<span>${projs.length} active${needsAttnProjs ? ` · ${needsAttnProjs} need attention` : ""}</span>`;
  const body = st === "loading" ? homeSkelCards(3, "Loading projects…") : `<div class="hb-bento">${projs.map((p) => renderProjBentoCard(p, queue)).join("")}</div>`;
  return `<div><section class="hb-proj-sec" id="hb-projects" aria-labelledby="hb-projs-h">
        <div class="hb-proj-head"><div class="hb-proj-head-l"><h2 id="hb-projs-h"><span class="lbl-std">Projects</span><span class="lbl-rep">Project Delivery Scorecard</span></h2>${countTxt}</div><a class="link" href="#/projects">All projects${ic("chev-right", "ic16")}</a></div>
        ${body}
      </section></div>`;
}

function pageHome() {
  const st = S.homeState, isEmpty = st === "empty";
  const open = isEmpty ? [] : unhandled(), queue = isEmpty ? [] : spotQueue();
  const riskRank = { "Off track": 0, "At risk": 1, "On track": 2 };
  const projs = isEmpty ? [] : Object.keys(P).filter((p) => !P[p].archived).sort((a, b) => (riskRank[effectiveHealth(a)] ?? 9) - (riskRank[effectiveHealth(b)] ?? 9));
  const offTrackProjs = projs.filter((p) => effectiveHealth(p) === "Off track").length;
  const atRiskProjs = projs.filter((p) => effectiveHealth(p) === "At risk").length;
  if (S.spotIdx >= queue.length || S.spotIdx < 0) S.spotIdx = 0;
  const allQueueCleared = queue.length > 0 && queue.every((i) => i.handled);
  const remainingInInbox = Math.max(0, open.length - queue.filter((i) => !i.handled).length);
  const fleet = summarizeHomeFleet(st);
  const headline = homeHeadline(st, open.filter((i) => i.expires).length, open.length);
  const metricsHtml = renderHomeMetrics(st, fleet, offTrackProjs, atRiskProjs);
  const spotlightHtml = renderHomeSpotlight({ st, queue, open, curItem: queue[S.spotIdx] || null, allQueueCleared, remainingInInbox });
  const mainBody = isEmpty
    ? `<div class="hb-stage is-single">${spotlightHtml}</div>${metricsHtml}`
    : `${metricsHtml}<div class="hb-stage${st === "error" ? " is-single" : ""}">${spotlightHtml}${renderHomeCatchup(st, fleet.liveWorkingN)}</div>`;
  return `<div class="hb"><header class="hb-hero"><div class="hb-brief"><h1 class="hb-title" tabindex="-1">${headline}</h1></div></header>${mainBody}${homeProjectsSectionHtml(st, projs, queue, offTrackProjs + atRiskProjs)}${renderTaskTimeSection(st, open.length, fleet.idleSpecsN)}</div>`;
}

export {
  answerOrgQuestion,
  itemActionsV4,
  pageHome,
};
