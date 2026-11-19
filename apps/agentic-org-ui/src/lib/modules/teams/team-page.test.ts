import { describe, expect, it } from 'vitest';
import {
	agentsText,
	lookUpTeam,
	membersOfMax,
	peopleText,
	shownMembers,
	tabFor,
	teamHref
} from './team-page';
import type { TeamDetail, TeamMember } from './types';

const member = (id: string): TeamMember => ({
	id,
	name: id,
	kind: 'agent',
	handle: id,
	lead: false
});

const detail = (members: number, maxMembers = 6): TeamDetail => ({
	team: {
		id: 'web',
		name: 'Web',
		ref: 'TEAM-2',
		people: 1,
		agents: members - 1,
		projects: [],
		maxMembers,
		status: 'active'
	},
	path: [],
	members: Array.from({ length: members }, (_, i) => member(`m${i}`))
});

describe('tabs', () => {
	it('puts Work at the team URL and the others under it', () => {
		expect(teamHref('web')).toBe('/teams/web');
		expect(teamHref('web', 'processes')).toBe('/teams/web/processes');
		expect(teamHref('a b', 'projects')).toBe('/teams/a%20b/projects');
	});
	it('reads the tab from the pathname, falling back to Work', () => {
		expect(tabFor('/teams/web', 'web').id).toBe('work');
		expect(tabFor('/teams/web/activity', 'web').id).toBe('activity');
		expect(tabFor('/teams/web/conversations/', 'web').id).toBe('conversations');
		expect(tabFor('/teams/web/nope', 'web').id).toBe('work');
		expect(tabFor('/teams/other/projects', 'web').id).toBe('work');
	});
});

describe('members above the tabs', () => {
	it('shows at most six until expanded', () => {
		const d = detail(9);
		expect(shownMembers(d.members, false)).toHaveLength(6);
		expect(shownMembers(d.members, true)).toHaveLength(9);
		expect(shownMembers(detail(3).members, false)).toHaveLength(3);
	});
	it('says the counts plural-correct and against the limit', () => {
		expect(peopleText(detail(2).team)).toBe('1 person');
		expect(agentsText(detail(2).team)).toBe('1 agent');
		expect(agentsText(detail(4).team)).toBe('3 agents');
		expect(membersOfMax(detail(5, 6))).toBe('5 of 6');
	});
});

describe('lookUpTeam', () => {
	it('returns the team', async () => {
		const d = detail(1);
		expect(await lookUpTeam(async () => d)).toEqual({ found: true, detail: d });
	});
	it('turns not_found into its own state', async () => {
		const gone = { source: 'api', code: 'not_found', message: "That team isn't here." };
		expect(
			await lookUpTeam(async () => {
				throw gone;
			})
		).toEqual({ found: false, message: "That team isn't here." });
	});
	it('rethrows every other failure, so it can be retried', async () => {
		const down = { source: 'api', code: 'unavailable', message: 'Not served.' };
		await expect(
			lookUpTeam(async () => {
				throw down;
			})
		).rejects.toBe(down);
	});
});
