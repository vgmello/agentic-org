import { CH, M, OFFICES, P, PAST_SESSIONS } from "../data.js";
import { agentConversations } from "./chat-model.js";
import { S } from "../state.js";
import { SEP, av, esc, ic, iconWithTooltip, pill, plural, toast } from "../ui.js";

/* ---------------------------------------------------------------- Neural Orb & Agent Profile Console */
const TAU = Math.PI * 2;
const HALF_PI = Math.PI / 2;
const GOLDEN_ANGLE = Math.PI * (3 - Math.sqrt(5));
const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v));
const clamp01 = (v) => clamp(v, 0, 1);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const fmtTok = (t) => (t < 1000 ? String(Math.round(t)) : `${(t / 1000).toFixed(1).replace(/\.0$/, "")}k`);
const fmtDuration = (ms) => (!ms || ms < 100 ? "" : `${(ms / 1000).toFixed(1)}s`);
const isLightTheme = () => document.documentElement.classList.contains("light");

function createSeededRandom(seed) {
  let s = seed;
  return () => { s = (s * 1664525 + 1013904223) % 4294967296; return s / 4294967296; };
}

const prefersReducedMotion = () => !!(window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches);

function isTraceOpen(entryId) {
  const override = S.openTraces[entryId];
  if (override !== undefined) return !!override;
  return !!S.consoleTools;
}

function toggleEntryTrace(entryId) {
  S.openTraces[entryId] = !isTraceOpen(entryId);
  return S.openTraces[entryId];
}

function setGlobalTrace(on) {
  S.consoleTools = !!on;
  S.openTraces = {};
}

function traceMetaText(en) {
  const parts = [];
  const toolCount = en.steps?.length || 0;
  if (toolCount) parts.push(plural(toolCount, "tool", "tools"));
  else if (en.thoughts?.length) parts.push("reasoning");
  const dur = fmtDuration(en.durationMs);
  if (dur) parts.push(dur);
  if (en.tokPerSec && !en.streaming) parts.push(`${Math.round(en.tokPerSec)} tok/s`);
  return parts.join(" · ");
}

function renderTraceBodyHtml(en) {
  const meta = traceMetaText(en);
  const thoughts = (en.thoughts || []).map((t) => `<li class="c-trace-thought"><span class="c-trace-tag">Reasoning</span><span>${esc(t)}</span></li>`).join("");
  const steps = (en.steps || []).map((s) => `<div class="c-step${s.running ? " is-running" : ""}">
    <span class="c-step-dot"></span>
    <span class="c-step-tool">${esc(s.tool)}</span>
    <span class="c-step-arg">${esc(s.arg)}</span>
    <span class="c-step-tok">${s.running ? "running…" : `+${fmtTok(s.tokens)}`}</span>
  </div>${s.out ? `<div class="c-step-out">${esc(s.out)}</div>` : ""}`).join("");
  const emptyNote = !thoughts && !steps ? `<p class="c-trace-empty">Preparing turn trace…</p>` : "";
  return `<div class="c-trace-panel" id="trace-${esc(en.id)}">
    ${meta ? `<div class="c-trace-summary">${esc(meta)}</div>` : ""}
    ${thoughts ? `<ul class="c-trace-thoughts" role="list">${thoughts}</ul>` : ""}
    ${steps ? `<div class="c-steps">${steps}</div>` : ""}
    ${emptyNote}
  </div>`;
}

function renderTurnTraceHtml(en) {
  const processing = en.phase === "thinking" || en.phase === "working";
  const hasTrace = processing || (en.steps?.length || 0) > 0 || (en.thoughts?.length || 0) > 0;
  if (!hasTrace) return "";
  const open = isTraceOpen(en.id);
  const statusChip = processing
    ? `<span class="c-thinking-cue" role="status"><span class="c-thinking-dots" aria-hidden="true"><i></i><i></i><i></i></span><span>Thinking…</span></span>`
    : "";
  const btnLabel = open ? "Hide trace" : "Follow trace";
  return `<div class="c-trace-wrap${open ? " is-open" : ""}" data-trace-entry="${esc(en.id)}">
    <div class="c-trace-bar">
      ${statusChip}
      <button type="button" class="c-trace-toggle" data-trace-toggle="${esc(en.id)}" aria-expanded="${open}" aria-controls="trace-${esc(en.id)}">
        <span class="c-trace-rule" aria-hidden="true"></span>
        <span>${btnLabel}</span>
      </button>
    </div>
    ${open ? renderTraceBodyHtml(en) : ""}
  </div>`;
}

function orbPalette(light) {
  if (light) {
    return {
      auraA: [29, 78, 137], auraB: [14, 116, 144], ring: [29, 78, 137],
      link: [29, 78, 137], spec: "rgba(255,255,255,0.42)",
      dot: (f, a) => [Math.round(18 + f * 24 + a * 34), Math.round(58 + f * 64 + a * 58), Math.round(128 + f * 72 + a * 42)]
    };
  }
  return {
    auraA: [56, 189, 248], auraB: [37, 99, 235], ring: [56, 189, 248],
    link: [125, 211, 252], spec: "rgba(224,242,254,0.45)",
    dot: (f, a) => [Math.round(28 + f * 44 + a * 86), Math.round(92 + f * 104 + a * 52), Math.round(205 + f * 48 + a * 2)]
  };
}

function createGlowSprite(r, g, b) {
  const size = 48, half = size / 2;
  const c = document.createElement("canvas");
  c.width = size; c.height = size;
  const gctx = c.getContext("2d");
  const grad = gctx.createRadialGradient(half, half, 1, half, half, half);
  grad.addColorStop(0, `rgba(${r},${g},${b},1)`);
  grad.addColorStop(0.45, `rgba(${r},${g},${b},0.35)`);
  grad.addColorStop(1, `rgba(${r},${g},${b},0)`);
  gctx.fillStyle = grad;
  gctx.fillRect(0, 0, size, size);
  return c;
}

function buildOrbColorTable(light) {
  const pal = orbPalette(light);
  const cool = [], warm = [];
  for (let i = 0; i < 8; i++) {
    const t = i / 7;
    const [dr, dg, db] = pal.dot(t, t);
    cool.push(`rgb(${dr},${dg},${db})`);
    warm.push(`rgb(${Math.round(dr + (249 - dr) * 0.85)},${Math.round(dg + (115 - dg) * 0.85)},${Math.round(db + (22 - db) * 0.85)})`);
  }
  const coolGlow = createGlowSprite(pal.auraA[0], pal.auraA[1], pal.auraA[2]);
  const warmGlow = createGlowSprite(249, 115, 22);
  return { pal, cool, warm, coolGlow, warmGlow, light };
}

