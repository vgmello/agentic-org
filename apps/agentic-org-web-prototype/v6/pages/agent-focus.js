import { M } from "../data.js";
import { S } from "../state.js";
import { av, esc, ic, iconWithTooltip, pill, plural } from "../ui.js";
import {
  NeuralOrb,
  agentSessionAvailability,
  fmtTok,
  getConsoleSession,
  registerFocusCloser,
  renderTurnTraceHtml,
  resumeRailOrbAfterFocus,
  setGlobalTrace,
  stopRailOrbOnly,
  toggleEntryTrace,
} from "./agent-profile.js";

/* ---------------------------------------------------------------- Full-Screen Agent Orb & Chat Focus View */
let focusState = null;

function isAgentFocusOpen() {
  return !!focusState;
}

function focusTopBarHtml(aid, m) {
  const modelMeta = m.setup ? `${m.setup.runtime} · ${m.setup.model}` : "";
  return `<header class="af-topbar">
    <div class="af-who">
      ${av(aid, 32, "word")}
      <div class="af-who-txt">
        <h2 id="af-title">${esc(m.name)} <span class="af-role">${esc(m.role)}</span></h2>
        ${modelMeta ? `<p class="af-meta">${esc(modelMeta)}</p>` : ""}
      </div>
    </div>
    <div class="af-header-actions">
      <button type="button" class="btn ghost icon sm icon-with-tooltip af-close-btn" id="af-close-btn" aria-label="Restore agent profile">
        ${iconWithTooltip("Restore agent profile", "minimize")}
      </button>
    </div>
  </header>`;
}

function focusLeftColumnHtml(m, sess, avail) {
  const st = sess.busy || avail.active ? sess.status : { state: avail.state, detail: avail.title.toLowerCase() };
  const pct = Math.round(sess.ratio * 100);
  const canCompact = avail.active && !sess.busy;
  return `<section class="af-orb-col${S.agentFocusTools ? " has-tools" : ""}" id="af-orb-col" aria-label="${esc(m.name)} neural orb and live telemetry">
    <div class="af-orb-wrap">
      <div class="af-orb-stage">
        <canvas id="af-orb-canvas" role="img" aria-label="Neural orb reacting to ${esc(m.name)} output tokens and speed"></canvas>
      </div>
      <div class="af-telemetry" id="af-telemetry" data-cstate="${esc(st.state)}">
        <span class="c-dot"></span>
        <span class="af-tel-lbl">STATE: <b class="c-state-txt" id="af-state-txt">${esc(st.state)}</b></span>
        <span class="af-tel-sep" aria-hidden="true">|</span>
        <span class="af-speed tnum" id="af-speed"><b id="af-tps">0.4 Hz</b></span>
      </div>
      <div class="af-live-actions">
        <button type="button" class="btn outline sm" id="af-stop-btn" ${avail.active && sess.busy ? "" : "hidden"}>${ic("pause", "ic16")}Stop turn</button>
      </div>
    </div>
    <div class="af-tools-drawer" id="af-tools-drawer" ${S.agentFocusTools ? "" : "hidden"}>
      <div class="af-tools-sec">
        <div class="c-sec-hd">
          <h3 class="c-lbl">Context window</h3>
          <button type="button" class="btn outline xs" id="af-compact-btn" ${canCompact ? "" : "disabled"}>${sess.compacting ? "Compacting…" : "Compact"}</button>
        </div>
        <div class="c-ctx-row">
          <span class="af-ctx-pct tnum" id="af-ctx-pct">${pct}%</span>
          <span class="muted mono" id="af-ctx-tok">${fmtTok(sess.used)} / ${fmtTok(sess.limit)}</span>
        </div>
        <div class="c-meter" id="af-meter" data-warn="${sess.ratio >= 0.75}">
          <div class="c-meter-fill" id="af-meter-fill" style="width:${pct}%"></div>
        </div>
      </div>
      <div class="af-tools-sec">
        <div class="c-sec-hd"><h3 class="c-lbl">Recent tool calls</h3><span class="muted tnum" id="af-tools-total">${plural(sess.stats.toolCalls, "call", "calls")}</span></div>
        <div class="af-tools-list" id="af-tools-list"></div>
      </div>
      <div class="af-tools-sec">
        <div class="c-sec-hd"><h3 class="c-lbl">Up next</h3></div>
        <ol class="c-queue-list" id="af-queue-list"></ol>
      </div>
    </div>
  </section>`;
}

