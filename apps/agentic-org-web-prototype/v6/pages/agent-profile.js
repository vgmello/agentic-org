import { CH, M, OFFICES, P, PAST_SESSIONS } from "../data.js";
import { S } from "../state.js";
import { SEP, av, esc, ic, pill, plural, toast } from "../ui.js";

/* ---------------------------------------------------------------- Neural Orb & Agent Profile Console */
const TAU = Math.PI * 2;
const HALF_PI = Math.PI / 2;
const GOLDEN_ANGLE = Math.PI * (3 - Math.sqrt(5));
const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v));
const clamp01 = (v) => clamp(v, 0, 1);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const fmtTok = (t) => t < 1000 ? String(Math.round(t)) : `${(t / 1000).toFixed(1).replace(/\.0$/, "")}k`;

function createSeededRandom(seed) {
  let s = seed;
  return () => { s = (s * 1664525 + 1013904223) % 4294967296; return s / 4294967296; };
}

const prefersReducedMotion = () => !!(window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches);

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
    this._raf = null;
    this.resize();
    this._draw(0.016);
  }
  _buildNodes() {
    const rand = createSeededRandom(7919);
    const out = [];
    const layers = [
      { count: 400, mult: 1, jitter: 0.14, yJit: 0.045, shell: () => 1.0, dot: [0.017, 0.005], core: false },
      { count: 90, mult: 1.37, jitter: 0.2, yJit: 0.08, shell: () => 0.48 + rand() * 0.14, dot: [0.014, 0.004], core: true }
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
      working: { e: 0.74, t: 0.6, s: 0.35 },
      streaming: { e: 0.9, t: 0.2, s: 1 },
      compacting: { e: 0.52, t: 1, s: 0 }
    };
    const cfg = map[st] || map.idle;
    this.state = st;
    this.targetEnergy = cfg.e;
    this.targetThinking = cfg.t;
    this.targetStream = cfg.s;
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
  onToken(ch = "") {
    if (this.state !== "streaming") this.setState("streaming");
    this.targetEnergy = 0.94;
    this.firing = Math.min(1, this.firing + 0.24);
    this.focusTheta += 0.42 + (Math.random() - 0.5) * 0.32;
    this.focusPhi = HALF_PI + Math.sin(this.fieldPhase * 1.35 + this.focusTheta * 0.65) * 1.22;
    const sp = Math.sin(this.focusPhi);
    const fx = sp * Math.sin(this.focusTheta), fy = Math.cos(this.focusPhi), fz = -Math.abs(sp * Math.cos(this.focusTheta)) * 0.88 - 0.12;
    if (/[.!?\n]/.test(ch)) this._ripple(fx, fy, fz, 1.15, 1.65, 0.54);
    else if (ch.trim()) this._ripple(fx, fy, fz, 0.65, 2.2, 0.36);
  }
  _ripple(fx, fy, fz, intensity, speed, width) {
    const l = Math.hypot(fx, fy, fz) || 1;
    this.ripples.push({ fx: fx / l, fy: fy / l, fz: fz / l, r: 0.02, age: 0, speed, intensity, width });
    if (this.ripples.length > 14) this.ripples.shift();
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
    if (prefersReducedMotion() && !this._raf) this._draw(0.016);
  }
  start() {
    if (this._raf) return;
    if (prefersReducedMotion()) {
      this._step(0.016);
      this._draw(0.016);
      return;
    }
    let last = performance.now();
    const tick = (now) => {
      if (!document.body.contains(this.canvas)) { this.stop(); return; }
      const dt = clamp((now - last) / 1000, 0.001, 0.05);
      last = now;
      this._step(dt);
      this._draw(dt);
      this._raf = requestAnimationFrame(tick);
    };
    this._raf = requestAnimationFrame(tick);
  }
  stop() {
    if (this._raf) cancelAnimationFrame(this._raf);
    this._raf = null;
  }
  _step(dt) {
    const k = 1 - Math.exp(-dt * 4.2);
    this.energy += (this.targetEnergy - this.energy) * k;
    this.thinking += (this.targetThinking - this.thinking) * k;
    this.stream += (this.targetStream - this.stream) * k;
    this.contextFill += (this.targetContext - this.contextFill) * k;
    this.firing *= Math.exp(-dt * 3.2);
    if (this.state === "streaming") this.targetEnergy = Math.max(0.58, this.targetEnergy - dt * 0.55);
    this.fieldPhase += dt * (0.62 + this.thinking * 0.42 + this.stream * 0.78);
    this.breathPhase += dt * (1.35 + this.thinking * 0.65 + this.stream * 0.55);

    const spin = dt * (0.24 + this.thinking * 0.14 + this.stream * 0.24);
    const nod = dt * Math.sin(this.breathPhase * 0.32) * 0.06;
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

    for (let i = this.ripples.length - 1; i >= 0; i--) {
      const rp = this.ripples[i];
      rp.age += dt; rp.r += rp.speed * dt; rp.intensity *= Math.pow(0.34, dt);
      if (rp.r > Math.PI * 1.15 || rp.intensity < 0.02) this.ripples.splice(i, 1);
    }
  }
  _drawAura(ctx, cx, cy, radius, warmth) {
    const mix = (a, b, t) => Math.round(a + (b - a) * t);
    const grad = ctx.createRadialGradient(cx - radius * 0.22, cy - radius * 0.24, radius * 0.12, cx, cy, radius * 1.24);
    grad.addColorStop(0, `rgba(${mix(56, 249, warmth)},${mix(189, 115, warmth)},${mix(248, 22, warmth)},${(0.18 + this.energy * 0.14).toFixed(3)})`);
    grad.addColorStop(0.58, `rgba(${mix(37, 234, warmth)},${mix(99, 88, warmth)},${mix(235, 12, warmth)},${(0.11 + this.energy * 0.08).toFixed(3)})`);
    grad.addColorStop(1, "rgba(30, 41, 57, 0)");
    ctx.fillStyle = grad;
    ctx.beginPath(); ctx.arc(cx, cy, radius * 1.24, 0, TAU); ctx.fill();
    ctx.strokeStyle = `rgba(${mix(56, 251, warmth)},${mix(189, 146, warmth)},${mix(248, 60, warmth)},${(0.18 + this.energy * 0.12).toFixed(3)})`;
    ctx.lineWidth = 1.25;
    ctx.beginPath(); ctx.arc(cx, cy, radius * 1.02, 0, TAU); ctx.stroke();
  }
  _rippleCrest(rx, ry, rz) {
    let crest = 0;
    for (const rp of this.ripples) {
      const ang = Math.acos(clamp(rx * rp.fx + ry * rp.fy + rz * rp.fz, -1, 1));
      const off = Math.abs(ang - rp.r);
      if (off >= rp.width) continue;
      const wv = Math.cos((off / rp.width) * HALF_PI);
      crest += wv * wv * rp.intensity * Math.min(1, rp.age * 14);
    }
    return crest;
  }
  _projectNodes(dt, cx, cy, radius, fill) {
    const { ax: X, ay: Y, az: Z, fieldPhase, breathPhase, fov } = this;
    const fieldGain = 0.28 + this.energy * 0.58;
    const morphAmp = (0.008 + this.thinking * 0.015 + this.firing * 0.048) * 0.35;
    const settle = 1 - Math.pow(0.75, dt * 60);
    const coreExp = 0.24 + fill * 0.68, coreGlow = 0.16 + fill * 0.44, coreDotScale = 0.82 + fill * 0.42;
    for (let i = 0; i < this.nodes.length; i++) {
      const n = this.nodes[i];
      const rx = n.nx * X.x + n.ny * Y.x + n.nz * Z.x;
      const ry = n.nx * X.y + n.ny * Y.y + n.nz * Z.y;
      const rz = n.nx * X.z + n.ny * Y.z + n.nz * Z.z;
      const w1 = Math.sin(rx * 2.4 + fieldPhase + Math.cos(ry * 2.1 - fieldPhase * 0.68));
      const w2 = Math.cos(ry * 2.8 - rz * 2.3 + fieldPhase * 0.85 + Math.sin(rz * 2.2 + rx * 1.7 - fieldPhase * 0.55));
      const w3 = Math.sin((rx - ry + rz) * 3.2 - fieldPhase * 1.25);
      const crest = this._rippleCrest(rx, ry, rz);
      const drive = Math.max(0, (w1 * 0.48 + w2 * 0.34 + w3 * 0.18 + 0.1) / 1.1) * fieldGain + crest + (n.core ? coreGlow : 0);
      n.act += (Math.min(1.25, drive) - n.act) * settle;
      const breathe = Math.sin(breathPhase + n.phase) * 0.012;
      const lobes = Math.sin(rx * 2.15 + ry * 1.75 + fieldPhase * 1.65) * 0.6 + Math.cos(rz * 2.35 - ry * 1.9 + fieldPhase * 1.35) * 0.4;
      const shell = n.core ? coreExp * (n.shell / 0.56) : n.shell;
      const rad = shell * (1 + breathe + lobes * morphAmp + (n.act * 0.044 + Math.min(1, crest) * 0.026) * 0.35);
      const wx = rx * rad, wy = ry * rad, wz = rz * rad, persp = fov / Math.max(0.8, fov + wz);
      const pt = this.pts[i];
      pt.sx = cx + wx * radius * persp; pt.sy = cy + wy * radius * persp; pt.wz = wz;
      pt.sr = Math.max(0.9, n.dot * radius * persp * (n.core ? coreDotScale : 1) * (0.88 + n.act * 0.46));
      pt.act = n.act; pt.core = n.core;
    }
  }
  _drawLinks(ctx, sorted, radius, fill, warmth) {
    const mix = (a, b, t) => Math.round(a + (b - a) * t);
    const linkable = sorted.filter((p) => p.wz < 0.15 && (p.core ? fill > 0.35 && p.act > 0.50 : p.act > 0.54));
    const maxD = radius * 0.27, maxDSq = maxD * maxD;
    ctx.lineCap = "round";
    for (let i = 0; i < linkable.length; i++) {
      const a = linkable[i];
      let links = 0;
      for (let j = i + 1; j < linkable.length && links < 3; j++) {
        const b = linkable[j];
        const dx = b.sx - a.sx, dy = b.sy - a.sy, d2 = dx * dx + dy * dy;
        if (d2 <= 16 || d2 >= maxDSq) continue;
        const str = Math.min(1, (a.act + b.act) * 0.46) * (1 - Math.sqrt(d2) / maxD);
        ctx.strokeStyle = `rgba(${mix(125, 253, warmth)},${mix(211, 186, warmth)},${mix(252, 116, warmth)},${(str * 0.46).toFixed(3)})`;
        ctx.lineWidth = 0.75 + str * 1.1;
        ctx.beginPath(); ctx.moveTo(a.sx, a.sy); ctx.lineTo(b.sx, b.sy); ctx.stroke();
        links++;
      }
    }
  }
  _drawParticles(ctx, sorted, warmth) {
    const mix = (a, b, t) => Math.round(a + (b - a) * t);
    for (const p of sorted) {
      const front = clamp01((1.1 - p.wz) / 2.2), act = Math.min(1, p.act);
      const r = mix(Math.round(28 + front * 44 + act * 86), 249, warmth);
      const g = mix(Math.round(92 + front * 104 + act * 52), 115, warmth);
      const b = mix(Math.round(205 + front * 48 + act * 2), 22, warmth);
      const alpha = Math.min(0.98, (p.core ? 0.26 : 0.30) + front * 0.52 + act * 0.22);
      if (act > 0.34 && front > 0.32) {
        const bloomR = p.sr * (2.3 + act * 1.15), bloomA = Math.min(0.42, (act - 0.3) * front * 0.38);
        const bloom = ctx.createRadialGradient(p.sx, p.sy, p.sr * 0.15, p.sx, p.sy, bloomR);
        bloom.addColorStop(0, `rgba(${r},${g},${b},${bloomA.toFixed(3)})`);
        bloom.addColorStop(0.55, `rgba(${r},${g},${b},${(bloomA * 0.35).toFixed(3)})`);
        bloom.addColorStop(1, `rgba(${r},${g},${b},0)`);
        ctx.fillStyle = bloom;
        ctx.beginPath(); ctx.arc(p.sx, p.sy, bloomR, 0, TAU); ctx.fill();
      }
      ctx.fillStyle = `rgba(${r},${g},${b},${alpha.toFixed(3)})`;
      ctx.beginPath(); ctx.arc(p.sx, p.sy, p.sr, 0, TAU); ctx.fill();
      if (!p.core && front > 0.68 && act > 0.62) {
        ctx.fillStyle = `rgba(224,242,254,${((act - 0.58) * 0.45).toFixed(3)})`;
        ctx.beginPath(); ctx.arc(p.sx - p.sr * 0.24, p.sy - p.sr * 0.24, p.sr * 0.36, 0, TAU); ctx.fill();
      }
    }
  }
  _draw(dt) {
    const { ctx, w, h } = this;
    const cx = w * 0.5, cy = h * 0.5, radius = Math.min(w, h) * 0.42;
    const fill = this.contextFill;
    const warmth = fill > 0.75 ? clamp01((fill - 0.75) / 0.25) : 0;
    ctx.clearRect(0, 0, w, h);
    this._drawAura(ctx, cx, cy, radius, warmth);
    this._projectNodes(dt, cx, cy, radius, fill);
    const sorted = [...this.pts].sort((a, b) => b.wz - a.wz);
    this._drawLinks(ctx, sorted, radius, fill, warmth);
    this._drawParticles(ctx, sorted, warmth);
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
    this.orb = null;
    this.onChange = null;
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
        id: "e1", from: requester, kind: "task", time: "18m ago",
        text: m.task || "Audit workspace contract invariants and report status",
        steps: [
          { tool: "search_code", arg: `"${m.proj || "agency"}"`, tokens: 6400 },
          { tool: "read_file", arg: `${m.setup.home}/RULES.md`, tokens: 8200 },
          { tool: "run_tests", arg: "--workspace --quiet", tokens: 12900 }
        ],
        reply: `${m.name} (${m.role}) is actively tracking ${projName}.\n\nAll local assertions pass and the branch is synced with HQ. Ask a question below or queue a task to see live execution.`
      },
      {
        id: "e2", from: "you", kind: "chat", time: "4m ago",
        text: "Give me a quick status check before standup.", steps: [],
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

  enqueue(from, kind, text) {
    const avail = agentSessionAvailability(this.agentId);
    if (!avail.active) return false;
    const item = { id: Date.now(), from, kind, text };
    this.queue.push(item);
    if (this.orb) this.orb.pulse(0.7);
    this.notify();
    this._drain();
    return true;
  }

  stopRun() {
    this._aborted = true;
    this.queue = [];
    this.current = null;
    this.busy = false;
    this.compacting = false;
    const streamingEntry = this.entries.find((e) => e.streaming);
    if (streamingEntry) {
      streamingEntry.streaming = false;
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

  async _drain() {
    if (this.busy) return;
    this.busy = true;
    this._aborted = false;
    try {
      while (this.queue.length) {
        if (this._aborted || !agentSessionAvailability(this.agentId).active) break;
        const next = this.queue.shift();
        await this._run(next);
      }
    } finally {
      this.busy = false;
      this.current = null;
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
    this.status = { state: "working", detail: st.tool };
    if (this.orb) { this.orb.setState("working"); this.orb.pulse(0.75); }
    this.shellLines.push({ cls: "c-shell-cmd", txt: st.cmd, running: true });
    this.notify();
    await sleep(750);
    this.shellLines[this.shellLines.length - 1].running = false;
    if (this._aborted) return false;
    this.shellLines.push({ cls: "c-shell-out", txt: st.out });
    entry.steps.push({ tool: st.tool, arg: st.arg, tokens: st.tokens });
    this.parts.tools += st.tokens;
    this.stats.toolCalls++;
    this.history.push({ used: this.used, label: `${st.tool} ${st.arg}` });
    if (this.orb) this.orb.setContext(this.ratio);
    this.notify();
    return true;
  }

  async _streamReply(reply, entry) {
    this.status = { state: "streaming", detail: "writing reply" };
    if (this.orb) this.orb.setState("streaming");
    for (const ch of reply) {
      if (this._aborted) return false;
      entry.reply += ch;
      if (this.orb) this.orb.onToken(ch);
      this.notify();
      await sleep(/[.!?]/.test(ch) ? 45 : 14);
    }
    return true;
  }

  async _run(item) {
    if (this._aborted) return;
    this.current = item;
    if (this.parts.system === 0) this.parts.system = 13800;
    const entry = { id: "e" + item.id, from: item.from, kind: item.kind, time: "just now", text: item.text, steps: [], reply: "", streaming: true };
    this.entries.push(entry);
    this.shellLines.push({ cls: "c-shell-c c-shell-q", txt: `# [ORD-${item.id % 1000}] ${item.kind} from ${M[item.from] ? M[item.from].name : item.from}: ${item.text}` });
    this.notify();
    const plan = this._plan(item.text);
    for (const st of plan.steps) {
      if (this._aborted || !(await this._runStep(st, entry))) return;
    }
    if (!(await this._streamReply(plan.reply, entry))) return;
    entry.streaming = false;
    this.parts.conversation += Math.max(40, Math.round(plan.reply.length / 4));
    this.stats.requests++;
    this.history.push({ used: this.used, label: "Agent reply" });
    this.shellLines.push({ cls: "c-shell-c", txt: "# reply sent" });
    if (this.orb) this.orb.setContext(this.ratio);
    this.notify();
  }

  _plan(text) {
    const m = M[this.agentId];
    if (/working on|status|doing/i.test(text)) {
      return {
        steps: [
          { tool: "read_file", arg: `${m.setup.home}/backlog.json`, cmd: `cat ~/${m.setup.home}/backlog.json`, out: "2 queued items, 1 active slice", tokens: 3200 }
        ],
        reply: `Right now I am focused on ${m.task || "pending organization items"}.\n\nI just checked my local backlog: ${this.queue.length} follow-up items are queued and all tests on the working tree are green.`
      };
    }
    if (/custody|lock|lease/i.test(text)) {
      return {
        steps: [
          { tool: "search_code", arg: '"custody_lock" crates/', cmd: 'rg -n "custody_lock" crates/', out: "8 matches in crates/office-sync/src/lock.rs", tokens: 4900 },
          { tool: "run_tests", arg: "office-sync::lock", cmd: "cargo test -p office-sync lock", out: "14 passed, 0 failed (1.8s)", tokens: 7400 }
        ],
        reply: "The lease-based lock automatically expires after 15s if a satellite drops offline, preventing split-brain writes.\n\nPer-office locks avoid lease renewal overhead but require manual recovery on crash. All 14 lock tests pass."
      };
    }
    const kw = (text.match(/[A-Za-z]{4,}/g) || ["workspace"]).sort((a, b) => b.length - a.length)[0];
    return {
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

function stopAgentOrb() {
  if (activeOrb) {
    if (activeOrb._ro) { activeOrb._ro.disconnect(); activeOrb._ro = null; }
    activeOrb.stop();
    activeOrb = null;
  }
  if (activeSession) { activeSession.onChange = null; activeSession.orb = null; activeSession = null; }
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

function agentRailHtml(aid, tab) {
  const m = M[aid], sess = getConsoleSession(aid), avail = agentSessionAvailability(aid);
  const st = sess.busy || avail.active ? sess.status : { state: avail.state, detail: avail.title.toLowerCase() };
  const curNow = avail.active ? sess.current : null;
  const liveLink = tab !== "sessions" && avail.active ? `<a class="link" style="margin-top:10px;font-size:0.75rem" href="#/members/${aid}/sessions">Open live session${ic("chev-right", "ic14")}</a>` : "";
  return `<aside class="agent-rail" aria-label="Agent orb and context">
    <div class="orb-section">
      <div class="orb-stage"><canvas id="c-orb-canvas" role="img" aria-label="Neural orb reflecting ${esc(m.name)} activity"></canvas></div>
      <div class="telemetry-strip" id="c-telemetry" data-cstate="${esc(st.state)}">
        <span class="c-dot"></span><span class="c-state-txt" id="c-state-txt">${esc(st.state)}</span><span class="muted">·</span><span class="c-detail-txt" id="c-detail-txt">${esc(st.detail)}</span>
      </div>
      ${liveLink}
    </div>
    ${agentRailNowHtml(curNow, avail, avail.active && (sess.busy || !!sess.current))}
    ${agentRailContextHtml(sess, avail.active && !sess.busy)}
  </aside>`;
}

function sessionsSubbarHtml(disabled) {
  const tabs = [["activity", "Activity"], ["chat", "Session prompt"], ["shell", "Shell"]]
    .map(([v, lbl]) => `<button type="button" role="tab" id="ctab-${v}" data-cview="${v}" aria-selected="${S.consoleView === v}" tabindex="${S.consoleView === v ? 0 : -1}">${lbl}</button>`).join("");
  return `<div class="subbar">
        <div class="seg" id="c-tabs" role="tablist" aria-label="Session view">${tabs}</div>
        <span style="flex:1"></span>
        <div id="c-act-filters" style="display:flex;flex-wrap:wrap;gap:8px;align-items:center" ${S.consoleView !== "activity" ? "hidden" : ""}>
          <div class="seg" id="c-scope-seg" role="group" aria-label="Filter requests">
            <button type="button" data-cscope="all" aria-pressed="${S.consoleScope === "all"}">Everyone</button>
            <button type="button" data-cscope="others" aria-pressed="${S.consoleScope === "others"}">Others only</button>
          </div>
          <div class="seg" id="c-tools-seg" role="group" aria-label="Tool calls">
            <button type="button" data-ctools="on" aria-pressed="${S.consoleTools}">Tool calls</button>
            <button type="button" data-ctools="off" aria-pressed="${!S.consoleTools}">Compact</button>
          </div>
          <button type="button" class="btn outline" id="c-sim-btn" ${disabled}>${ic("play", "ic16")}Simulate request</button>
        </div>
      </div>`;
}

function sessionsComposerHtml(aid, m, avail, disabled) {
  const chKey = CH["dm-" + aid] ? "dm-" + aid : "general";
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
  return `${sessionsSubbarHtml(disabled)}
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
      <text x="${x(samples.length - 1) + 6}" y="${y(endRatio) + 4}" fill="#d1d5dc" font-family="ui-monospace, monospace" font-size="12">${Math.round(endRatio * 100)}%</text>
    </svg>`;
}

function paintAgentNowAndControls(aid, sess, tel) {
  const curAvail = agentSessionAvailability(aid);
  const curState = sess.busy || curAvail.active ? sess.status : { state: curAvail.state, detail: curAvail.title.toLowerCase() };
  tel.dataset.cstate = curState.state;
  document.getElementById("c-state-txt").textContent = curState.state;
  document.getElementById("c-detail-txt").textContent = curState.detail;
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
  const stopBtn = document.getElementById("c-stop-btn");
  if (stopBtn) stopBtn.hidden = !(curAvail.active && (sess.busy || !!sess.current));
  const compactBtn = document.getElementById("c-compact-btn");
  if (compactBtn) {
    compactBtn.disabled = !curAvail.active || sess.busy;
    compactBtn.setAttribute("aria-busy", String(!!sess.compacting));
    compactBtn.textContent = sess.compacting ? "Compacting…" : "Compact now";
  }
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
    const stepsHtml = S.consoleTools && en.steps?.length
      ? `<details class="c-work-box" ${en.streaming ? "open" : ""}><summary>Worked · ${plural(en.steps.length, "tool call", "tool calls")}</summary><div class="c-steps">${en.steps.map((s) => `<div class="c-step"><span class="c-step-dot"></span><span class="c-step-tool">${esc(s.tool)}</span><span class="c-step-arg">${esc(s.arg)}</span><span class="c-step-tok">+${fmtTok(s.tokens)}</span></div>`).join("")}</div></details>`
      : "";
    return `<article class="c-entry" data-from="${esc(en.from)}"><div class="c-entry-hd"><span class="c-entry-who">${av(en.from, 24)}${esc(M[en.from] ? M[en.from].name : en.from)} ${pill(en.kind, "gray")}</span><span class="muted" style="font-size:0.75rem">${esc(en.time)}</span></div><div class="c-entry-prompt">${esc(en.text)}</div>${stepsHtml}<div class="c-reply">${esc(en.reply)}${en.streaming ? '<span class="c-caret-blink"></span>' : ""}</div></article>`;
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

function paintAgentFeedAndShell(sess) {
  paintAgentFeed(sess);
  paintAgentShell(sess);
}

function bindSessionsListeners(sessionsRoot, aid, sess, avail, paintAll) {
  const setView = (next, focusPrompt = false) => {
    S.consoleView = next;
    sessionsRoot.querySelectorAll("#c-tabs [role='tab']").forEach((b) => { const on = b.dataset.cview === next; b.setAttribute("aria-selected", String(on)); b.tabIndex = on ? 0 : -1; });
    const msgBtn = document.querySelector('.profile-actions button[data-v^="#/chat/"]');
    if (msgBtn) { const demote = next === "chat" && avail.active; msgBtn.classList.toggle("primary", !demote); msgBtn.classList.toggle("outline", demote); }
    document.getElementById("c-act-filters").hidden = next !== "activity";
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
  document.getElementById("c-tools-seg").addEventListener("click", (e) => {
    const b = e.target.closest("button[data-ctools]"); if (!b) return;
    S.consoleTools = b.dataset.ctools === "on";
    sessionsRoot.querySelectorAll("#c-tools-seg button").forEach((x) => x.setAttribute("aria-pressed", String(x.dataset.ctools === b.dataset.ctools)));
    paintAll();
  });
  document.getElementById("c-sim-btn").addEventListener("click", () => {
    if (!agentSessionAvailability(aid).active) return;
    const samples = [["rowan", "task", "Check custody lock lease renewal timings"], ["tomas", "task", "Verify test coverage on sync recovery"], ["maya", "chat", "Confirm v1.0 release notes status"]];
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

function mountAgentProfile(aid, tab) {
  stopAgentOrb();
  const canvas = document.getElementById("c-orb-canvas");
  if (!canvas) return;
  const m = M[aid], sess = getConsoleSession(aid), avail = agentSessionAvailability(aid);
  activeSession = sess;
  const sessionsRoot = document.getElementById("agent-sessions-root");
  if (sessionsRoot && tab === "sessions") sessionsRoot.innerHTML = renderSessionsPaneHtml(aid, m, avail);
  const orb = new NeuralOrb(canvas);
  orb.setState(avail.active ? sess.status.state : "paused");
  orb.setContext(sess.ratio, true);
  orb.start();
  if (avail.active) orb.pulse(0.7);
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
    paintAgentFeedAndShell(sess);
  };
  sess.onChange = paintAll;
  paintAll();
  document.getElementById("c-compact-btn")?.addEventListener("click", () => sess.compactNow());
  document.getElementById("c-stop-btn")?.addEventListener("click", () => sess.stopRun());
  if (sessionsRoot && tab === "sessions") bindSessionsListeners(sessionsRoot, aid, sess, avail, paintAll);
}

export {
  agentRailHtml,
  agentSessionAvailability,
  consoleSessions,
  getConsoleSession,
  mountAgentProfile,
  stopAgentOrb,
};
