import { toAppError, type AppError, type Member, type OrgSnapshot, type Team } from '$lib/proto';
import {
	unwrapGatewayResult,
	type GatewayHttpTransport,
	type GatewayReply
} from '$lib/platform/http/gateway-errors';
import { unavailableCall } from '$lib/platform/unavailable-api';
import type { OrgApi } from '$lib/modules/organization/api';
import { newStableId } from '$lib/modules/organization/commands';
import type { TeamRef } from '$lib/modules/members';
import { aggregateProjectWork } from '$lib/modules/projects/live-hydration';
import { openTasks } from '$lib/modules/projects/present';
import type { WorkItemsApi } from '$lib/modules/workitems/api';
import type { TaskRow } from '$lib/modules/workitems/types';
import type { TeamsApi } from './api';
import { buildTeamActivity, buildTeamWorkRows, readTeamConversations } from './live-hydration';
import type {
	NewTeamPerson,
	TeamDetail,
	TeamMember,
	TeamProject,
	TeamProjectRow,
	TeamRow
} from './types';

interface GatewayProjectSummary {
	id: string;
	key?: string;
	name: string;
	owner_team_id: string;
	contributing_team_ids: string[];
	archived?: boolean;
}

function teamRef(team: Team): TeamRef {
	return { id: team.id, name: team.name, ref: '' };
}

function isMemberActive(m: Member): boolean {
	return m.agent?.status !== 'offboarded';
}

function memberHandle(m: Member): string {
	return m.handle ?? (m.kind === 'agent' ? (m.agent?.messaging_name ?? m.id) : m.name);
}

function teamProjectsOf(teamId: string, projects: GatewayProjectSummary[]): TeamProject[] {
	const out: TeamProject[] = [];
	for (const p of projects) {
		if (p.archived) continue;
		if (p.owner_team_id === teamId) out.push({ id: p.id, name: p.name, role: 'owner' });
		else if (p.contributing_team_ids.includes(teamId)) {
			out.push({ id: p.id, name: p.name, role: 'contributor' });
		}
	}
	return out.sort((a, b) => a.name.localeCompare(b.name));
}

function buildTeamRow(
	team: Team,
	snapshot: OrgSnapshot,
	projects: GatewayProjectSummary[],
	membersById: ReadonlyMap<string, Member>
): TeamRow {
	const active = snapshot.members.filter((m) => isMemberActive(m) && m.home_team_id === team.id);
	const lead = team.lead_member_id ? membersById.get(team.lead_member_id) : undefined;
	return {
		...teamRef(team),
		...(team.owner_team_id ? { parentId: team.owner_team_id } : {}),
		...(lead && isMemberActive(lead) ? { lead: { id: lead.id, name: lead.name } } : {}),
		people: active.filter((m) => m.kind === 'person').length,
		agents: active.filter((m) => m.kind === 'agent').length,
		projects: teamProjectsOf(team.id, projects),
		maxMembers: 6,
		status: 'active'
	};
}

function buildPath(team: Team, byId: ReadonlyMap<string, Team>): TeamRef[] {
	const path: TeamRef[] = [];
	const seen = new Set<string>([team.id]);
	let current: Team | undefined = team.owner_team_id ? byId.get(team.owner_team_id) : undefined;
	while (current && !seen.has(current.id)) {
		seen.add(current.id);
		path.unshift(teamRef(current));
		current = current.owner_team_id ? byId.get(current.owner_team_id) : undefined;
	}
	return path;
}

function buildMembers(team: Team, snapshot: OrgSnapshot): TeamMember[] {
	return snapshot.members
		.filter((m) => isMemberActive(m) && m.home_team_id === team.id)
		.map((m) => ({
			id: m.id,
			name: m.name,
			kind: m.kind === 'agent' ? ('agent' as const) : ('person' as const),
			...(m.agent?.role ? { role: m.agent.role } : {}),
			handle: memberHandle(m),
			lead: m.id === team.lead_member_id
		}))
		.sort((a, b) => Number(b.lead) - Number(a.lead) || a.name.localeCompare(b.name));
}