class NeuralOrb {
  constructor(canvas) {
    this.canvas = canvas;
    this.ctx = canvas.getContext("2d");
    this.state = "idle";
    this.fov = 2.85;
    this.fieldPhase = 0;
    this.breathPhase = 0;
    this.energy = 0.22;
    this.targetEnergy = 0.22;
    this.thinking = 0;
    this.targetThinking = 0;
    this.stream = 0;
    this.targetStream = 0;
    this.firing = 0;
    this.spinKick = 0;
    this.tokenTimes = [];
    this.tokensPerSec = 0;
    this.speedFactor = 0;
    this.tokenPulseCount = 0;
    this.contextFill = 0.54;
    this.targetContext = 0.54;
    this.ax = { x: 1, y: 0, z: 0 };
    this.ay = { x: 0, y: 1, z: 0 };
    this.az = { x: 0, y: 0, z: 1 };
    this.focusTheta = -0.35;
    this.focusPhi = HALF_PI;
    this.ripples = [];
    this.nodes = this._buildNodes();
    this.pts = this.nodes.map(() => ({ sx: 0, sy: 0, wz: 0, sr: 0, act: 0, core: false }));
    this.sorted = this.pts.slice();
    this.linkable = [];
    this._colors = buildOrbColorTable(isLightTheme());
    this._raf = null;
    this._lastDraw = 0;
    this.resize();
    this._draw(0.016);
  }
  _buildNodes() {
    const rand = createSeededRandom(7919);
    const out = [];
    const layers = [
      { count: 420, mult: 1, jitter: 0.14, yJit: 0.045, shell: () => 1.0, dot: [0.0125, 0.0045], core: false },
      { count: 96, mult: 1.37, jitter: 0.2, yJit: 0.08, shell: () => 0.46 + rand() * 0.16, dot: [0.010, 0.0035], core: true }
    ];
    for (const L of layers) {
      for (let i = 0; i < L.count; i++) {
        const y0 = 1 - (i / (L.count - 1)) * 2;
        const sr = Math.sqrt(Math.max(0, 1 - y0 * y0));
        const th = GOLDEN_ANGLE * i * L.mult + (rand() - 0.5) * L.jitter;
        const y = y0 + (rand() - 0.5) * L.yJit;
        const nx = Math.cos(th) * sr, nz = Math.sin(th) * sr;
        const len = Math.hypot(nx, y, nz) || 1;
        out.push({ nx: nx / len, ny: y / len, nz: nz / len, shell: L.shell(), dot: L.dot[0] + rand() * L.dot[1], core: L.core, phase: rand() * TAU, act: 0 });
      }
    }
    return out;
  }
  setState(st) {
    const map = {
      idle: { e: 0.22, t: 0, s: 0 },
      paused: { e: 0.08, t: 0, s: 0 },
      thinking: { e: 0.58, t: 1, s: 0 },
      working: { e: 0.64, t: 0.85, s: 0.18 },
      streaming: { e: 0.86, t: 0.15, s: 1 },
      compacting: { e: 0.52, t: 1, s: 0 }
    };
    const cfg = map[st] || map.idle;
    this.state = st;
    this.targetEnergy = cfg.e;
    this.targetThinking = cfg.t;
    this.targetStream = cfg.s;
    if (st !== "streaming") this.tokenTimes = [];
    if (prefersReducedMotion() && !this._raf && this.canvas) {
      this.energy = cfg.e; this.thinking = cfg.t; this.stream = cfg.s;
      this._draw(0.016);
    }
  }
  setContext(ratio, immediate = false) {
    this.targetContext = clamp01(ratio);
    if (immediate || (prefersReducedMotion() && !this._raf)) {
      this.contextFill = this.targetContext;
      if (prefersReducedMotion() && !this._raf && this.canvas) this._draw(0.016);
    }
  }
  pulse(intensity = 1) {
    if (prefersReducedMotion() && !this._raf) return;
    this.firing = Math.min(1, this.firing + 0.45 * intensity);
    const th = (Math.random() - 0.5) * Math.PI * 1.5;
    const ph = HALF_PI + (Math.random() - 0.5) * 2.2;
    this._ripple(Math.sin(ph) * Math.sin(th), Math.cos(ph), -Math.abs(Math.sin(ph) * Math.cos(th)) * 0.85 - 0.15, Math.min(1.3, intensity), 1.85, 0.48);
  }
  onToken(tok = "") {
    const now = performance.now();
    this.tokenTimes.push(now);
    const cutoff = now - 1000;
    while (this.tokenTimes.length && this.tokenTimes[0] < cutoff) this.tokenTimes.shift();
    const instRate = this.tokenTimes.length;
    this.tokensPerSec = this.tokensPerSec ? (this.tokensPerSec * 0.72 + instRate * 0.28) : instRate;
    this.speedFactor = clamp01(this.tokensPerSec / 90);
    this.tokenPulseCount++;
    if (this.state !== "streaming") this.setState("streaming");
    this.targetEnergy = clamp(0.68 + this.speedFactor * 0.32, 0.68, 1);
    this.firing = Math.min(1, this.firing + 0.22 + this.speedFactor * 0.24);
    this.spinKick = Math.min(1.65, this.spinKick + 0.24 + this.speedFactor * 0.38);
    this.focusTheta += 0.36 + this.speedFactor * 0.48 + (Math.random() - 0.5) * 0.26;
    this.focusPhi = HALF_PI + Math.sin(this.fieldPhase * 1.45 + this.focusTheta * 0.68) * 1.24;
    const sp = Math.sin(this.focusPhi);
    const fx = sp * Math.sin(this.focusTheta), fy = Math.cos(this.focusPhi), fz = -Math.abs(sp * Math.cos(this.focusTheta)) * 0.88 - 0.12;
    const isBreak = /[.!?\n]/.test(tok);
    if (isBreak) this._ripple(fx, fy, fz, 1.05 + this.speedFactor * 0.3, 1.75 + this.speedFactor * 1.25, 0.52);
    else if (tok.trim()) this._ripple(fx, fy, fz, 0.58 + this.speedFactor * 0.42, 2.0 + this.speedFactor * 1.8, 0.34);
  }
  _ripple(fx, fy, fz, intensity, speed, width) {
    const l = Math.hypot(fx, fy, fz) || 1;
    this.ripples.push({ fx: fx / l, fy: fy / l, fz: fz / l, r: 0.02, age: 0, speed, intensity, width });
    if (this.ripples.length > 20) this.ripples.shift();
  }
  resize() {
    if (!this.canvas) return;
    const parent = this.canvas.parentElement;
    const rect = parent ? parent.getBoundingClientRect() : { width: 200, height: 200 };
    const w = Math.max(160, Math.round(rect.width || 200));
    const h = Math.max(160, Math.round(rect.height || 200));
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    this.w = w; this.h = h;
    if (this.canvas.width !== Math.round(w * dpr) || this.canvas.height !== Math.round(h * dpr)) {
      this.canvas.width = Math.round(w * dpr);
      this.canvas.height = Math.round(h * dpr);
    }
    this.ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    this._draw(0.016);
  }
  start() {
    if (this._raf) return;
    if (prefersReducedMotion()) {
      this._step(0.016);
      this._draw(0.016);
      return;
    }
    let last = performance.now();
    this._lastDraw = last;
    const tick = (now) => {
      if (!document.body.contains(this.canvas)) { this.stop(); return; }
      this._raf = requestAnimationFrame(tick);
      if (document.hidden) { last = now; return; }
      const active = this.state !== "idle" && this.state !== "paused" || this.ripples.length > 0 || this.firing > 0.03;
      if (!active && now - this._lastDraw < 30) return;
      const dt = clamp((now - last) / 1000, 0.001, 0.05);
      last = now;
      this._lastDraw = now;
      this._step(dt);
      this._draw(dt);
    };
    this._raf = requestAnimationFrame(tick);
  }
  stop() {
    if (this._raf) cancelAnimationFrame(this._raf);
    this._raf = null;
  }
  _decayTokenRate(dt) {
    const cutoff = performance.now() - 1000;
    while (this.tokenTimes.length && this.tokenTimes[0] < cutoff) this.tokenTimes.shift();
    const targetRate = this.state === "streaming" ? this.tokenTimes.length : 0;
    const rateK = 1 - Math.exp(-dt * (targetRate > this.tokensPerSec ? 8 : 3.2));
    this.tokensPerSec += (targetRate - this.tokensPerSec) * rateK;
    if (this.tokensPerSec < 0.25) this.tokensPerSec = 0;
    this.speedFactor = clamp01(this.tokensPerSec / 90);
    this.spinKick *= Math.exp(-dt * 5.2);
  }
  _rotateBasis(spin, nod) {
    const cx = Math.cos(nod), sx = Math.sin(nod), cy = Math.cos(spin), sy = Math.sin(spin);
    const rot = (v) => {
      const y = v.y * cx - v.z * sx, z = v.y * sx + v.z * cx, x = v.x;
      v.x = x * cy + z * sy; v.y = y; v.z = -x * sy + z * cy;
    };
    rot(this.ax); rot(this.ay);
    const norm = (v) => { const l = Math.hypot(v.x, v.y, v.z) || 1; v.x /= l; v.y /= l; v.z /= l; };
    norm(this.ax);
    const dot = this.ax.x * this.ay.x + this.ax.y * this.ay.y + this.ax.z * this.ay.z;
    this.ay.x -= dot * this.ax.x; this.ay.y -= dot * this.ax.y; this.ay.z -= dot * this.ax.z;
    norm(this.ay);
    this.az.x = this.ax.y * this.ay.z - this.ax.z * this.ay.y;
    this.az.y = this.ax.z * this.ay.x - this.ax.x * this.ay.z;
    this.az.z = this.ax.x * this.ay.y - this.ax.y * this.ay.x;
  }
  _step(dt) {
    this._decayTokenRate(dt);
    const k = 1 - Math.exp(-dt * 4.6);
    this.energy += (this.targetEnergy - this.energy) * k;
    this.thinking += (this.targetThinking - this.thinking) * k;
    this.stream += (this.targetStream - this.stream) * k;
    this.contextFill += (this.targetContext - this.contextFill) * k;
    this.firing *= Math.exp(-dt * 3.4);
    if (this.state === "streaming") this.targetEnergy = Math.max(0.54, this.targetEnergy - dt * 0.5);
    const spd = this.speedFactor;
    this.fieldPhase += dt * (0.58 + this.thinking * 0.45 + this.stream * (0.65 + spd * 1.65));
    this.breathPhase += dt * (1.3 + this.thinking * 0.7 + this.stream * (0.45 + spd * 0.95));
    const spin = dt * (0.22 + this.thinking * 0.14 + this.stream * (0.22 + spd * 0.95) + this.spinKick * 0.55);
    const nod = dt * Math.sin(this.breathPhase * 0.35) * (0.06 + spd * 0.08 + this.spinKick * 0.05);
    this._rotateBasis(spin, nod);
    for (let i = this.ripples.length - 1; i >= 0; i--) {
      const rp = this.ripples[i];
      rp.age += dt; rp.r += rp.speed * dt; rp.intensity *= Math.pow(0.32, dt);
      rp.cosLo = Math.cos(Math.min(Math.PI, rp.r + rp.width));
      rp.cosHi = Math.cos(Math.max(0, rp.r - rp.width));
      if (rp.r > Math.PI * 1.15 || rp.intensity < 0.02) this.ripples.splice(i, 1);
    }
  }
  _drawAura(ctx, cx, cy, radius, warmth, pal) {
    const mix = (a, b, t) => Math.round(a + (b - a) * t);
    const spd = this.speedFactor;
    const grad = ctx.createRadialGradient(cx - radius * 0.22, cy - radius * 0.24, radius * 0.12, cx, cy, radius * (1.22 + spd * 0.1));
    grad.addColorStop(0, `rgba(${mix(pal.auraA[0], 249, warmth)},${mix(pal.auraA[1], 115, warmth)},${mix(pal.auraA[2], 22, warmth)},${(0.16 + this.energy * 0.14 + spd * 0.08).toFixed(3)})`);
    grad.addColorStop(0.58, `rgba(${mix(pal.auraB[0], 234, warmth)},${mix(pal.auraB[1], 88, warmth)},${mix(pal.auraB[2], 12, warmth)},${(0.1 + this.energy * 0.08).toFixed(3)})`);
    grad.addColorStop(1, "rgba(30, 41, 57, 0)");
    ctx.fillStyle = grad;
    ctx.beginPath(); ctx.arc(cx, cy, radius * 1.32, 0, TAU); ctx.fill();
    ctx.strokeStyle = `rgba(${mix(pal.ring[0], 251, warmth)},${mix(pal.ring[1], 146, warmth)},${mix(pal.ring[2], 60, warmth)},${(0.16 + this.energy * 0.14).toFixed(3)})`;
    ctx.lineWidth = 1.25;
    ctx.beginPath(); ctx.arc(cx, cy, radius * 1.02, 0, TAU); ctx.stroke();
    if (spd > 0.04) {
      const start = -HALF_PI + this.fieldPhase * 0.4;
      const sweep = Math.max(0.28, spd * TAU * 0.88);
      ctx.strokeStyle = `rgba(${mix(pal.ring[0], 52, 0.35)},${mix(pal.ring[1], 211, 0.35)},${mix(pal.ring[2], 153, 0.35)},${(0.28 + spd * 0.45).toFixed(3)})`;
      ctx.lineWidth = 2.2;
      ctx.lineCap = "round";
      ctx.beginPath(); ctx.arc(cx, cy, radius * 1.08, start, start + sweep); ctx.stroke();
    }
  }
  _rippleCrest(rx, ry, rz) {
    if (!this.ripples.length) return 0;
    let crest = 0;
    for (let i = 0; i < this.ripples.length; i++) {
      const rp = this.ripples[i];
      const dot = rx * rp.fx + ry * rp.fy + rz * rp.fz;
      if (dot < rp.cosLo || dot > rp.cosHi) continue;
      const off = Math.abs(Math.acos(clamp(dot, -1, 1)) - rp.r);
      if (off >= rp.width) continue;
      const wv = Math.cos((off / rp.width) * HALF_PI);
      crest += wv * wv * rp.intensity * Math.min(1, rp.age * 14);
    }
    return crest;
  }
  _projectNodes(dt, cx, cy, radius, fill) {
    const { ax: X, ay: Y, az: Z, fieldPhase, breathPhase, fov } = this;
    const spd = this.speedFactor;
    const fieldGain = 0.28 + this.energy * 0.54 + spd * 0.16;
    const morphAmp = (0.008 + this.thinking * 0.016 + this.firing * 0.056 + spd * 0.028) * 0.38;
    const settle = 1 - Math.pow(0.74, dt * 60);
    const coreExp = 0.24 + fill * 0.68, coreGlow = 0.16 + fill * 0.44, coreDotScale = 0.82 + fill * 0.42;
    const hasRipples = this.ripples.length > 0;
    for (let i = 0; i < this.nodes.length; i++) {
      const n = this.nodes[i];
      const rx = n.nx * X.x + n.ny * Y.x + n.nz * Z.x;
      const ry = n.nx * X.y + n.ny * Y.y + n.nz * Z.y;
      const rz = n.nx * X.z + n.ny * Y.z + n.nz * Z.z;
      const w1 = Math.sin(rx * 2.4 + fieldPhase + Math.cos(ry * 2.1 - fieldPhase * 0.68));
      const w2 = Math.cos(ry * 2.8 - rz * 2.3 + fieldPhase * 0.85 + Math.sin(rz * 2.2 + rx * 1.7 - fieldPhase * 0.55));
      const w3 = Math.sin((rx - ry + rz) * 3.2 - fieldPhase * 1.25);
      const crest = hasRipples ? this._rippleCrest(rx, ry, rz) : 0;
      const drive = Math.max(0, (w1 * 0.48 + w2 * 0.34 + w3 * 0.18 + 0.1) / 1.1) * fieldGain + crest + (n.core ? coreGlow : 0);
      n.act += (Math.min(1.28, drive) - n.act) * settle;
      const breathe = Math.sin(breathPhase + n.phase) * (0.012 + this.thinking * 0.008);
      const lobes = Math.sin(rx * 2.15 + ry * 1.75 + fieldPhase * 1.65) * 0.6 + Math.cos(rz * 2.35 - ry * 1.9 + fieldPhase * 1.35) * 0.4;
      const shell = n.core ? coreExp * (n.shell / 0.56) : n.shell;
      const rad = shell * (1 + breathe + lobes * morphAmp + (n.act * 0.048 + Math.min(1, crest) * 0.038) * 0.38);
      const wx = rx * rad, wy = ry * rad, wz = rz * rad, persp = fov / Math.max(0.8, fov + wz);
      const pt = this.pts[i];
      pt.sx = cx + wx * radius * persp; pt.sy = cy + wy * radius * persp; pt.wz = wz;
      pt.sr = Math.max(0.9, n.dot * radius * persp * (n.core ? coreDotScale : 1) * (0.88 + n.act * 0.48));
      pt.act = n.act; pt.core = n.core;
    }
  }
  _drawLinks(ctx, sorted, radius, fill, warmth, pal) {
    const mix = (a, b, t) => Math.round(a + (b - a) * t);
    const thresh = Math.max(0.42, 0.54 - this.speedFactor * 0.1);
    const linkable = this.linkable;
    linkable.length = 0;
    for (let i = 0; i < sorted.length; i++) {
      const p = sorted[i];
      if (p.wz < 0.15 && (p.core ? fill > 0.35 && p.act > 0.50 : p.act > thresh)) linkable.push(p);
    }
    if (linkable.length < 2) return;
    const maxD = radius * 0.27, maxDSq = maxD * maxD;
    ctx.lineCap = "round";
    ctx.lineWidth = 1.1;
    ctx.strokeStyle = `rgb(${mix(pal.link[0], 253, warmth)},${mix(pal.link[1], 186, warmth)},${mix(pal.link[2], 116, warmth)})`;
    ctx.globalAlpha = 0.28 + this.speedFactor * 0.14;
    ctx.beginPath();
    for (let i = 0; i < linkable.length; i++) {
      const a = linkable[i];
      let links = 0;
      for (let j = i + 1; j < linkable.length && links < 3; j++) {
        const b = linkable[j];
        const dx = b.sx - a.sx, dy = b.sy - a.sy, d2 = dx * dx + dy * dy;
        if (d2 <= 16 || d2 >= maxDSq) continue;
        ctx.moveTo(a.sx, a.sy); ctx.lineTo(b.sx, b.sy);
        links++;
      }
    }
    ctx.stroke();
    ctx.globalAlpha = 1;
  }
  _drawParticles(ctx, sorted, warmth, colors) {
    const table = warmth > 0.45 ? colors.warm : colors.cool;
    const glow = warmth > 0.45 ? colors.warmGlow : colors.coolGlow;
    const spdBoost = this.speedFactor * 0.35;
    for (let i = 0; i < sorted.length; i++) {
      const p = sorted[i];
      const front = clamp01((1.1 - p.wz) / 2.2), act = Math.min(1, p.act);
      const idx = Math.min(7, (front * 4 + act * 3.5) | 0);
      const alpha = Math.min(0.96, (p.core ? 0.26 : 0.30) + front * 0.52 + act * 0.2);
      if (act > 0.45 && front > 0.38) {
        const bloomR = p.sr * (2.1 + act * 0.85 + spdBoost);
        ctx.globalAlpha = Math.min(0.32, (act - 0.38) * front * 0.32);
        ctx.drawImage(glow, p.sx - bloomR, p.sy - bloomR, bloomR * 2, bloomR * 2);
      }
      ctx.globalAlpha = alpha;
      ctx.fillStyle = table[idx];
      ctx.beginPath(); ctx.arc(p.sx, p.sy, p.sr, 0, TAU); ctx.fill();
    }
    ctx.globalAlpha = 1;
  }
  _draw(dt) {
    const { ctx, w, h } = this;
    const light = isLightTheme();
    if (this._colors.light !== light) this._colors = buildOrbColorTable(light);
    const cx = w * 0.5, cy = h * 0.5, radius = Math.min(w, h) * 0.41;
    const fill = this.contextFill;
    const warmth = fill > 0.75 ? clamp01((fill - 0.75) / 0.25) : 0;
    ctx.clearRect(0, 0, w, h);
    this._drawAura(ctx, cx, cy, radius, warmth, this._colors.pal);
    this._projectNodes(dt, cx, cy, radius, fill);
    this.sorted.sort((a, b) => b.wz - a.wz);
    this._drawLinks(ctx, this.sorted, radius, fill, warmth, this._colors.pal);
    this._drawParticles(ctx, this.sorted, warmth, this._colors);
  }
}

