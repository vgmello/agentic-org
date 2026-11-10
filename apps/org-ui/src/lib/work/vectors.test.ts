import { describe, expect, it } from 'vitest';
import vectors from '../../../../../crates/org-work/test-vectors/work-commands.json';
import { applyWork, emptyWork, itemOf, type WorkCall, type WorkContext, type WorkState } from './engine';

// The same cases run against the Rust mock office (src-tauri/src/work_engine.rs): the two engines must agree.
type Step = { call?: WorkCall; now?: string; error?: string; items?: Record<string, unknown>[]; get?: string; expect?: Record<string, unknown> };

/** Every field in `want` is in `have`, recursively (arrays element by element). */
function subset(have: unknown, want: unknown, path = ''): string | undefined {
	if (want !== null && typeof want === 'object') {
		if (have === null || typeof have !== 'object') return `${path}: expected an object`;
		if (Array.isArray(want)) {
			if (!Array.isArray(have)) return `${path}: expected an array`;
			for (let i = 0; i < want.length; i++) {
				const bad = subset(have[i], want[i], `${path}[${i}]`);
				if (bad) return bad;
			}
			return undefined;
		}
		for (const [k, v] of Object.entries(want)) {
			const bad = subset((have as Record<string, unknown>)[k], v, `${path}.${k}`);
			if (bad) return bad;
		}
		return undefined;
	}
	return have === want ? undefined : `${path}: expected ${JSON.stringify(want)}, got ${JSON.stringify(have)}`;
}

describe('work-commands.json (shared with the Rust mock office)', () => {
	it('preserves every legacy work scenario', () => {
		expect(vectors.cases).toHaveLength(29);
	});
	for (const c of vectors.cases as { name: string; steps: Step[] }[]) {
		it(c.name, () => {
			let state: WorkState = emptyWork();
			for (const [n, step] of c.steps.entries()) {
				const ctx: WorkContext = { ...(vectors.context as WorkContext), now: step.now ?? (vectors.context as WorkContext).now };
				if (step.get) {
					expect(subset(itemOf(state, step.get, step.now ?? ctx.now), step.expect), `step ${n}`).toBeUndefined();
					continue;
				}
				if (step.error) {
					try {
						applyWork(state, step.call!, ctx);
					} catch (e) {
						expect((e as { code: string }).code, `step ${n}`).toBe(step.error);
						continue;
					}
					throw new Error(`step ${n}: expected ${step.error}`);
				}
				const done = applyWork(state, step.call!, ctx);
				state = done.state;
				expect(subset(done.accepted.items, step.items), `step ${n}`).toBeUndefined();
			}
		});
	}
});
