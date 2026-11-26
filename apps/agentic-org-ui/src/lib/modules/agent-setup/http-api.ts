import { isBuiltInAgent, type AppError, type Member, type OrgSnapshot } from '$lib/proto';
import {
	unwrapGatewayResult,
	type GatewayHttpTransport,
	type GatewayReply
} from '$lib/platform/http/gateway-errors';
import { readJsonObject, readOfficeList, readResponse } from '$lib/platform/http/wire';
import { readOrganizationsList, type OrganizationView } from '$lib/platform/organizations/readers';
import type { OrgApi } from '$lib/modules/organization/api';
import { newStableId } from '$lib/modules/organization/commands';
import type { AgentSetupApi } from './api';
import type {
	AgentSetupDetail,
	AgentSetupList,
	AgentSetupRow,
	Blocker,
	ConnectionState,
	ConnectionView,
	ExecutionDetail,
	ExecutionEdit,
	ExecutionSummary,
	ModelCatalog,
	Placement,
	ProfileChoice,
	ProfileModel,
	Readiness,
	Ref,
	RoleEdit
} from './types';

type Rec = Record<string, unknown>;

interface WireConnection {
	id: string;
	name: string;
	runtimeDriver: string;
	enabled: boolean;
}

interface WireProfile {
	id: string;
	name: string;
	connectionIds: string[];
	archived: boolean;
}

interface WireExecution {
	modelProfileId: string;
	modelId: string;
	enabled: boolean;
	revision: number;
	toolsConfig: string[];
	permissionPreset?: string;
	systemPromptMarkdown: string;
}

interface WireReadiness {
	ready: boolean;
	status: string;
	candidates: {
		connectionId: string;
		eligible: boolean;
		blockerCode?: string;
		reason?: string;
		fix?: string;
	}[];
}

const asRec = (v: unknown): Rec | undefined =>
	v && typeof v === 'object' && !Array.isArray(v) ? (v as Rec) : undefined;
const asStr = (v: unknown): string | undefined => (typeof v === 'string' ? v : undefined);
const asNum = (v: unknown): number | undefined =>
	typeof v === 'number' && Number.isFinite(v) ? v : undefined;
const asStrList = (v: unknown): string[] =>
	Array.isArray(v) ? v.filter((x): x is string => typeof x === 'string') : [];

function runtimeLabel(driver: string | undefined): string {
	switch ((driver ?? '').toLowerCase()) {
		case 'claude':
		case 'claude_code':
		case 'anthropic':
			return 'Claude Code';
		case 'codex':
		case 'openai':
			return 'Codex';
		case 'agy':
		case 'antigravity':
		case 'google':
			return 'Antigravity';
		default:
			return driver ? driver : 'Runtime';
	}
}

function runtimeFromMember(member: Member): string[] {
	const r = member.agent?.runtime;
	if (!r) return [];
	if (typeof r === 'string') return r === 'unknown' ? [] : [runtimeLabel(r)];
	return r.other ? [r.other] : [];
}

function decodeConnections(raw: unknown): WireConnection[] {
	const rec = asRec(raw);
	const list = Array.isArray(rec?.connections) ? rec.connections : [];
	return list
		.map((item): WireConnection | undefined => {
			const r = asRec(item);
			const id = asStr(r?.id);
			if (!r || !id) return undefined;
			return {
				id,
				name: asStr(r.name) ?? asStr(r.key) ?? id,
				runtimeDriver: asStr(r.runtime_driver) ?? asStr(r.provider_type) ?? '',
				enabled: r.enabled !== false && r.archived !== true
			};
		})
		.filter((c): c is WireConnection => c !== undefined);
}

function decodeProfiles(raw: unknown): WireProfile[] {
	const rec = asRec(raw);
	const list = Array.isArray(rec?.profiles) ? rec.profiles : [];
	return list
		.map((item): WireProfile | undefined => {
			const r = asRec(item);
			const id = asStr(r?.id);
			if (!r || !id) return undefined;
			return {
				id,
				name: asStr(r.name) ?? asStr(r.key) ?? id,
				connectionIds: asStrList(r.connection_ids),
				archived: r.archived === true
			};
		})
		.filter((p): p is WireProfile => p !== undefined && !p.archived);
}

