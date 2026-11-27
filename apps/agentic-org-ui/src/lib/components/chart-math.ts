/** Pure SVG chart math shared by Insights TrendChart and Home sparklines (TASK-193 / TASK-229). */

export interface ChartPoint {
	x: number;
	y: number;
}

/** Clean axis ticks from 0 to a rounded top: 0 / 5 / 10 / 15 / 20, never 0 / 3.7 / 7.4. */
export function niceTicks(max: number, integer = false): number[] {
	if (!(max > 0)) return [0, 1];
	const raw = max / 4;
	const magnitude = 10 ** Math.floor(Math.log10(raw));
	const candidates = (integer ? [1, 2, 5, 10] : [1, 2, 2.5, 5, 10]).map((s) => s * magnitude);
	const found = candidates.find((v) => v >= raw - 1e-9) ?? candidates[candidates.length - 1];
	const step = Math.max(integer ? 1 : 0, found);
	const top = Math.ceil(max / step - 1e-9) * step;
	const count = Math.round(top / step) + 1;
	return Array.from({ length: count }, (_, idx) => Number((idx * step).toPrecision(6)));
}

/**
 * Nested x-axis label visibility counted back from the latest point (always labelled):
 * about 8 labels on a wide chart (`many`), 4 on medium (`some`), 2 on narrow (`few`).
 */
export function axisLevel(index: number, count: number): 'few' | 'some' | 'many' | 'none' {
	if (count <= 1) return 'few';
	const fromEnd = count - 1 - index;
	const step = Math.max(1, Math.ceil(count / 8));
	if (fromEnd % (4 * step) === 0) return 'few';
	if (fromEnd % (2 * step) === 0) return 'some';
	return fromEnd % step === 0 ? 'many' : 'none';
}

/** Splits a series into contiguous non-null runs so unknown gaps break the curve cleanly. */
export function contiguousRuns<T>(items: readonly T[], hasValue: (item: T) => boolean): T[][] {
	const out: T[][] = [];
	let current: T[] = [];
	for (const item of items) {
		if (hasValue(item)) {
			current.push(item);
		} else if (current.length > 0) {
			out.push(current);
			current = [];
		}
	}
	if (current.length > 0) out.push(current);
	return out;
}

const clamp = (v: number, a: number, b: number) =>
	Math.min(Math.max(v, Math.min(a, b)), Math.max(a, b));

/**
 * Smooth Catmull-Rom cubic Bezier SVG path through `pts`.
 * Control points are clamped between neighbouring y-values so the curve never overshoots or dips below baseline.
 */
export function smoothCurvePath(pts: readonly ChartPoint[]): string {
	if (pts.length === 0) return '';
	if (pts.length < 3) {
		return pts
			.map((p, i) => `${i === 0 ? 'M' : 'L'} ${p.x.toFixed(2)} ${p.y.toFixed(2)}`)
			.join(' ');
	}
	let d = `M ${pts[0].x.toFixed(2)} ${pts[0].y.toFixed(2)}`;
	for (let i = 0; i < pts.length - 1; i++) {
		const p0 = pts[i - 1] ?? pts[i];
		const p1 = pts[i];
		const p2 = pts[i + 1];
		const p3 = pts[i + 2] ?? p2;
		const c1x = p1.x + (p2.x - p0.x) / 6;
		const c1y = clamp(p1.y + (p2.y - p0.y) / 6, p1.y, p2.y);
		const c2x = p2.x - (p3.x - p1.x) / 6;
		const c2y = clamp(p2.y - (p3.y - p1.y) / 6, p1.y, p2.y);
		d += ` C ${c1x.toFixed(2)} ${c1y.toFixed(2)}, ${c2x.toFixed(2)} ${c2y.toFixed(2)}, ${p2.x.toFixed(2)} ${p2.y.toFixed(2)}`;
	}
	return d;
}

/** Closed SVG area path beneath `smoothCurvePath(pts)` down to `baselineY`. */
export function smoothAreaPath(pts: readonly ChartPoint[], baselineY: number): string {
	if (pts.length < 2) return '';
	const line = smoothCurvePath(pts);
	const first = pts[0];
	const last = pts[pts.length - 1];
	return `${line} L ${last.x.toFixed(2)} ${baselineY.toFixed(2)} L ${first.x.toFixed(2)} ${baselineY.toFixed(2)} Z`;
}