function agentSessionAvailability(aid) {
  const m = M[aid];
  if (!m) return { active: false, state: "unavailable", title: "Unknown agent", reason: "Agent record not found." };
  if (m.offboarded) return { active: false, state: "paused", title: "Offboarded", reason: "This agent has been offboarded. Journals, memory, and records remain read-only." };
  if (!m.setup || !m.setup.office) return { active: false, state: "unplaced", title: "Not placed in an office", reason: "Assign an office in Settings before starting a session." };
  const office = OFFICES[m.setup.office];
  if (office && office.status === "Offline") return { active: false, state: "offline", title: `${office.name} is offline`, reason: "Requests wait until the office is back online." };
  if (m.paused) return { active: false, state: "paused", title: "Paused by owner", reason: "Resume this agent before sending prompts or simulating requests." };
  if (m.live === "asleep" && !m.task) return { active: false, state: "asleep", title: "Asleep", reason: m.setup.how || "Wakes when called." };
  return { active: true, state: null, title: "", reason: "" };
}

function tokenizeReply(reply) {
  return reply.match(/[A-Za-z0-9_'-]{1,5}\s*|[^\sA-Za-z0-9_'-]\s*|\n+/g) || [reply];
}

function tokenDelayMs(tok, mode = "normal") {
  if (/\n/.test(tok)) return 95;
  if (/[.!?]\s*$/.test(tok)) return 70;
  if (/[,;:]\s*$/.test(tok)) return 38;
  const base = mode === "fast" ? 11 : mode === "deep" ? 26 : 17;
  return base + Math.round(Math.random() * (mode === "fast" ? 5 : 10));
}

