// Lightweight 3D particle Neural Orb renderer ported from prototype v6/pages/agent-profile.js.
// Reads active look tokens from CSS custom properties so it adapts to all 6 themes automatically,
// and honors prefers-reduced-motion.

export type OrbState = 'idle' | 'paused' | 'working' | 'thinking' | 'attention' | 'offline';

interface Vec3 {
	x: number;
	y: number;
	z: number;
}

interface OrbNode {
	nx: number;
	ny: number;
	nz: number;
	shell: number;
	dot: number;
	core: boolean;
	phase: number;
	act: number;
}

interface ProjectedPoint {
	sx: number;
	sy: number;
	wz: number;
	sr: number;
	act: number;
	core: boolean;
}

interface OrbColors {
	primary: string;
	secondary: string;
	ring: string;
}

const TAU = Math.PI * 2;
const GOLDEN_ANGLE = Math.PI * (3 - Math.sqrt(5));

const STATE_TARGETS: Record<OrbState, { energy: number; thinking: number }> = {
	idle: { energy: 0.28, thinking: 0.12 },
	paused: { energy: 0.1, thinking: 0 },
	offline: { energy: 0.08, thinking: 0 },
	working: { energy: 0.76, thinking: 0.78 },
	thinking: { energy: 0.62, thinking: 1 },
	attention: { energy: 0.68, thinking: 0.45 }
};

function createSeededRandom(seed: number): () => number {
	let s = seed;
	return () => {
		s = (s * 1664525 + 1013904223) % 4294967296;
		return s / 4294967296;
	};
}

function clamp(v: number, lo: number, hi: number): number {
	return Math.min(hi, Math.max(lo, v));
}

function prefersReducedMotion(): boolean {
	return (
		typeof window !== 'undefined' &&
		Boolean(window.matchMedia?.('(prefers-reduced-motion: reduce)').matches)
	);
}

function readToken(name: string, fallback: string): string {
	if (typeof document === 'undefined' || typeof getComputedStyle === 'undefined') return fallback;
	const value = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
	return value || fallback;
}

function resolveOrbColors(state: OrbState): OrbColors {
	if (state === 'attention') {
		const warn = readToken('--tone-warn', 'currentColor');
		return { primary: warn, secondary: readToken('--tone-accent', warn), ring: warn };
	}
	if (state === 'working' || state === 'thinking') {
		const ok = readToken('--tone-ok', 'currentColor');
		const accent = readToken('--tone-accent', ok);
		return { primary: accent, secondary: ok, ring: accent };
	}
	if (state === 'paused' || state === 'offline') {
		const muted = readToken('--fg-muted', 'currentColor');
		return { primary: muted, secondary: muted, ring: muted };
	}
	const accent = readToken('--tone-accent', 'currentColor');
	const link = readToken('--link', accent);
	return { primary: accent, secondary: link, ring: accent };
}

function buildOrbNodes(): OrbNode[] {
	const rand = createSeededRandom(7919);
	const out: OrbNode[] = [];
	const layers = [
		{
			count: 320,
			mult: 1,
			jitter: 0.14,
			yJit: 0.045,
			shell: () => 1.0,
			dot: [0.013, 0.0045],
			core: false
		},
		{
			count: 72,
			mult: 1.37,
			jitter: 0.2,
			yJit: 0.08,
			shell: () => 0.48 + rand() * 0.16,
			dot: [0.011, 0.0035],
			core: true
		}
	];
	for (const layer of layers) {
		for (let i = 0; i < layer.count; i++) {
			const y0 = 1 - (i / (layer.count - 1)) * 2;
			const sr = Math.sqrt(Math.max(0, 1 - y0 * y0));
			const th = GOLDEN_ANGLE * i * layer.mult + (rand() - 0.5) * layer.jitter;
			const y = y0 + (rand() - 0.5) * layer.yJit;
			const nx = Math.cos(th) * sr;
			const nz = Math.sin(th) * sr;
			const len = Math.hypot(nx, y, nz) || 1;
			out.push({
				nx: nx / len,
				ny: y / len,
				nz: nz / len,
				shell: layer.shell(),
				dot: layer.dot[0] + rand() * layer.dot[1],
				core: layer.core,
				phase: rand() * TAU,
				act: 0.25
			});
		}
	}
	return out;
}

