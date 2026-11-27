import type { OrgApi } from '$lib/modules/organization/api';
import { newStableId } from '$lib/modules/organization/commands';
import {
	unwrapGatewayResult,
	type GatewayHttpTransport,
	type GatewayReply
} from '$lib/platform/http/gateway-errors';
import { readJsonObject, readOfficeList, readResponse } from '$lib/platform/http/wire';
import { readOrganizationsList, type OrganizationView } from '$lib/platform/organizations/readers';
import type { AppError, OrgSnapshot } from '$lib/proto';
import type { EmployeesApi } from './api';
import type {
	ExecutionProfile,
	HireAccess,
	HireChoices,
	HireOffice,
	HirePreview,
	HireProposal,
	HireRequest,
	HireState,
	HireStep,
	HireTeam,
	HrLeadStatus,
	RuntimeCheck
} from './types';

import { asNum, asRec, asStr, asStrList, type Rec } from '$lib/platform/http/decode';

const RESERVED_HANDLES = ['hr', 'system', 'office', 'admin', 'owner', 'all', 'here', 'channel'];

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
			return driver || 'Runtime';
	}
}

function normalizeHireState(raw: unknown): HireState {
	switch (asStr(raw)?.toLowerCase()) {
		case 'draft':
		case 'drafting':
		case 'proposed':
			return 'drafting';
		case 'hiring':
		case 'provisioning':
		case 'confirmed':
			return 'hiring';
		case 'hired':
		case 'completed':
		case 'active':
			return 'hired';
		case 'withdrawn':
		case 'cancelled':
			return 'withdrawn';
		case 'failed':
			return 'failed';
		default:
			return 'drafting';
	}
}

function normalizeStep(raw: unknown): HireStep | undefined {
	const s = asStr(raw)?.toLowerCase();
	if (s === 'record' || s === 'home' || s === 'messaging') return s;
	return undefined;
}

function doneStepsFor(state: HireState, failedStep?: HireStep): HireStep[] {
	if (state === 'hired') return ['record', 'home', 'messaging'];
	if (state === 'failed') {
		if (failedStep === 'messaging') return ['record', 'home'];
		if (failedStep === 'home') return ['record'];
		return [];
	}
	if (state === 'hiring') return ['record'];
	return [];
}

function decodeHireProposal(
	propRec: Rec | undefined,
	r: Rec,
	homeTeamId: string
): Partial<HireProposal> | undefined {
	if (!propRec) return undefined;
	const entries: [keyof HireProposal, string | undefined][] = [
		['name', asStr(propRec.name)],
		['handle', asStr(propRec.handle)],
		['roleId', asStr(r.role_id)],
		['roleText', asStr(propRec.role_text)],
		['title', asStr(propRec.title)],
		['focus', asStr(propRec.focus)],
		['homeTeamId', homeTeamId || undefined],
		['reportsToId', asStr(propRec.reports_to_employee_id)],
		['executionProfileId', asStr(propRec.model_profile_id)],
		['model', asStr(propRec.model_id)],
		['officeId', asStr(propRec.office_id)]
	];
	const out: Partial<HireProposal> = {};
	for (const [key, val] of entries) {
		if (val) out[key] = val;
	}
	return out;
}

