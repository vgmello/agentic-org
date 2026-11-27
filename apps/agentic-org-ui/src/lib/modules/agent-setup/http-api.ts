import { isBuiltInAgent, type AppError, type Member, type OrgSnapshot } from '$lib/proto';
import {
	unwrapGatewayResult,
	type GatewayHttpTransport,
	type GatewayReply
} from '$lib/platform/http/gateway-errors';
import { asRec, asStr } from '$lib/platform/http/decode';
import { readJsonObject, readOfficeList, readResponse } from '$lib/platform/http/wire';
import { readOrganizationsList, type OrganizationView } from '$lib/platform/organizations/readers';
import type { OrgApi } from '$lib/modules/organization/api';
import { newStableId } from '$lib/modules/organization/commands';
import type { AgentSetupApi } from './api';
import {
	decodeConnections,
	decodeExecution,
	decodeProfileModels,
	decodeProfiles,
	decodeReadiness,
	mapConnectionState,
	mapReadiness,
	runtimeFromMember,
	runtimeLabel,
	type WireConnection,
	type WireExecution,
	type WireProfile,
	type WireReadiness
} from './http-wire';
import type {
	AgentSetupDetail,
	AgentSetupList,
	AgentSetupRow,
	ConnectionView,
	ExecutionDetail,
	ExecutionEdit,
	ExecutionSummary,
	ModelCatalog,
	Placement,
	ProfileChoice,
	ProfileModel,
	Ref,
	RoleEdit
} from './types';

/**
 * Production HTTP adapter for Organization settings › Agent setup backed by `/snapshot`, `/offices`,
 * `/provider-connections`, `/model-profiles`, `/roles`, `/agents/{id}/{execution,readiness}`,
 * `POST /agents/execution`, and `POST /agents/update`.
 */
