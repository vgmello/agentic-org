import { describe, expect, it } from 'vitest';
import vectors from '../../../test-vectors/org-commands.json';
import type { AppError } from '$lib/proto';
import type { OrgCommand } from './commands';
import { applyCommand, type Ctx } from './reducer';
import { sampleOrg } from './sample';

// The same cases run against the Rust mock HQ (src-tauri/src/org_engine.rs): the two engines must agree.
type Step = { method: string; params: Record<string, unknown> };
// The clock and the one office the shared cases use (the Rust runner uses the same).
const ctx: Ctx = { now: '2026-11-08T12:00:00.000Z', offices: ['office-1'] };
const cmd = (step: Step, n: number): OrgCommand => ({ method: step.method, params: { command_id: `c-${n}`, ...step.params } }) as OrgCommand;

describe('shared organization command cases', () => {
	it.each(vectors.cases as { name: string; setup?: unknown[]; command: unknown; expect: string; after?: unknown[] }[])('$name', ({ setup, command, expect: outcome, after }) => {
		let s = sampleOrg('office-1');
		(setup ?? []).forEach((step, i) => (s = applyCommand(s, cmd(step as Step, i), ctx)));
		if (outcome === 'ok') {
			const next = applyCommand(s, cmd(command as Step, 99), ctx);
			// What the command left behind: entities found by id in the snapshot's lists (design §4.8).
			type Check = { in: string; id: string; absent?: boolean; has?: Record<string, unknown>; lacks?: string[] };
			for (const check of (after ?? []) as Check[]) {
				const found = ((next as unknown as Record<string, { id: string }[]>)[check.in] ?? []).find((x) => x.id === check.id) as Record<string, unknown> | undefined;
				if (check.absent) {
					expect(found, `${check.in} ${check.id} should be gone`).toBeUndefined();
					continue;
				}
				expect(found, `no ${check.in} ${check.id}`).toBeDefined();
				for (const [k, v] of Object.entries(check.has ?? {})) expect(found![k], `${check.in} ${check.id} ${k}`).toEqual(v);
				for (const k of check.lacks ?? []) expect(found, `${check.in} ${check.id} should lack ${k}`).not.toHaveProperty(k);
			}
		} else {
			const before = structuredClone(s);
			let thrown: AppError | undefined;
			try {
				applyCommand(s, cmd(command as Step, 99), ctx);
			} catch (e) {
				thrown = e as AppError;
			}
			expect(thrown?.code).toBe(outcome);
			expect(s).toEqual(before); // a refused command changes nothing
		}
	});
});