function decodeHireRequest(
	raw: unknown,
	snapshot: OrgSnapshot,
	viewerMemberId?: string
): (HireRequest & { revision: number }) | undefined {
	const r = asRec(raw);
	const id = asStr(r?.id);
	if (!r || !id) return undefined;
	const state = normalizeHireState(r.state);
	const homeTeamId = asStr(r.home_team_id) ?? '';
	const homeTeam = snapshot.teams.find((t) => t.id === homeTeamId);
	const requesterId = asStr(r.requested_by_employee_id) ?? viewerMemberId ?? 'unknown';
	const requester = snapshot.members.find((m) => m.id === requesterId);
	const failedStep = normalizeStep(r.failed_step);
	const proposal = decodeHireProposal(asRec(r.proposal), r, homeTeamId);
	const role = proposal?.title ?? proposal?.roleText;
	const closedReason = asStr(r.closed_reason);
	const employeeId = asStr(r.employee_id);
	const createdAt = asStr(r.created_at) ?? new Date(0).toISOString();

	return {
		id,
		revision: asNum(r.revision) ?? 1,
		state,
		...(proposal?.name ? { name: proposal.name } : {}),
		...(role ? { role } : {}),
		requestedBy: { id: requesterId, name: requester?.name ?? requesterId },
		byViewer: Boolean(viewerMemberId && requesterId === viewerMemberId),
		homeTeam: { id: homeTeamId, name: homeTeam?.name ?? homeTeamId, ref: '' },
		createdAt,
		updatedAt: asStr(r.updated_at) ?? createdAt,
		reason: asStr(r.reason) ?? '',
		...(proposal ? { proposal } : {}),
		doneSteps: doneStepsFor(state, failedStep),
		...(failedStep ? { failedStep } : {}),
		...(closedReason ? { closedReason } : {}),
		...(employeeId ? { employeeId } : {})
	};
}

function buildProposalWire(proposal: HireProposal) {
	return {
		name: proposal.name.trim(),
		handle: proposal.handle.trim().replace(/^@/, ''),
		...(proposal.title?.trim() ? { title: proposal.title.trim() } : {}),
		...(proposal.focus?.trim() ? { focus: proposal.focus.trim() } : {}),
		...(proposal.roleText?.trim() ? { role_text: proposal.roleText.trim() } : {}),
		...(proposal.reportsToId ? { reports_to_employee_id: proposal.reportsToId } : {}),
		office_id: proposal.officeId,
		model_profile_id: proposal.executionProfileId,
		model_id: proposal.model,
		permission_preset: 'autonomous_sandboxed',
		tools_config: [],
		system_prompt_markdown: `You are ${proposal.name.trim()} (@${proposal.handle.trim().replace(/^@/, '')}).`
	};
}

/**
 * Production HTTP adapter for HR Hiring (`/teams/hire` and `/teams/hires`) backed by
 * `/api/v1/organizations/{org}/{hires,roles,model-profiles,provider-connections,offices,snapshot}`.
 */
