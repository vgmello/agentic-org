import type { AppError, Member, OrgSnapshot, Team } from '$lib/proto';
import {
	gatewayStatusCode,
	readGatewayAppError,
	unwrapGatewayResult,
	type GatewayHttpTransport,
	type GatewayReply
} from '$lib/platform/http/gateway-errors';
import { readJsonObject, readOfficeList, readResponse } from '$lib/platform/http/wire';
import { readOrganizationsList, type OrganizationView } from '$lib/platform/organizations/readers';
import { unavailableCall } from '$lib/platform/unavailable-api';
import type { OrgApi } from '$lib/modules/organization/api';
import { newStableId } from '$lib/modules/organization/commands';
import { readRoleRecordWithId } from '$lib/modules/roles/wire';
import type { WorkItemsApi } from '$lib/modules/workitems/api';
import type { TaskRow } from '$lib/modules/workitems/types';
import type { MembersApi } from './api';
import {
	buildAssignableTasks,
	buildMemberActivity,
	buildMemberWorkRows,
	pickCurrentTask,
	readMemberConversations
} from './live-hydration';
import type {
	EmployeeStatus,
	MemberProfile,
	MemberRow,
	Placement,
	RoleView,
	RunNowResult,
	SignInLink,
	TeamRef,
	Turn,
	TurnState
} from './types';

export interface GatewayAccessEntry {
	member_id: string;
	account_id?: string | null;
	login?: string | null;
	admin: boolean;
	owner: boolean;
	pending_invite: boolean;
	pending_password_reset: boolean;
}

export interface MembersSessionInfo {
	accountAdmin: boolean;
}

function teamRef(team: Team | undefined): TeamRef | undefined {
	if (!team) return undefined;
	return { id: team.id, name: team.name, ref: '' };
}

function memberHandle(m: Member): string {
	return m.handle ?? (m.kind === 'agent' ? (m.agent?.messaging_name ?? m.id) : m.name);
}

function memberStatus(m: Member): EmployeeStatus {
	if (m.kind === 'agent' && m.agent?.status === 'offboarded') return 'inactive';
	if (m.kind === 'agent' && m.agent?.status === 'paused') return 'paused';
	return 'active';
}

function agentPlacement(
	m: Member,
	org: OrganizationView | undefined,
	officeNamesById?: ReadonlyMap<string, string>
): Placement | null | undefined {
	if (m.kind !== 'agent') return undefined;
	if (!m.office_id) return null;
	const name =
		officeNamesById?.get(m.office_id) ??
		org?.offices.find((o) => o.id === m.office_id)?.name ??
		m.office_id;
	return { id: m.office_id, name };
}

function parseLinkResult(reply: GatewayReply, fallback: string): SignInLink {
	const result = unwrapGatewayResult(reply, fallback);
	if (typeof result.url !== 'string' || typeof result.expires_at_ms !== 'number') {
		throw { source: 'api', code: 'unavailable', message: fallback } satisfies AppError;
	}
	return { url: result.url, expiresAt: new Date(result.expires_at_ms).toISOString() };
}

function mapMemberRow(
	m: Member,
	snapshot: OrgSnapshot,
	teamsById: ReadonlyMap<string, Team>,
	membersById: ReadonlyMap<string, Member>,
	accessById: ReadonlyMap<string, GatewayAccessEntry> | undefined,
	org: OrganizationView | undefined,
	officeNamesById?: ReadonlyMap<string, string>
): MemberRow {
	const status = memberStatus(m);
	const homeTeam = m.home_team_id ? teamRef(teamsById.get(m.home_team_id)) : undefined;
	const manager = m.reports_to_member_id ? membersById.get(m.reports_to_member_id) : undefined;
	const access = accessById?.get(m.id);
	const canSignIn =
		status !== 'inactive' &&
		m.kind === 'person' &&
		(access ? Boolean(access.account_id) : Boolean(m.owner && org?.employee_id === m.id));
	const directReports = snapshot.members.filter(
		(other) => memberStatus(other) !== 'inactive' && other.reports_to_member_id === m.id
	).length;
	return {
		id: m.id,
		kind: m.kind === 'agent' ? 'agent' : 'person',
		name: m.name,
		handle: memberHandle(m),
		...(m.agent?.role ? { role: m.agent.role } : {}),
		status,
		owner: Boolean(m.owner),
		canSignIn,
		...(homeTeam ? { homeTeam } : {}),
		...(m.kind === 'agent' ? { office: agentPlacement(m, org, officeNamesById) } : {}),
		...(manager ? { reportsTo: { id: manager.id, name: manager.name } } : {}),
		...(m.contact ? { contact: m.contact } : {}),
		directReports
	};
}

