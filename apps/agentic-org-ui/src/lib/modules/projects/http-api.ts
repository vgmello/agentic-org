import type { AppError, Member, OrgSnapshot, Team } from '$lib/proto';
import {
	gatewayStatusCode,
	readGatewayAppError,
	unwrapGatewayResult,
	type GatewayHttpTransport,
	type GatewayReply
} from '$lib/platform/http/gateway-errors';
import { formatUuidV7 } from '$lib/platform/http/clock';
import { readOperationId } from '$lib/platform/http/wire';
import { unavailableCall } from '$lib/platform/unavailable-api';
import type { OrgApi } from '$lib/modules/organization/api';
import { newStableId } from '$lib/modules/organization/commands';
import type { TeamRef } from '$lib/modules/members';
import { OUTCOME_UNKNOWN } from './outcome';
import type { ProjectsApi } from './api';
import type {
	CreateOutcome,
	NewProject,
	ProjectDetail,
	ProjectField,
	ProjectMember,
	ProjectRow,
	ProjectTeamEntry,
	ProjectTeamView,
	ProjectsOverview,
	TeamChoice
} from './types';

export interface GatewayProjectRecord {
	id: string;
	number: number;
	key: string;
	slug: string;
	name: string;
	owner_team_id: string;
	contributing_team_ids: string[];
	archived: boolean;
	revision: number;
}

const ZERO_TASKS = Object.freeze({ todo: 0, inProgress: 0, blocked: 0, done: 0, cancelled: 0 });
const ZERO_BUGS = Object.freeze({ critical: 0, major: 0, minor: 0, trivial: 0, unset: 0 });
const ZERO_FEATURES = Object.freeze({ planned: 0, inProgress: 0, live: 0 });

function teamRef(team: Team | undefined, fallbackId: string): TeamRef {
	if (!team) return { id: fallbackId, name: fallbackId, ref: '' };
	return { id: team.id, name: team.name, ref: '' };
}

function teamPath(team: Team, byId: ReadonlyMap<string, Team>): string {
	const names: string[] = [];
	const seen = new Set<string>();
	let current: Team | undefined = team;
	while (current && !seen.has(current.id)) {
		seen.add(current.id);
		names.unshift(current.name);
		current = current.owner_team_id ? byId.get(current.owner_team_id) : undefined;
	}
	return names.join(' › ');
}

function isProjectRecord(value: unknown): value is GatewayProjectRecord {
	if (!value || typeof value !== 'object') return false;
	const r = value as Partial<GatewayProjectRecord>;
	return (
		typeof r.id === 'string' &&
		typeof r.number === 'number' &&
		typeof r.slug === 'string' &&
		typeof r.name === 'string' &&
		typeof r.owner_team_id === 'string' &&
		Array.isArray(r.contributing_team_ids) &&
		typeof r.archived === 'boolean' &&
		typeof r.revision === 'number'
	);
}

function toProjectRow(
	record: GatewayProjectRecord,
	teamsById: ReadonlyMap<string, Team>
): ProjectRow {
	return {
		id: record.id,
		number: record.number,
		slug: record.slug,
		name: record.name,
		ownerTeam: teamRef(teamsById.get(record.owner_team_id), record.owner_team_id),
		contributors: record.contributing_team_ids.map((id) => teamRef(teamsById.get(id), id)),
		archived: record.archived,
		tasks: { ...ZERO_TASKS },
		bugs: { ...ZERO_BUGS },
		developmentProcess: null
	};
}

function isMemberActive(m: Member): boolean {
	return m.agent?.status !== 'offboarded';
}

function toProjectTeamEntry(
	teamId: string,
	teamsById: ReadonlyMap<string, Team>,
	membersById: ReadonlyMap<string, Member>
): ProjectTeamEntry {
	const t = teamsById.get(teamId);
	const lead = t?.lead_member_id ? membersById.get(t.lead_member_id) : undefined;
	return {
		...teamRef(t, teamId),
		...(lead && isMemberActive(lead) ? { lead: { id: lead.id, name: lead.name } } : {})
	};
}

