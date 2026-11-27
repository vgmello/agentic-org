import type { AppError, OrgSnapshot, Team } from '$lib/proto';
import {
	gatewayStatusCode,
	readGatewayAppError,
	unwrapGatewayResult,
	type GatewayHttpTransport,
	type GatewayReply
} from '$lib/platform/http/gateway-errors';
import { unavailableCall } from '$lib/platform/unavailable-api';
import type { OrgApi } from '$lib/modules/organization/api';
import { newStableId } from '$lib/modules/organization/commands';
import type { WorkItemsApi } from '$lib/modules/workitems/api';
import type { TaskRow } from '$lib/modules/workitems/types';
import {
	aggregateProjectWork,
	buildProjectActivity,
	readProjectConversations
} from './live-hydration';
import {
	defaultUuidV7,
	inferProjectField,
	isMemberActive,
	isProjectRecord,
	normalizeUuidV7,
	teamPath,
	teamRef,
	toProjectRow,
	toProjectTeamEntry,
	ZERO_FEATURES,
	type GatewayProjectRecord
} from './http-api-map';
import { OUTCOME_UNKNOWN } from './outcome';
import type { ProjectsApi } from './api';
import type {
	CreateOutcome,
	NewProject,
	ProjectDetail,
	ProjectMember,
	ProjectTeamView,
	ProjectsOverview,
	TeamChoice
} from './types';

export type { GatewayProjectRecord } from './http-api-map';

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
	workItemsApi,
	instanceId,
	newId = defaultUuidV7,
	newEntityId = newStableId
}: {
	http: GatewayHttpTransport;
	orgApi: Pick<OrgApi, 'view' | 'command'>;
	workItemsApi?: Pick<WorkItemsApi, 'queue'>;
	instanceId: () => string | undefined;
	newId?: () => string;
	newEntityId?: () => string;
}): ProjectsApi {
	const pendingCreates = new Map<string, PendingCreate>();

	async function safeWorkRows(organizationId: string, signal?: AbortSignal): Promise<TaskRow[]> {
		if (!workItemsApi) return [];
		try {
			const queue = await workItemsApi.queue(organizationId, { kind: 'organization' }, signal);
			return queue.rows;
		} catch (err) {
			if (signal?.aborted) throw err;
			return [];
		}
	}

	async function readProjects(
		organizationId: string,
		signal?: AbortSignal
	): Promise<GatewayProjectRecord[]> {
		let reply: GatewayReply;
		const path = `/api/v1/organizations/${encodeURIComponent(organizationId)}/projects?include_archived=true`;
		try {
			reply = await (signal ? http.read(path, signal) : http.read(path));
		} catch (err) {
			if (signal?.aborted) throw err;
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
		projectId: string,
		signal?: AbortSignal
	): Promise<GatewayProjectRecord> {
		let reply: GatewayReply;
		const path = `/api/v1/organizations/${encodeURIComponent(organizationId)}/projects/${encodeURIComponent(projectId)}`;
		try {
			reply = await (signal ? http.read(path, signal) : http.read(path));
		} catch (err) {
			if (signal?.aborted) throw err;
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
		async list(organizationId: string, signal?: AbortSignal): Promise<ProjectsOverview> {
			const [records, view, workRows] = await Promise.all([
				readProjects(organizationId, signal),
				orgApi.view(organizationId, signal),
				safeWorkRows(organizationId, signal)
			]);
			const teamsById = new Map(view.snapshot.teams.map((t) => [t.id, t]));
			const totals = aggregateProjectWork(workRows);
			return {
				projects: records.map((r) => toProjectRow(r, teamsById, totals.get(r.id))),
				viewer: { canCreate: true }
			};
		},

		async project(
			organizationId: string,
			projectId: string,
			signal?: AbortSignal
		): Promise<ProjectDetail> {
			const [record, view, workRows] = await Promise.all([
				readProject(organizationId, projectId, signal),
				orgApi.view(organizationId, signal),
				safeWorkRows(organizationId, signal)
			]);
			const teamsById = new Map(view.snapshot.teams.map((t) => [t.id, t]));
			const totals = aggregateProjectWork(workRows);
			return {
				...toProjectRow(record, teamsById, totals.get(record.id)),
				revision: record.revision,
				brief: null,
				features: { ...ZERO_FEATURES },
				releases: 0,
				processTabs: [],
				repositories: [],
				viewer: { canEdit: true }
			};
		},

		async team(
			organizationId: string,
			projectId: string,
			signal?: AbortSignal
		): Promise<ProjectTeamView> {
			const [record, view] = await Promise.all([
				readProject(organizationId, projectId, signal),
				orgApi.view(organizationId, signal)
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

		async activity(organizationId: string, projectId: string, signal?: AbortSignal) {
			const rows = await safeWorkRows(organizationId, signal);
			return buildProjectActivity(rows, projectId);
		},

		async conversations(organizationId: string, projectId: string) {
			let slug = '';
			try {
				const record = await readProject(organizationId, projectId);
				slug = record.slug;
			} catch {
				// Leave slug empty when project lookup fails so conversations can still match by id.
			}
			return readProjectConversations(http, organizationId, projectId, slug);
		},

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
		async ownerChangePreview(organizationId: string, projectId: string) {
			const rows = await safeWorkRows(organizationId);
			return { tasks: rows.filter((r) => r.project?.id === projectId).length };
		},
		setTeams,
		setRepositories: unavailableCall,
		processPreview: unavailableCall,
		setProcess: unavailableCall
	});
}