export function createHttpAgentSetupApi(deps: {
	http: GatewayHttpTransport;
	orgApi: Pick<OrgApi, 'view'>;
	session: () => { accountAdmin: boolean } | undefined;
	now?: () => string;
}): AgentSetupApi {
	const now = deps.now ?? (() => new Date().toISOString());

	async function readOrgView(organizationId: string): Promise<OrganizationView | undefined> {
		try {
			const reply = await deps.http.read('/api/v1/organizations');
			if (reply.status !== 200) return undefined;
			const env = readResponse(reply.body, readJsonObject);
			if ('error' in env) return undefined;
			return readOrganizationsList(env.result).organizations.find((o) => o.id === organizationId);
		} catch {
			return undefined;
		}
	}

	async function readOffices(organizationId: string) {
		try {
			const reply = await deps.http.read(
				`/api/v1/organizations/${encodeURIComponent(organizationId)}/offices`
			);
			if (reply.status !== 200) return [];
			const env = readResponse(reply.body, readOfficeList);
			return 'result' in env ? env.result.offices : [];
		} catch {
			return [];
		}
	}

	async function readConnections(organizationId: string): Promise<WireConnection[]> {
		try {
			const reply = await deps.http.read(
				`/api/v1/organizations/${encodeURIComponent(organizationId)}/provider-connections`
			);
			if (reply.status !== 200) return [];
			return decodeConnections(unwrapGatewayResult(reply, 'Provider connections failed.'));
		} catch {
			return [];
		}
	}

	async function readProfiles(organizationId: string): Promise<WireProfile[]> {
		try {
			const reply = await deps.http.read(
				`/api/v1/organizations/${encodeURIComponent(organizationId)}/model-profiles`
			);
			if (reply.status !== 200) return [];
			return decodeProfiles(unwrapGatewayResult(reply, 'Model profiles failed.'));
		} catch {
			return [];
		}
	}

	async function readRoles(organizationId: string): Promise<Ref[]> {
		try {
			const reply = await deps.http.read(
				`/api/v1/organizations/${encodeURIComponent(organizationId)}/roles`
			);
			if (reply.status !== 200) return [];
			const res = unwrapGatewayResult(reply, 'Roles failed.');
			const list = Array.isArray(res.roles) ? res.roles : [];
			return list
				.map((item): Ref | undefined => {
					const r = asRec(item);
					const id = asStr(r?.id);
					const name = asStr(r?.name);
					return id && name ? { id, name } : undefined;
				})
				.filter((r): r is Ref => r !== undefined)
				.sort((a, b) => a.name.localeCompare(b.name));
		} catch {
			return [];
		}
	}

	async function readExecution(
		organizationId: string,
		memberId: string
	): Promise<WireExecution | null> {
		try {
			const reply = await deps.http.read(
				`/api/v1/organizations/${encodeURIComponent(organizationId)}/agents/${encodeURIComponent(memberId)}/execution`
			);
			if (reply.status !== 200) return null;
			return decodeExecution(unwrapGatewayResult(reply, 'Execution config failed.'));
		} catch {
			return null;
		}
	}

	async function readReadiness(
		organizationId: string,
		memberId: string
	): Promise<WireReadiness | undefined> {
		try {
			const reply = await deps.http.read(
				`/api/v1/organizations/${encodeURIComponent(organizationId)}/agents/${encodeURIComponent(memberId)}/readiness`
			);
			if (reply.status !== 200) return undefined;
			return decodeReadiness(unwrapGatewayResult(reply, 'Readiness failed.'));
		} catch {
			return undefined;
		}
	}

	async function readProfileModels(
		organizationId: string,
		profileId: string
	): Promise<{ models: ProfileModel[]; asOf?: string }> {
		try {
			const reply = await deps.http.read(
				`/api/v1/organizations/${encodeURIComponent(organizationId)}/model-profiles/${encodeURIComponent(profileId)}/models`
			);
			if (reply.status !== 200) return { models: [] };
			return decodeProfileModels(unwrapGatewayResult(reply, 'Profile models failed.'));
		} catch {
			return { models: [] };
		}
	}

	function canViewerChange(snapshot: OrgSnapshot, org: OrganizationView | undefined): boolean {
		if (deps.session()?.accountAdmin) return true;
		const viewer = org?.employee_id
			? snapshot.members.find((m) => m.id === org.employee_id)
			: undefined;
		return Boolean(viewer?.owner);
	}

	function profileRuntimes(
		profile: WireProfile | undefined,
		connById: ReadonlyMap<string, WireConnection>,
		member: Member
	): string[] {
		if (profile) {
			const names = profile.connectionIds
				.map((id) => connById.get(id))
				.filter((c): c is WireConnection => c !== undefined)
				.map((c) => runtimeLabel(c.runtimeDriver));
			const unique = [...new Set(names)];
			if (unique.length > 0) return unique;
		}
		return runtimeFromMember(member);
	}

	function buildPlacement(
		member: Member,
		officeById: ReadonlyMap<string, { id: string; name: string }>
	): Placement | null {
		const office = member.office_id ? officeById.get(member.office_id) : undefined;
		return office ? { office: { id: office.id, name: office.name }, generation: 1 } : null;
	}

	function buildAgentIdentity(
		member: Member,
		teamById: ReadonlyMap<string, { id: string; name: string }>,
		roleById: ReadonlyMap<string, { id: string; name: string }>
	) {
		const homeTeam = member.home_team_id ? teamById.get(member.home_team_id) : undefined;
		const roleRef = member.role_id ? roleById.get(member.role_id) : undefined;
		return {
			memberId: member.id,
			name: member.name,
			handle: (member.handle ?? member.agent?.messaging_name ?? member.name).replace(/^@/, ''),
			...(roleRef ? { role: roleRef.name, roleId: roleRef.id } : {}),
			...(member.agent?.role ? { roleText: member.agent.role } : {}),
			builtIn: isBuiltInAgent(member),
			status: (member.agent?.status === 'paused' ? 'paused' : 'active') as 'paused' | 'active',
			...(homeTeam ? { homeTeam: { id: homeTeam.id, name: homeTeam.name } } : {})
		};
	}

	function buildExecutionDetail(
		exec: WireExecution | null,
		activeProfile: WireProfile | undefined,
		connById: ReadonlyMap<string, WireConnection>,
		member: Member,
		readinessWire: WireReadiness | null | undefined,
		checkedAt: string
	): ExecutionDetail | null {
		if (!exec) return null;
		const candByConnId = new Map((readinessWire?.candidates ?? []).map((c) => [c.connectionId, c]));
		const connections = (activeProfile?.connectionIds ?? [])
			.map((cid): ConnectionView | undefined => {
				const conn = connById.get(cid);
				if (!conn) return undefined;
				return {
					id: conn.id,
					name: conn.name,
					runtime: runtimeLabel(conn.runtimeDriver),
					state: mapConnectionState(conn, candByConnId.get(conn.id)),
					checkedAt
				};
			})
			.filter((c): c is ConnectionView => c !== undefined);
		return {
			profile: { id: exec.modelProfileId, name: activeProfile?.name ?? exec.modelProfileId },
			modelId: exec.modelId,
			enabled: exec.enabled,
			runtimes: profileRuntimes(activeProfile, connById, member),
			revision: exec.revision,
			connections,
			tools: exec.toolsConfig,
			hasPrompt: exec.systemPromptMarkdown.trim().length > 0
		};
	}

	async function loadAgentDetail(
		organizationId: string,
		memberId: string
	): Promise<AgentSetupDetail> {
		const [{ snapshot }, org, offices, connections, profiles, roles, exec, readinessWire] =
			await Promise.all([
				deps.orgApi.view(organizationId),
				readOrgView(organizationId),
				readOffices(organizationId),
				readConnections(organizationId),
				readProfiles(organizationId),
				readRoles(organizationId),
				readExecution(organizationId, memberId),
				readReadiness(organizationId, memberId)
			]);

		const member = snapshot.members.find((m) => m.id === memberId && m.kind === 'agent');
		if (!member || member.agent?.status === 'offboarded') {
			throw { source: 'api', code: 'not_found', message: 'Agent not found.' } satisfies AppError;
		}

		const checkedAt = now();
		const connById = new Map(connections.map((c) => [c.id, c]));
		const profileById = new Map(profiles.map((p) => [p.id, p]));
		const teamById = new Map(snapshot.teams.map((t) => [t.id, t]));
		const officeById = new Map(offices.map((o) => [o.id, o]));
		const roleById = new Map(roles.map((r) => [r.id, r]));

		const profileModelsList = await Promise.all(
			profiles.map(async (p) => ({
				profile: p,
				...(await readProfileModels(organizationId, p.id))
			}))
		);
		const profileChoices: ProfileChoice[] = profileModelsList.map(({ profile, models }) => ({
			id: profile.id,
			name: profile.name,
			runtimes: profileRuntimes(profile, connById, member),
			models: models.length > 0 ? models : null
		}));

		const activeProfileModels = exec
			? profileModelsList.find((x) => x.profile.id === exec.modelProfileId)
			: undefined;
		const catalog: ModelCatalog =
			activeProfileModels && activeProfileModels.models.length > 0
				? {
						reported: true,
						models: activeProfileModels.models.map((m) => m.id),
						asOf: activeProfileModels.asOf ?? checkedAt
					}
				: { reported: false };

		const placement = buildPlacement(member, officeById);
		const activeProfile = exec ? profileById.get(exec.modelProfileId) : undefined;
		const execution = buildExecutionDetail(
			exec,
			activeProfile,
			connById,
			member,
			readinessWire,
			checkedAt
		);
		const reportsToMember = member.reports_to_member_id
			? snapshot.members.find((m) => m.id === member.reports_to_member_id)
			: undefined;

		return {
			...buildAgentIdentity(member, teamById, roleById),
			execution,
			placement,
			readiness: mapReadiness(readinessWire, placement, checkedAt),
			catalog,
			employeeRevision: member.version,
			...(reportsToMember
				? { reportsTo: { id: reportsToMember.id, name: reportsToMember.name } }
				: {}),
			...(member.agent?.home ? { managedHome: member.agent.home } : {}),
			holds: [],
			stopPending: false,
			profiles: profileChoices,
			roles,
			viewer: { canChange: canViewerChange(snapshot, org) }
		};
	}

	async function buildAgentRow(
		organizationId: string,
		member: Member,
		maps: {
			connById: ReadonlyMap<string, WireConnection>;
			profileById: ReadonlyMap<string, WireProfile>;
			teamById: ReadonlyMap<string, { id: string; name: string }>;
			officeById: ReadonlyMap<string, { id: string; name: string }>;
			roleById: ReadonlyMap<string, { id: string; name: string }>;
			modelCatalogByProfile: ReadonlyMap<string, ModelCatalog>;
		},
		checkedAt: string
	): Promise<AgentSetupRow> {
		const readinessWire = await readReadiness(organizationId, member.id);
		const exec =
			readinessWire?.status === 'NO_EXECUTION_CONFIG'
				? null
				: await readExecution(organizationId, member.id);
		const placement = buildPlacement(member, maps.officeById);
		const profile = exec ? maps.profileById.get(exec.modelProfileId) : undefined;
		const execution: ExecutionSummary | null = exec
			? {
					profile: { id: exec.modelProfileId, name: profile?.name ?? exec.modelProfileId },
					modelId: exec.modelId,
					enabled: exec.enabled,
					runtimes: profileRuntimes(profile, maps.connById, member)
				}
			: null;
		const identity = buildAgentIdentity(member, maps.teamById, maps.roleById);
		const { roleId: _unusedRoleId, ...rowIdentity } = identity;
		void _unusedRoleId;
		return {
			...rowIdentity,
			execution,
			placement,
			readiness: mapReadiness(readinessWire, placement, checkedAt),
			catalog: exec
				? (maps.modelCatalogByProfile.get(exec.modelProfileId) ?? { reported: false })
				: { reported: false }
		};
	}

	return Object.freeze<AgentSetupApi>({
		async list(organizationId: string): Promise<AgentSetupList> {
			const [{ snapshot }, org, offices, connections, profiles, roles] = await Promise.all([
				deps.orgApi.view(organizationId),
				readOrgView(organizationId),
				readOffices(organizationId),
				readConnections(organizationId),
				readProfiles(organizationId),
				readRoles(organizationId)
			]);

			const checkedAt = now();
			const modelCatalogByProfile = new Map<string, ModelCatalog>();
			await Promise.all(
				profiles.map(async (p) => {
					const { models, asOf } = await readProfileModels(organizationId, p.id);
					modelCatalogByProfile.set(
						p.id,
						models.length > 0
							? { reported: true, models: models.map((m) => m.id), asOf: asOf ?? checkedAt }
							: { reported: false }
					);
				})
			);

			const maps = {
				connById: new Map(connections.map((c) => [c.id, c])),
				profileById: new Map(profiles.map((p) => [p.id, p])),
				teamById: new Map(snapshot.teams.map((t) => [t.id, t])),
				officeById: new Map(offices.map((o) => [o.id, o])),
				roleById: new Map(roles.map((r) => [r.id, r])),
				modelCatalogByProfile
			};
			const activeAgents = snapshot.members.filter(
				(m) => m.kind === 'agent' && m.agent?.status !== 'offboarded'
			);
			const rows = await Promise.all(
				activeAgents.map((member) => buildAgentRow(organizationId, member, maps, checkedAt))
			);
			rows.sort((a, b) => a.name.localeCompare(b.name));
			return {
				agents: rows,
				viewer: { canChange: canViewerChange(snapshot, org) }
			};
		},

		agent: loadAgentDetail,

		async saveExecution(
			organizationId: string,
			memberId: string,
			edit: ExecutionEdit
		): Promise<AgentSetupDetail> {
			const existing = await readExecution(organizationId, memberId);
			let reply: GatewayReply;
			try {
				reply = await deps.http.submit(
					`/api/v1/organizations/${encodeURIComponent(organizationId)}/agents/execution`,
					{
						command_id: newStableId(),
						employee_id: memberId,
						...(existing ? { expected_version: edit.expectedRevision } : {}),
						model_profile_id: edit.profileId,
						model_id: edit.modelId,
						...(existing?.effort ? { effort: existing.effort } : {}),
						tools_config: existing?.toolsConfig ?? [],
						...(existing?.permissionPreset ? { permission_preset: existing.permissionPreset } : {}),
						enabled: existing?.enabled ?? true,
						system_prompt_markdown: existing?.systemPromptMarkdown ?? ''
					}
				);
			} catch {
				throw {
					source: 'api',
					code: 'unavailable',
					message: 'Agent execution could not be saved.'
				} satisfies AppError;
			}
			unwrapGatewayResult(reply, 'Agent execution update was refused.');
			return loadAgentDetail(organizationId, memberId);
		},

		async setRole(
			organizationId: string,
			memberId: string,
			edit: RoleEdit
		): Promise<AgentSetupDetail> {
			let reply: GatewayReply;
			try {
				reply = await deps.http.submit(
					`/api/v1/organizations/${encodeURIComponent(organizationId)}/agents/update`,
					{
						command_id: newStableId(),
						member_id: memberId,
						expected_version: edit.expectedRevision,
						...(edit.roleId ? { role_id: edit.roleId } : { clear_role_id: true })
					}
				);
			} catch {
				throw {
					source: 'api',
					code: 'unavailable',
					message: 'Agent role could not be updated.'
				} satisfies AppError;
			}
			unwrapGatewayResult(reply, 'Agent role update was refused.');
			return loadAgentDetail(organizationId, memberId);
		}
	});
}
