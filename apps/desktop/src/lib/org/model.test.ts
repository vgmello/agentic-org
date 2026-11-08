import { describe, expect, it } from 'vitest';
import {
	agentsOf,
	highlightedByOffice,
	leadOf,
	membersOfTeam,
	kindWord,
	normalizeKinds,
	notInATeam,
	othersOf,
	organizationCounts,
	otherTeamsOf,
	otherTeamsOnProject,
	owner,
	peopleOf,
	projectsOfTeam,
	subTeamCount,
	teamTree,
	teamsOfMember,
	unassignedProjects,
	type OfficeFilter
} from './model';
import { emptyOrg, sampleOrg } from './sample';

const s = sampleOrg('office-1');
const names = (xs: { name: string }[]) => xs.map((x) => x.name);

describe('TEAM-1 the ownership tree', () => {
	it('top-level teams A–Z, each with its sub-teams A–Z', () => {
		const tree = teamTree(s);
		expect(names(tree.map((n) => n.team))).toEqual(['Design', 'Engineering', 'People']);
		const eng = tree.find((n) => n.team.name === 'Engineering')!;
		expect(names(eng.children.map((c) => c.team))).toEqual(['Platform', 'Web']);
		expect(subTeamCount(eng)).toBe(2);
		expect(subTeamCount(tree[0])).toBe(0);
	});

	it('shows the lead, or none', () => {
		const eng = s.teams.find((t) => t.id === 'eng')!;
		expect(leadOf(s, eng)?.name).toBe('Sam Okafor');
		expect(leadOf(s, { ...eng, lead_member_id: undefined })).toBeUndefined();
	});

	it('splits a team into agents and people, A–Z', () => {
		const people = membersOfTeam(s, 'people');
		expect(names(agentsOf(people))).toEqual(['Inês Duarte']);
		expect(names(peopleOf(people))).toEqual(['Hana Weiss', 'Leo Brandt']);
	});

	it('lists a team’s projects', () => {
		expect(names(projectsOfTeam(s, 'platform'))).toEqual(['Atlas']);
		expect(projectsOfTeam(s, 'eng')).toEqual([]);
	});

	it('survives a team that points at a missing owner, itself, or a loop', () => {
		const odd = {
			...s,
			teams: [
				{ id: 'a', name: 'A', owner_team_id: 'ghost', version: 1 },
				{ id: 'b', name: 'B', owner_team_id: 'b', version: 1 },
				{ id: 'c', name: 'C', owner_team_id: 'd', version: 1 },
				{ id: 'd', name: 'D', owner_team_id: 'c', version: 1 }
			]
		};
		const seen: string[] = [];
		const walk = (ns: ReturnType<typeof teamTree>) => ns.forEach((n) => (seen.push(n.team.id), walk(n.children)));
		walk(teamTree(odd));
		expect(new Set(seen).size).toBe(seen.length); // nobody twice, and it terminates
		expect(seen.sort()).toEqual(['a', 'b', 'c', 'd']); // nobody lost, even teams that own each other
	});
});

describe('TEAM-2 a member in several teams', () => {
	it('is found in each team and the badge lists the others', () => {
		expect(names(teamsOfMember(s, 'mira'))).toEqual(['Platform', 'Web']);
		expect(names(otherTeamsOf(s, 'mira', 'web'))).toEqual(['Platform']);
		expect(names(otherTeamsOf(s, 'noor', 'web'))).toEqual(['Design']);
		expect(otherTeamsOf(s, 'sam', 'eng')).toEqual([]);
		expect(membersOfTeam(s, 'platform').map((m) => m.id)).toContain('mira');
		expect(membersOfTeam(s, 'web').map((m) => m.id)).toContain('mira');
	});
});

describe('TEAM-3 and OWNER-1 not in a team', () => {
	it('lists agents and people in no team, and never the owner', () => {
		expect(names(notInATeam(s))).toEqual(['Avery Stone', 'Quinn Adeyemi']);
		expect(notInATeam(s).some((m) => m.owner)).toBe(false);
	});

	it('a member whose only membership points at a team that is gone is not lost', () => {
		const stale = { ...s, memberships: [...s.memberships, { id: 'g', team_id: 'deleted', member_id: 'quinn', version: 1 }] };
		expect(notInATeam(stale).map((m) => m.id)).toContain('quinn');
	});

	it('a lead is a member of the team and never the owner', () => {
		const eng = s.teams.find((t) => t.id === 'eng')!;
		expect(leadOf(s, { ...eng, lead_member_id: 'you' })).toBeUndefined();
		expect(leadOf(s, { ...eng, lead_member_id: 'juno' })).toBeUndefined(); // Juno isn't in Engineering
		expect(leadOf(s, { ...eng, lead_member_id: 'sam' })?.id).toBe('sam');
	});

	it('members of a kind this build does not know are kept apart, not dropped', () => {
		const odd = { ...s, members: s.members.map((m) => (m.id === 'leo' ? { ...m, kind: 'unknown' as const } : m)) };
		const people = membersOfTeam(odd, 'people');
		expect(othersOf(people).map((m) => m.id)).toEqual(['leo']);
		expect(agentsOf(people).length + peopleOf(people).length + othersOf(people).length).toBe(people.length);
	});

	it('a team never lists the owner even if a membership names them', () => {
		const withOwner = { ...s, memberships: [...s.memberships, { id: 'x', team_id: 'eng', member_id: 'you', version: 1 }] };
		expect(membersOfTeam(withOwner, 'eng').some((m) => m.owner)).toBe(false);
	});

	it('counts leave the owner out', () => {
		expect(owner(s)?.name).toBe('You');
		expect(organizationCounts(s)).toEqual({ agents: 8, people: 3, others: 0, teams: 5 });
		expect(organizationCounts(emptyOrg())).toEqual({ agents: 0, people: 0, others: 0, teams: 0 });
		const odd = { ...s, members: s.members.map((m) => (m.id === 'leo' ? { ...m, kind: 'unknown' as const } : m)) };
		expect(organizationCounts(odd)).toMatchObject({ people: 2, others: 1 });
	});
});