export class NeuralOrbEngine {
	private canvas: HTMLCanvasElement;
	private ctx: CanvasRenderingContext2D | null;
	private state: OrbState = 'idle';
	private w = 180;
	private h = 180;
	private fov = 2.85;
	private fieldPhase = 0;
	private breathPhase = 0;
	private energy = 0.28;
	private targetEnergy = 0.28;
	private thinking = 0.12;
	private targetThinking = 0.12;
	private ax: Vec3 = { x: 1, y: 0, z: 0 };
	private ay: Vec3 = { x: 0, y: 1, z: 0 };
	private az: Vec3 = { x: 0, y: 0, z: 1 };
	private nodes: OrbNode[];
	private pts: ProjectedPoint[];
	private sorted: ProjectedPoint[];
	private raf: number | null = null;
	private lastDraw = 0;

	constructor(canvas: HTMLCanvasElement, initialState: OrbState = 'idle') {
		this.canvas = canvas;
		this.ctx = canvas.getContext('2d');
		this.nodes = buildOrbNodes();
		this.pts = this.nodes.map(() => ({ sx: 0, sy: 0, wz: 0, sr: 1, act: 0, core: false }));
		this.sorted = this.pts.slice();
		this.setState(initialState, true);
		this.resize();
	}

	setState(next: OrbState, immediate = false): void {
		const target = STATE_TARGETS[next] ?? STATE_TARGETS.idle;
		this.state = next;
		this.targetEnergy = target.energy;
		this.targetThinking = target.thinking;
		if (immediate || prefersReducedMotion()) {
			this.energy = target.energy;
			this.thinking = target.thinking;
			this.draw(0.016);
		}
	}

	resize(): void {
		if (!this.ctx) return;
		const parent = this.canvas.parentElement;
		const rect = parent ? parent.getBoundingClientRect() : { width: 180, height: 180 };
		const w = Math.max(140, Math.round(rect.width || 180));
		const h = Math.max(140, Math.round(rect.height || 180));
		const dpr = Math.min(window.devicePixelRatio || 1, 2);
		this.w = w;
		this.h = h;
		const pxW = Math.round(w * dpr);
		const pxH = Math.round(h * dpr);
		if (this.canvas.width !== pxW || this.canvas.height !== pxH) {
			this.canvas.width = pxW;
			this.canvas.height = pxH;
		}
		this.ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
		this.draw(0.016);
	}

	start(): void {
		if (this.raf !== null) return;
		if (prefersReducedMotion()) {
			this.step(0.016);
			this.draw(0.016);
			return;
		}
		let last = performance.now();
		this.lastDraw = last;
		const tick = (now: number) => {
			if (!document.body.contains(this.canvas)) {
				this.stop();
				return;
			}
			this.raf = requestAnimationFrame(tick);
			if (document.hidden) {
				last = now;
				return;
			}
			const active =
				this.state === 'working' || this.state === 'thinking' || this.state === 'attention';
			if (!active && now - this.lastDraw < 34) return;
			const dt = clamp((now - last) / 1000, 0.001, 0.05);
			last = now;
			this.lastDraw = now;
			this.step(dt);
			this.draw(dt);
		};
		this.raf = requestAnimationFrame(tick);
	}

	stop(): void {
		if (this.raf !== null) {
			cancelAnimationFrame(this.raf);
			this.raf = null;
		}
	}

	private rotateBasis(spin: number, nod: number): void {
		const cx = Math.cos(nod);
		const sx = Math.sin(nod);
		const cy = Math.cos(spin);
		const sy = Math.sin(spin);
		const rot = (v: Vec3) => {
			const y = v.y * cx - v.z * sx;
			const z = v.y * sx + v.z * cx;
			const x = v.x;
			v.x = x * cy + z * sy;
			v.y = y;
			v.z = -x * sy + z * cy;
		};
		const norm = (v: Vec3) => {
			const l = Math.hypot(v.x, v.y, v.z) || 1;
			v.x /= l;
			v.y /= l;
			v.z /= l;
		};
		rot(this.ax);
		rot(this.ay);
		norm(this.ax);
		const dot = this.ax.x * this.ay.x + this.ax.y * this.ay.y + this.ax.z * this.ay.z;
		this.ay.x -= dot * this.ax.x;
		this.ay.y -= dot * this.ax.y;
		this.ay.z -= dot * this.ax.z;
		norm(this.ay);
		this.az.x = this.ax.y * this.ay.z - this.ax.z * this.ay.y;
		this.az.y = this.ax.z * this.ay.x - this.ax.x * this.ay.z;
		this.az.z = this.ax.x * this.ay.y - this.ax.y * this.ay.x;
	}

