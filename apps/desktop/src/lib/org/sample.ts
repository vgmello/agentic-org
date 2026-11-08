import type { Member, Membership, OrgSnapshot, ProjectSummary, ProjectTeam, Team } from '$lib/proto';

// A sample organization for the mock office: invented people, none from a real workspace.
// `officeId` is the HQ office the agents are placed in.

const team = (id: string, name: string, owner?: string, lead?: string): Team => ({
	id,
	name,
	version: 1,
	...(owner ? { owner_team_id: owner } : {}),
	...(lead ? { lead_member_id: lead } : {})
});

const member = (id: string, kind: 'agent' | 'person', name: string, extra: Partial<Member> = {}): Member => ({
	id,
	kind,
	name,
	version: 1,
	...extra
});

let n = 0;
const joined = (team_id: string, member_id: string, title_in_team?: string): Membership => ({
	id: `ms-${++n}`,
	team_id,
	member_id,
	version: 1,
	...(title_in_team ? { title_in_team } : {})
});

export function sampleOrg(officeId: string): OrgSnapshot {
	n = 0;
	const agent = (id: string, name: string, extra: Partial<Member> = {}) =>
		member(id, 'agent', name, { office_id: officeId, ...extra });
	const members: Member[] = [
		member('you', 'person', 'Alex Kim', { owner: true }),
		member('avery', 'person', 'Avery Stone', { contact: 'avery@example.com' }),
		member('hana', 'person', 'Hana Weiss', { reports_to_member_id: 'avery' }),
		member('leo', 'person', 'Leo Brandt', { reports_to_member_id: 'hana' }),
		agent('sam', 'Sam Okafor', { reports_to_member_id: 'avery' }),
		agent('noor', 'Noor Haddad', { reports_to_member_id: 'sam' }),
		agent('kai', 'Kai Lindqvist', { reports_to_member_id: 'sam' }),
		agent('mira', 'Mira Castillo', { reports_to_member_id: 'sam' }),
		agent('juno', 'Juno Park', { reports_to_member_id: 'avery' }),
		agent('pax', 'Pax Moreau', { reports_to_member_id: 'juno' }),
		member('ines', 'agent', 'Inês Duarte', { reports_to_member_id: 'hana' }), // not placed in an office
		agent('quinn', 'Quinn Adeyemi')
	];
	const teams: Team[] = [
		team('eng', 'Engineering', undefined, 'sam'),
		team('web', 'Web', 'eng', 'noor'),
		team('platform', 'Platform', 'eng', 'kai'),
		team('design', 'Design', undefined, 'juno'),
		team('people', 'People', undefined, 'hana')
	];
	const memberships: Membership[] = [
		joined('eng', 'sam', 'Staff engineer'),
		joined('web', 'noor'),
		joined('web', 'mira'),
		joined('platform', 'kai'),
		joined('platform', 'mira'),
		joined('design', 'juno'),
		joined('design', 'pax'),
		joined('design', 'noor'),
		joined('people', 'hana'),
		joined('people', 'leo'),
		joined('people', 'ines')
	];
	const projects: ProjectSummary[] = [
		{ id: 'atlas', name: 'Atlas' },
		{ id: 'beacon', name: 'Beacon' },
		{ id: 'onboarding', name: 'Onboarding kit' },
		{ id: 'scratch', name: 'Scratchpad' }
	];
	const project_teams: ProjectTeam[] = [
		{ id: 'pt-1', project_id: 'atlas', team_id: 'platform', version: 1 },
		{ id: 'pt-2', project_id: 'beacon', team_id: 'web', version: 1 },
		{ id: 'pt-3', project_id: 'beacon', team_id: 'design', version: 1 },
		{ id: 'pt-4', project_id: 'onboarding', team_id: 'people', version: 1 }
	];
	return { position: { epoch: 1, seq: 1 }, teams, members, memberships, projects, project_teams };
}

/** An organization with an owner and nothing else. */
export const emptyOrg = (): OrgSnapshot => ({
	position: { epoch: 1, seq: 1 },
	teams: [],
	members: [{ id: 'you', kind: 'person', name: 'Alex Kim', owner: true, version: 1 }],
	memberships: []
});