class ConsoleSession {
  constructor(agentId) {
    this.agentId = agentId;
    const m = M[agentId];
    this.limit = 200000;
    this.compactAt = 0.95;
    const unplaced = !m.setup?.office;
    const office = m.setup?.office ? OFFICES[m.setup.office] : null;
    const officeOffline = office?.status === "Offline";
    const dormant = unplaced || officeOffline || (m.live === "asleep" && !m.task);
    if (dormant) this._initDormant(m, unplaced, office, officeOffline);
    else this._initActive(m, agentId);
    this.busy = false;
    this.compacting = false;
    this._aborted = false;
    this.liveTokPerSec = 0;
    this.orb = null;
    this.onChange = null;
    this.onTokenFrame = null;
  }
  _initDormant(m, unplaced, office, officeOffline) {
    this.parts = { system: 0, summary: 0, conversation: 0, tools: 0 };
    this.stats = { requests: 0, toolCalls: 0, compactions: 0 };
    this.status = unplaced
      ? { state: "unplaced", detail: "no office assigned" }
      : officeOffline
        ? { state: "offline", detail: `${office.name} offline` }
        : { state: "asleep", detail: m.setup?.how || "wakes when called" };
    this.current = null;
    this.queue = [];
    this.history = [];
    this.entries = [];
    const msg = unplaced ? `# ${m.name} is not placed in an office yet` : officeOffline ? `# ${office.name} is offline` : `# ${m.name} is asleep`;
    this.shellLines = [{ cls: "c-shell-c", txt: msg }];
  }
  _buildInitialEntries(m, requester) {
    const projName = m.proj ? P[m.proj].name : "assigned organization work";
    return [
      {
        id: "e1", from: requester, kind: "task", time: "18m ago", phase: "done", durationMs: 2400, tokPerSec: 62,
        text: m.task || "Audit workspace contract invariants and report status",
        thoughts: [
          "Locate active project contract files and verify rule constraints before running checks.",
          "Workspace assertions passed cleanly; summarize current sync state concisely."
        ],
        steps: [
          { tool: "search_code", arg: `"${m.proj || "agency"}"`, out: "14 matches across 4 files", tokens: 6400 },
          { tool: "read_file", arg: `${m.setup.home}/RULES.md`, out: "42 lines loaded", tokens: 8200 },
          { tool: "run_tests", arg: "--workspace --quiet", out: "All local assertions passed (1.4s)", tokens: 12900 }
        ],
        reply: `${m.name} (${m.role}) is actively tracking ${projName}.\n\nAll local assertions pass and the branch is synced with HQ. Ask a question below or queue a task to see live execution.`
      },
      {
        id: "e2", from: "you", kind: "chat", time: "4m ago", phase: "done", durationMs: 600, tokPerSec: 78,
        text: "Give me a quick status check before standup.",
        thoughts: ["Inspect active task focus and current context utilization before replying."],
        steps: [],
        reply: `Current focus: ${m.task || "Standing by for incoming tasks"}.\n\nNo blockers on my side—context window is healthy at ${Math.round((this.used / this.limit) * 100)}%.`
      }
    ];
  }
  _initActive(m, agentId) {
    const requester = m.mgr && m.mgr !== "you" ? m.mgr : (agentId === "ada" ? "maya" : "ada");
    this.parts = { system: 13800, summary: 7400, conversation: 9600, tools: 76400 };
    this.stats = { requests: 9, toolCalls: 17, compactions: 1 };
    this.status = { state: m.paused ? "paused" : "working", detail: m.paused ? "paused by owner" : (m.task || "awaiting requests") };
    this.current = !m.paused && m.task ? { id: 101, from: m.mgr || "you", kind: "task", text: m.task } : null;
    this.queue = m.paused ? [] : [
      { id: 102, from: "tomas", kind: "task", text: "Verify branch findings and update test assertions" },
      { id: 103, from: "maya", kind: "chat", text: "Confirm readiness for the v1.0 release checklist" }
    ];
    this.history = [
      { used: 13800, label: "Session start" }, { used: 42100, label: "read_file services/core.rs" },
      { used: 78600, label: "run_tests workspace" }, { used: 148200, label: "git_diff main..HEAD" },
      { used: 33400, label: "Auto-compaction" }, { used: 68900, label: "search_code policy_check" },
      { used: this.used, label: "Current working context" }
    ];
    this.entries = this._buildInitialEntries(m, requester);
    this.shellLines = [
      { cls: "c-shell-c", txt: `# session connected · ${m.name} (${m.setup.runtime} · ${m.setup.model})` },
      { cls: "c-shell-cmd", txt: `cd ~/${m.setup.home} && git status -sb` },
      { cls: "c-shell-out", txt: "## main...origin/main [ahead 1]" },
      { cls: "c-shell-cmd", txt: "rg -n \"contract_version\" crates/" },
      { cls: "c-shell-out", txt: "14 matches across 4 files" }
    ];
  }
  get used() { return this.parts.system + this.parts.summary + this.parts.conversation + this.parts.tools; }
  get ratio() { return this.used / this.limit; }
  notify() { if (this.onChange) this.onChange(); }

  enqueue(from, kind, text, immediate = false) {
    const avail = agentSessionAvailability(this.agentId);
    if (!avail.active) return false;
    const item = { id: Date.now(), from, kind, text };
    if (immediate) this.queue.unshift(item);
    else this.queue.push(item);
    if (this.orb) this.orb.pulse(0.7);
    this.notify();
    this._drain(immediate);
    return true;
  }

  stopRun() {
    this._aborted = true;
    this.queue = [];
    this.current = null;
    this.busy = false;
    this.compacting = false;
    this.liveTokPerSec = 0;
    const streamingEntry = this.entries.find((e) => e.streaming);
    if (streamingEntry) {
      streamingEntry.streaming = false;
      streamingEntry.phase = "stopped";
      streamingEntry.steps.forEach((s) => { s.running = false; });
      streamingEntry.reply = (streamingEntry.reply ? streamingEntry.reply + "\n\n" : "") + "[Stopped by owner]";
    }
    this.shellLines.push({ cls: "c-shell-c", txt: "# run stopped by owner" });
    const avail = agentSessionAvailability(this.agentId);
    this.status = avail.active
      ? { state: "idle", detail: "stopped by owner" }
      : { state: avail.state, detail: avail.title.toLowerCase() };
    if (this.orb) this.orb.setState(avail.active ? "idle" : "paused");
    this.notify();
  }

  async compactNow() {
    const avail = agentSessionAvailability(this.agentId);
    if (!avail.active || this.busy) return false;
    this.busy = true;
    this.compacting = true;
    this._aborted = false;
    const before = this.used;
    this.status = { state: "compacting", detail: "summarizing history" };
    if (this.orb) { this.orb.setState("compacting"); this.orb.pulse(1.2); }
    this.shellLines.push({ cls: "c-shell-c", txt: `# compacting context window (${fmtTok(before)} tokens)…` });
    this.notify();
    try {
      await sleep(1200);
      if (this._aborted || !agentSessionAvailability(this.agentId).active) return false;
      this.parts.system = Math.max(this.parts.system, 13800);
      this.parts.summary = 7200;
      this.parts.conversation = 2800;
      this.parts.tools = 8600;
      this.stats.compactions++;
      this.history.push({ used: this.used, label: "Manual compaction" });
      this.shellLines.push({ cls: "c-shell-c", txt: `# context compacted · ${fmtTok(before)} → ${fmtTok(this.used)} tokens` });
      if (this.orb) { this.orb.setContext(this.ratio); this.orb.setState("idle"); }
      this.status = { state: "idle", detail: "awaiting requests" };
    } finally {
      this.compacting = false;
      this.busy = false;
      this.notify();
    }
    return true;
  }

