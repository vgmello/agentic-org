import { describe, expect, it } from 'vitest';
import vectors from '../../../../../../crates/org-office/test-vectors/org-commands.json';
import seed from '../../../../../../crates/org-office/test-vectors/organization.json';
import type { AppError } from '$lib/proto';
import type { OrgCommand } from './commands';
import { applyCommand, type Ctx } from './reducer';
import { sampleOrg } from './sample';

// Shared cases against the canonical single-home-team organization contract.
// Retired multi-membership vectors (`team.add_member`, `team.remove_member`, `ms-*` membership IDs)
// remain only in the legacy fixture file and are explicitly excluded here.
type Step = { method: string; params: Record<string, unknown> };
type Case = {
	name: string;
	setup?: Step[];
	command: Step;
	expect: string;
	after?: {
		in: string;
		id: string;
		absent?: boolean;
		has?: Record<string, unknown>;
		lacks?: string[];
	}[];
};
const RETIRED_METHODS = new Set(['team.add_member', 'team.remove_member']);
const RETIRED_MULTI_TEAM_CASES = new Set([
	'area.set_team: a lead who is in the new team stays',
	'project.remove_team: a lead who is in the team taking over keeps leading'
]);
const usesRetiredMembership = (c: Case): boolean => {
	if (RETIRED_MULTI_TEAM_CASES.has(c.name)) return true;
	const steps = [...(c.setup ?? []), c.command];
	return steps.some(
		(s) =>
			RETIRED_METHODS.has(s.method) ||
			(typeof s.params.membership_id === 'string' && s.params.membership_id.startsWith('ms-'))
	);
};
const activeCases = (vectors.cases as Case[]).filter((c) => !usesRetiredMembership(c));

// The clock and the one office the shared cases use (the Rust runner uses the same).
const ctx: Ctx = { now: '2026-11-08T12:00:00.000Z', offices: ['office-1'] };
const cmd = (step: Step, n: number): OrgCommand =>
	({
		method: step.method,
		params: {
			command_id: `01ARZ3NDEKTSV4RRFFQ69G5F${String(n).padStart(2, '0')}`,
			...step.params
		}
	}) as OrgCommand;

describe('shared organization command cases', () => {
	it('runs every non-retired shared organization scenario', () => {
		expect(vectors.cases).toHaveLength(252);
		expect(activeCases).toHaveLength(227);
	});
	it('projects the shared seed without legacy memberships', () => {
		const sample = sampleOrg('office-1');
		expect(sample.position).toEqual(seed.position);
		expect(sample.teams).toEqual(seed.teams);
		expect(sample.projects).toEqual(seed.projects);
		expect(sample.project_teams).toEqual(seed.project_teams);
		expect(sample.areas).toEqual(seed.areas);
		expect(sample.members.map((m) => m.id)).toEqual(seed.members.map((m) => m.id));
		expect(sample).not.toHaveProperty('memberships');
	});
	it.each(activeCases)('$name', ({ setup, command, expect: outcome, after }) => {
		let s = sampleOrg('office-1');
		(setup ?? []).forEach((step, i) => (s = applyCommand(s, cmd(step, i), ctx)));
		if (outcome === 'ok') {
			const next = applyCommand(s, cmd(command, 99), ctx);
			for (const check of after ?? []) {
				const found = ((next as unknown as Record<string, { id: string }[]>)[check.in] ?? []).find(
					(x) => x.id === check.id
				) as Record<string, unknown> | undefined;
				if (check.absent) {
					expect(found, `${check.in} ${check.id} should be gone`).toBeUndefined();
					continue;
				}
				expect(found, `no ${check.in} ${check.id}`).toBeDefined();
				for (const [k, v] of Object.entries(check.has ?? {}))
					expect(found![k], `${check.in} ${check.id} ${k}`).toEqual(v);
				for (const k of check.lacks ?? [])
					expect(found, `${check.in} ${check.id} should lack ${k}`).not.toHaveProperty(k);
			}
		} else {
			const before = structuredClone(s);
			let thrown: AppError | undefined;
			try {
				applyCommand(s, cmd(command, 99), ctx);
			} catch (e) {
				thrown = e as AppError;
			}
			expect(thrown?.code).toBe(outcome);
			expect(s).toEqual(before); // a refused command changes nothing
		}
	});
});
