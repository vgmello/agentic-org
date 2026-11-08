import { describe, expect, it } from 'vitest';
import { createMockOrg, type MockStorage } from './api';
import { sessionFor } from './session';
import { createTeam, deleteConsequences, deleteTeam, moveTargets, moveTeam, setTeamMembers, wouldClearLead } from './actions';
import { membersOfTeam, teamTree } from './model';

const memory = (): MockStorage => {
	const m = new Map<string, string>();
	return { getItem: (k) => m.get(k) ?? null, setItem: (k, v) => void m.set(k, v) };
};
const make = () => sessionFor(createMockOrg(async () => 'office-1', { storage: memory() }), 'o');
const names = (xs: { name: string }[]) => xs.map((x) => x.name);

describe('TEAM-5 creating a team with its members and lead', () => {
	it('creates it under a team, adds the members, and sets the lead from them', async () => {
		const api = make();
		const r = await createTeam(api, { name: 'QA', ownerTeamId: 'eng', memberIds: ['mira', 'quinn'], leadId: 'quinn' });
		expect(r.created.error).toBeUndefined();
		expect(r.steps.every((s) => !s.error)).toBe(true);
		const s = await api.snapshot();
		const team = s.teams.find((t) => t.id === r.teamId)!;
		expect(team).toMatchObject({ name: 'QA', owner_team_id: 'eng', lead_member_id: 'quinn' });
		expect(names(membersOfTeam(s, r.teamId))).toEqual(['Mira Castillo', 'Quinn Adeyemi']);
	});

	it('a duplicate sibling name fails the team and tries nothing else', async () => {
		const api = make();
		const r = await createTeam(api, { name: 'design', memberIds: ['quinn'] });
		expect(r.created.error?.code).toBe('already_exists');
		expect(r.steps).toEqual([]);
		expect((await api.snapshot()).memberships.some((m) => m.member_id === 'quinn')).toBe(false);
	});

	it('the owner can’t be a member; the rest still go in, and a lead that failed to join is not set', async () => {
		const api = make();
		const r = await createTeam(api, { name: 'Ops', memberIds: ['you', 'quinn'], leadId: 'you' });
		expect(r.steps.find((s) => s.error)?.error?.code).toBe('owner_protected');
		expect(names(membersOfTeam(await api.snapshot(), r.teamId))).toEqual(['Quinn Adeyemi']);
		expect((await api.snapshot()).teams.find((t) => t.id === r.teamId)?.lead_member_id).toBeUndefined();
	});
});

describe('TEAM-6 editing members', () => {
	it('adds who is missing and removes who is no longer wanted, in one go', async () => {
		const api = make();
		const { steps } = await setTeamMembers(api, 'web', ['noor', 'quinn']); // Mira out, Quinn in
		expect(steps).toHaveLength(2);
		expect(steps.every((s) => !s.error)).toBe(true);
		expect(names(membersOfTeam(await api.snapshot(), 'web'))).toEqual(['Noor Haddad', 'Quinn Adeyemi']);
	});

	it('removing the lead clears it, and wouldClearLead warns before', async () => {
		const api = make();
		const s = await api.snapshot();
		const web = s.teams.find((t) => t.id === 'web')!; // Noor leads Web
		expect(wouldClearLead(s, web, ['mira'])).toBe(true);
		expect(wouldClearLead(s, web, ['noor', 'mira'])).toBe(false);
		await setTeamMembers(api, 'web', ['mira']);
		expect((await api.snapshot()).teams.find((t) => t.id === 'web')?.lead_member_id).toBeUndefined();
	});

	it('one failing change is reported and does not lose the others', async () => {
		const api = make();
		const { steps } = await setTeamMembers(api, 'web', ['noor', 'mira', 'you', 'quinn']);
		expect(steps.filter((s) => s.error).map((s) => s.error?.code)).toEqual(['owner_protected']);
		expect(names(membersOfTeam(await api.snapshot(), 'web'))).toContain('Quinn Adeyemi');
	});
});

describe('TEAM-7 moving a team', () => {
	it('never offers the team, its owner, or anything below it as a target', async () => {
		const s = await make().snapshot();
		expect(names(moveTargets(s, s.teams.find((t) => t.id === 'eng')!))).toEqual(['Design', 'People']); // not Web/Platform (below)
		expect(names(moveTargets(s, s.teams.find((t) => t.id === 'web')!))).toEqual(['Design', 'People', 'Platform']); // not Engineering (its owner now), not itself
	});

	it('moves a team, and undo puts it back', async () => {
		const api = make();
		const { undo } = await moveTeam(api, 'platform', 'design');
		expect((await api.snapshot()).teams.find((t) => t.id === 'platform')?.owner_team_id).toBe('design');
		await undo();
		expect((await api.snapshot()).teams.find((t) => t.id === 'platform')?.owner_team_id).toBe('eng');
	});

	it('moving to the top level works and undoes', async () => {
		const api = make();
		const { undo } = await moveTeam(api, 'web', undefined);
		expect(teamTree(await api.snapshot()).map((n) => n.team.name)).toContain('Web');
		await undo();
		expect((await api.snapshot()).teams.find((t) => t.id === 'web')?.owner_team_id).toBe('eng');
	});

	it('a team that was deleted meanwhile is reported', async () => {
		await expect(moveTeam(make(), 'ghost', 'eng')).rejects.toMatchObject({ code: 'not_found' });
	});
});

describe('TEAM-9 deleting a team', () => {
	it('is refused up front for a team that owns teams, and lists them', async () => {
		const s = await make().snapshot();
		const c = deleteConsequences(s, s.teams.find((t) => t.id === 'eng')!);
		expect(c.refused).toBe(true);
		expect(names(c.owns)).toEqual(['Web', 'Platform']);
	});

	it('names who moves to "Not in a team" and which projects become unassigned', async () => {
		const s = await make().snapshot();
		const c = deleteConsequences(s, s.teams.find((t) => t.id === 'people')!);
		expect(c.refused).toBe(false);
		expect(names(c.toNotInATeam)).toEqual(['Hana Weiss', 'Inês Duarte', 'Leo Brandt']);
		expect(names(c.unassigned)).toEqual(['Onboarding kit']);
		// Mira is also in Platform, so deleting Web doesn't leave her without a team; Beacon is shared with Design.
		const web = deleteConsequences(s, s.teams.find((t) => t.id === 'web')!);
		expect(names(web.toNotInATeam)).not.toContain('Mira Castillo');
		expect(web.unassigned).toEqual([]);
	});

	it('deletes it for real, and HQ refuses one that owns teams', async () => {
		const api = make();
		const s = await api.snapshot();
		await deleteTeam(api, s.teams.find((t) => t.id === 'people')!);
		expect((await api.snapshot()).teams.some((t) => t.id === 'people')).toBe(false);
		await expect(deleteTeam(api, s.teams.find((t) => t.id === 'eng')!)).rejects.toMatchObject({ code: 'not_empty' });
	});
});
