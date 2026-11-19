// Mock for src/lib/modules/teams/api.ts under `bun run dev:mock`. Teams, members and projects come from the org
// mock; the team number, purpose and member limit are JSON in e2e/fakes/data/teams. A team's counts are of the
// employees whose home team it is (TEAM-2). `?mock=slow|fails|empty` exercises the page states; on a team's
// page `?mock=unavailable` answers as the production adapter does and `?mock=archived` shows the team archived.
// Archive and Delete are refused while teams, home employees, owned projects or open tasks remain (§7.10, TEAM-9);
// Add a person gives one home team and an explicit handle (PEOPLE-3/4). A team created here gets the next TEAM-n.
import type { TeamsApi } from '../../../src/lib/modules/teams/api';
import type { TeamDetail, TeamProjectRow, TeamRow } from '../../../src/lib/modules/teams/types';
import type { OrgSnapshot } from '$lib/proto';
import { orgApi } from '$lib/org/api';
import { newId } from '$lib/org/commands';
import { unavailableCall } from '$lib/platform/unavailable-api';
import teamsData from '../data/teams/teams.json' with { type: 'json' };
import tasksData from '../data/members/tasks.json' with { type: 'json' };
import {
	activityFor,
	conversationsFor,
	fail,
	gate,
	processesForTeam,
	rows as memberRows,
	scenario,
	snapshotFor,
	teamRef,
	workRows
} from './members';

const extras = teamsData as Record<
	string,
	{ number: number; purpose?: string; maxMembers?: number }
>;
const archived = new Set<string>();
/** Numbers for teams created while the mock runs: stable once given, after the fixture's. */
const numbers = new Map<string, number>();
/** Handles chosen in Add a person, until the members mock records them itself. */
const handles = new Map<string, string>();
const RESERVED = new Set(['owner', 'all', 'everyone', 'here', 'channel', 'hr', 'me']);

function numberOf(teamId: string): number {
	const fixed = extras[teamId]?.number;
	if (fixed) return fixed;
	let n = numbers.get(teamId);
	if (!n) {
		n = Math.max(0, ...Object.values(extras).map((e) => e.number), ...numbers.values()) + 1;
		numbers.set(teamId, n);
	}
	return n;
}
const refOf = (s: OrgSnapshot, teamId: string) => {
	const ref = teamRef(s, teamId);
	return ref && { ...ref, ref: `TEAM-${numberOf(teamId)}` };
};

/** The first team listed on a project is its accountable owner; the others contribute (TEAM-13). */
function projectRole(s: OrgSnapshot, projectId: string, teamId: string): 'owner' | 'contributor' {
	return (s.project_teams ?? []).find((p) => p.project_id === projectId)?.team_id === teamId
		? 'owner'
		: 'contributor';
}

async function teamRows(organizationId: string): Promise<TeamRow[]> {
	const s = await snapshotFor(organizationId);
	const members = await memberRows(organizationId);
	return s.teams.map((t) => {
		const home = members.filter((m) => m.homeTeam?.id === t.id && m.status !== 'inactive');
		const lead = s.members.find((m) => m.id === t.lead_member_id);
		return {
			...refOf(s, t.id)!,
			parentId: t.owner_team_id,
			lead: lead ? { id: lead.id, name: lead.name } : undefined,
			people: home.filter((m) => m.kind === 'person').length,
			agents: home.filter((m) => m.kind === 'agent').length,
			projects: (s.project_teams ?? [])
				.filter((p) => p.team_id === t.id)
				.flatMap((p) => {
					const project = s.projects?.find((x) => x.id === p.project_id && !x.archived);
					return project
						? [{ id: project.id, name: project.name, role: projectRole(s, project.id, t.id) }]
						: [];
				}),
			maxMembers: extras[t.id]?.maxMembers ?? 6,
			status: archived.has(t.id) ? 'archived' : 'active'
		};
	});
}