function decodeTurnState(raw: unknown): TurnState {
	switch (typeof raw === 'string' ? raw.toLowerCase() : '') {
		case 'running':
		case 'active':
			return 'running';
		case 'succeeded':
		case 'completed':
		case 'idle':
			return 'succeeded';
		case 'failed':
		case 'error':
			return 'failed';
		case 'cancelled':
		case 'stopped':
			return 'cancelled';
		default:
			return 'accepted';
	}
}

function readOptString(value: unknown): string | undefined {
	return typeof value === 'string' ? value : undefined;
}

function decodeSessionTurn(item: unknown): Turn | undefined {
	if (!item || typeof item !== 'object') return undefined;
	const r = item as Record<string, unknown>;
	const id = readOptString(r.session_id) ?? readOptString(r.id);
	if (!id) return undefined;
	const startedAt =
		readOptString(r.started_at) ?? readOptString(r.created_at) ?? new Date(0).toISOString();
	const endedAt = readOptString(r.ended_at);
	const summary = readOptString(r.summary);
	const failure = readOptString(r.error);
	return {
		id,
		startedAt,
		...(endedAt ? { endedAt } : {}),
		startedBy: { kind: 'run' },
		state: decodeTurnState(r.state ?? r.status),
		...(summary ? { summary } : {}),
		...(failure ? { failure } : {}),
		attempts: [],
		hasLog: false
	};
}

/**
 * Production HTTP adapter for Members directory, member profiles, home-team moves,
 * and A6 sign-in access/handover endpoints. Never falls back to browser mocks.
 */