function focusRightColumnHtml(m, sess, avail) {
  const disabled = avail.active ? "" : "disabled";
  const notice = avail.active ? "" : `<div class="callout af-notice" role="status">${ic("info", "ic20")}<div><p style="font-weight:600;color:var(--white)">${esc(avail.title)}</p><p style="font-size:0.8125rem">${esc(avail.reason)}</p></div></div>`;
  const placeholder = avail.active ? `Ask ${esc(m.name)} anything…` : `${esc(avail.title)} — chat unavailable`;
  return `<section class="af-chat-col" aria-label="Chat with ${esc(m.name)}">
    <div class="af-chat-card">
      <div class="af-card-head">
        <div class="af-card-title">
          <span class="af-card-eyebrow">${esc(m.name.toUpperCase())} CONVERSATION</span>
          <span class="af-tok-pill tnum" id="af-turn-tokens" hidden>0 tokens</span>
        </div>
        <div class="af-card-controls">
          <button type="button" class="btn outline xs af-toggle" id="af-trace-toggle" aria-pressed="${!!S.consoleTools}">
            ${ic("eye", "ic14")}<span>Show traces</span>
          </button>
          <button type="button" class="btn outline xs af-toggle" id="af-tools-toggle" aria-pressed="${!!S.agentFocusTools}" aria-controls="af-tools-drawer">
            ${ic("wrench", "ic14")}<span>Tools</span><span class="af-badge tnum" id="af-tools-count">${sess.stats.toolCalls}</span>
          </button>
        </div>
      </div>
      ${notice}
      <div class="af-messages" id="af-messages">
        <div class="af-thread" id="af-thread"></div>
      </div>
      <div class="af-prompts-bar">
        <span class="af-prompts-lbl">PROMPTS:</span>
        <div class="c-chips">
          <button type="button" class="c-chip-btn" data-af-ask="Give me a fast release status summary." ${disabled}>Fast Status Summary</button>
          <button type="button" class="c-chip-btn" data-af-ask="Explain the custody lock trade-offs." ${disabled}>Custody Lock Trade-Offs</button>
          <button type="button" class="c-chip-btn" data-af-ask="What are you working on right now?" ${disabled}>Current Focus</button>
        </div>
      </div>
    </div>
    <form class="af-composer-pill" id="af-prompt-form">
      <label class="sr-only" for="af-prompt-input">Message ${esc(m.name)}</label>
      <input class="af-input" id="af-prompt-input" placeholder="${placeholder}" autocomplete="off" ${disabled}>
      <button type="button" class="btn outline sm" id="af-composer-stop" hidden>${ic("pause", "ic16")}Stop</button>
      <button type="submit" class="btn primary af-send-btn" id="af-send-btn" ${disabled}>Send</button>
    </form>
  </section>`;
}

function focusTurnHtml(aid, en) {
  const isYou = en.from === "you";
  const whoName = M[en.from] ? M[en.from].name : en.from;
  const userMeta = isYou
    ? `<span class="af-turn-time">${esc(en.time)}</span>`
    : `<span class="af-turn-who">${av(en.from, 20)}${esc(whoName)} ${pill(en.kind, "gray")}</span><span class="af-turn-time">${esc(en.time)}</span>`;
  const traceHtml = renderTurnTraceHtml(en);
  const showReply = Boolean(en.reply) || en.phase === "streaming";
  const replyHtml = showReply
    ? `<div class="af-msg-agent">
        ${av(aid, 32)}
        <div class="af-agent-body">
          <div class="af-reply-text" data-af-reply="${esc(en.id)}">${esc(en.reply)}${en.streaming ? '<span class="c-caret-blink"></span>' : ""}</div>
        </div>
      </div>`
    : "";
  return `<article class="af-turn" data-entry-id="${esc(en.id)}">
    <div class="af-msg-user${isYou ? " is-you" : ""}">
      <div class="af-user-meta">${userMeta}</div>
      <div class="af-user-bubble">${esc(en.text)}</div>
    </div>
    <div class="af-turn-between">${traceHtml}</div>
    ${replyHtml}
  </article>`;
}

function setText(id, text) {
  const el = document.getElementById(id);
  if (el) el.textContent = text;
}

function paintFocusContextSection(sess, avail) {
  const pct = Math.round(sess.ratio * 100);
  setText("af-ctx-pct", `${pct}%`);
  setText("af-ctx-tok", `${fmtTok(sess.used)} / ${fmtTok(sess.limit)}`);
  const meter = document.getElementById("af-meter");
  if (meter) meter.dataset.warn = String(sess.ratio >= 0.75);
  const fill = document.getElementById("af-meter-fill");
  if (fill) fill.style.width = `${pct}%`;
  const compactBtn = document.getElementById("af-compact-btn");
  if (!compactBtn) return;
  compactBtn.disabled = !avail.active || sess.busy;
  compactBtn.textContent = sess.compacting ? "Compacting…" : "Compact";
}

