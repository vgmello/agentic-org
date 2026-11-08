import { describe, expect, it } from 'vitest';
import vectors from '../../../test-vectors/org-commands.json';
import type { AppError } from '$lib/proto';
import type { OrgCommand } from './commands';
import { applyCommand } from './reducer';
import { sampleOrg } from './sample';

// The same cases run against the Rust mock HQ (src-tauri/src/org_engine.rs): the two engines must agree.
type Step = { method: string; params: Record<string, unknown> };
const cmd = (step: Step, n: number): OrgCommand => ({ method: step.method, params: { command_id: `c-${n}`, ...step.params } }) as OrgCommand;

describe('shared organization command cases', () => {
	it.each(vectors.cases)('$name', ({ setup, command, expect: outcome }) => {
		let s = sampleOrg('office-1');
		(setup ?? []).forEach((step, i) => (s = applyCommand(s, cmd(step as Step, i))));
		if (outcome === 'ok') {
			expect(() => applyCommand(s, cmd(command as Step, 99))).not.toThrow();
		} else {
			const before = structuredClone(s);
			let thrown: AppError | undefined;
			try {
				applyCommand(s, cmd(command as Step, 99));
			} catch (e) {
				thrown = e as AppError;
			}
			expect(thrown?.code).toBe(outcome);
			expect(s).toEqual(before); // a refused command changes nothing
		}
	});
});