function inferProjectField(error: AppError): ProjectField | undefined {
	const msg = error.message.toLowerCase();
	if (msg.includes('slug')) return 'slug';
	if (msg.includes('team')) return 'owner';
	if (error.code === 'invalid_name' || msg.includes('name')) return 'name';
	return undefined;
}

function defaultUuidV7(): string {
	return formatUuidV7(Date.now(), globalThis.crypto.getRandomValues(new Uint8Array(10)));
}

function normalizeUuidV7(candidate: string, fallback: () => string): string {
	try {
		return readOperationId(candidate);
	} catch {
		return readOperationId(fallback());
	}
}

interface PendingCreate {
	operationId: string;
	projectId: string;
	createTeamId?: string;
}

/**
 * Production HTTP adapter for `/api/v1/organizations/{org}/projects` and contributor team commands.
 * Never falls back to mock reducers or sample projects.
 */
export function createHttpProjectsApi({
	http,
	orgApi,
	instanceId,
	newId = defaultUuidV7,
	newEntityId = newStableId
}: {
	http: GatewayHttpTransport;
	orgApi: Pick<OrgApi, 'view' | 'command'>;
	instanceId: () => string | undefined;
	newId?: () => string;
	newEntityId?: () => string;
}): ProjectsApi {
	const pendingCreates = new Map<string, PendingCreate>();

	async function readProjects(organizationId: string): Promise<GatewayProjectRecord[]> {
		let reply: GatewayReply;
		try {
			reply = await http.read(
				`/api/v1/organizations/${encodeURIComponent(organizationId)}/projects?include_archived=true`
			);
		} catch {
			throw {
				source: 'api',
				code: 'unavailable',
				message: 'Projects could not be reached.'
			} satisfies AppError;
		}
		const result = unwrapGatewayResult(reply, 'Projects could not be loaded.');
		if (!Array.isArray(result.projects) || !result.projects.every(isProjectRecord)) {
			throw {
				source: 'api',
				code: 'unavailable',
				message: 'The projects list could not be read.'
			} satisfies AppError;
		}
		return result.projects;
	}

	async function readProject(
		organizationId: string,
		projectId: string
	): Promise<GatewayProjectRecord> {
		let reply: GatewayReply;
		try {
			reply = await http.read(
				`/api/v1/organizations/${encodeURIComponent(organizationId)}/projects/${encodeURIComponent(projectId)}`
			);
		} catch {
			throw {
				source: 'api',
				code: 'unavailable',
				message: 'The project could not be reached.'
			} satisfies AppError;
		}
		const result = unwrapGatewayResult(reply, 'The project could not be loaded.');
		if (!isProjectRecord(result)) {
			throw {
				source: 'api',
				code: 'unavailable',
				message: 'The project record could not be read.'
			} satisfies AppError;
		}
		return result;
	}

	function mapTeamMembers(
		record: GatewayProjectRecord,
		snapshot: OrgSnapshot,
		teamsById: ReadonlyMap<string, Team>
	): ProjectMember[] {
		const involved = new Set([record.owner_team_id, ...record.contributing_team_ids]);
		return snapshot.members
			.filter((m) => isMemberActive(m) && m.home_team_id && involved.has(m.home_team_id))
			.map((m) => ({
				id: m.id,
				name: m.name,
				kind: m.kind === 'agent' ? ('agent' as const) : ('person' as const),
				...(m.agent?.role ? { role: m.agent.role } : {}),
				homeTeam: teamRef(teamsById.get(m.home_team_id!), m.home_team_id!)
			}))
			.sort((a, b) => a.name.localeCompare(b.name));
	}

	function resolvePendingCreate(draft: NewProject): PendingCreate {
		const existing = pendingCreates.get(draft.operationId);
		if (existing) return existing;
		const created: PendingCreate = {
			operationId: normalizeUuidV7(draft.operationId, newId),
			projectId: newEntityId(),
			...(draft.owner.kind === 'new' ? { createTeamId: newEntityId() } : {})
		};
		pendingCreates.set(draft.operationId, created);
		return created;
	}

	function buildCreateParams(draft: NewProject, entry: PendingCreate): Record<string, unknown> {
		const base: Record<string, unknown> = {
			project_id: entry.projectId,
			name: draft.name,
			...(draft.slug ? { slug: draft.slug } : {})
		};
		if (draft.owner.kind === 'existing') {
			return { ...base, owner_team_id: draft.owner.teamId };
		}
		entry.createTeamId ??= newEntityId();
		return {
			...base,
			create_team_id: entry.createTeamId,
			...(draft.owner.parentTeamId ? { parent_team_id: draft.owner.parentTeamId } : {})
		};
	}

	function assertCreateCompleted(reply: GatewayReply, draftOperationId: string): void {
		const root =
			reply.body && typeof reply.body === 'object'
				? (reply.body as Record<string, unknown>)
				: undefined;
		if (reply.status === 504 || root?.outcome === 'unknown') {
			throw {
				source: 'api',
				code: OUTCOME_UNKNOWN,
				message: 'The outcome of this request is not confirmed yet.'
			};
		}
		pendingCreates.delete(draftOperationId);
		if (reply.status === 200 && root?.outcome === 'completed') return;
		const err = readGatewayAppError(
			reply.body,
			gatewayStatusCode(reply.status),
			'The project could not be created.'
		);
		const field = inferProjectField(err);
		throw field ? { ...err, field } : err;
	}

	async function create(
		organizationId: string,
		draft: NewProject
	): Promise<{ projectId: string; number: number }> {
		const instance = instanceId();
		if (!instance) {
			throw {
				source: 'api',
				code: 'not_authorized',
				message: 'Sign in to continue.'
			} satisfies AppError;
		}
		const entry = resolvePendingCreate(draft);
		let reply: GatewayReply;
		try {
			reply = await http.submit(
				`/api/v1/organizations/${encodeURIComponent(organizationId)}/projects`,
				{
					operation_id: entry.operationId,
					scope: { kind: 'organization', instance_id: instance, org_id: organizationId },
					params: buildCreateParams(draft, entry)
				}
			);
		} catch {
			throw {
				source: 'api',
				code: OUTCOME_UNKNOWN,
				message: 'The outcome of this request is not confirmed yet.'
			};
		}
		assertCreateCompleted(reply, draft.operationId);
		const created = await readProject(organizationId, entry.projectId);
		return { projectId: created.id, number: created.number };
	}

	async function createOperation(
		organizationId: string,
		operationId: string
	): Promise<CreateOutcome> {
		const instance = instanceId();
		if (!instance) {
			throw {
				source: 'api',
				code: 'not_authorized',
				message: 'Sign in to continue.'
			} satisfies AppError;
		}
		const entry = pendingCreates.get(operationId);
		const wireOpId = entry?.operationId ?? operationId;
		let reply: GatewayReply;
		try {
			reply = await http.read(
				`/api/v1/operations/org/${encodeURIComponent(wireOpId)}?instance_id=${encodeURIComponent(instance)}&org_id=${encodeURIComponent(organizationId)}`
			);
		} catch {
			return { state: 'pending' };
		}
		if (reply.status === 404) return { state: 'pending' };
		const result = unwrapGatewayResult(reply, 'Could not check the create request.');
		const outcome = result.outcome;
		if (outcome === 'completed' && entry) {
			pendingCreates.delete(operationId);
			const record = await readProject(organizationId, entry.projectId);
			return { state: 'applied', projectId: record.id, number: record.number };
		}
		if (outcome === 'rejected') {
			pendingCreates.delete(operationId);
			const err = result.error as { message?: unknown } | undefined;
			return {
				state: 'not_applied',
				reason: typeof err?.message === 'string' ? err.message : 'The request was refused.'
			};
		}
		return { state: 'pending' };
	}

	async function setTeams(
		organizationId: string,
		projectId: string,
		change: { ownerTeamId: string; contributorTeamIds: string[]; expectedRevision: number }
	): Promise<void> {
		const current = await readProject(organizationId, projectId);
		if (current.revision !== change.expectedRevision) {
			throw {
				source: 'api',
				code: 'conflict',
				message: 'The project changed while you were editing it.'
			} satisfies AppError;
		}
		if (change.ownerTeamId !== current.owner_team_id) {
			throw {
				source: 'api',
				code: 'unsupported',
				message: 'Changing the accountable team is not available over the browser gateway yet.'
			} satisfies AppError;
		}
		const before = new Set(current.contributing_team_ids);
		const desired = new Set(change.contributorTeamIds.filter((id) => id !== current.owner_team_id));
		for (const teamId of desired) {
			if (!before.has(teamId)) {
				await orgApi.command(organizationId, {
					method: 'project.add_team',
					params: { project_team_id: newEntityId(), project_id: projectId, team_id: teamId }
				});
			}
		}
		for (const teamId of before) {
			if (!desired.has(teamId)) {
				const latest = await readProject(organizationId, projectId);
				await orgApi.command(organizationId, {
					method: 'project.remove_team',
					params: {
						project_team_id: `${projectId}:${teamId}`,
						expected_version: latest.revision
					}
				});
			}
		}
	}

	return Object.freeze({
		async list(organizationId: string): Promise<ProjectsOverview> {
			const [records, view] = await Promise.all([
				readProjects(organizationId),
				orgApi.view(organizationId)
			]);
			const teamsById = new Map(view.snapshot.teams.map((t) => [t.id, t]));
			return {
				projects: records.map((r) => toProjectRow(r, teamsById)),
				viewer: { canCreate: true }
			};
		},

		async project(organizationId: string, projectId: string): Promise<ProjectDetail> {
			const [record, view] = await Promise.all([
				readProject(organizationId, projectId),
				orgApi.view(organizationId)
			]);
			const teamsById = new Map(view.snapshot.teams.map((t) => [t.id, t]));
			return {
				...toProjectRow(record, teamsById),
				revision: record.revision,
				brief: null,
				features: { ...ZERO_FEATURES },
				releases: 0,
				processTabs: [],
				repositories: [],
				viewer: { canEdit: true }
			};
		},

		async team(organizationId: string, projectId: string): Promise<ProjectTeamView> {
			const [record, view] = await Promise.all([
				readProject(organizationId, projectId),
				orgApi.view(organizationId)
			]);
			const teamsById = new Map(view.snapshot.teams.map((t) => [t.id, t]));
			const membersById = new Map(view.snapshot.members.map((m) => [m.id, m]));
			return {
				owner: toProjectTeamEntry(record.owner_team_id, teamsById, membersById),
				contributors: record.contributing_team_ids.map((id) =>
					toProjectTeamEntry(id, teamsById, membersById)
				),
				members: mapTeamMembers(record, view.snapshot, teamsById)
			};
		},

		activity: async () => [],
		conversations: async () => [],

		async teamChoices(organizationId: string): Promise<TeamChoice[]> {
			const { snapshot } = await orgApi.view(organizationId);
			const active = snapshot.teams;
			const byId = new Map(active.map((t) => [t.id, t]));
			const roots = active.filter((t) => !t.owner_team_id);
			const orgRootId = roots.length === 1 ? roots[0].id : undefined;
			return active
				.map((t) => ({
					...teamRef(t, t.id),
					path: teamPath(t, byId),
					organizationTeam: t.id === orgRootId
				}))
				.sort((a, b) => a.path.localeCompare(b.path));
		},

		repositoryChoices: async () => [],
		create,
		createOperation,
		rename: unavailableCall,
		ownerChangePreview: async () => ({ tasks: 0 }),
		setTeams,
		setRepositories: unavailableCall,
		processPreview: unavailableCall,
		setProcess: unavailableCall
	});
}
