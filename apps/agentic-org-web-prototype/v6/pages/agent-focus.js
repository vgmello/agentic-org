import { M } from "../data.js";
import { S } from "../state.js";
import { esc, ic, iconWithTooltip, plural } from "../ui.js";
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

function setText(id, text) {
  const el = document.getElementById(id);
  if (el && el.textContent !== text) el.textContent = text;
}

function focusCornerActionsHtml(m, sess) {
  return `<div class="af-corner-actions" role="toolbar" aria-label="${esc(m.name)} Analysis Mode controls">
    <button type="button" class="btn ghost icon sm icon-with-tooltip af-icon-toggle" id="af-trace-toggle" aria-pressed="${!!S.consoleTools}" aria-label="Show all traces">
      ${iconWithTooltip("Show all traces", "eye")}
    </button>
    <button type="button" class="btn ghost icon sm icon-with-tooltip af-icon-toggle" id="af-tools-toggle" aria-pressed="${!!S.agentFocusTools}" aria-controls="af-tools-drawer" aria-label="Tools & context (${sess.stats.toolCalls})">
      ${iconWithTooltip("Tools & context", "wrench")}
    </button>
    <button type="button" class="btn ghost icon sm icon-with-tooltip af-close-btn" id="af-close-btn" aria-label="Exit Analysis Mode">
      ${iconWithTooltip("Exit Analysis Mode (Esc)", "minimize")}
    </button>
  </div>`;
}

function focusLeftColumnHtml(m, sess, avail) {
  const st = sess.busy || avail.active ? sess.status : { state: avail.state, detail: avail.title.toLowerCase() };
  const pct = Math.round(sess.ratio * 100);
  const canCompact = avail.active && !sess.busy;
  return `<section class="af-orb-col${S.agentFocusTools ? " has-tools" : ""}" id="af-orb-col" aria-label="${esc(m.name)} neural orb">
    <div class="af-orb-wrap">
      <div class="af-orb-stage">
        <canvas id="af-orb-canvas" role="img" aria-label="Neural orb reacting to ${esc(m.name)} output tokens and speed"></canvas>
      </div>
      <div class="af-telemetry" id="af-telemetry" data-cstate="${esc(st.state)}">
        <span class="c-dot"></span>
        <span class="af-tel-lbl">STATE: <b class="c-state-txt" id="af-state-txt">${esc(st.state.toUpperCase())}</b></span>
        <span class="af-tel-sep" aria-hidden="true">|</span>
        <span class="af-speed tnum" id="af-speed"><b id="af-tps">0.4 Hz</b></span>
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

function focusRightColumnHtml(m, avail) {
  const disabled = avail.active ? "" : "disabled";
  const notice = avail.active ? "" : `<div class="callout af-notice" role="status">${ic("info", "ic20")}<div><p style="font-weight:600;color:var(--white)">${esc(avail.title)}</p><p style="font-size:0.8125rem">${esc(avail.reason)}</p></div></div>`;
  const placeholder = avail.active ? `Ask ${esc(m.name)} anything…` : `${esc(avail.title)} — chat unavailable`;
  return `<section class="af-chat-col" aria-label="Chat with ${esc(m.name)}">
    <div class="af-chat-card">
      ${notice}
      <div class="af-messages" id="af-messages">
        <div class="af-thread" id="af-thread"></div>
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

function focusTurnHtml(en) {
  const traceHtml = renderTurnTraceHtml(en);
  const showReply = Boolean(en.reply) || en.phase === "streaming";
  const replyHtml = showReply
    ? `<div class="af-reply-text" data-af-reply="${esc(en.id)}">${esc(en.reply)}${en.streaming ? '<span class="c-caret-blink"></span>' : ""}</div>`
    : "";
  return `<article class="af-turn" data-entry-id="${esc(en.id)}">
    <div class="af-msg-user is-you"><div class="af-user-bubble">${esc(en.text)}</div></div>
    ${replyHtml}
    <div class="af-turn-trace">${traceHtml}</div>
  </article>`;
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
  setText("af-state-txt", st.state.toUpperCase());
  const rate = telemetryRateLabel(st.state, orb, sess);
  setText("af-tps", rate.text);
  const speedWrap = document.getElementById("af-speed");
  if (speedWrap) speedWrap.classList.toggle("is-live", rate.live);
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
  const chatEntries = sess.entries.filter((en) => en.from === "you");
  const shown = chatEntries.length ? chatEntries : sess.entries;
  thread.innerHTML = shown.length
    ? shown.map((en) => focusTurnHtml(en)).join("")
    : `<div class="af-empty"><p>Start a conversation with ${esc(M[aid].name)}.</p></div>`;
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
}

function finalizeCloseFocus(state, restoreRail) {
  const { overlay, orb, triggerEl, sess } = state;
  if (state.tokenRaf) cancelAnimationFrame(state.tokenRaf);
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

function closeAgentFocus(restoreRail = true) {
  if (!focusState) return;
  const state = focusState;
  focusState = null;
  const shell = state.overlay.firstElementChild;
  const reduced = !!(window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches);
  if (!restoreRail || reduced || !shell) {
    finalizeCloseFocus(state, restoreRail);
    return;
  }
  shell.removeAttribute("data-expanded");
  shell.classList.add("is-closing");
  let done = false;
  const finish = () => { if (done) return; done = true; finalizeCloseFocus(state, true); };
  shell.addEventListener("transitionend", (e) => { if (e.target === shell && e.propertyName === "width") finish(); }, { once: true });
  setTimeout(finish, 300);
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
  overlay.innerHTML = `<section class="af-shell" role="dialog" aria-modal="true" aria-label="${esc(m.name)} Analysis Mode" data-agent="${esc(aid)}">
    ${focusCornerActionsHtml(m, sess)}
    <div class="af-stage"><div class="af-body">${focusLeftColumnHtml(m, sess, avail)}${focusRightColumnHtml(m, avail)}</div></div>
  </section>`;
  (document.getElementById("content") || document.body).appendChild(overlay);
  document.body.classList.add("agent-focus-open");
  const shell = overlay.firstElementChild;
  const reduced = !!(window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches);
  if (reduced) shell.setAttribute("data-expanded", "");
  else requestAnimationFrame(() => requestAnimationFrame(() => shell.setAttribute("data-expanded", "")));
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
  const state = { aid, overlay, orb, triggerEl, sess, tokenRaf: 0, pendingEntry: null };
  sess.onChange = () => paintFocusAll(aid, sess, orb, false);
  sess.onTokenFrame = (entry) => {
    state.pendingEntry = entry;
    if (state.tokenRaf) return;
    state.tokenRaf = requestAnimationFrame(() => {
      state.tokenRaf = 0;
      const cur = state.pendingEntry;
      if (!cur || focusState !== state) return;
      const el = document.querySelector(`#af-thread [data-af-reply="${CSS.escape(cur.id)}"]`);
      if (el) el.innerHTML = `${esc(cur.reply)}${cur.streaming ? '<span class="c-caret-blink"></span>' : ""}`;
      paintFocusTelemetry(aid, sess, orb);
      scrollFocusToBottom(false);
    });
  };
  focusState = state;
  bindFocusEvents(shell, aid, sess, orb);
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
