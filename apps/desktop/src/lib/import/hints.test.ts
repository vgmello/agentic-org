import { describe, expect, it } from 'vitest';
import { applyHints } from './hints';
import { SAMPLE_HINTS, SAMPLE_SCAN } from './mock';
import { buildRows } from './rows';
import { suggestTeams, teamsToCreate } from './teams';

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
		expect(fitted.notFound).toEqual([]);
		expect(fitted.fromFile).toBe(6);
		const placed = Object.fromEntries(fitted.rows.map((r) => [r.name, r.teamKey]));
		expect(placed).toMatchObject({
			ada: 'Engineering',
			Eden: 'Design',
			'app-frontend-dev': 'App',
			Riley: 'App',
			'server-dev': 'Office',
			River: 'Office',
			Remy: 'Agent tooling',
			'codex-dev': 'Agent tooling',
			'agy-senior-dev': 'Agent tooling',
			reviewer: 'Quality'
		});
		const app = fitted.drafts.find((d) => d.key === 'app')!;
		expect(app).toMatchObject({ parentKey: 'engineering', leadKey: 'app-frontend-dev' });
		expect(fitted.drafts.find((d) => d.key === 'engineering')?.leadKey).toBe('ac-staff-engineer-ada');
		// The department teams are the file's: Engineering and Design are not added twice.
		expect(fitted.drafts.filter((d) => d.key === 'engineering' || d.key === 'design')).toHaveLength(2);
		expect(teamsToCreate(fitted.rows, fitted.drafts).map((d) => d.key).sort()).toEqual(['agent tooling', 'app', 'design', 'engineering', 'office', 'quality']);
	});

	it('leaves the rows it does not mention in their department team', () => {
		const all = rows();
		const partial = { path: 'f', teams: [{ key: 'app', name: 'App', members: ['app-frontend-dev'] }] };
		const fitted = applyHints(all, partial, suggestTeams(all));
		expect(teamOf(fitted, 'app-frontend-dev')).toBe('App');
		expect(teamOf(fitted, 'ada')).toBe('Engineering');
		expect(fitted.drafts.map((d) => d.key)).toEqual(expect.arrayContaining(['app', 'engineering', 'design']));
	});

	it('makes a lead a member of the team even when the file did not list them', () => {
		const all = rows();
		const hints = { path: 'f', teams: [{ key: 'quality', name: 'Quality', lead: 'reviewer', members: [] }] };
		const fitted = applyHints(all, hints, []);
		expect(teamOf(fitted, 'reviewer')).toBe('Quality');
		expect(fitted.drafts[0].leadKey).toBe('reviewer');
	});

	it('puts an agent listed in two teams in the deeper one', () => {
		const all = rows();
		const hints = {
			path: 'f',
			teams: [
				{ key: 'engineering', name: 'Engineering', members: ['ada', 'server-dev'] },
				{ key: 'office', name: 'Office', parent: 'engineering', members: ['server-dev'] }
			]
		};
		expect(teamOf(applyHints(all, hints, []), 'server-dev')).toBe('Office');
		expect(teamOf(applyHints(all, { ...hints, teams: [...hints.teams].reverse() }, []), 'server-dev')).toBe('Office');
	});

	it('lists names it cannot find and parents it does not define, and goes on', () => {
		const all = rows();
		const hints = { path: 'f', teams: [{ key: 'app', name: 'App', parent: 'nowhere', lead: 'ghost', members: ['app-frontend-dev', 'ghost', 'phantom'] }] };
		const fitted = applyHints(all, hints, []);
		expect(fitted.notFound).toEqual(['nowhere', 'ghost', 'phantom']);
		expect(teamOf(fitted, 'app-frontend-dev')).toBe('App');
		expect(fitted.drafts[0].parentKey).toBeUndefined();
	});

	it('matches by messaging name or HR id, whatever the capitals, and changes nothing when the file is unusable', () => {
		const all = rows();
		const hints = { path: 'f', teams: [{ key: 'x', name: 'X', members: ['AC-SOFTWARE-ENGINEER-RILEY', 'Riley'] }] };
		const fitted = applyHints(all, hints, []);
		expect(teamOf(fitted, 'Riley')).toBe('X');
		expect(fitted.notFound).toEqual(['Riley']);
		const broken = applyHints(all, { path: 'f', error: 'no', teams: [] }, suggestTeams(all));
		expect(broken.rows).toBe(all);
		expect(broken.fromFile).toBe(0);
	});
});