export function createHttpEmployeesApi(deps: {
	http: GatewayHttpTransport;
	orgApi: Pick<OrgApi, 'view'>;
	session: () => { accountAdmin: boolean } | undefined;
}): EmployeesApi {
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

	async function readHrStatus(
		organizationId: string,
		snapshot: OrgSnapshot
	): Promise<HrLeadStatus> {
		const hr = snapshot.members.find(
			(m) =>
				m.kind === 'agent' &&
				(m.handle?.toLowerCase() === 'hr' || m.agent?.messaging_name?.toLowerCase() === 'hr')
		);
		if (!hr) return { state: 'not_running' };
		try {
			const reply = await deps.http.read(
				`/api/v1/organizations/${encodeURIComponent(organizationId)}/agents/${encodeURIComponent(hr.id)}/readiness`
			);
			if (reply.status !== 200) return { state: 'not_running' };
			const res = unwrapGatewayResult(reply, 'HR readiness failed.');
			if (res.ready === true) return { state: 'can_run' };
			const status = asStr(res.status);
			if (status === 'NO_EXECUTION_CONFIG') {
				return {
					state: 'cant_start',
					reason:
						'HR lead has no execution configuration yet. Configure it in Agent setup or fill in the proposal by hand.'
				};
			}
			return {
				state: 'cant_start',
				reason: status ? `HR lead is not ready (${status}).` : 'HR lead is not ready.'
			};
		} catch {
			return { state: 'not_running' };
		}
	}

	async function readRawHires(organizationId: string): Promise<unknown[]> {
		const reply = await deps.http
			.read(`/api/v1/organizations/${encodeURIComponent(organizationId)}/hires`)
			.catch(() => undefined);
		if (!reply) {
			throw {
				source: 'api',
				code: 'unavailable',
				message: 'Hire requests could not be reached.'
			} satisfies AppError;
		}
		const result = unwrapGatewayResult(reply, 'Hire requests could not be loaded.');
		return Array.isArray(result.hires) ? result.hires : [];
	}

	function decodeConnectionsMap(
		connReply: GatewayReply | undefined
	): Map<string, { driver: string; enabled: boolean }> {
		const connections = new Map<string, { driver: string; enabled: boolean }>();
		if (!connReply || connReply.status !== 200) return connections;
		const res = unwrapGatewayResult(connReply, 'Provider connections failed.');
		for (const item of Array.isArray(res.connections) ? res.connections : []) {
			const r = asRec(item);
			const id = asStr(r?.id);
			if (!r || !id) continue;
			connections.set(id, {
				driver: runtimeLabel(asStr(r.runtime_driver) ?? asStr(r.provider_type)),
				enabled: r.enabled !== false && r.archived !== true
			});
		}
		return connections;
	}

	function decodeRolesList(roleReply: GatewayReply | undefined): { id: string; name: string }[] {
		if (!roleReply || roleReply.status !== 200) return [];
		const res = unwrapGatewayResult(roleReply, 'Roles failed.');
		const roles: { id: string; name: string }[] = [];
		for (const item of Array.isArray(res.roles) ? res.roles : []) {
			const r = asRec(item);
			const id = asStr(r?.id);
			const name = asStr(r?.name);
			if (id && name) roles.push({ id, name });
		}
		return roles.sort((a, b) => a.name.localeCompare(b.name));
	}

	async function readProfileModelIds(organizationId: string, pid: string): Promise<string[]> {
		try {
			const mReply = await deps.http.read(
				`/api/v1/organizations/${encodeURIComponent(organizationId)}/model-profiles/${encodeURIComponent(pid)}/models`
			);
			if (mReply.status !== 200) return [];
			const mRes = unwrapGatewayResult(mReply, 'Profile models failed.');
			return (Array.isArray(mRes.models) ? mRes.models : [])
				.map((m) => asStr(asRec(m)?.model_id))
				.filter((id): id is string => Boolean(id));
		} catch {
			return [];
		}
	}

	async function buildProfileChoice(
		organizationId: string,
		p: Rec,
		connections: ReadonlyMap<string, { driver: string; enabled: boolean }>,
		offices: readonly { id: string; status: string }[]
	): Promise<ExecutionProfile> {
		const pid = asStr(p.id)!;
		const firstConn = asStrList(p.connection_ids)
			.map((id) => connections.get(id))
			.find(Boolean);
		const models = await readProfileModelIds(organizationId, pid);
		const readiness: Record<string, RuntimeCheck> = {};
		for (const office of offices) {
			readiness[office.id] = office.status === 'active' && firstConn?.enabled ? 'ready' : 'unknown';
		}
		return {
			id: pid,
			name: asStr(p.name) ?? pid,
			driver: firstConn?.driver ?? 'Runtime',
			models: models.length > 0 ? models : ['sonnet', 'opus'],
			readiness
		};
	}

	async function loadChoices(organizationId: string): Promise<HireChoices> {
		const [{ snapshot }, offices, connReply, profReply, roleReply] = await Promise.all([
			deps.orgApi.view(organizationId),
			readOffices(organizationId),
			deps.http
				.read(`/api/v1/organizations/${encodeURIComponent(organizationId)}/provider-connections`)
				.catch(() => undefined),
			deps.http
				.read(`/api/v1/organizations/${encodeURIComponent(organizationId)}/model-profiles`)
				.catch(() => undefined),
			deps.http
				.read(`/api/v1/organizations/${encodeURIComponent(organizationId)}/roles`)
				.catch(() => undefined)
		]);

		const connections = decodeConnectionsMap(connReply);
		const rawProfiles =
			profReply && profReply.status === 200
				? ((unwrapGatewayResult(profReply, 'Model profiles failed.').profiles as unknown[]) ?? [])
				: [];

		const activeProfiles = rawProfiles
			.map(asRec)
			.filter((p): p is Rec => Boolean(p && asStr(p.id) && p.archived !== true));
		const profiles = await Promise.all(
			activeProfiles.map((p) => buildProfileChoice(organizationId, p, connections, offices))
		);

		const roles = decodeRolesList(roleReply);
		const hireOffices: HireOffice[] = offices.map((o) => ({
			id: o.id,
			name: o.name,
			instance: o.host
		}));
		const people = snapshot.members
			.filter((m) => m.agent?.status !== 'offboarded')
			.map((m) => ({ id: m.id, name: m.name }))
			.sort((a, b) => a.name.localeCompare(b.name));
		const takenHandles = [
			...new Set([
				...RESERVED_HANDLES,
				...snapshot.members
					.map((m) => (m.handle ?? m.agent?.messaging_name)?.replace(/^@/, '').toLowerCase())
					.filter((h): h is string => Boolean(h))
			])
		];

		return { profiles, offices: hireOffices, roles, people, takenHandles };
	}

	async function loadHireRequest(
		organizationId: string,
		requestId: string
	): Promise<HireRequest & { revision: number }> {
		const [{ snapshot }, org, reply] = await Promise.all([
			deps.orgApi.view(organizationId),
			readOrgView(organizationId),
			deps.http
				.read(
					`/api/v1/organizations/${encodeURIComponent(organizationId)}/hires/${encodeURIComponent(requestId)}`
				)
				.catch(() => undefined)
		]);
		if (!reply) {
			throw {
				source: 'api',
				code: 'unavailable',
				message: 'Hire request could not be reached.'
			} satisfies AppError;
		}
		const result = unwrapGatewayResult(reply, 'Hire request could not be loaded.');
		const decoded = decodeHireRequest(result, snapshot, org?.employee_id);
		if (!decoded) {
			throw {
				source: 'api',
				code: 'not_found',
				message: 'Hire request not found.'
			} satisfies AppError;
		}
		return decoded;
	}

	return Object.freeze<EmployeesApi>({
		async hireAccess(organizationId: string): Promise<HireAccess> {
			const [{ snapshot }, org] = await Promise.all([
				deps.orgApi.view(organizationId),
				readOrgView(organizationId)
			]);
			const viewer = org?.employee_id
				? snapshot.members.find((m) => m.id === org.employee_id)
				: undefined;
			const seesAll = Boolean(deps.session()?.accountAdmin || viewer?.owner);
			const teams: HireTeam[] = snapshot.teams
				.map((t) => ({
					id: t.id,
					name: t.name,
					ref: '',
					...(t.owner_team_id ? { parentId: t.owner_team_id } : {}),
					members: snapshot.members.filter(
						(m) => m.home_team_id === t.id && m.agent?.status !== 'offboarded'
					).length,
					maxMembers: 6
				}))
				.sort((a, b) => a.name.localeCompare(b.name));
			const hr = await readHrStatus(organizationId, snapshot);
			return {
				teams: seesAll ? teams : teams.filter((t) => t.id === viewer?.home_team_id),
				seesAll,
				hr
			};
		},

		hireChoices: loadChoices,

		async hireRequests(organizationId: string): Promise<HireRequest[]> {
			const [{ snapshot }, org, rawHires] = await Promise.all([
				deps.orgApi.view(organizationId),
				readOrgView(organizationId),
				readRawHires(organizationId)
			]);
			return rawHires
				.map((item) => decodeHireRequest(item, snapshot, org?.employee_id))
				.filter((r): r is HireRequest & { revision: number } => r !== undefined)
				.sort((a, b) => b.updatedAt.localeCompare(a.updatedAt));
		},

		hireRequest: loadHireRequest,

		async draft(
			organizationId: string,
			input: { reason: string; homeTeamId: string; requestId?: string }
		): Promise<HireRequest> {
			const choices = await loadChoices(organizationId);
			const requestId = input.requestId ?? newStableId();
			const defaultOffice = choices.offices[0]?.id;
			const defaultProfile = choices.profiles[0];
			let reply: GatewayReply;
			try {
				reply = await deps.http.submit(
					`/api/v1/organizations/${encodeURIComponent(organizationId)}/hires`,
					{
						command_id: newStableId(),
						request_id: requestId,
						home_team_id: input.homeTeamId,
						reason: input.reason.trim(),
						proposal: {
							name: 'New agent',
							handle: `agent-${requestId.slice(-6)}`,
							...(defaultOffice ? { office_id: defaultOffice } : {}),
							...(defaultProfile
								? { model_profile_id: defaultProfile.id, model_id: defaultProfile.models[0] }
								: {})
						}
					}
				);
			} catch {
				throw {
					source: 'api',
					code: 'unavailable',
					message: 'Hire draft could not be submitted.'
				} satisfies AppError;
			}
			unwrapGatewayResult(reply, 'Hire draft was refused.');
			return loadHireRequest(organizationId, requestId);
		},

		async preview(organizationId: string, proposal: HireProposal): Promise<HirePreview> {
			const choices = await loadChoices(organizationId);
			const office = choices.offices.find((o) => o.id === proposal.officeId) ?? {
				id: proposal.officeId,
				name: proposal.officeId,
				instance: 'Office'
			};
			const profile = choices.profiles.find((p) => p.id === proposal.executionProfileId);
			const handle = proposal.handle.trim().replace(/^@/, '') || 'agent';
			return {
				homeFolder: `agents/${handle}`,
				office,
				driver: profile?.driver ?? 'Runtime',
				runtime: profile?.readiness[office.id] ?? 'unknown'
			};
		},

		async confirm(
			organizationId: string,
			input: { requestId?: string; reason: string; proposal: HireProposal }
		): Promise<HireRequest> {
			const employeeId = newStableId();
			if (input.requestId) {
				const existing = await loadHireRequest(organizationId, input.requestId);
				const reply = await deps.http.submit(
					`/api/v1/organizations/${encodeURIComponent(organizationId)}/hires/confirm`,
					{
						command_id: newStableId(),
						request_id: input.requestId,
						employee_id: existing.employeeId ?? employeeId,
						expected_version: existing.revision
					}
				);
				unwrapGatewayResult(reply, 'Hire confirmation was refused.');
				return loadHireRequest(organizationId, input.requestId);
			}

			const requestId = newStableId();
			const reply = await deps.http.submit(
				`/api/v1/organizations/${encodeURIComponent(organizationId)}/hires/direct`,
				{
					command_id: newStableId(),
					request_id: requestId,
					employee_id: employeeId,
					home_team_id: input.proposal.homeTeamId,
					...(input.proposal.roleId ? { role_id: input.proposal.roleId } : {}),
					proposal: buildProposalWire(input.proposal),
					reason: input.reason.trim()
				}
			);
			unwrapGatewayResult(reply, 'Hire confirmation was refused.');
			return loadHireRequest(organizationId, requestId);
		},

		async withdraw(organizationId: string, requestId: string): Promise<void> {
			const existing = await loadHireRequest(organizationId, requestId);
			const reply = await deps.http.submit(
				`/api/v1/organizations/${encodeURIComponent(organizationId)}/hires/withdraw`,
				{
					command_id: newStableId(),
					request_id: requestId,
					expected_version: existing.revision,
					closed_reason: 'Withdrawn by hirer'
				}
			);
			unwrapGatewayResult(reply, 'Hire withdrawal was refused.');
		},

		async retry(organizationId: string, requestId: string): Promise<void> {
			const existing = await loadHireRequest(organizationId, requestId);
			const reply = await deps.http.submit(
				`/api/v1/organizations/${encodeURIComponent(organizationId)}/hires/confirm`,
				{
					command_id: newStableId(),
					request_id: requestId,
					employee_id: existing.employeeId ?? newStableId(),
					expected_version: existing.revision
				}
			);
			unwrapGatewayResult(reply, 'Hire retry was refused.');
		}
	});
}