function paintFocusToolsDrawer(sess, avail) {
  paintFocusContextSection(sess, avail);
  setText("af-tools-count", String(sess.stats.toolCalls));
  setText("af-tools-total", plural(sess.stats.toolCalls, "call", "calls"));
  const allSteps = sess.entries.flatMap((en) => en.steps || []).slice(-6).reverse();
  const listEl = document.getElementById("af-tools-list");
  if (listEl) {
    listEl.innerHTML = allSteps.length
      ? allSteps.map((s) => `<div class="c-step"><span class="c-step-dot"></span><span class="c-step-tool">${esc(s.tool)}</span><span class="c-step-arg">${esc(s.arg)}</span><span class="c-step-tok">+${fmtTok(s.tokens)}</span></div>`).join("")
      : `<p class="muted" style="font-size:0.75rem">No tools called yet.</p>`;
  }
  const qEl = document.getElementById("af-queue-list");
  if (qEl) {
    qEl.innerHTML = sess.queue.length
      ? sess.queue.slice(0, 3).map((q) => `<li><span class="c-queue-who">${esc(M[q.from] ? M[q.from].name : q.from)}</span><span class="c-queue-tx">${esc(q.text)}</span></li>`).join("")
      : `<li class="muted">Nothing queued</li>`;
  }
}

function telemetryRateLabel(stState, orb, sess) {
  if (stState === "streaming") {
    const tps = Math.max(1, Math.round(orb?.tokensPerSec || sess.liveTokPerSec || 0));
    return { text: `${tps} tok/s`, live: true };
  }
  if (stState === "thinking" || stState === "working") return { text: "1.4 Hz", live: false };
  if (stState === "compacting") return { text: "1.1 Hz", live: false };
  if (stState === "idle") return { text: "0.4 Hz", live: false };
  return { text: "—", live: false };
}

function paintFocusTelemetry(aid, sess, orb) {
  const avail = agentSessionAvailability(aid);
  const st = sess.busy || avail.active ? sess.status : { state: avail.state, detail: avail.title.toLowerCase() };
  const tel = document.getElementById("af-telemetry");
  if (tel) tel.dataset.cstate = st.state;
  const stEl = document.getElementById("af-state-txt");
  if (stEl) stEl.textContent = st.state.toUpperCase();
  const rate = telemetryRateLabel(st.state, orb, sess);
  const tpsEl = document.getElementById("af-tps");
  if (tpsEl) tpsEl.textContent = rate.text;
  const speedWrap = document.getElementById("af-speed");
  if (speedWrap) speedWrap.classList.toggle("is-live", rate.live);
  const last = sess.entries[sess.entries.length - 1];
  const tokPill = document.getElementById("af-turn-tokens");
  if (tokPill) {
    const estTokens = last?.reply ? Math.max(1, Math.round(last.reply.length / 4)) : 0;
    tokPill.hidden = !estTokens;
    if (estTokens) tokPill.textContent = `${estTokens} tokens`;
  }
  const stopBtn = document.getElementById("af-stop-btn");
  if (stopBtn) stopBtn.hidden = !(avail.active && sess.busy);
  const cStop = document.getElementById("af-composer-stop");
  if (cStop) cStop.hidden = !(avail.active && sess.busy);
}

function scrollFocusToBottom(force = false) {
  const box = document.getElementById("af-messages");
  if (!box) return;
  const nearBottom = box.scrollHeight - box.scrollTop - box.clientHeight < 120;
  if (force || nearBottom) box.scrollTop = box.scrollHeight;
}

function paintFocusAll(aid, sess, orb, forceScroll = false) {
  const avail = agentSessionAvailability(aid);
  paintFocusTelemetry(aid, sess, orb);
  paintFocusToolsDrawer(sess, avail);
  const thread = document.getElementById("af-thread");
  if (!thread) return;
  thread.innerHTML = sess.entries.length
    ? sess.entries.map((en) => focusTurnHtml(aid, en)).join("")
    : `<div class="af-empty"><p>Start a conversation with ${esc(M[aid].name)}.</p><p class="muted">Watch the orb respond live to every output token and speed change.</p></div>`;
  const traceBtn = document.getElementById("af-trace-toggle");
  if (traceBtn) traceBtn.setAttribute("aria-pressed", String(!!S.consoleTools));
  scrollFocusToBottom(forceScroll);
}

