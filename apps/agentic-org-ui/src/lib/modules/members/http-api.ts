import { isBuiltInAgent, type AppError } from '$lib/proto';
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
	decodeSessionTurn,
	mapMemberRow,
	parseLinkResult,
	teamRef,
	type GatewayAccessEntry,
	type MembersSessionInfo
} from './http-api-map';
import {
	buildAssignableTasks,
	buildMemberActivity,
	buildMemberWorkRows,
	pickCurrentTask,
	readMemberConversations
} from './live-hydration';
import type { MemberProfile, MemberRow, RoleView, RunNowResult, SignInLink, Turn } from './types';

export type { GatewayAccessEntry, MembersSessionInfo } from './http-api-map';

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
	async function readAccessMap(
		organizationId: string,
		signal?: AbortSignal
	): Promise<ReadonlyMap<string, GatewayAccessEntry> | undefined> {
		let reply: GatewayReply;
		const path = `/api/v1/organizations/${encodeURIComponent(organizationId)}/access`;
		try {
			reply = await (signal ? http.read(path, signal) : http.read(path));
		} catch (err) {
			if (signal?.aborted) throw err;
			return undefined;
		}
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

	async function readOrgView(
		organizationId: string,
		signal?: AbortSignal
	): Promise<OrganizationView | undefined> {
		try {
			const reply = await (signal
				? http.read('/api/v1/organizations', signal)
				: http.read('/api/v1/organizations'));
			if (reply.status !== 200) return undefined;
			const envelope = readResponse(reply.body, readJsonObject);
			if ('error' in envelope) return undefined;
			return readOrganizationsList(envelope.result).organizations.find(
				(o) => o.id === organizationId
			);
		} catch (err) {
			if (signal?.aborted) throw err;
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

	async function readOfficeNames(
		organizationId: string,
		signal?: AbortSignal
	): Promise<Map<string, string>> {
		const map = new Map<string, string>();
		const path = `/api/v1/organizations/${encodeURIComponent(organizationId)}/offices`;
		try {
			const reply = await (signal ? http.read(path, signal) : http.read(path));
			if (reply.status !== 200) return map;
			const envelope = readResponse(reply.body, readOfficeList);
			if ('result' in envelope) {
				for (const o of envelope.result.offices) map.set(o.id, o.name);
			}
		} catch (err) {
			if (signal?.aborted) throw err;
			// Fall back to org.offices when offices endpoint is unavailable.
		}
		return map;
	}

	async function loadProfile(
		organizationId: string,
		memberId: string,
		signal?: AbortSignal
	): Promise<MemberProfile> {
		const [{ snapshot }, accessById, org, workRows, officeNames] = await Promise.all([
			orgApi.view(organizationId, signal),
			readAccessMap(organizationId, signal),
			readOrgView(organizationId, signal),
			safeWorkRows(organizationId, signal),
			readOfficeNames(organizationId, signal)
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
		async directory(organizationId: string, signal?: AbortSignal): Promise<MemberRow[]> {
			const [{ snapshot }, accessById, org, workRows, officeNames] = await Promise.all([
				orgApi.view(organizationId, signal),
				readAccessMap(organizationId, signal),
				readOrgView(organizationId, signal),
				safeWorkRows(organizationId, signal),
				readOfficeNames(organizationId, signal)
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
			if (isBuiltInAgent(member)) {
				throw {
					source: 'api',
					code: 'not_authorized',
					message: 'Built-in agents cannot be moved to another home team.'
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
