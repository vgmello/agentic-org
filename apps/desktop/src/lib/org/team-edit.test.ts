import { describe, expect, it } from 'vitest';
import { sampleOrg } from './sample';
import { checkTeamName, deleteSummary, teamOptions, teamPath } from './team-edit';

const s = sampleOrg('office-1');

describe('teamPath', () => {
	it('names the owners first', () => {
		expect(teamPath(s, 'web')).toBe('Engineering › Web');
		expect(teamPath(s, 'design')).toBe('Design');
		expect(teamPath(s, 'ghost')).toBe('');
	});
	it('stops on an ownership loop instead of running forever', () => {
		const loop = { ...s, teams: s.teams.map((t) => (t.id === 'eng' ? { ...t, owner_team_id: 'web' } : t)) };
		expect(teamPath(loop, 'web').split(' › ').length).toBeLessThanOrEqual(2);
	});
});

describe('teamOptions', () => {
	it('lists the tree in order with depth and path', () => {
		const names = teamOptions(s).map((o) => `${o.depth}:${o.team.name}`);
		expect(names).toEqual(['0:Design', '0:Engineering', '1:Platform', '1:Web', '0:People']);
		expect(teamOptions(s).find((o) => o.team.id === 'web')?.path).toBe('Engineering › Web');
	});
	it('leaves out the excluded teams and everything under them', () => {
		expect(teamOptions(s, new Set(['eng'])).map((o) => o.team.name)).toEqual(['Design', 'People']);
	});
});

describe('TEAM-5 checkTeamName', () => {
	it('asks for a name, 1 to 64 characters', () => {
		expect(checkTeamName(s, '   ', undefined)).toEqual({ ok: false, message: 'Name a team (1–64 characters).' });
		expect(checkTeamName(s, 'x'.repeat(65), undefined).ok).toBe(false);
		expect(checkTeamName(s, '  Ops  ', undefined)).toEqual({ ok: true, name: 'Ops' });
	});
	it('refuses a sibling’s name, ignoring case, and says which team', () => {
		expect(checkTeamName(s, 'WEB', 'eng')).toEqual({ ok: false, message: 'Engineering already has a team called WEB.' });
		expect(checkTeamName(s, 'design', undefined)).toEqual({ ok: false, message: "There's already a top-level team called design." });
	});
	it('allows the same name under another team, and a team keeping its own name', () => {
		expect(checkTeamName(s, 'Web', 'design').ok).toBe(true);
		expect(checkTeamName(s, 'Web', 'eng', 'web').ok).toBe(true);
	});
});

describe('TEAM-9 deleteSummary', () => {
	it('refuses a team that owns teams, naming them', () => {
		const r = deleteSummary(s, s.teams.find((t) => t.id === 'eng')!);
		expect(r.refused).toBe('Engineering still has 2 teams under it (Web, Platform). Move or delete them first.');
		expect(r.lines).toEqual([]);
	});

	it('says who moves to Not in a team and which project loses its only team', () => {
		const r = deleteSummary(s, s.teams.find((t) => t.id === 'people')!);
		expect(r.refused).toBeUndefined();
		expect(r.lines).toContain('Hana Weiss, Inês Duarte and Leo Brandt move to Not in a team.');
		expect(r.lines).toContain('Onboarding kit has no other team, so it moves to Unassigned projects.');
	});

	it('says who stays elsewhere and which project stays with another team', () => {
		const r = deleteSummary(s, s.teams.find((t) => t.id === 'design')!);
		expect(r.lines).toContain('Noor Haddad stays in the organization and in their other teams.');
		expect(r.lines).toContain('Juno Park and Pax Moreau move to Not in a team.');
		expect(r.lines).toContain('Beacon stays with Web.');
	});

	it('says so when no one is in the team', () => {
		const r = deleteSummary({ ...s, memberships: [], project_teams: [] }, s.teams.find((t) => t.id === 'web')!);
		expect(r.lines).toEqual(['No one is in Web.']);
	});
});
