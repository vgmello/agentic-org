import type { AppError, Member, OrgSnapshot, Team } from '$lib/proto';
import {
	gatewayStatusCode,
	readGatewayAppError,
	unwrapGatewayResult,
	type GatewayHttpTransport,
	type GatewayReply
} from '$lib/platform/http/gateway-errors';
import { readJsonObject, readResponse } from '$lib/platform/http/wire';
import { readOrganizationsList, type OrganizationView } from '$lib/platform/organizations/readers';
import { unavailableCall } from '$lib/platform/unavailable-api';
import type { OrgApi } from '$lib/modules/organization/api';
import { newStableId } from '$lib/modules/organization/commands';
import { readRoleRecordWithId } from '$lib/modules/roles/wire';
import type { MembersApi } from './api';
import type {
	EmployeeStatus,
	MemberProfile,
	MemberRow,
	Placement,
	RoleView,
	SignInLink,
	TeamRef
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
	org: OrganizationView | undefined
): Placement | null | undefined {
	if (m.kind !== 'agent') return undefined;
	if (!m.office_id) return null;
	const office = org?.offices.find((o) => o.id === m.office_id);
	return { id: m.office_id, name: office?.name ?? m.office_id };
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
	org: OrganizationView | undefined
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
		...(m.kind === 'agent' ? { office: agentPlacement(m, org) } : {}),
		...(manager ? { reportsTo: { id: manager.id, name: manager.name } } : {}),
		...(m.contact ? { contact: m.contact } : {}),
		directReports
	};
}

/**
 * Production HTTP adapter for Members directory, member profiles, home-team moves,
 * and A6 sign-in access/handover endpoints. Never falls back to browser mocks.
 */
export function createHttpMembersApi({
	http,
	orgApi,
	session,
	newEntityId = newStableId
}: {
	http: GatewayHttpTransport;
	orgApi: Pick<OrgApi, 'view' | 'command'>;
	session: () => MembersSessionInfo | undefined;
	newEntityId?: () => string;
}): MembersApi {
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

	return Object.freeze({
		async directory(organizationId: string): Promise<MemberRow[]> {
			const [{ snapshot }, accessById, org] = await Promise.all([
				orgApi.view(organizationId),
				readAccessMap(organizationId),
				readOrgView(organizationId)
			]);
			const teamsById = new Map(snapshot.teams.map((t) => [t.id, t]));
			const membersById = new Map(snapshot.members.map((m) => [m.id, m]));
			return snapshot.members.map((m) =>
				mapMemberRow(m, snapshot, teamsById, membersById, accessById, org)
			);
		},

		async profile(organizationId: string, memberId: string): Promise<MemberProfile> {
			const [{ snapshot }, accessById, org] = await Promise.all([
				orgApi.view(organizationId),
				readAccessMap(organizationId),
				readOrgView(organizationId)
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
			const row = mapMemberRow(member, snapshot, teamsById, membersById, accessById, org);
			const leads = snapshot.teams
				.filter((t) => t.lead_member_id === memberId)
				.map((t) => teamRef(t)!);
			const viewerMember = org?.employee_id ? membersById.get(org.employee_id) : undefined;
			const isOwnerOrAdmin = Boolean(session()?.accountAdmin || viewerMember?.owner);
			const access = accessById?.get(memberId);
			return {
				...row,
				...(member.agent?.title ? { title: member.agent.title } : {}),
				...(member.agent?.focus ? { focus: member.agent.focus } : {}),
				leads,
				allowHiring: { on: false, canChange: false },
				...(access?.pending_invite ? { invitePending: { expiresAt: '' } } : {}),
				viewer: {
					isSelf: org?.employee_id === memberId,
					isOwnerOrAdmin,
					costVisible: false
				}
			};
		},

		work: async () => [],
		conversations: async () => [],
		activity: async () => [],
		runs: async () => [],
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

		runNow: unavailableCall,
		setAllowHiring: unavailableCall,

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

		assignableTasks: async () => [],
		assignTask: unavailableCall,

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
