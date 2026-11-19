// Mock for src/lib/modules/teams/api.ts under `bun run dev:mock`. Teams, members and projects come from the org
// mock; the team number, purpose and member limit are JSON in e2e/fakes/data/teams. A team's counts are of the
// employees whose home team it is (TEAM-2). `?mock=slow|fails|empty` exercises the page states.
import type { TeamsApi } from '../../../src/lib/modules/teams/api';
import type { TeamDetail, TeamProjectRow, TeamRow } from '../../../src/lib/modules/teams/types';
import type { OrgSnapshot } from '$lib/proto';
import teamsData from '../data/teams/teams.json' with { type: 'json' };
import tasksData from '../data/members/tasks.json' with { type: 'json' };
import {
	activityFor,
	conversationsFor,
	fail,
	gate,
	homeTeamId,
	processesForTeam,
	rows as memberRows,
	scenario,
	snapshotFor,
	teamRef,
	workRows
} from './members';

const extras = teamsData as Record<string, { number: number; purpose?: string; maxMembers?: number }>;
const archived = new Set<string>();

/** The first team listed on a project is its accountable owner; the others contribute (TEAM-13). */
function projectRole(s: OrgSnapshot, projectId: string, teamId: string): 'owner' | 'contributor' {
	return (s.project_teams ?? []).find((p) => p.project_id === projectId)?.team_id === teamId ? 'owner' : 'contributor';
}

async function teamRows(organizationId: string): Promise<TeamRow[]> {
	const s = await snapshotFor(organizationId);
	const members = await memberRows(organizationId);
	return s.teams.map((t) => {
		const home = members.filter((m) => m.homeTeam?.id === t.id && m.status !== 'inactive');
		const lead = s.members.find((m) => m.id === t.lead_member_id);
		return {
			...teamRef(s, t.id)!,
			parentId: t.owner_team_id,
			lead: lead ? { id: lead.id, name: lead.name } : undefined,
			people: home.filter((m) => m.kind === 'person').length,
			agents: home.filter((m) => m.kind === 'agent').length,
			projects: (s.project_teams ?? [])
				.filter((p) => p.team_id === t.id)
				.flatMap((p) => {
					const project = s.projects?.find((x) => x.id === p.project_id && !x.archived);
					return project ? [{ id: project.id, name: project.name, role: projectRole(s, project.id, t.id) }] : [];
				}),
			maxMembers: extras[t.id]?.maxMembers ?? 6,
			status: archived.has(t.id) ? 'archived' : 'active'
		};
	});
}

async function found(organizationId: string, teamId: string) {
	const s = await snapshotFor(organizationId);
	if (!s.teams.some((t) => t.id === teamId)) throw fail('not_found', "That team isn't in this organization. It may have been deleted.");
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
		const path: TeamDetail['path'] = [];
		const seen = new Set<string>();
		for (let up = s.teams.find((t) => t.id === teamId)?.owner_team_id; up && !seen.has(up); up = s.teams.find((t) => t.id === up)?.owner_team_id) {
			seen.add(up);
			const ref = teamRef(s, up);
			if (ref) path.unshift(ref);
		}
		const members = (await memberRows(organizationId))
			.filter((m) => m.homeTeam?.id === teamId && m.status !== 'inactive')
			.map((m) => ({ id: m.id, name: m.name, kind: m.kind, role: m.role, handle: m.handle, presence: m.presence?.id, lead: m.id === team.lead?.id }))
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
				return [{ id: project.id, name: project.name, key: project.key, role, ownerTeam: role === 'contributor' ? teamRef(s, owner) : undefined, openWork: tasksData.filter((t) => t.projectId === project.id && t.state !== 'done').length }];
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
		return scenario() === 'empty' ? [] : conversationsFor((c) => c.teams.includes(teamId));
	},
	async processes(organizationId, teamId) {
		await gate();
		const s = await found(organizationId, teamId);
		return scenario() === 'empty' ? [] : processesForTeam(s, teamId);
	},
	async archive(organizationId, teamId) {
		await gate();
		const s = await found(organizationId, teamId);
		const children = s.teams.filter((t) => t.owner_team_id === teamId && !archived.has(t.id)).length;
		const home = s.members.filter((m) => homeTeamId(s, m.id) === teamId && m.agent?.status !== 'offboarded').length;
		const name = s.teams.find((t) => t.id === teamId)!.name;
		if (children || home) {
			const parts = [children && `${children} child ${children === 1 ? 'team' : 'teams'}`, home && `${home} home ${home === 1 ? 'employee' : 'employees'}`].filter(Boolean);
			throw fail('conflict', `${name} still has ${parts.join(' and ')}. Resolve them first.`);
		}
		archived.add(teamId);
	}
};
