import { describe, expect, it } from 'vitest';
import { SAMPLE_SCAN } from './mock';
import { buildRows } from './rows';
import { applyHints } from './hints';
import { SAMPLE_HINTS } from './mock';
import { membersOf, suggestProject, suggestTeams, teamsToCreate, whatHappens, withDefaultManagers, type TeamDraft } from './teams';

const rows = () => buildRows(SAMPLE_SCAN, { exclude: ['sample'] }).rows;

describe('IMP-5 the suggested teams, leads and project', () => {
	it('makes a team of each HR department, led by the one the others report to', () => {
		expect(suggestTeams(rows())).toEqual([
			{ key: 'design', name: 'Design', leadKey: 'ac-product-designer-eden' },
			{ key: 'engineering', name: 'Engineering', leadKey: 'ac-staff-engineer-ada' }
		]);
	});

	it('has no lead when nobody in a bigger team reports to anyone in it', () => {
		const flat = rows().filter((r) => r.teamKey === 'Engineering');
		flat.forEach((r) => (r.managerKey = 'owner'));
		expect(suggestTeams(flat)[0].leadKey).toBeUndefined();
	});

	it('leaves agents without a department in no team', () => {
		expect(rows().filter((r) => !r.teamKey).map((r) => r.name)).toContain('server-dev');
	});

	it('creates only the teams someone is brought into, and the teams that own them', () => {
		const all = rows();
		const drafts: TeamDraft[] = [
			...suggestTeams(all),
			{ key: 'app', name: 'App', parentKey: 'engineering' },
			{ key: 'empty', name: 'Empty' }
		];
		const river = all.find((r) => r.name === 'River')!;
		river.teamKey = 'App';
		expect(teamsToCreate(all, drafts).map((t) => t.key)).toEqual(['design', 'engineering', 'app']);
		// A team whose only member is still missing details is not created yet.
		const server = all.find((r) => r.name === 'server-dev')!;
		server.teamKey = 'Empty';
		expect(teamsToCreate(all, drafts).map((t) => t.key)).not.toContain('empty');
		Object.assign(server, { role: 'software engineer', title: 'software engineer', runtime: 'codex' });
		expect(teamsToCreate(all, drafts).map((t) => t.key)).toContain('empty');
		expect(membersOf(all, drafts[3]).map((r) => r.name)).toEqual(['server-dev']);
	});

	it('suggests the HR project code, shared by the top-level teams that will exist', () => {
		const all = rows();
		expect(suggestProject(all, suggestTeams(all))).toEqual({ key: 'ac', name: 'ac', teamKeys: ['design', 'engineering'] });
		expect(suggestProject(all.map((r) => ({ ...r, hr: undefined })), [])).toBeUndefined();
	});
});

describe('IMP-6 what happens, in words', () => {
	it('names the counts and says nothing changes outside', () => {
		const all = rows();
		const drafts = suggestTeams(all);
		expect(whatHappens(all, drafts, suggestProject(all, drafts), 'Agentic Consulting')).toBe(
			'Creates 5 agents, 2 teams and 1 project in Agentic Consulting. Nothing changes in your HR workspace or in messaging. Agents keep running where they are.'
		);
		expect(whatHappens([], [], undefined, 'Acme')).toBe('Creates 0 agents and 0 teams in Acme. Nothing changes in your HR workspace or in messaging. Agents keep running where they are.');
	});

	it('says "1 agent" and "1 team" in the singular and leaves the project out when nobody is brought in', () => {
		const one = rows().filter((r) => r.name === 'Eden');
		const drafts = suggestTeams(one);
		expect(whatHappens(one, drafts, undefined, 'Acme')).toBe('Creates 1 agent and 1 team in Acme. Nothing changes in your HR workspace or in messaging. Agents keep running where they are.');
		expect(whatHappens([], [], { key: 'ac', name: 'ac', teamKeys: [] }, 'Acme')).toBe('Creates 0 agents and 0 teams in Acme. Nothing changes in your HR workspace or in messaging. Agents keep running where they are.');
	});

	it('does not make a team of a blank department, and does not let a skipped row lead', () => {
		const all = rows();
		all.find((r) => r.name === 'Eden')!.teamKey = '   ';
		expect(suggestTeams(all).map((t) => t.key)).toEqual(['engineering']);
		all.find((r) => r.name === 'ada')!.choice = 'skip';
		expect(suggestTeams(all).find((t) => t.key === 'engineering')?.leadKey).toBeUndefined();
	});
});

describe('rev 15 who a row reports to when HR does not say', () => {
	const fitted = () => {
		const all = rows();
		const f = applyHints(all, SAMPLE_HINTS, suggestTeams(all));
		return withDefaultManagers(f.rows, f.drafts);
	};
	const managerOf = (all: ReturnType<typeof fitted>, name: string) => all.find((r) => r.name === name)!.managerKey;

	it('is the lead of the top-level team, so the shape of our team needs no edits', () => {
		const all = fitted();
		for (const name of ['server-dev', 'app-frontend-dev', 'codex-dev', 'agy-senior-dev', 'reviewer']) expect(managerOf(all, name)).toBe('ac-staff-engineer-ada');
		// HR's own lines stay: ada and Eden report to the owner, River to ada.
		expect(managerOf(all, 'ada')).toBe('owner');
		expect(managerOf(all, 'Eden')).toBe('owner');
		expect(managerOf(all, 'River')).toBe('ac-staff-engineer-ada');
	});

	it('is the owner for the lead of the top-level team itself, for a row in no team, and when the lead is skipped', () => {
		const all = rows();
		const f = applyHints(all, SAMPLE_HINTS, suggestTeams(all));
		f.rows.find((r) => r.name === 'codex-dev')!.teamKey = undefined;
		f.rows.find((r) => r.name === 'ada')!.choice = 'skip';
		const out = withDefaultManagers(f.rows, f.drafts);
		expect(out.find((r) => r.name === 'codex-dev')!.managerKey).toBe('owner');
		expect(out.find((r) => r.name === 'server-dev')!.managerKey).toBe('owner');
	});

	it('does not leave a circle', () => {
		const all = rows();
		const hints = { path: 'f', teams: [{ key: 'x', name: 'X', lead: 'server-dev', members: ['server-dev', 'codex-dev'] }, { key: 'y', name: 'Y', parent: 'x', lead: 'codex-dev', members: ['codex-dev'] }] };
		const f = applyHints(all, hints, []);
		const out = withDefaultManagers(f.rows, f.drafts);
		const boss = (k: string) => out.find((r) => r.key === k)!.managerKey;
		expect(boss('server-dev')).toBe('owner');
		expect(boss('codex-dev')).toBe('server-dev');
	});
});