export function createHttpMembersApi({
	http,
	orgApi,
	workItemsApi,
	session,
	newEntityId = newStableId
}: {
	http: GatewayHttpTransport;
	orgApi: Pick<OrgApi, 'view' | 'command'>;
	workItemsApi?: Pick<WorkItemsApi, 'queue'> & Partial<Pick<WorkItemsApi, 'assign'>>;
	session: () => MembersSessionInfo | undefined;
	newEntityId?: () => string;
}): MembersApi {
	async function safeWorkRows(organizationId: string): Promise<TaskRow[]> {
		if (!workItemsApi) return [];
		try {
			const queue = await workItemsApi.queue(organizationId, { kind: 'organization' });
			return queue.rows;
		} catch {
			return [];
		}
	}
	async function readAccessMap(
		organizationId: string
	): Promise<ReadonlyMap<string, GatewayAccessEntry> | undefined> {
		let reply: GatewayReply;
		try {
			reply = await http.read(`/api/v1/organizations/${encodeURIComponent(organizationId)}/access`);
		} catch {
			return undefined;
		}
		if (reply.status === 403 || reply.status === 401) return undefined;
		if (reply.status !== 200) return undefined;
		const result = unwrapGatewayResult(reply, 'Access details could not be loaded.');
		if (!Array.isArray(result.entries)) return undefined;
		const map = new Map<string, GatewayAccessEntry>();
		for (const item of result.entries) {
			if (
				item &&
				typeof item === 'object' &&
				typeof (item as { member_id?: unknown }).member_id === 'string'
			) {
				map.set((item as GatewayAccessEntry).member_id, item as GatewayAccessEntry);
			}
		}
		return map;
	}

	async function readOrgView(organizationId: string): Promise<OrganizationView | undefined> {
		try {
			const reply = await http.read('/api/v1/organizations');
			if (reply.status !== 200) return undefined;
			const envelope = readResponse(reply.body, readJsonObject);
			if ('error' in envelope) return undefined;
			return readOrganizationsList(envelope.result).organizations.find(
				(o) => o.id === organizationId
			);
		} catch {
			return undefined;
		}
	}

	async function submitAccessPost(
		path: string,
		body: Record<string, unknown>,
		fallbackMessage: string
	): Promise<GatewayReply> {
		try {
			return await http.submit(path, body);
		} catch {
			throw { source: 'api', code: 'unavailable', message: fallbackMessage } satisfies AppError;
		}
	}

	async function readOfficeNames(organizationId: string): Promise<Map<string, string>> {
		const map = new Map<string, string>();
		try {
			const reply = await http.read(
				`/api/v1/organizations/${encodeURIComponent(organizationId)}/offices`
			);
			if (reply.status !== 200) return map;
			const envelope = readResponse(reply.body, readOfficeList);
			if ('result' in envelope) {
				for (const o of envelope.result.offices) map.set(o.id, o.name);
			}
		} catch {
			// Fall back to org.offices when offices endpoint is unavailable.
		}
		return map;
	}

	async function loadProfile(organizationId: string, memberId: string): Promise<MemberProfile> {
		const [{ snapshot }, accessById, org, workRows, officeNames] = await Promise.all([
			orgApi.view(organizationId),
			readAccessMap(organizationId),
			readOrgView(organizationId),
			safeWorkRows(organizationId),
			readOfficeNames(organizationId)
		]);
		const member = snapshot.members.find((m) => m.id === memberId);
		if (!member) {
			throw {
				source: 'api',
				code: 'not_found',
				message: 'That member could not be found.'
			} satisfies AppError;
		}
		const teamsById = new Map(snapshot.teams.map((t) => [t.id, t]));
		const membersById = new Map(snapshot.members.map((m) => [m.id, m]));
		const row = mapMemberRow(
			member,
			snapshot,
			teamsById,
			membersById,
			accessById,
			org,
			officeNames
		);
		const leads = snapshot.teams
			.filter((t) => t.lead_member_id === memberId)
			.map((t) => teamRef(t)!);
		const viewerMember = org?.employee_id ? membersById.get(org.employee_id) : undefined;
		const isOwnerOrAdmin = Boolean(session()?.accountAdmin || viewerMember?.owner);
		const access = accessById?.get(memberId);
		const currentTask = row.status !== 'inactive' ? pickCurrentTask(workRows, memberId) : undefined;
		return {
			...row,
			...(currentTask ? { workingOn: currentTask, currentTask } : {}),
			...(member.agent?.title ? { title: member.agent.title } : {}),
			...(member.agent?.focus ? { focus: member.agent.focus } : {}),
			leads,
			allowHiring: { on: false, canChange: isOwnerOrAdmin },
			...(access?.pending_invite ? { invitePending: { expiresAt: '' } } : {}),
			viewer: {
				isSelf: org?.employee_id === memberId,
				isOwnerOrAdmin,
				costVisible: isOwnerOrAdmin
			}
		};
	}

	return Object.freeze({
		async directory(organizationId: string): Promise<MemberRow[]> {
			const [{ snapshot }, accessById, org, workRows, officeNames] = await Promise.all([
				orgApi.view(organizationId),
				readAccessMap(organizationId),
				readOrgView(organizationId),
				safeWorkRows(organizationId),
				readOfficeNames(organizationId)
			]);
			const teamsById = new Map(snapshot.teams.map((t) => [t.id, t]));
			const membersById = new Map(snapshot.members.map((m) => [m.id, m]));
			return snapshot.members.map((m) => {
				const row = mapMemberRow(m, snapshot, teamsById, membersById, accessById, org, officeNames);
				const workingOn = row.status !== 'inactive' ? pickCurrentTask(workRows, m.id) : undefined;
				return workingOn ? { ...row, workingOn } : row;
			});
		},

		profile: loadProfile,

		async work(organizationId: string, memberId: string) {
			const [rows, org] = await Promise.all([
				safeWorkRows(organizationId),
				readOrgView(organizationId)
			]);
			return buildMemberWorkRows(rows, memberId, org?.employee_id);
		},
		async conversations(organizationId: string, memberId: string) {
			return readMemberConversations(http, organizationId, memberId);
		},
		async activity(organizationId: string, memberId: string) {
			const rows = await safeWorkRows(organizationId);
			return buildMemberActivity(rows, memberId);
		},
		async runs(organizationId: string, memberId: string): Promise<Turn[]> {
			let reply: GatewayReply;
			try {
				reply = await http.read(
					`/api/v1/organizations/${encodeURIComponent(organizationId)}/agents/${encodeURIComponent(memberId)}/sessions`
				);
			} catch {
				return [];
			}
			if (reply.status !== 200) return [];
			const result = unwrapGatewayResult(reply, 'Agent sessions could not be loaded.');
			const rawSessions = Array.isArray(result.sessions) ? result.sessions : [];
			return rawSessions.map(decodeSessionTurn).filter((t): t is Turn => t !== undefined);
		},
		sessionFile: unavailableCall,

		async role(organizationId: string, memberId: string): Promise<RoleView> {
			const { snapshot } = await orgApi.view(organizationId);
			const member = snapshot.members.find((m) => m.id === memberId);
			if (!member) {
				throw {
					source: 'api',
					code: 'not_found',
					message: 'That member could not be found.'
				} satisfies AppError;
			}
			const title = member.agent?.title || member.title_in_team || undefined;
			let role: RoleView['role'];
			if (member.role_id) {
				const fallback = "Couldn't load the member's role.";
				let reply: GatewayReply;
				try {
					reply = await http.read(
						`/api/v1/organizations/${encodeURIComponent(organizationId)}/roles/${encodeURIComponent(member.role_id)}`
					);
				} catch {
					throw { source: 'api', code: 'unavailable', message: fallback } satisfies AppError;
				}
				if (reply.status !== 404) {
					const result = unwrapGatewayResult(reply, fallback);
					const record = readRoleRecordWithId(result, member.role_id, fallback);
					role = { name: record.name, revision: record.revision, body: record.body };
				}
			}
			return {
				...(role ? { role } : {}),
				...(member.agent?.role ? { roleText: member.agent.role } : {}),
				...(title ? { title } : {})
			};
		},

		async runNow(organizationId: string, memberId: string, note: string): Promise<RunNowResult> {
			const commandId = newEntityId();
			const reply = await submitAccessPost(
				`/api/v1/organizations/${encodeURIComponent(organizationId)}/agents/${encodeURIComponent(memberId)}/start`,
				{
					command_id: commandId,
					...(note.trim() ? { instruction: note.trim() } : {})
				},
				'Could not start a run for this agent.'
			);
			const result = unwrapGatewayResult(reply, 'Could not start a run for this agent.');
			const turnId =
				typeof result.session_id === 'string'
					? result.session_id
					: typeof result.run_id === 'string'
						? result.run_id
						: commandId;
			const startedAt = new Date().toISOString();
			const turn: Turn = {
				id: turnId,
				startedAt,
				startedBy: { kind: 'run', byViewer: true },
				state: 'accepted',
				...(note.trim() ? { summary: note.trim() } : {}),
				attempts: [],
				hasLog: false
			};
			const profile = await loadProfile(organizationId, memberId);
			return { turn, profile };
		},

		async setAllowHiring(organizationId: string, memberId: string, on: boolean): Promise<void> {
			const { snapshot } = await orgApi.view(organizationId);
			const member = snapshot.members.find((m) => m.id === memberId);
			if (!member) {
				throw {
					source: 'api',
					code: 'not_found',
					message: 'That member could not be found.'
				} satisfies AppError;
			}
			const reply = await submitAccessPost(
				`/api/v1/organizations/${encodeURIComponent(organizationId)}/members/set-may-hire`,
				{
					command_id: newEntityId(),
					member_id: memberId,
					expected_version: member.version,
					may_hire: on
				},
				'Could not update hiring permission.'
			);
			unwrapGatewayResult(reply, 'Could not update hiring permission.');
		},

		async moveHomeTeam(organizationId: string, memberId: string, toTeamId: string): Promise<void> {
			const { snapshot } = await orgApi.view(organizationId);
			const member = snapshot.members.find((m) => m.id === memberId);
			if (!member) {
				throw {
					source: 'api',
					code: 'not_found',
					message: 'That member could not be found.'
				} satisfies AppError;
			}
			await orgApi.command(organizationId, {
				method: 'member.move',
				params: {
					membership_id: member.id,
					expected_version: member.version,
					new_membership_id: newEntityId(),
					to_team_id: toTeamId
				}
			});
		},

		async assignableTasks(organizationId: string) {
			const rows = await safeWorkRows(organizationId);
			return buildAssignableTasks(rows);
		},

		async assignTask(organizationId: string, memberId: string, ref: string): Promise<void> {
			if (!workItemsApi?.assign) {
				throw {
					source: 'api',
					code: 'unavailable',
					message: 'Task assignment is not available right now.'
				} satisfies AppError;
			}
			const rows = await safeWorkRows(organizationId);
			const target = rows.find((r) => r.ref === ref || r.id === ref);
			if (!target) {
				throw {
					source: 'api',
					code: 'not_found',
					message: `Task ${ref} was not found.`
				} satisfies AppError;
			}
			await workItemsApi.assign(organizationId, target.id, {
				assignedTeamId: target.assignedTeam?.id ?? target.team.id ?? null,
				assigneeId: memberId,
				expectedRevision: target.revision
			});
		},

		async inviteToSignIn(organizationId: string, memberId: string): Promise<SignInLink> {
			const reply = await submitAccessPost(
				`/api/v1/organizations/${encodeURIComponent(organizationId)}/access/invite`,
				{ member_id: memberId },
				'Could not create the invitation link.'
			);
			return parseLinkResult(reply, 'Could not create the invitation link.');
		},

		async newSignInLink(organizationId: string, memberId: string): Promise<SignInLink> {
			const reply = await submitAccessPost(
				`/api/v1/organizations/${encodeURIComponent(organizationId)}/access/reset-link`,
				{ member_id: memberId },
				'Could not create a new sign-in link.'
			);
			return parseLinkResult(reply, 'Could not create a new sign-in link.');
		},

		async removeAccess(organizationId: string, memberId: string): Promise<void> {
			const reply = await submitAccessPost(
				`/api/v1/organizations/${encodeURIComponent(organizationId)}/access/remove`,
				{ member_id: memberId },
				'Could not remove sign-in access.'
			);
			unwrapGatewayResult(reply, 'Could not remove sign-in access.');
		},

		async handOverOwnership(
			organizationId: string,
			toMemberId: string,
			password: string
		): Promise<void> {
			const reply = await submitAccessPost(
				`/api/v1/organizations/${encodeURIComponent(organizationId)}/owner/handover`,
				{ to_member_id: toMemberId, password },
				'Could not hand over ownership.'
			);
			if (reply.status === 200) {
				unwrapGatewayResult(reply, 'Could not hand over ownership.');
				return;
			}
			const err = readGatewayAppError(
				reply.body,
				gatewayStatusCode(reply.status),
				'Could not hand over ownership.'
			);
			if (reply.status === 403 && /password|credential/i.test(err.message)) {
				throw { source: 'api', code: 'wrong_password', message: err.message };
			}
			throw err;
		}
	});
}
