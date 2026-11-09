import { describe, expect, it } from 'vitest';
import { applyHints } from './hints';
import { SAMPLE_HINTS, SAMPLE_SCAN } from './mock';
import { buildRows } from './rows';
import { suggestTeams, teamsToCreate } from './teams';
import type { Hints } from './types';

const rows = () => {
	const { rows } = buildRows(SAMPLE_SCAN, { exclude: ['sample'] });
	// The five messaging-only agents are filled in, as the owner would.
	for (const r of rows) if (!r.runtime) Object.assign(r, { role: 'software engineer', title: 'Software Engineer', runtime: 'codex' });
	return rows;
};
const teamOf = (fitted: { rows: { name: string; teamKey?: string }[] }, name: string) => fitted.rows.find((r) => r.name === name)?.teamKey;

describe('IMP-10 the team hints file', () => {
	it('puts every agent in the team the file says, with the leads and parents of our own shape', () => {
		const all = rows();
		const fitted = applyHints(all, SAMPLE_HINTS, suggestTeams(all));
		expect(fitted.notes).toEqual({ parents: [], leads: [], members: [], circles: [] });
		expect(fitted.teamCount).toBe(6);
		const placed = Object.fromEntries(fitted.rows.map((r) => [r.name, r.teamKey]));
		expect(placed).toMatchObject({
			ada: 'engineering',
			Eden: 'design',
			'app-frontend-dev': 'app',
			Riley: 'app',
			'server-dev': 'office',
			River: 'office',
			Remy: 'agent-tooling',
			'codex-dev': 'agent-tooling',
			'agy-senior-dev': 'agent-tooling',
			reviewer: 'quality'
		});
		expect(fitted.drafts.find((d) => d.key === 'app')).toMatchObject({ name: 'App', parentKey: 'engineering', leadKey: 'app-frontend-dev' });
		expect(fitted.drafts.find((d) => d.key === 'agent-tooling')?.name).toBe('Agent tooling');
		expect(fitted.drafts.find((d) => d.key === 'engineering')?.leadKey).toBe('ac-staff-engineer-ada');
		expect(fitted.drafts.filter((d) => d.key === 'engineering' || d.key === 'design')).toHaveLength(2);
		expect(teamsToCreate(fitted.rows, fitted.drafts).map((d) => d.key).sort()).toEqual(['agent-tooling', 'app', 'design', 'engineering', 'office', 'quality']);
	});

	it('leaves the rows it does not mention in their department team, and counts every team in the plan', () => {
		const all = rows();
		const partial: Hints = { path: 'f', teams: [{ key: 'app', name: 'App', members: ['app-frontend-dev'] }] };
		const fitted = applyHints(all, partial, suggestTeams(all));
		expect(teamOf(fitted, 'app-frontend-dev')).toBe('app');
		expect(teamOf(fitted, 'ada')).toBe('Engineering');
		expect(fitted.drafts.map((d) => d.key)).toEqual(expect.arrayContaining(['app', 'engineering', 'design']));
		expect(fitted.teamCount).toBe(3);
	});

	it('a table whose id is a department team’s sets that team’s name, lead and members instead of adding a second one', () => {
		const all = rows();
		const hints: Hints = { path: 'f', teams: [{ key: 'engineering', name: 'Eng', lead: 'ac-software-engineer-river', members: ['ac-staff-engineer-ada'] }] };
		const fitted = applyHints(all, hints, suggestTeams(all));
		const engineering = fitted.drafts.filter((d) => d.key === 'engineering');
		expect(engineering).toEqual([{ key: 'engineering', name: 'Eng', leadKey: 'ac-software-engineer-river' }]);
		expect(teamOf(fitted, 'ada')).toBe('engineering');
	});

	it('a parent can be a department team the file does not define', () => {
		const all = rows();
		const hints: Hints = { path: 'f', teams: [{ key: 'app', name: 'App', parent: 'Engineering', members: ['app-frontend-dev'] }] };
		const fitted = applyHints(all, hints, suggestTeams(all));
		expect(fitted.drafts.find((d) => d.key === 'app')?.parentKey).toBe('engineering');
		expect(fitted.notes.parents).toEqual([]);
	});

	it('makes a lead a member even when the file did not list them', () => {
		const all = rows();
		const hints: Hints = { path: 'f', teams: [{ key: 'quality', name: 'Quality', lead: 'reviewer', members: [] }] };
		const fitted = applyHints(all, hints, []);
		expect(teamOf(fitted, 'reviewer')).toBe('quality');
		expect(fitted.drafts[0].leadKey).toBe('reviewer');
	});

	it('puts an agent listed in two teams in the deeper one, in either order', () => {
		const all = rows();
		const hints: Hints = {
			path: 'f',
			teams: [
				{ key: 'engineering', name: 'Engineering', members: ['ada', 'server-dev'] },
				{ key: 'office', name: 'Office', parent: 'engineering', members: ['server-dev'] }
			]
		};
		expect(teamOf(applyHints(all, hints, []), 'server-dev')).toBe('office');
		expect(teamOf(applyHints(all, { ...hints, teams: [...hints.teams].reverse() }, []), 'server-dev')).toBe('office');
	});

	it('keeps two teams of the same name under different owners apart, by their ids', () => {
		const all = rows();
		const hints: Hints = {
			path: 'f',
			teams: [
				{ key: 'a', name: 'A', members: [] },
				{ key: 'b', name: 'B', members: [] },
				{ key: 'qa-a', name: 'QA', parent: 'a', members: ['reviewer'] },
				{ key: 'qa-b', name: 'QA', parent: 'b', members: ['codex-dev'] }
			]
		};
		const fitted = applyHints(all, hints, []);
		expect(fitted.drafts.filter((d) => d.name === 'QA').map((d) => d.parentKey)).toEqual(['a', 'b']);
		expect(teamOf(fitted, 'reviewer')).toBe('qa-a');
		expect(teamOf(fitted, 'codex-dev')).toBe('qa-b');
	});

	it('lists what it cannot find in three notes (parent, lead, member) and goes on', () => {
		const all = rows();
		const hints: Hints = { path: 'f', teams: [{ key: 'app', name: 'App', parent: 'nowhere', lead: 'ghost', members: ['app-frontend-dev', 'phantom'] }] };
		const fitted = applyHints(all, hints, []);
		expect(fitted.notes).toMatchObject({ parents: ['nowhere'], leads: ['ghost'], members: ['phantom'] });
		expect(teamOf(fitted, 'app-frontend-dev')).toBe('app');
		expect(fitted.drafts[0].parentKey).toBeUndefined();
		expect(fitted.drafts[0].leadKey).toBeUndefined();
	});

	it('matches by messaging name or HR id, whatever the capitals, and changes nothing when the file is unusable', () => {
		const all = rows();
		const hints: Hints = { path: 'f', teams: [{ key: 'x', name: 'X', members: ['AC-SOFTWARE-ENGINEER-RILEY', 'Riley'] }] };
		const fitted = applyHints(all, hints, []);
		expect(teamOf(fitted, 'Riley')).toBe('x');
		expect(fitted.notes.members).toEqual(['Riley']);
		const broken = applyHints(all, { path: 'f', error: 'no', teams: [] }, suggestTeams(all));
		expect(broken.rows).toBe(all);
		expect(broken.teamCount).toBe(2);
	});

	it('breaks a circle of owner teams at the team that closes it and says so', () => {
		const all = rows();
		const hints: Hints = {
			path: 'f',
			teams: [
				{ key: 'a', name: 'A', parent: 'b', members: ['ada'] },
				{ key: 'b', name: 'B', parent: 'a', members: ['Eden'] },
				{ key: 'c', name: 'C', parent: 'c', members: ['reviewer'] }
			]
		};
		const fitted = applyHints(all, hints, []);
		const parents = Object.fromEntries(fitted.drafts.map((d) => [d.key, d.parentKey]));
		expect(Object.values(parents).filter(Boolean).length).toBeLessThan(3);
		expect(parents.c).toBeUndefined();
		expect(fitted.notes.circles.length).toBeGreaterThan(0);
	});
});