async function found(organizationId: string, teamId: string) {
	// Exactly what the production adapter answers, so the page shows its unavailable state.
	if (scenario() === 'unavailable') await unavailableCall();
	const s = await snapshotFor(organizationId);
	if (!s.teams.some((t) => t.id === teamId))
		throw fail('not_found', "That team isn't in this organization. It may have been deleted.");
	return s;
}

export const teamsApi: TeamsApi = {
	async list(organizationId) {
		await gate();
		return scenario() === 'empty' ? [] : teamRows(organizationId);
	},
	async team(organizationId, teamId) {
		await gate();
		const s = await found(organizationId, teamId);
		const team = (await teamRows(organizationId)).find((t) => t.id === teamId)!;
		if (scenario() === 'archived') team.status = 'archived';
		const path: TeamDetail['path'] = [];
		const seen = new Set<string>();
		for (
			let up = s.teams.find((t) => t.id === teamId)?.owner_team_id;
			up && !seen.has(up);
			up = s.teams.find((t) => t.id === up)?.owner_team_id
		) {
			seen.add(up);
			const ref = refOf(s, up);
			if (ref) path.unshift(ref);
		}
		const members = (await memberRows(organizationId))
			.filter((m) => m.homeTeam?.id === teamId && m.status !== 'inactive')
			.map((m) => ({
				id: m.id,
				name: m.name,
				kind: m.kind,
				role: m.role,
				handle: handles.get(m.id) ?? m.handle,
				presence: m.presence?.id,
				lead: m.id === team.lead?.id
			}))
			.sort((a, b) => Number(b.lead) - Number(a.lead) || a.name.localeCompare(b.name));
		return { team, path, purpose: extras[teamId]?.purpose, members };
	},
	async work(organizationId, teamId) {
		await gate();
		const s = await found(organizationId, teamId);
		return scenario() === 'empty' ? [] : workRows(s, (t) => t.teamId === teamId);
	},
	async projects(organizationId, teamId) {
		await gate();
		const s = await found(organizationId, teamId);
		if (scenario() === 'empty') return [];
		return (s.project_teams ?? [])
			.filter((p) => p.team_id === teamId)
			.flatMap((p): TeamProjectRow[] => {
				const project = s.projects?.find((x) => x.id === p.project_id && !x.archived);
				if (!project) return [];
				const role = projectRole(s, project.id, teamId);
				const owner = (s.project_teams ?? []).find((x) => x.project_id === project.id)?.team_id;
				return [
					{
						id: project.id,
						name: project.name,
						key: project.key,
						role,
						ownerTeam: role === 'contributor' ? refOf(s, owner!) : undefined,
						contributors: (s.project_teams ?? [])
							.filter(
								(x) => x.project_id === project.id && x.team_id !== owner && x.team_id !== teamId
							)
							.flatMap((x) => refOf(s, x.team_id) ?? []),
						openWork: tasksData.filter((t) => t.projectId === project.id && t.state !== 'done')
							.length
					}
				];
			});
	},
	async activity(organizationId, teamId) {
		await gate();
		await found(organizationId, teamId);
		return scenario() === 'empty' ? [] : activityFor((a) => a.teams.includes(teamId));
	},
	async conversations(organizationId, teamId) {
		await gate();
		await found(organizationId, teamId);
		// Only conversations Chat can open: a team's channel is Chat's `c-team-<id>`; threads and others are left out.
		return scenario() === 'empty'
			? []
			: conversationsFor((c) => c.kind === 'channel' && c.teams.includes(teamId)).map((c) => ({
					...c,
					id: `c-team-${teamId}`
				}));
	},
	async processes(organizationId, teamId) {
		await gate();
		const s = await found(organizationId, teamId);
		return scenario() === 'empty' ? [] : processesForTeam(s, teamId);
	},
	async archive(organizationId, teamId) {
		await gate();
		const s = await found(organizationId, teamId);
		refuseWhileBlocked(s, teamId, 'archive', await homeEmployees(organizationId, teamId));
		archived.add(teamId);
	},
	async delete(organizationId, teamId) {
		await gate();
		const s = await found(organizationId, teamId);
		refuseWhileBlocked(s, teamId, 'delete', await homeEmployees(organizationId, teamId));
		const team = s.teams.find((t) => t.id === teamId)!;
		await orgApi.command(organizationId, {
			method: 'team.delete',
			params: { team_id: teamId, expected_version: team.version }
		});
		archived.delete(teamId);
	},
	async addPerson(organizationId, person) {
		await gate();
		const s = await snapshotFor(organizationId);
		if (!s.teams.some((t) => t.id === person.homeTeamId))
			throw fail('invalid_request', 'Choose a home team that is still in the organization.');
		const handle = person.handle.trim();
		if (!/^[a-z0-9][a-z0-9._-]{0,31}$/.test(handle))
			throw handleRefusal(
				'invalid_request',
				`@${handle} isn't a handle: use lowercase letters, digits, dots, underscores and hyphens.`
			);
		const taken =
			RESERVED.has(handle) ||
			[...handles.values()].includes(handle) ||
			(await memberRows(organizationId)).some((m) => m.handle === handle);
		if (taken) {
			let n = 2;
			while ((await memberRows(organizationId)).some((m) => m.handle === `${handle}-${n}`)) n++;
			throw handleRefusal('already_exists', `@${handle} is taken. Try @${handle}-${n}.`);
		}
		const memberId = newId();
		await orgApi.command(organizationId, {
			method: 'member.add_person',
			params: {
				member_id: memberId,
				name: person.name.trim(),
				contact: person.contact || undefined
			}
		});
		handles.set(memberId, handle);
		await orgApi.command(organizationId, {
			method: 'team.add_member',
			params: { membership_id: newId(), team_id: person.homeTeamId, member_id: memberId }
		});
		if (person.reportsToId) {
			const me = (await snapshotFor(organizationId)).members.find((m) => m.id === memberId)!;
			await orgApi.command(organizationId, {
				method: 'member.set_reports_to',
				params: {
					member_id: memberId,
					expected_version: me.version,
					reports_to_member_id: person.reportsToId
				}
			});
		}
		return { memberId };
	}
};

