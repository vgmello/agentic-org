// Applies one organization command to a snapshot with the contract's rules (crates/org-proto/src/org.rs):
// what HQ validates. The TEMPORARY mock office uses it so the UI can be built and tested before the
// real HQ answers; the real HQ is the authority. Pure: it returns a new snapshot, or throws the same
// error codes HQ would.
import type { ApiErrorCode, AppError, Member, Membership, OrgSnapshot, ProjectTeam, Team } from '$lib/proto';
import { validateName } from '$lib/name';
import type { OrgCommand } from './commands';

const fail = (code: ApiErrorCode, message: string): never => {
	throw { source: 'api', code, message } satisfies AppError;
};

const sameName = (a: string, b: string) => a.trim().toLowerCase() === b.trim().toLowerCase();

function checkedName(raw: string): string {
	const checked = validateName(raw);
	return checked.ok ? checked.name : fail('invalid_name', checked.message);
}

const owner = (s: OrgSnapshot) => s.members.find((m) => m.owner);

/**
 * Whether `name` is another member's handle (one namespace, ignoring case): a person's name, an agent's
 * name (the mock has no separate messaging name) or the owner's reserved `owner`. `exceptId` is the
 * member being renamed.
 */
const handleTaken = (s: OrgSnapshot, name: string, exceptId?: string) =>
	sameName(name, 'owner') || s.members.some((m) => m.id !== exceptId && !m.owner && sameName(m.name, name));

/** Every team below `teamId`, directly or through others. */
export function teamsBelow(s: OrgSnapshot, teamId: string): Set<string> {
	const below = new Set<string>();
	const walk = (id: string) => s.teams.filter((t) => t.owner_team_id === id).forEach((t) => !below.has(t.id) && (below.add(t.id), walk(t.id)));
	walk(teamId);
	return below;
}

/** Everyone who reports to `memberId`, directly or through others. */
export function reportsBelow(s: OrgSnapshot, memberId: string): Set<string> {
	const below = new Set<string>();
	const walk = (id: string) =>
		s.members.filter((m) => !m.owner && m.reports_to_member_id === id).forEach((m) => !below.has(m.id) && m.id !== memberId && (below.add(m.id), walk(m.id)));
	walk(memberId);
	return below;
}

function team(s: OrgSnapshot, id: string, version?: number): Team {
	const t = s.teams.find((x) => x.id === id) ?? fail('not_found', "That team isn't in the organization any more.");
	if (version !== undefined && t.version !== version) fail('conflict', `${t.name} changed since you looked.`);
	return t;
}
function member(s: OrgSnapshot, id: string, version?: number): Member {
	const m = s.members.find((x) => x.id === id) ?? fail('not_found', "That person isn't in the organization any more.");
	if (version !== undefined && m.version !== version) fail('conflict', `${m.name} changed since you looked.`);
	return m;
}
function membership(s: OrgSnapshot, id: string, version: number): Membership {
	const m = s.memberships.find((x) => x.id === id) ?? fail('not_found', "That membership isn't there any more.");
	if (m.version !== version) fail('conflict', 'That membership changed since you looked.');
	return m;
}

/** Whether a sibling of `parent` (a team with the same owner) already has this name, other than `exceptId`. */
const siblingTaken = (s: OrgSnapshot, parent: string | undefined, name: string, exceptId?: string) =>
	s.teams.some((t) => t.id !== exceptId && (t.owner_team_id ?? undefined) === parent && sameName(t.name, name));

const bump = <T extends { version: number }>(x: T): T => ({ ...x, version: x.version + 1 });
const replace = <T extends { id: string }>(xs: T[], next: T) => xs.map((x) => (x.id === next.id ? next : x));

/** Apply a command. Returns the next snapshot with its position advanced by one. */
export function applyCommand(s: OrgSnapshot, command: OrgCommand): OrgSnapshot {
	const next = apply(s, command);
	return { ...next, position: { epoch: s.position.epoch, seq: s.position.seq + 1 } };
}