  async _drain(singleTurn = false) {
    if (this.busy) return;
    this.busy = true;
    this._aborted = false;
    try {
      while (this.queue.length) {
        if (this._aborted || !agentSessionAvailability(this.agentId).active) break;
        const next = this.queue.shift();
        await this._run(next);
        if (singleTurn) break;
      }
    } finally {
      this.busy = false;
      this.current = null;
      this.liveTokPerSec = 0;
      const avail = agentSessionAvailability(this.agentId);
      if (!this._aborted) {
        this.status = avail.active
          ? { state: "idle", detail: "awaiting requests" }
          : { state: avail.state, detail: avail.title.toLowerCase() };
        if (this.orb) this.orb.setState(avail.active ? "idle" : "paused");
      }
      this.notify();
    }
  }

  async _runStep(st, entry) {
    entry.phase = "working";
    this.status = { state: "thinking", detail: "Thinking…" };
    if (this.orb) { this.orb.setState("thinking"); this.orb.pulse(0.65); }
    const stepRec = { tool: st.tool, arg: st.arg, out: "", tokens: st.tokens, running: true };
    entry.steps.push(stepRec);
    this.shellLines.push({ cls: "c-shell-cmd", txt: st.cmd, running: true });
    this.notify();
    await sleep(620);
    this.shellLines[this.shellLines.length - 1].running = false;
    stepRec.running = false;
    if (this._aborted) return false;
    stepRec.out = st.out;
    this.shellLines.push({ cls: "c-shell-out", txt: st.out });
    this.parts.tools += st.tokens;
    this.stats.toolCalls++;
    this.history.push({ used: this.used, label: `${st.tool} ${st.arg}` });
    if (this.orb) this.orb.setContext(this.ratio);
    this.notify();
    return true;
  }

  async _streamReply(reply, entry, cadence = "normal") {
    entry.phase = "streaming";
    this.status = { state: "streaming", detail: "Responding" };
    if (this.orb) this.orb.setState("streaming");
    this.notify();
    const tokens = tokenizeReply(reply);
    const start = performance.now();
    let emitted = 0;
    for (const tok of tokens) {
      if (this._aborted) return false;
      entry.reply += tok;
      emitted++;
      if (this.orb) this.orb.onToken(tok);
      const elapsedSec = Math.max(0.12, (performance.now() - start) / 1000);
      this.liveTokPerSec = this.orb ? Math.round(this.orb.tokensPerSec || emitted / elapsedSec) : Math.round(emitted / elapsedSec);
      entry.tokPerSec = Math.max(entry.tokPerSec || 0, this.liveTokPerSec);
      if (this.onTokenFrame) this.onTokenFrame(entry);
      else this.notify();
      await sleep(tokenDelayMs(tok, cadence));
    }
    return true;
  }

  async _run(item) {
    if (this._aborted) return;
    this.current = item;
    if (this.parts.system === 0) this.parts.system = 13800;
    const t0 = performance.now();
    const entry = {
      id: "e" + item.id, from: item.from, kind: item.kind, time: "just now",
      text: item.text, phase: "thinking", thoughts: [], steps: [], reply: "", streaming: true, durationMs: 0, tokPerSec: 0
    };
    this.entries.push(entry);
    this.status = { state: "thinking", detail: "Thinking…" };
    if (this.orb) { this.orb.setState("thinking"); this.orb.pulse(0.55); }
    this.shellLines.push({ cls: "c-shell-c c-shell-q", txt: `# [ORD-${item.id % 1000}] ${item.kind} from ${M[item.from] ? M[item.from].name : item.from}: ${item.text}` });
    const plan = this._plan(item.text);
    if (plan.thoughts[0]) entry.thoughts.push(plan.thoughts[0]);
    this.notify();
    await sleep(480);
    if (this._aborted) return;
    for (const st of plan.steps) {
      if (this._aborted || !(await this._runStep(st, entry))) return;
    }
    if (plan.thoughts[1]) entry.thoughts.push(plan.thoughts[1]);
    entry.durationMs = Math.round(performance.now() - t0);
    if (!(await this._streamReply(plan.reply, entry, plan.cadence))) return;
    entry.streaming = false;
    entry.phase = "done";
    this.parts.conversation += Math.max(40, Math.round(plan.reply.length / 4));
    this.stats.requests++;
    this.history.push({ used: this.used, label: "Agent reply" });
    this.shellLines.push({ cls: "c-shell-c", txt: "# reply sent" });
    if (this.orb) this.orb.setContext(this.ratio);
    this.notify();
  }

  _plan(text) {
    const m = M[this.agentId];
    if (/fast|burst|speed|summary/i.test(text)) {
      return {
        cadence: "fast",
        thoughts: [
          "Gather branch status and active milestone summary for a rapid readout.",
          "Everything is green; stream a crisp executive breakdown at high cadence."
        ],
        steps: [
          { tool: "search_code", arg: '"release_gate" crates/', cmd: 'rg -n "release_gate" crates/', out: "5 checks active, 0 failing", tokens: 2900 }
        ],
        reply: `Quick sync snapshot for ${m.proj ? P[m.proj].name : "the workspace"}:\n\n• Core contract tests: 100% green across workspace crates.\n• Active slice: ${m.task || "Standing by for incoming tasks"}.\n• Queue depth: ${this.queue.length} pending follow-ups.\n• Context budget: ${Math.round(this.ratio * 100)}% used, plenty of headroom before auto-compaction.`
      };
    }
    if (/working on|status|doing/i.test(text)) {
      return {
        cadence: "normal",
        thoughts: [
          "Read local backlog state to confirm active slice and queued follow-ups.",
          "Synthesize concise status update without dumping raw JSON."
        ],
        steps: [
          { tool: "read_file", arg: `${m.setup.home}/backlog.json`, cmd: `cat ~/${m.setup.home}/backlog.json`, out: "2 queued items, 1 active slice", tokens: 3200 }
        ],
        reply: `Right now I am focused on ${m.task || "pending organization items"}.\n\nI just checked my local backlog: ${this.queue.length} follow-up items are queued and all tests on the working tree are green.`
      };
    }
    if (/custody|lock|lease/i.test(text)) {
      return {
        cadence: "deep",
        thoughts: [
          "Check custody_lock implementation and verify current lease expiry tests.",
          "Compare lease-based auto-expiry against manual crash recovery trade-offs."
        ],
        steps: [
          { tool: "search_code", arg: '"custody_lock" crates/', cmd: 'rg -n "custody_lock" crates/', out: "8 matches in crates/office-sync/src/lock.rs", tokens: 4900 },
          { tool: "run_tests", arg: "office-sync::lock", cmd: "cargo test -p office-sync lock", out: "14 passed, 0 failed (1.8s)", tokens: 7400 }
        ],
        reply: "The lease-based lock automatically expires after 15s if a satellite drops offline, preventing split-brain writes.\n\nPer-office locks avoid lease renewal overhead but require manual recovery on crash. All 14 lock tests pass."
      };
    }
    const kw = (text.match(/[A-Za-z]{4,}/g) || ["workspace"]).sort((a, b) => b.length - a.length)[0];
    return {
      cadence: "normal",
      thoughts: [
        `Scan references to "${kw}" and cross-check recent journal entries.`,
        "No contract drift detected; draft a direct answer with next-step offer."
      ],
      steps: [
        { tool: "search_code", arg: `"${kw}"`, cmd: `rg -n "${kw}" .`, out: "6 matching references found", tokens: 4100 },
        { tool: "read_file", arg: `${m.setup.home}/journal.md`, cmd: `tail -n 20 ~/${m.setup.home}/journal.md`, out: "20 lines read", tokens: 3600 }
      ],
      reply: `Checked "${kw}" across ${m.proj ? P[m.proj].name : "the workspace"}.\n\nEverything aligns with our current milestone constraints. Let me know if you want me to open a work item or push a patch.`
    };
  }
}

const consoleSessions = {};
function getConsoleSession(agentId) {
  if (!consoleSessions[agentId]) consoleSessions[agentId] = new ConsoleSession(agentId);
  return consoleSessions[agentId];
}

let activeOrb = null;
let activeSession = null;
let activeProfileAid = null;
let activeProfileTab = null;
let focusCloser = null;

function registerFocusCloser(fn) { focusCloser = fn; }

function stopRailOrbOnly() {
  if (activeOrb) {
    if (activeOrb._ro) { activeOrb._ro.disconnect(); activeOrb._ro = null; }
    activeOrb.stop();
    activeOrb = null;
  }
}