async function homeEmployees(organizationId: string, teamId: string) {
	return (await memberRows(organizationId)).filter(
		(m) => m.homeTeam?.id === teamId && m.status !== 'inactive'
	).length;
}

/** HQ's own rule, independent of the page's check: what still points at the team (§7.10, TEAM-9). */
function refuseWhileBlocked(
	s: OrgSnapshot,
	teamId: string,
	removal: 'archive' | 'delete',
	home: number
) {
	const children = s.teams.filter(
		(t) => t.owner_team_id === teamId && (removal === 'delete' || !archived.has(t.id))
	).length;
	const owned = (s.project_teams ?? []).filter(
		(p) =>
			p.team_id === teamId &&
			projectRole(s, p.project_id, teamId) === 'owner' &&
			s.projects?.some((x) => x.id === p.project_id && !x.archived)
	).length;
	const open = tasksData.filter((t) => t.teamId === teamId && t.state !== 'done').length;
	const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;
	const parts = [
		children && plural(children, 'child team', 'child teams'),
		home && plural(home, 'home employee', 'home employees'),
		owned && plural(owned, 'project it owns', 'projects it owns'),
		open && plural(open, 'open task', 'open tasks')
	].filter((p): p is string => !!p);
	if (parts.length === 0) return;
	const name = s.teams.find((t) => t.id === teamId)!.name;
	const list =
		parts.length === 1 ? parts[0] : `${parts.slice(0, -1).join(', ')} and ${parts.at(-1)}`;
	throw fail('conflict', `${name} still has ${list}. Resolve them first.`);
}

/** A refusal about the handle, marked so the form shows it on the Handle field. */
const handleRefusal = (code: string, message: string) =>
	Object.assign(fail(code, message), { field: 'handle' as const });