function decodeExecution(raw: unknown): WireExecution | null {
	const r = asRec(raw);
	const modelProfileId = asStr(r?.model_profile_id);
	const modelId = asStr(r?.model_id);
	if (!r || !modelProfileId || !modelId) return null;
	return {
		modelProfileId,
		modelId,
		enabled: r.enabled !== false,
		revision: asNum(r.revision) ?? 1,
		toolsConfig: asStrList(r.tools_config),
		permissionPreset: asStr(r.permission_preset),
		systemPromptMarkdown: asStr(r.system_prompt_markdown) ?? ''
	};
}

function decodeReadiness(raw: unknown): WireReadiness | undefined {
	const r = asRec(raw);
	if (!r) return undefined;
	const rawCands = Array.isArray(r.candidates) ? r.candidates : [];
	const candidates = rawCands
		.map((item) => {
			const c = asRec(item);
			const connectionId = asStr(c?.connection_id);
			if (!c || !connectionId) return undefined;
			return {
				connectionId,
				eligible: c.eligible === true,
				blockerCode: asStr(c.blocker_code) ?? asStr(c.code),
				reason: asStr(c.reason) ?? asStr(c.message),
				fix: asStr(c.fix)
			};
		})
		.filter((c): c is NonNullable<typeof c> => c !== undefined);
	return {
		ready: r.ready === true,
		status: asStr(r.status) ?? 'UNKNOWN',
		candidates
	};
}

function decodeProfileModels(raw: unknown): { models: ProfileModel[]; asOf?: string } {
	const r = asRec(raw);
	const list = Array.isArray(r?.models) ? r.models : [];
	let asOf: string | undefined;
	const models: ProfileModel[] = [];
	for (const item of list) {
		const m = asRec(item);
		const id = asStr(m?.model_id) ?? asStr(m?.id);
		if (!m || !id) continue;
		const itemAsOf = asStr(m.as_of);
		if (itemAsOf && (!asOf || itemAsOf > asOf)) asOf = itemAsOf;
		const supporting = Array.isArray(m.supporting_connections)
			? m.supporting_connections.length
			: (asNum(m.supporting) ?? 0);
		const ready = asNum(m.ready_connections) ?? asNum(m.ready) ?? 0;
		models.push({
			id,
			supporting,
			ready,
			stale: m.stale === true
		});
	}
	return { models, asOf };
}

function mapConnectionState(
	conn: WireConnection,
	cand: WireReadiness['candidates'][number] | undefined
): ConnectionState {
	if (!conn.enabled) return 'unknown';
	if (cand?.eligible) return 'ready';
	const code = (cand?.blockerCode ?? '').toUpperCase();
	if (code.includes('AUTH') || code.includes('SIGN')) return 'signed_out';
	if (code.includes('DRIVER') || code.includes('INSTALL')) return 'not_installed';
	if (code.includes('BINDING')) return 'binding_missing';
	if (code.includes('COOLDOWN') || code.includes('QUOTA')) return 'cooling_down';
	return 'unknown';
}

function mapReadiness(
	wire: WireReadiness | undefined,
	placement: Placement | null,
	checkedAt: string
): Readiness {
	if (!placement) return { known: false, why: 'This agent is not placed in an office yet.' };
	if (!wire) return { known: false, why: 'Readiness was not reported by the office.' };
	if (wire.ready) return { known: true, checkedAt, blockers: [] };
	if (wire.status === 'NO_EXECUTION_CONFIG') {
		return {
			known: true,
			checkedAt,
			blockers: [
				{
					code: 'NO_EXECUTION_CONFIG',
					reason: 'Execution is not configured for this agent yet.',
					fix: 'Choose a model profile and model below.'
				}
			]
		};
	}
	const blockers: Blocker[] = wire.candidates
		.filter((c) => !c.eligible)
		.map((c) => ({
			code: c.blockerCode ?? wire.status,
			reason: c.reason ?? `Connection ${c.connectionId} is not eligible (${wire.status}).`,
			...(c.fix ? { fix: c.fix } : {})
		}));
	if (blockers.length === 0) {
		blockers.push({
			code: wire.status,
			reason: `Office reported status: ${wire.status}.`
		});
	}
	return { known: true, checkedAt, blockers };
}

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