function stopAgentOrb() {
  if (focusCloser) focusCloser(false);
  stopRailOrbOnly();
  if (activeSession) {
    activeSession.onChange = null;
    activeSession.onTokenFrame = null;
    activeSession.orb = null;
    activeSession = null;
  }
  activeProfileAid = null;
  activeProfileTab = null;
}

function agentRailNowHtml(curNow, avail, canStop) {
  const titleText = curNow ? esc(curNow.text) : (avail.active ? "Nothing in progress" : esc(avail.title));
  const subText = curNow ? "Requested by " + esc(M[curNow.from] ? M[curNow.from].name : curNow.from) : (avail.active ? "Waiting for the next request" : esc(avail.reason));
  return `<div class="c-rail-sec">
      <div class="c-sec-hd">
        <h2 class="c-lbl">Now</h2>
        <div style="display:flex;align-items:center;gap:6px">
          <span id="c-now-kind" class="pill t-indigo" style="padding:1px 7px;font-size:0.75rem" ${curNow ? "" : "hidden"}>${curNow ? esc(curNow.kind) : ""}</span>
          <button type="button" class="btn outline xs" id="c-stop-btn" ${canStop ? "" : "hidden"}>Stop run</button>
        </div>
      </div>
      <div>
        <div id="c-now-title" class="c-now-title${curNow ? "" : " idle"}">${titleText}</div>
        <div id="c-now-sub" class="c-now-sub">${subText}</div>
      </div>
      <div class="c-queue"><h3 class="c-lbl" style="display:block">Up next</h3><ol id="c-queue-list" class="c-queue-list"></ol></div>
    </div>`;
}

function agentRailContextHtml(sess, canCompact) {
  const pct = Math.round(sess.ratio * 100);
  return `<div class="c-rail-sec">
      <div class="c-sec-hd">
        <h2 class="c-lbl">Context window</h2>
        <button type="button" class="btn outline xs" id="c-compact-btn" ${canCompact ? "" : "disabled"} aria-busy="${!!sess.compacting}">${sess.compacting ? "Compacting…" : "Compact now"}</button>
      </div>
      <div class="c-ctx-row">
        <span class="c-ctx-big" id="c-ctx-pct">${pct}%</span>
        <span class="muted mono" id="c-ctx-tok">${fmtTok(sess.used)} / ${fmtTok(sess.limit)}</span>
      </div>
      <div class="c-meter" id="c-meter" data-warn="${sess.ratio >= 0.75}">
        <div class="c-meter-fill" id="c-meter-fill" style="width:${pct}%"></div>
        <span class="c-meter-mark" style="left:95%" title="Auto-compact at 95%"></span>
      </div>
      <figure class="c-chart"><div id="c-chart-svg"></div><figcaption><span class="c-key-line"></span><span>Auto-compact at 95%</span></figcaption></figure>
      <div id="c-breakdown" style="display:flex;flex-direction:column;gap:6px"></div>
      <div class="muted" style="font-size:0.75rem" id="c-stats"></div>
    </div>`;
}

/* Conversations with other agents: every agents' conversation this agent is in, newest first, each opening in Chat
   (read as the owner). */
function agentConversationRow(aid, k) {
  const ch = CH[k], other = ch.pair.find((x) => x !== aid) || aid, last = ch.msgs[ch.msgs.length - 1];
  return `<li><a class="c-a2a-row" href="#/chat/${esc(k)}">${av(other, 24)}<span class="c-a2a-who">With ${esc(M[other].name)}</span>${last ? `<time class="c-a2a-time">${esc(last.time)}</time>` : ""}</a></li>`;
}
function agentConversationsHtml(aid) {
  const keys = agentConversations(aid);
  /* The agent whose page lists them: an opened agents' conversation offers "Back to <agent>". */
  S.chatA2AFrom = aid;
  const body = keys.length
    ? `<ul class="c-a2a-list">${keys.map((k) => agentConversationRow(aid, k)).join("")}</ul>`
    : `<p class="muted c-a2a-empty">No conversations with other agents yet.</p>`;
  return `<section class="c-rail-sec c-a2a" aria-labelledby="c-a2a-h"><div class="c-sec-hd"><h2 class="c-lbl" id="c-a2a-h">Conversations with other agents</h2></div>${body}</section>`;
}

function agentRailHtml(aid, tab) {
  const m = M[aid], sess = getConsoleSession(aid), avail = agentSessionAvailability(aid);
  const st = sess.busy || avail.active ? sess.status : { state: avail.state, detail: avail.title.toLowerCase() };
  const curNow = avail.active ? sess.current : null;
  const expandLabel = `Open ${m.name} in Analysis Mode`;
  const liveLink = tab !== "sessions" && avail.active ? `<a class="link" style="margin-top:10px;font-size:0.75rem" href="#/members/${aid}/sessions">Open live session${ic("chev-right", "ic14")}</a>` : "";
  return `<aside class="agent-rail" aria-label="Agent orb and context">
    <div class="orb-section">
      <button type="button" class="btn ghost icon sm icon-with-tooltip orb-expand-btn" data-act="agent-focus-open" data-id="${aid}" aria-label="${esc(expandLabel)}">${iconWithTooltip("Analysis Mode", "maximize")}</button>
      <button type="button" class="orb-stage orb-stage-btn" data-act="agent-focus-open" data-id="${aid}" aria-label="${esc(expandLabel)}" title="Open Analysis Mode">
        <canvas id="c-orb-canvas" role="img" aria-label="Neural orb reflecting ${esc(m.name)} activity"></canvas>
      </button>
      <div class="telemetry-strip" id="c-telemetry" data-cstate="${esc(st.state)}">
        <span class="c-dot"></span><span class="c-state-txt" id="c-state-txt">${esc(st.state)}</span><span class="muted">·</span><span class="c-detail-txt" id="c-detail-txt">${esc(st.detail)}</span>
      </div>
      ${liveLink}
    </div>
    ${agentRailNowHtml(curNow, avail, avail.active && (sess.busy || !!sess.current))}
    ${agentRailContextHtml(sess, avail.active && !sess.busy)}
    ${agentConversationsHtml(aid)}
  </aside>`;
}

function sessionsSubbarHtml(aid, disabled) {
  const tabs = [["activity", "Activity"], ["chat", "Session prompt"], ["shell", "Shell"]]
    .map(([v, lbl]) => `<button type="button" role="tab" id="ctab-${v}" data-cview="${v}" aria-selected="${S.consoleView === v}" tabindex="${S.consoleView === v ? 0 : -1}">${lbl}</button>`).join("");
  return `<div class="subbar">
        <div class="seg" id="c-tabs" role="tablist" aria-label="Session view">${tabs}</div>
        <span style="flex:1"></span>
        <div id="c-act-filters" style="display:flex;flex-wrap:wrap;gap:8px;align-items:center" ${S.consoleView === "shell" ? "hidden" : ""}>
          <div class="seg" id="c-scope-seg" role="group" aria-label="Filter requests" ${S.consoleView !== "activity" ? "hidden" : ""}>
            <button type="button" data-cscope="all" aria-pressed="${S.consoleScope === "all"}">Everyone</button>
            <button type="button" data-cscope="others" aria-pressed="${S.consoleScope === "others"}">Others only</button>
          </div>
          <button type="button" class="btn outline sm" id="c-trace-global" aria-pressed="${!!S.consoleTools}">${ic("eye", "ic16")}Show traces</button>
          <button type="button" class="btn outline sm" id="c-sim-btn" ${S.consoleView !== "activity" ? "hidden" : ""} ${disabled}>${ic("play", "ic16")}Simulate request</button>
          <button type="button" class="btn outline sm" data-act="agent-focus-open" data-id="${aid}">${ic("maximize", "ic16")}Analysis Mode</button>
        </div>
      </div>`;
}

function sessionsComposerHtml(aid, m, avail, disabled) {
  const chKey = "dm-" + aid; /* Chat opens (or creates) the DM */
  const promptPlaceholder = avail.active ? `Send a prompt to ${esc(m.name)}’s session…` : `${esc(avail.title)} — prompting unavailable`;
  return `<div class="c-composer" id="c-composer" ${S.consoleView !== "chat" ? "hidden" : ""}>
        <div class="c-composer-in">
          <div class="c-chips">
            <span class="muted" style="font-size:0.75rem;font-weight:600">Ask:</span>
            <button type="button" class="c-chip-btn" data-cask="What are you working on right now?" ${disabled}>What are you working on right now?</button>
            <button type="button" class="c-chip-btn" data-cask="Explain the custody lock trade-offs." ${disabled}>Explain the custody lock trade-offs.</button>
          </div>
          <form class="c-form" id="c-prompt-form">
            <label class="sr-only" for="c-prompt-input">Send session prompt to ${esc(m.name)}</label>
            <input class="field" id="c-prompt-input" placeholder="${promptPlaceholder}" autocomplete="off" ${disabled}>
            <button class="btn primary" type="submit" ${disabled}>${ic("send", "ic16")}Send</button>
          </form>
          <div style="display:flex;justify-content:space-between;align-items:center;gap:8px;flex-wrap:wrap;font-size:0.75rem;color:var(--gray-400)">
            <span>Prompts run inside ${esc(m.name)}’s active runtime session and consume context.</span>
            <a class="link" style="font-size:0.75rem" href="#/chat/${chKey}">Open Message DM${ic("chev-right", "ic14")}</a>
          </div>
        </div>
      </div>`;
}

