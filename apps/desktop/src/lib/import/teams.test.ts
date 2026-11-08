import { describe, expect, it } from 'vitest';
import { SAMPLE_SCAN } from './mock';
import { buildRows } from './rows';
import { membersOf, suggestProject, suggestTeams, teamsToCreate, whatHappens, type TeamDraft } from './teams';

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
});