function isProjectSummary(value: unknown): value is GatewayProjectSummary {
	if (!value || typeof value !== 'object') return false;
	const p = value as Partial<GatewayProjectSummary>;
	return (
		typeof p.id === 'string' &&
		typeof p.name === 'string' &&
		typeof p.owner_team_id === 'string' &&
		Array.isArray(p.contributing_team_ids)
	);
}

/**
 * Production HTTP adapter for the Teams index, team detail page, team deletion and Add person.
 * Reads live org snapshots and live project lists; never falls back to browser mocks.
 */
export function createHttpTeamsApi({
	http,
	orgApi,
	workItemsApi,
	newEntityId = newStableId
}: {
	http: GatewayHttpTransport;
	orgApi: Pick<OrgApi, 'view' | 'command'>;
	workItemsApi?: Pick<WorkItemsApi, 'queue'>;
	newEntityId?: () => string;
}): TeamsApi {
	async function safeWorkRows(organizationId: string): Promise<TaskRow[] | undefined> {
		if (!workItemsApi) return undefined;
		try {
			const queue = await workItemsApi.queue(organizationId, { kind: 'organization' });
			return queue.rows;
		} catch {
			return [];
		}
	}

	async function teamMemberIds(organizationId: string, teamId: string): Promise<Set<string>> {
		const { snapshot } = await orgApi.view(organizationId);
		return new Set(
			snapshot.members
				.filter((m) => isMemberActive(m) && m.home_team_id === teamId)
				.map((m) => m.id)
		);
	}
	async function readProjects(organizationId: string): Promise<GatewayProjectSummary[]> {
		let reply: GatewayReply;
		try {
			reply = await http.read(
				`/api/v1/organizations/${encodeURIComponent(organizationId)}/projects?include_archived=false`
			);
		} catch {
			throw {
				source: 'api',
				code: 'unavailable',
				message: 'Projects could not be reached.'
			} satisfies AppError;
		}
		const result = unwrapGatewayResult(reply, 'Projects could not be loaded.');
		return Array.isArray(result.projects) ? result.projects.filter(isProjectSummary) : [];
	}

	async function addPerson(
		organizationId: string,
		person: NewTeamPerson
	): Promise<{ memberId: string }> {
		const memberId = newEntityId();
		try {
			await orgApi.command(organizationId, {
				method: 'member.add_person',
				params: {
					member_id: memberId,
					name: person.name,
					handle: person.handle,
					home_team_id: person.homeTeamId,
					...(person.contact ? { contact: person.contact } : {})
				}
			});
		} catch (e) {
			const failure = toAppError(e);
			if (
				failure.code === 'already_exists' ||
				((failure.code === 'invalid_request' || failure.code === 'invalid_name') &&
					/handle/i.test(failure.message))
			) {
				throw { ...failure, field: 'handle' };
			}
			throw failure;
		}
		if (person.reportsToId) {
			const { snapshot } = await orgApi.view(organizationId);
			const added = snapshot.members.find((m) => m.id === memberId);
			if (added) {
				await orgApi.command(organizationId, {
					method: 'member.set_reports_to',
					params: {
						member_id: memberId,
						expected_version: added.version,
						reports_to_member_id: person.reportsToId
					}
				});
			}
		}
		return { memberId };
	}

	return Object.freeze({
		async list(organizationId: string): Promise<TeamRow[]> {
			const [{ snapshot }, projects] = await Promise.all([
				orgApi.view(organizationId),
				readProjects(organizationId)
			]);
			const membersById = new Map(snapshot.members.map((m) => [m.id, m]));
			return snapshot.teams.map((t) => buildTeamRow(t, snapshot, projects, membersById));
		},

		async team(organizationId: string, teamId: string): Promise<TeamDetail> {
			const [{ snapshot }, projects] = await Promise.all([
				orgApi.view(organizationId),
				readProjects(organizationId)
			]);
			const teamsById = new Map(snapshot.teams.map((t) => [t.id, t]));
			const team = teamsById.get(teamId);
			if (!team) {
				throw {
					source: 'api',
					code: 'not_found',
					message: 'That team could not be found.'
				} satisfies AppError;
			}
			const membersById = new Map(snapshot.members.map((m) => [m.id, m]));
			return {
				team: buildTeamRow(team, snapshot, projects, membersById),
				path: buildPath(team, teamsById),
				members: buildMembers(team, snapshot)
			};
		},

		async work(organizationId: string, teamId: string) {
			if (!workItemsApi) return unavailableCall();
			const [rows, memberIds] = await Promise.all([
				safeWorkRows(organizationId),
				teamMemberIds(organizationId, teamId)
			]);
			return buildTeamWorkRows(rows ?? [], teamId, memberIds);
		},

		async projects(organizationId: string, teamId: string): Promise<TeamProjectRow[]> {
			const [{ snapshot }, projects, workRows] = await Promise.all([
				orgApi.view(organizationId),
				readProjects(organizationId),
				safeWorkRows(organizationId)
			]);
			const workTotals = workRows ? aggregateProjectWork(workRows) : undefined;
			const teamsById = new Map(snapshot.teams.map((t) => [t.id, t]));
			const rows: TeamProjectRow[] = [];
			for (const p of projects) {
				if (p.archived) continue;
				const isOwner = p.owner_team_id === teamId;
				const isContributor = p.contributing_team_ids.includes(teamId);
				if (!isOwner && !isContributor) continue;
				const ownerTeam = teamsById.get(p.owner_team_id);
				const contributors = p.contributing_team_ids
					.filter((id) => id !== teamId && id !== p.owner_team_id)
					.map((id) => {
						const t = teamsById.get(id);
						return t ? teamRef(t) : { id, name: id, ref: '' };
					});
				const totals = workTotals?.get(p.id);
				rows.push({
					id: p.id,
					name: p.name,
					role: isOwner ? 'owner' : 'contributor',
					...(p.key ? { key: p.key } : {}),
					...(!isOwner && ownerTeam ? { ownerTeam: teamRef(ownerTeam) } : {}),
					...(contributors.length > 0 ? { contributors } : {}),
					...(totals
						? { openWork: openTasks(totals.tasks), tasks: totals.tasks, bugs: totals.bugs }
						: {})
				});
			}
			return rows.sort((a, b) => a.name.localeCompare(b.name));
		},

		async activity(organizationId: string, teamId: string) {
			if (!workItemsApi) return unavailableCall();
			const [rows, memberIds] = await Promise.all([
				safeWorkRows(organizationId),
				teamMemberIds(organizationId, teamId)
			]);
			return buildTeamActivity(rows ?? [], teamId, memberIds);
		},

		async conversations(organizationId: string, teamId: string) {
			const { snapshot } = await orgApi.view(organizationId);
			const team = snapshot.teams.find((t) => t.id === teamId);
			const memberIds = new Set(
				snapshot.members
					.filter((m) => isMemberActive(m) && m.home_team_id === teamId)
					.map((m) => m.id)
			);
			return readTeamConversations(http, organizationId, teamId, team?.name ?? '', memberIds);
		},
		archive: unavailableCall,

		async delete(organizationId: string, teamId: string): Promise<void> {
			const { snapshot } = await orgApi.view(organizationId);
			const team = snapshot.teams.find((t) => t.id === teamId);
			if (!team) {
				throw {
					source: 'api',
					code: 'not_found',
					message: 'That team could not be found.'
				} satisfies AppError;
			}
			await orgApi.command(organizationId, {
				method: 'team.delete',
				params: { team_id: team.id, expected_version: team.version }
			});
		},

		addPerson
	});
}