	private step(dt: number): void {
		const k = 1 - Math.exp(-dt * 4.2);
		this.energy += (this.targetEnergy - this.energy) * k;
		this.thinking += (this.targetThinking - this.thinking) * k;
		this.fieldPhase += dt * (0.55 + this.thinking * 0.55 + this.energy * 0.35);
		this.breathPhase += dt * (1.2 + this.thinking * 0.6);
		const spin = dt * (0.18 + this.thinking * 0.18 + this.energy * 0.14);
		const nod = dt * Math.sin(this.breathPhase * 0.35) * 0.06;
		this.rotateBasis(spin, nod);
	}

	private projectNodes(dt: number, cx: number, cy: number, radius: number): void {
		const { ax: X, ay: Y, az: Z, fieldPhase, breathPhase, fov } = this;
		const fieldGain = 0.28 + this.energy * 0.56;
		const settle = 1 - Math.pow(0.74, dt * 60);
		for (let i = 0; i < this.nodes.length; i++) {
			const n = this.nodes[i];
			const rx = n.nx * X.x + n.ny * Y.x + n.nz * Z.x;
			const ry = n.nx * X.y + n.ny * Y.y + n.nz * Z.y;
			const rz = n.nx * X.z + n.ny * Y.z + n.nz * Z.z;
			const w1 = Math.sin(rx * 2.4 + fieldPhase + Math.cos(ry * 2.1 - fieldPhase * 0.65));
			const w2 = Math.cos(ry * 2.7 - rz * 2.2 + fieldPhase * 0.85);
			const drive =
				Math.max(0, (w1 * 0.58 + w2 * 0.42 + 0.14) / 1.14) * fieldGain + (n.core ? 0.25 : 0);
			n.act += (Math.min(1.2, drive) - n.act) * settle;
			const breathe = Math.sin(breathPhase + n.phase) * (0.012 + this.thinking * 0.008);
			const rad = n.shell * (1 + breathe + n.act * 0.02);
			const persp = fov / Math.max(0.8, fov + rz * rad);
			const pt = this.pts[i];
			pt.sx = cx + rx * rad * radius * persp;
			pt.sy = cy + ry * rad * radius * persp;
			pt.wz = rz * rad;
			pt.sr = Math.max(0.9, n.dot * radius * persp * (0.9 + n.act * 0.42));
			pt.act = n.act;
			pt.core = n.core;
		}
	}

	private draw(dt: number): void {
		const ctx = this.ctx;
		if (!ctx) return;
		const colors = resolveOrbColors(this.state);
		const cx = this.w * 0.5;
		const cy = this.h * 0.5;
		const radius = Math.min(this.w, this.h) * 0.4;
		ctx.clearRect(0, 0, this.w, this.h);

		const grad = ctx.createRadialGradient(
			cx - radius * 0.2,
			cy - radius * 0.22,
			radius * 0.1,
			cx,
			cy,
			radius * 1.24
		);
		grad.addColorStop(0, colors.primary);
		grad.addColorStop(0.55, colors.secondary);
		grad.addColorStop(1, 'transparent');
		ctx.globalAlpha = 0.14 + this.energy * 0.12;
		ctx.fillStyle = grad;
		ctx.beginPath();
		ctx.arc(cx, cy, radius * 1.28, 0, TAU);
		ctx.fill();

		ctx.globalAlpha = 0.22 + this.energy * 0.14;
		ctx.strokeStyle = colors.ring;
		ctx.lineWidth = 1.2;
		ctx.beginPath();
		ctx.arc(cx, cy, radius * 1.02, 0, TAU);
		ctx.stroke();

		this.projectNodes(dt, cx, cy, radius);
		this.sorted.sort((a, b) => b.wz - a.wz);

		for (let i = 0; i < this.sorted.length; i++) {
			const p = this.sorted[i];
			const front = clamp((1.1 - p.wz) / 2.2, 0, 1);
			const act = Math.min(1, p.act);
			ctx.globalAlpha = Math.min(0.95, (p.core ? 0.26 : 0.28) + front * 0.48 + act * 0.22);
			ctx.fillStyle = p.core || act > 0.55 ? colors.primary : colors.secondary;
			ctx.beginPath();
			ctx.arc(p.sx, p.sy, p.sr, 0, TAU);
			ctx.fill();
		}
		ctx.globalAlpha = 1;
	}
}