function sessionsStatusNoticeHtml(avail) {
  if (avail.active) return "";
  const icon = { paused: "pause", unplaced: "warn", offline: "warn" }[avail.state] || "info";
  return `<div class="callout" id="c-unavail-callout" role="status">${ic(icon, "ic20")}<div><p style="font-weight:600;color:#fff">${esc(avail.title)}</p><p style="font-size:0.8125rem">${esc(avail.reason)}</p></div></div>`;
}

function renderSessionsPaneHtml(aid, m, avail) {
  const statusNotice = sessionsStatusNoticeHtml(avail);
  const recentHtml = `<ul class="rows" role="list" style="margin:0 calc(var(--gutter) * -1)">${PAST_SESSIONS.map((ps) => `<li>${ic("terminal", "ic20 chev")}<div class="grow">
            <p class="rt"><span class="mono">${esc(ps.id)}</span><span>${esc(ps.label)}</span>${ps.active && avail.active ? pill("Active", "green", "bolt") : ""}</p>
            <p class="rowline">${esc(ps.summary)}</p>
            <p class="meta"><span>${esc(ps.when)}</span>${SEP}<code>agentic-org ${ps.active ? "attach " + esc(aid) : "resume " + esc(ps.id)}</code></p>
          </div></li>`).join("")}</ul>`;
  const disabled = avail.active ? "" : "disabled";
  const liveLabel = avail.active ? `Live session <span class="mono">s-0941</span> · <code>agentic-org attach ${esc(aid)}</code> · ${esc(m.setup.runtime)} (${esc(m.setup.model)})` : `Session offline · <code>agentic-org status ${esc(aid)}</code>`;
  return `${sessionsSubbarHtml(aid, disabled)}
      <div class="c-feed-pane" id="c-feed-pane" ${S.consoleView === "shell" ? "hidden" : ""}>
        ${statusNotice}
        <div class="c-sec-hd" style="padding-bottom:2px"><span class="muted" style="font-size:0.75rem">${liveLabel}</span></div>
        <div id="c-feed-list" style="display:flex;flex-direction:column;gap:12px"></div>
      </div>
      <div class="c-shell-wrap" id="c-shell-pane" ${S.consoleView !== "shell" ? "hidden" : ""}>
        ${statusNotice ? `<div style="margin-bottom:14px">${statusNotice}</div>` : ""}
        <div class="c-shell-pane">
          <div class="c-shell-hd">
            <span>${esc(aid)}@${esc(m.setup.office || "unplaced")}: ~/${esc(m.setup.home)}</span>
            <div style="display:flex;gap:8px">
              <button type="button" class="btn outline xs" id="c-shell-copy">Copy log</button>
              <button type="button" class="btn outline xs" id="c-shell-clear">Clear</button>
            </div>
          </div>
          <div class="c-shell-body" id="c-shell-body"></div>
        </div>
      </div>
      ${sessionsComposerHtml(aid, m, avail, disabled)}
      <section class="section" aria-labelledby="h-past-sess"><div class="sec-h"><h2 id="h-past-sess">Recent sessions</h2></div>${recentHtml}</section>`;
}

function renderSessionChart(sess) {
  const chartEl = document.getElementById("c-chart-svg");
  if (!chartEl) return;
  if (!sess.history.length) {
    chartEl.innerHTML = `<div style="height:78px;display:flex;align-items:center;justify-content:center;color:var(--gray-400);font-size:0.75rem">No session context yet</div>`;
    return;
  }
  const samples = sess.history, W_SVG = 280, H_SVG = 78, padL = 8, padR = 40, padT = 8, padB = 8;
  const last = Math.max(1, samples.length - 1);
  const x = (i) => padL + (i / last) * (W_SVG - padL - padR);
  const y = (r) => padT + (1 - clamp01(r)) * (H_SVG - padT - padB);
  let d = `M${x(0).toFixed(1)} ${y(samples[0].used / sess.limit).toFixed(1)}`;
  for (let i = 1; i < samples.length; i++) d += ` H${x(i).toFixed(1)} V${y(samples[i].used / sess.limit).toFixed(1)}`;
  const endRatio = samples[samples.length - 1].used / sess.limit;
  chartEl.innerHTML = `<svg viewBox="0 0 ${W_SVG} ${H_SVG}" aria-hidden="true">
      <line x1="${padL}" x2="${W_SVG - padR}" y1="${y(0.5)}" y2="${y(0.5)}" stroke="rgba(255,255,255,.07)" stroke-width="1"/>
      <line x1="${padL}" x2="${W_SVG - padR}" y1="${y(0.95)}" y2="${y(0.95)}" stroke="var(--tone-warn)" stroke-width="1" stroke-dasharray="3 2"/>
      <path d="${d}" fill="none" stroke="var(--tone-accent)" stroke-width="1.75"/>
      <circle cx="${x(samples.length - 1)}" cy="${y(endRatio)}" r="3.5" fill="var(--tone-accent)"/>
      <text x="${x(samples.length - 1) + 6}" y="${y(endRatio) + 4}" fill="currentColor" font-family="ui-monospace, monospace" font-size="12">${Math.round(endRatio * 100)}%</text>
    </svg>`;
}

function paintAgentActionButtons(curAvail, sess) {
  const stopBtn = document.getElementById("c-stop-btn");
  if (stopBtn) stopBtn.hidden = !(curAvail.active && (sess.busy || !!sess.current));
  const compactBtn = document.getElementById("c-compact-btn");
  if (!compactBtn) return;
  compactBtn.disabled = !curAvail.active || sess.busy;
  compactBtn.setAttribute("aria-busy", String(!!sess.compacting));
  compactBtn.textContent = sess.compacting ? "Compacting…" : "Compact now";
}

function paintAgentNowAndControls(aid, sess, tel) {
  const curAvail = agentSessionAvailability(aid);
  const curState = sess.busy || curAvail.active ? sess.status : { state: curAvail.state, detail: curAvail.title.toLowerCase() };
  tel.dataset.cstate = curState.state;
  document.getElementById("c-state-txt").textContent = curState.state;
  const detailText = curState.state === "streaming" && sess.liveTokPerSec ? `${sess.liveTokPerSec} tok/s` : curState.detail;
  document.getElementById("c-detail-txt").textContent = detailText;
  const nowKind = document.getElementById("c-now-kind"), nowTitle = document.getElementById("c-now-title"), nowSub = document.getElementById("c-now-sub");
  const curNow = curAvail.active ? sess.current : null;
  nowKind.hidden = !curNow;
  nowTitle.classList.toggle("idle", !curNow);
  if (curNow) {
    nowKind.textContent = curNow.kind;
    nowTitle.textContent = curNow.text;
    nowSub.textContent = "Requested by " + (M[curNow.from] ? M[curNow.from].name : curNow.from);
  } else {
    nowTitle.textContent = curAvail.active ? "Nothing in progress" : curAvail.title;
    nowSub.textContent = curAvail.active ? "Waiting for the next request" : curAvail.reason;
  }
  paintAgentActionButtons(curAvail, sess);
}

function paintAgentContext(sess) {
  const qItems = sess.queue;
  document.getElementById("c-queue-list").innerHTML = qItems.length
    ? qItems.slice(0, 3).map((q) => `<li><span class="c-queue-who">${esc(M[q.from] ? M[q.from].name : q.from)}</span><span class="c-queue-tx">${esc(q.text)}</span></li>`).join("")
    : `<li class="muted">Nothing waiting</li>`;
  const pct = Math.round(sess.ratio * 100);
  document.getElementById("c-ctx-pct").textContent = pct + "%";
  document.getElementById("c-ctx-tok").textContent = `${fmtTok(sess.used)} / ${fmtTok(sess.limit)}`;
  document.getElementById("c-meter").dataset.warn = String(sess.ratio >= 0.75);
  document.getElementById("c-meter-fill").style.width = pct + "%";
  renderSessionChart(sess);
  const bd = [["System & tools", sess.parts.system], ["Summary", sess.parts.summary], ["Conversation", sess.parts.conversation], ["Tool results", sess.parts.tools]];
  document.getElementById("c-breakdown").innerHTML = bd.map(([lbl, val]) => `<div class="c-bd-row"><span class="muted">${lbl}</span><div class="c-bd-bar"><i style="width:${Math.round((val / sess.limit) * 100)}%"></i></div><span class="tnum" style="text-align:right;color:var(--gray-300)">${fmtTok(val)}</span></div>`).join("");
  document.getElementById("c-stats").textContent = `${sess.stats.requests} requests · ${sess.stats.toolCalls} tool calls · ${plural(sess.stats.compactions, "compaction", "compactions")}`;
}

