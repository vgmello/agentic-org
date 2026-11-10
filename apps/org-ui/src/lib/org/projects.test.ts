import { describe, expect, it } from 'vitest';
import { leadsLine, projectById, projectRows } from './projects';
import { sampleOrg } from './sample';

const s = sampleOrg('office-1');

describe('the Projects list', () => {
	it('lists every project A–Z with its teams, its leads and whether it is shared or unassigned', () => {
		const rows = projectRows(s);
		expect(rows.map((r) => r.project.name)).toEqual(['Atlas', 'Beacon', 'Onboarding kit', 'Scratchpad']);
		const beacon = rows.find((r) => r.project.id === 'beacon')!;
		expect(beacon.teams.map((t) => t.name)).toEqual(['Design', 'Web']);
		expect(beacon.shared).toBe(true);
		expect(beacon.leads.map((l) => `${l.lead.name} (${l.team.name})`)).toEqual(['Juno Park (Design)', 'Noor Haddad (Web)']);
		expect(rows.find((r) => r.project.id === 'scratch')).toMatchObject({ unassigned: true, shared: false, teams: [] });
	});

	it('says the leads in words: one lead, one per team, none, and nothing for an unassigned project', () => {
		const rows = projectRows(s);
		expect(leadsLine(rows.find((r) => r.project.id === 'atlas')!)).toBe('Lead: Kai Lindqvist');
		expect(leadsLine(rows.find((r) => r.project.id === 'beacon')!)).toBe('Leads: Juno Park (Design), Noor Haddad (Web)');
		expect(leadsLine(rows.find((r) => r.project.id === 'scratch')!)).toBe('');
		const noLead = { ...s, teams: s.teams.map((t) => (t.id === 'platform' ? { ...t, lead_member_id: undefined } : t)) };
		expect(leadsLine(projectRows(noLead).find((r) => r.project.id === 'atlas')!)).toBe('No lead');
	});

	it('finds one project by id', () => {
		expect(projectById(s, 'atlas')?.project.name).toBe('Atlas');
		expect(projectById(s, 'ghost')).toBeUndefined();
	});

	it('an organization with no projects has an empty list', () => {
		expect(projectRows({ ...s, projects: undefined, project_teams: undefined })).toEqual([]);
	});
});