function apply(s: OrgSnapshot, c: OrgCommand): OrgSnapshot {
	switch (c.method) {
		case 'team.create': {
			const p = c.params;
			if (s.teams.some((t) => t.id === p.team_id)) fail('already_exists', 'That team id is taken.');
			const name = checkedName(p.name);
			if (p.owner_team_id) team(s, p.owner_team_id);
			if (siblingTaken(s, p.owner_team_id, name)) fail('already_exists', `There's already a team called ${name} here.`);
			return { ...s, teams: [...s.teams, { id: p.team_id, name, version: 1, ...(p.owner_team_id ? { owner_team_id: p.owner_team_id } : {}) }] };
		}
		case 'team.rename': {
			const p = c.params;
			const t = team(s, p.team_id, p.expected_version);
			const name = checkedName(p.name);
			if (siblingTaken(s, t.owner_team_id, name, t.id)) fail('already_exists', `There's already a team called ${name} here.`);
			return { ...s, teams: replace(s.teams, bump({ ...t, name })) };
		}
		case 'team.move': {
			const p = c.params;
			const t = team(s, p.team_id, p.expected_version);
			if (p.owner_team_id) {
				team(s, p.owner_team_id);
				if (p.owner_team_id === t.id || teamsBelow(s, t.id).has(p.owner_team_id)) fail('cycle', `${t.name} can't go under itself or a team below it.`);
			}
			if (siblingTaken(s, p.owner_team_id, t.name, t.id)) fail('already_exists', `There's already a team called ${t.name} there.`);
			const moved = bump({ ...t, owner_team_id: p.owner_team_id });
			if (!p.owner_team_id) delete moved.owner_team_id;
			return { ...s, teams: replace(s.teams, moved) };
		}
		case 'team.set_lead': {
			const p = c.params;
			const t = team(s, p.team_id, p.expected_version);
			if (p.lead_member_id) {
				const m = member(s, p.lead_member_id);
				if (m.owner) fail('owner_protected', "The owner can't lead a team.");
				if (!s.memberships.some((x) => x.team_id === t.id && x.member_id === m.id)) fail('not_a_member', `${m.name} isn't in ${t.name}.`);
			}
			const led = bump({ ...t, lead_member_id: p.lead_member_id });
			if (!p.lead_member_id) delete led.lead_member_id;
			return { ...s, teams: replace(s.teams, led) };
		}
		case 'team.delete': {
			const p = c.params;
			const t = team(s, p.team_id, p.expected_version);
			if (s.teams.some((x) => x.owner_team_id === t.id)) fail('not_empty', `${t.name} still has teams under it.`);
			return {
				...s,
				teams: s.teams.filter((x) => x.id !== t.id),
				memberships: s.memberships.filter((m) => m.team_id !== t.id),
				project_teams: (s.project_teams ?? []).filter((pt) => pt.team_id !== t.id)
			};
		}
		case 'team.add_member': {
			const p = c.params;
			const t = team(s, p.team_id);
			const m = member(s, p.member_id);
			if (m.owner) fail('owner_protected', "The owner isn't on a team: they are in every team already.");
			if (s.memberships.some((x) => x.id === p.membership_id || (x.team_id === t.id && x.member_id === m.id)))
				fail('already_exists', `${m.name} is already in ${t.name}.`);
			const ms: Membership = { id: p.membership_id, team_id: t.id, member_id: m.id, version: 1, ...(p.title_in_team ? { title_in_team: p.title_in_team } : {}) };
			return { ...s, memberships: [...s.memberships, ms] };
		}
		case 'team.remove_member': {
			const p = c.params;
			const ms = membership(s, p.membership_id, p.expected_version);
			const teams = s.teams.map((t) => (t.id === ms.team_id && t.lead_member_id === ms.member_id ? bump({ ...t, lead_member_id: undefined }) : t));
			teams.forEach((t) => t.lead_member_id === undefined && delete t.lead_member_id);
			return { ...s, teams, memberships: s.memberships.filter((x) => x.id !== ms.id) };
		}
		case 'team.set_title': {
			const p = c.params;
			const ms = membership(s, p.membership_id, p.expected_version);
			const titled = bump({ ...ms, title_in_team: p.title_in_team });
			if (!p.title_in_team) delete titled.title_in_team;
			return { ...s, memberships: replace(s.memberships, titled) };
		}
		case 'member.add_person': {
			const p = c.params;
			if (s.members.some((m) => m.id === p.member_id)) fail('already_exists', 'That person id is taken.');
			const name = checkedName(p.name);
			if (handleTaken(s, name)) fail('already_exists', `${name} is already taken as a name here.`);
			const person: Member = { id: p.member_id, kind: 'person', name, version: 1, ...(p.contact ? { contact: p.contact } : {}) };
			return { ...s, members: [...s.members, person] };
		}
		case 'member.update_person': {
			const p = c.params;
			const m = member(s, p.member_id, p.expected_version);
			if (m.kind !== 'person') fail('invalid_request', 'Agents are edited through hiring.');
			if (p.contact !== undefined && p.clear_contact) fail('invalid_request', 'Give a contact or clear it, not both.');
			const renamed = p.name !== undefined ? checkedName(p.name) : undefined;
			if (renamed !== undefined && !m.owner && handleTaken(s, renamed, m.id)) fail('already_exists', `${renamed} is already taken as a name here.`);
			const updated: Member = { ...m, ...(renamed !== undefined ? { name: renamed } : {}), ...(p.contact !== undefined ? { contact: p.contact } : {}) };
			if (p.clear_contact) delete updated.contact;
			return { ...s, members: replace(s.members, bump(updated)) };
		}
		case 'member.remove': {
			const p = c.params;
			const m = member(s, p.member_id, p.expected_version);
			if (m.owner) fail('owner_protected', "The owner can't be removed.");
			// Whoever reported to them now reports to their own manager, or to the owner if they had none.
			const members = s.members
				.filter((x) => x.id !== m.id)
				.map((x) => {
					if (x.reports_to_member_id !== m.id) return x;
					const { reports_to_member_id: _gone, ...rest } = x;
					return bump(m.reports_to_member_id ? { ...rest, reports_to_member_id: m.reports_to_member_id } : rest);
				});
			const teams = s.teams.map((t) => (t.lead_member_id === m.id ? (({ lead_member_id: _gone, ...rest }) => bump(rest))(t) : t));
			return { ...s, members, teams, memberships: s.memberships.filter((x) => x.member_id !== m.id) };
		}
		case 'member.set_reports_to': {
			const p = c.params;
			const m = member(s, p.member_id, p.expected_version);
			if (m.owner) fail('owner_protected', 'The owner reports to no one.');
			const boss = p.reports_to_member_id ? member(s, p.reports_to_member_id) : undefined;
			if (boss && (boss.id === m.id || reportsBelow(s, m.id).has(boss.id))) fail('cycle', `${boss.name} reports to ${m.name}, directly or through others.`);
			const to = boss && !boss.owner ? boss.id : undefined; // the owner's own id is stored as empty
			const updated = bump({ ...m, reports_to_member_id: to });
			if (!to) delete updated.reports_to_member_id;
			return { ...s, members: replace(s.members, updated) };
		}
		case 'member.move': {
			const p = c.params;
			const old = membership(s, p.membership_id, p.expected_version);
			const to = team(s, p.to_team_id);
			if (s.memberships.some((x) => x.id === p.new_membership_id || (x.team_id === to.id && x.member_id === old.member_id)))
				fail('already_exists', `They are already in ${to.name}.`);
			const teams = s.teams.map((t) => (t.id === old.team_id && t.lead_member_id === old.member_id ? bump({ ...t, lead_member_id: undefined }) : t));
			teams.forEach((t) => t.lead_member_id === undefined && delete t.lead_member_id);
			const moved: Membership = { id: p.new_membership_id, team_id: to.id, member_id: old.member_id, version: 1, ...(p.title_in_team ? { title_in_team: p.title_in_team } : {}) };
			return { ...s, teams, memberships: [...s.memberships.filter((x) => x.id !== old.id), moved] };
		}
		case 'project.add_team': {
			const p = c.params;
			team(s, p.team_id);
			if (!(s.projects ?? []).some((x) => x.id === p.project_id)) fail('not_found', "That project isn't there any more.");
			if ((s.project_teams ?? []).some((x) => x.id === p.project_team_id || (x.project_id === p.project_id && x.team_id === p.team_id)))
				fail('already_exists', 'That team already works on this project.');
			const link: ProjectTeam = { id: p.project_team_id, project_id: p.project_id, team_id: p.team_id, version: 1 };
			return { ...s, project_teams: [...(s.project_teams ?? []), link] };
		}
		case 'project.remove_team': {
			const p = c.params;
			const link = (s.project_teams ?? []).find((x) => x.id === p.project_team_id) ?? fail('not_found', "That link isn't there any more.");
			if (link.version !== p.expected_version) fail('conflict', 'That link changed since you looked.');
			return { ...s, project_teams: (s.project_teams ?? []).filter((x) => x.id !== link.id) };
		}
	}
}

export { owner };