function paintAgentFeed(sess) {
  const feedList = document.getElementById("c-feed-list");
  if (!feedList) return;
  const visible = sess.entries.filter((en) => (S.consoleView === "chat" ? en.from === "you" : S.consoleScope === "others" ? en.from !== "you" : true));
  feedList.innerHTML = visible.length ? visible.map((en) => {
    const traceHtml = renderTurnTraceHtml(en);
    const replyHtml = en.reply || en.phase === "streaming"
      ? `<div class="c-reply" data-entry-reply="${esc(en.id)}">${esc(en.reply)}${en.streaming ? '<span class="c-caret-blink"></span>' : ""}</div>`
      : "";
    return `<article class="c-entry" data-from="${esc(en.from)}"><div class="c-entry-hd"><span class="c-entry-who">${av(en.from, 24)}${esc(M[en.from] ? M[en.from].name : en.from)} ${pill(en.kind, "gray")}</span><span class="muted" style="font-size:0.75rem">${esc(en.time)}</span></div><div class="c-entry-prompt">${esc(en.text)}</div>${replyHtml}${traceHtml}</article>`;
  }).join("") : `<p class="empty-row">No messages yet in this view.</p>`;
}

function paintAgentShell(sess) {
  const shellBody = document.getElementById("c-shell-body");
  if (!shellBody) return;
  shellBody.innerHTML = sess.shellLines.map((l) => l.cls === "c-shell-cmd"
    ? `<div class="c-shell-cmd" data-status="${l.running ? "running" : "done"}"><span class="c-shell-prompt">$ </span>${esc(l.txt)}</div>`
    : `<div class="${l.cls}">${esc(l.txt)}</div>`).join("");
  const sp = document.getElementById("c-shell-pane");
  if (sp && !sp.hidden) shellBody.scrollTop = shellBody.scrollHeight;
}

function paintRailTokenFrame(sess, entry) {
  const replyEl = document.querySelector(`#c-feed-list [data-entry-reply="${CSS.escape(entry.id)}"]`);
  if (replyEl) replyEl.innerHTML = `${esc(entry.reply)}${entry.streaming ? '<span class="c-caret-blink"></span>' : ""}`;
  const detail = document.getElementById("c-detail-txt");
  if (detail && sess.status.state === "streaming" && sess.liveTokPerSec) detail.textContent = `${sess.liveTokPerSec} tok/s`;
}

function bindSessionsListeners(sessionsRoot, aid, sess, avail, paintAll) {
  const setView = (next, focusPrompt = false) => {
    S.consoleView = next;
    sessionsRoot.querySelectorAll("#c-tabs [role='tab']").forEach((b) => { const on = b.dataset.cview === next; b.setAttribute("aria-selected", String(on)); b.tabIndex = on ? 0 : -1; });
    const msgBtn = document.querySelector('.profile-actions button[data-v^="#/chat/"]');
    if (msgBtn) { const demote = next === "chat" && avail.active; msgBtn.classList.toggle("primary", !demote); msgBtn.classList.toggle("outline", demote); }
    document.getElementById("c-act-filters").hidden = next === "shell";
    document.getElementById("c-scope-seg").hidden = next !== "activity";
    document.getElementById("c-sim-btn").hidden = next !== "activity";
    document.getElementById("c-feed-pane").hidden = next === "shell";
    document.getElementById("c-shell-pane").hidden = next !== "shell";
    document.getElementById("c-composer").hidden = next !== "chat";
    paintAll();
    if (focusPrompt && next === "chat" && avail.active) document.getElementById("c-prompt-input").focus();
  };
  document.getElementById("c-tabs").addEventListener("click", (e) => { const b = e.target.closest("button[data-cview]"); if (b) setView(b.dataset.cview, e.detail > 0); });
  document.getElementById("c-scope-seg").addEventListener("click", (e) => {
    const b = e.target.closest("button[data-cscope]"); if (!b) return;
    S.consoleScope = b.dataset.cscope;
    sessionsRoot.querySelectorAll("#c-scope-seg button").forEach((x) => x.setAttribute("aria-pressed", String(x.dataset.cscope === S.consoleScope)));
    paintAll();
  });
  document.getElementById("c-trace-global").addEventListener("click", (e) => {
    setGlobalTrace(!S.consoleTools);
    e.currentTarget.setAttribute("aria-pressed", String(S.consoleTools));
    paintAll();
  });
  document.getElementById("c-feed-list").addEventListener("click", (e) => {
    const btn = e.target.closest("[data-trace-toggle]");
    if (!btn) return;
    toggleEntryTrace(btn.dataset.traceToggle);
    paintAll();
  });
  document.getElementById("c-sim-btn").addEventListener("click", () => {
    if (!agentSessionAvailability(aid).active) return;
    const samples = [["rowan", "task", "Check custody lock lease renewal timings"], ["tomas", "task", "Verify test coverage on sync recovery"], ["maya", "chat", "Give me a fast release status summary"]];
    const [from, kind, text] = samples[Math.floor(Math.random() * samples.length)];
    sess.enqueue(from, kind, text);
  });
  document.getElementById("c-shell-clear").addEventListener("click", () => { sess.shellLines = [{ cls: "c-shell-c", txt: "# terminal output cleared" }]; paintAll(); });
  document.getElementById("c-shell-copy").addEventListener("click", () => {
    const ta = document.createElement("textarea");
    ta.value = document.getElementById("c-shell-body").innerText;
    ta.style.cssText = "position:fixed;opacity:0";
    document.body.appendChild(ta); ta.select();
    try { document.execCommand("copy"); toast("Shell log copied.", "check-circle"); } finally { ta.remove(); }
  });
  document.getElementById("c-prompt-form").addEventListener("submit", (e) => {
    e.preventDefault();
    if (!agentSessionAvailability(aid).active) return;
    const inp = document.getElementById("c-prompt-input"), txt = inp.value.trim();
    if (!txt) return;
    inp.value = "";
    sess.enqueue("you", "chat", txt);
  });
  sessionsRoot.querySelectorAll("[data-cask]").forEach((btn) => {
    btn.addEventListener("click", () => { if (agentSessionAvailability(aid).active) sess.enqueue("you", "chat", btn.dataset.cask); });
  });
}

function attachRailSession(aid) {
  const canvas = document.getElementById("c-orb-canvas");
  if (!canvas) return null;
  stopRailOrbOnly();
  const sess = getConsoleSession(aid), avail = agentSessionAvailability(aid);
  activeSession = sess;
  const orb = new NeuralOrb(canvas);
  orb.setState(avail.active ? sess.status.state : "paused");
  orb.setContext(sess.ratio, true);
  orb.start();
  if (avail.active) orb.pulse(0.65);
  requestAnimationFrame(() => orb.resize());
  if ("ResizeObserver" in window && canvas.parentElement) {
    const ro = new ResizeObserver(() => orb.resize());
    ro.observe(canvas.parentElement);
    orb._ro = ro;
  }
  activeOrb = orb;
  sess.orb = orb;
  const paintAll = () => {
    const tel = document.getElementById("c-telemetry");
    if (!tel) return;
    paintAgentNowAndControls(aid, sess, tel);
    paintAgentContext(sess);
    paintAgentFeed(sess);
    paintAgentShell(sess);
    const gt = document.getElementById("c-trace-global");
    if (gt) gt.setAttribute("aria-pressed", String(!!S.consoleTools));
  };
  sess.onChange = paintAll;
  sess.onTokenFrame = (entry) => paintRailTokenFrame(sess, entry);
  paintAll();
  return { sess, avail, paintAll };
}

function mountAgentProfile(aid, tab) {
  stopAgentOrb();
  const canvas = document.getElementById("c-orb-canvas");
  if (!canvas) return;
  activeProfileAid = aid;
  activeProfileTab = tab;
  const m = M[aid], avail = agentSessionAvailability(aid);
  const sessionsRoot = document.getElementById("agent-sessions-root");
  if (sessionsRoot && tab === "sessions") sessionsRoot.innerHTML = renderSessionsPaneHtml(aid, m, avail);
  const attached = attachRailSession(aid);
  if (!attached) return;
  document.getElementById("c-compact-btn")?.addEventListener("click", () => attached.sess.compactNow());
  document.getElementById("c-stop-btn")?.addEventListener("click", () => attached.sess.stopRun());
  if (sessionsRoot && tab === "sessions") bindSessionsListeners(sessionsRoot, aid, attached.sess, attached.avail, attached.paintAll);
}

function resumeRailOrbAfterFocus() {
  if (activeProfileAid && document.getElementById("c-orb-canvas")) attachRailSession(activeProfileAid);
}

export {
  NeuralOrb,
  agentRailHtml,
  agentSessionAvailability,
  consoleSessions,
  fmtDuration,
  fmtTok,
  getConsoleSession,
  isTraceOpen,
  mountAgentProfile,
  registerFocusCloser,
  renderTurnTraceHtml,
  resumeRailOrbAfterFocus,
  setGlobalTrace,
  stopAgentOrb,
  stopRailOrbOnly,
  toggleEntryTrace,
};