describe('TEAM-4 the office filter', () => {
	const hq: OfficeFilter = { kind: 'office', id: 'office-1' };
	const member = (id: string) => s.members.find((m) => m.id === id)!;

	it('highlights agents placed in that office and dims everyone else, people included', () => {
		expect(highlightedByOffice(member('sam'), hq)).toBe(true);
		expect(highlightedByOffice(member('sam'), { kind: 'office', id: 'other' })).toBe(false);
		expect(highlightedByOffice(member('avery'), hq)).toBe(false);
	});

	it('Unplaced highlights agents with no office only', () => {
		const unplaced: OfficeFilter = { kind: 'unplaced' };
		expect(highlightedByOffice(member('ines'), unplaced)).toBe(true);
		expect(highlightedByOffice(member('sam'), unplaced)).toBe(false);
		expect(highlightedByOffice(member('hana'), unplaced)).toBe(false);
	});

	it('an agent placed in an office that is no longer listed counts as unplaced', () => {
		const known = new Set(['office-1']);
		const orphan = { ...member('sam'), office_id: 'removed' };
		expect(highlightedByOffice(orphan, { kind: 'unplaced' }, known)).toBe(true);
		expect(highlightedByOffice(orphan, hq, known)).toBe(false);
		expect(highlightedByOffice(member('sam'), { kind: 'unplaced' }, known)).toBe(false);
	});

	it('no filter means nothing is highlighted or dimmed (and PEOPLE-6: people never highlight)', () => {
		expect(highlightedByOffice(member('sam'), { kind: 'all' })).toBeUndefined();
		for (const f of [hq, { kind: 'unplaced' } as const]) expect(highlightedByOffice(member('hana'), f)).toBe(false);
	});
});

describe('TEAM-13 and TEAM-14 projects', () => {
	it('a project worked on by several teams lists the other teams', () => {
		expect(names(otherTeamsOnProject(s, 'beacon', 'web'))).toEqual(['Design']);
		expect(names(otherTeamsOnProject(s, 'beacon', 'design'))).toEqual(['Web']);
		expect(otherTeamsOnProject(s, 'atlas', 'platform')).toEqual([]);
	});

	it('a project whose only team is gone is unassigned, not invisible', () => {
		const gone = { ...s, project_teams: [...(s.project_teams ?? []), { id: 'x', project_id: 'scratch', team_id: 'deleted', version: 1 }] };
		expect(names(unassignedProjects(gone))).toEqual(['Scratchpad']);
	});

	it('projects with no team are unassigned', () => {
		expect(names(unassignedProjects(s))).toEqual(['Scratchpad']);
		expect(unassignedProjects(emptyOrg())).toEqual([]);
	});
});

describe('what an office sends', () => {
	it('a member kind this build does not know becomes unknown', () => {
		const odd = { ...s, members: s.members.map((m) => (m.id === 'leo' ? { ...m, kind: 'robot-dog' as never } : m)) };
		const fixed = normalizeKinds(odd);
		expect(fixed.members.find((m) => m.id === 'leo')?.kind).toBe('unknown');
		expect(fixed.members.find((m) => m.id === 'sam')?.kind).toBe('agent');
		expect(fixed.members.find((m) => m.id === 'hana')?.kind).toBe('person');
	});
});

describe('kindWord', () => {
	it('says Agent, Person, or Member for a kind this build does not know', () => {
		expect([kindWord('agent'), kindWord('person'), kindWord('unknown')]).toEqual(['Agent', 'Person', 'Member']);
	});
});

describe('normalizeKinds and an agent’s profile', () => {
	const agent = (over: Record<string, unknown>) => ({
		id: 'a',
		kind: 'agent' as const,
		name: 'A',
		version: 1,
		agent: { role: 'r', title: 't', runtime: 'claude_code', messaging_name: 'a', status: 'active', ...over }
	});
	const read = (m: unknown) => normalizeKinds({ position: { epoch: 1, seq: 1 }, teams: [], memberships: [], members: [m as never] }).members[0];

	it('keeps what it knows', () => {
		const m = read(agent({ run_mode: 'on_call', runtime: { other: 'my-runtime' } }));
		expect(m.agent).toMatchObject({ status: 'active', run_mode: 'on_call', runtime: { other: 'my-runtime' } });
	});

	it('reads an unknown runtime, status or run mode as unknown, never as working', () => {
		const m = read(agent({ status: 'sleeping', runtime: 'quantum', run_mode: 'whenever' }));
		expect(m.agent).toMatchObject({ status: 'unknown', runtime: 'unknown', run_mode: 'unknown' });
	});

	it('leaves a missing run mode missing, and a person without a profile alone', () => {
		expect(read(agent({})).agent).not.toHaveProperty('run_mode');
		const person = { id: 'p', kind: 'person', name: 'P', version: 1 };
		expect(read(person)).toEqual(person);
	});
});