function bindFocusEvents(shell, aid, sess, orb) {
  const paint = (forceScroll = false) => paintFocusAll(aid, sess, orb, forceScroll);
  shell.querySelector("#af-close-btn").addEventListener("click", () => closeAgentFocus(true));
  shell.querySelector("#af-trace-toggle").addEventListener("click", () => {
    setGlobalTrace(!S.consoleTools);
    paint(false);
  });
  shell.querySelector("#af-tools-toggle").addEventListener("click", (e) => {
    S.agentFocusTools = !S.agentFocusTools;
    e.currentTarget.setAttribute("aria-pressed", String(S.agentFocusTools));
    document.getElementById("af-tools-drawer").hidden = !S.agentFocusTools;
    document.getElementById("af-orb-col").classList.toggle("has-tools", S.agentFocusTools);
    requestAnimationFrame(() => orb.resize());
  });
  shell.querySelector("#af-stop-btn").addEventListener("click", () => sess.stopRun());
  shell.querySelector("#af-composer-stop").addEventListener("click", () => sess.stopRun());
  shell.querySelector("#af-compact-btn").addEventListener("click", () => sess.compactNow());
  shell.querySelector("#af-thread").addEventListener("click", (e) => {
    const btn = e.target.closest("[data-trace-toggle]");
    if (!btn) return;
    toggleEntryTrace(btn.dataset.traceToggle);
    paint(false);
  });
  shell.querySelector("#af-prompt-form").addEventListener("submit", (e) => {
    e.preventDefault();
    if (!agentSessionAvailability(aid).active) return;
    const inp = document.getElementById("af-prompt-input"), txt = inp.value.trim();
    if (!txt) return;
    inp.value = "";
    sess.enqueue("you", "chat", txt, true);
    scrollFocusToBottom(true);
  });
  shell.querySelectorAll("[data-af-ask]").forEach((btn) => {
    btn.addEventListener("click", () => {
      if (!agentSessionAvailability(aid).active) return;
      sess.enqueue("you", "chat", btn.dataset.afAsk, true);
      scrollFocusToBottom(true);
    });
  });
}

function closeAgentFocus(restoreRail = true) {
  if (!focusState) return;
  const { overlay, orb, meterTimer, triggerEl, sess } = focusState;
  focusState = null;
  clearInterval(meterTimer);
  if (orb._ro) { orb._ro.disconnect(); orb._ro = null; }
  orb.stop();
  overlay.remove();
  document.body.classList.remove("agent-focus-open");
  if (sess && sess.orb === orb) {
    sess.orb = null;
    sess.onChange = null;
    sess.onTokenFrame = null;
  }
  if (restoreRail) {
    resumeRailOrbAfterFocus();
    if (triggerEl?.isConnected) triggerEl.focus({ preventScroll: true });
  }
}

function openAgentFocus(aid, triggerEl = null) {
  const m = M[aid];
  if (!m) return;
  if (focusState) closeAgentFocus(false);
  stopRailOrbOnly();
  const sess = getConsoleSession(aid);
  const avail = agentSessionAvailability(aid);
  const overlay = document.createElement("div");
  overlay.id = "agent-focus-root";
  overlay.innerHTML = `<section class="af-shell" role="dialog" aria-modal="true" aria-labelledby="af-title" data-agent="${esc(aid)}">
    ${focusTopBarHtml(aid, m)}
    <div class="af-stage"><div class="af-body">${focusLeftColumnHtml(m, sess, avail)}${focusRightColumnHtml(m, sess, avail)}</div></div>
  </section>`;
  document.body.appendChild(overlay);
  document.body.classList.add("agent-focus-open");
  const canvas = document.getElementById("af-orb-canvas");
  const orb = new NeuralOrb(canvas);
  orb.setState(avail.active ? sess.status.state : "paused");
  orb.setContext(sess.ratio, true);
  orb.start();
  if (avail.active) orb.pulse(0.8);
  requestAnimationFrame(() => orb.resize());
  if ("ResizeObserver" in window && canvas.parentElement) {
    const ro = new ResizeObserver(() => orb.resize());
    ro.observe(canvas.parentElement);
    orb._ro = ro;
  }
  sess.orb = orb;
  sess.onChange = () => paintFocusAll(aid, sess, orb, false);
  sess.onTokenFrame = (entry) => {
    const el = document.querySelector(`#af-thread [data-af-reply="${CSS.escape(entry.id)}"]`);
    if (el) el.innerHTML = `${esc(entry.reply)}${entry.streaming ? '<span class="c-caret-blink"></span>' : ""}`;
    paintFocusTelemetry(aid, sess, orb);
    scrollFocusToBottom(false);
  };
  const meterTimer = setInterval(() => paintFocusTelemetry(aid, sess, orb), 140);
  focusState = { aid, overlay, orb, meterTimer, triggerEl, sess };
  bindFocusEvents(overlay.firstElementChild, aid, sess, orb);
  paintFocusAll(aid, sess, orb, true);
  const input = document.getElementById("af-prompt-input");
  (input && !input.disabled ? input : document.getElementById("af-close-btn"))?.focus({ preventScroll: true });
}

registerFocusCloser(closeAgentFocus);

export {
  closeAgentFocus,
  isAgentFocusOpen,
  openAgentFocus,
};
