import type { AppError, Member, OrgSnapshot } from '$lib/proto';
import { NotDispatchedError } from '$lib/platform/http/errors';
import {
	gatewayStatusCode,
	readGatewayAppError,
	unwrapGatewayResult,
	type GatewayHttpTransport,
	type GatewayReply
} from '$lib/platform/http/gateway-errors';
import { readJsonObject, readResponse } from '$lib/platform/http/wire';
import { readOrganizationsList, type OrganizationView } from '$lib/platform/organizations/readers';
import type { OrgApi } from '$lib/modules/organization/api';
import { newStableId } from '$lib/modules/organization/commands';
import { HR_CANT_RUN } from './present';
import {
	readRoleEntityAck,
	readRoleRecord,
	readRoleRecordWithId,
	type WireRoleRecord
} from './wire';
import type {
	Attribution,
	Role,
	RoleDetail,
	RoleDraft,
	RoleEdit,
	RoleEmployee,
	RoleField,
	RoleSummary,
	RolesApi,
	RolesOverview
} from './types';

const CONFLICT_MESSAGE =
	"This role changed while you were editing. Here's the newer version; your changes are kept.";
const UNCERTAIN_OUTCOME_MESSAGE = 'The outcome of this request is not confirmed yet.';
const SESSION_CHANGED_ERROR: AppError = {
	source: 'api',
	code: 'not_authorized',
	message: 'Your session changed. Sign in and try again.'
};

type SessionToken = {
	caller: string;
	generation: number;
};

type SubmittedRolePayload = {
	name: string;
	key?: string;
	bodyMarkdown: string;
};

type PendingCreate = {
	caller: string;
	commandId: string;
	roleId: string;
	submitted: SubmittedRolePayload;
	dispatched?: boolean;
	committedRevision?: number;
};

type PendingUpdate = {
	caller: string;
	commandId: string;
	submitted: SubmittedRolePayload;
	dispatched?: boolean;
	committedRevision?: number;
};

export interface HttpRolesApi extends RolesApi {
	/** Drops abandoned pending retry state for `caller` (or all callers when omitted) on sign-out. */
	clearCaller(caller?: string): void;
}

function samePayload(a: SubmittedRolePayload, b: SubmittedRolePayload): boolean {
	return (
		a.name === b.name &&
		(a.key ?? undefined) === (b.key ?? undefined) &&
		a.bodyMarkdown === b.bodyMarkdown
	);
}

function toSubmittedPayload(draft: RoleDraft): SubmittedRolePayload {
	return {
		name: draft.name,
		...(draft.key ? { key: draft.key } : {}),
		bodyMarkdown: draft.bodyMarkdown
	};
}

function mapAttribution(
	at: string | undefined,
	byKindRaw: string | undefined,
	byId: string | undefined,
	membersById: ReadonlyMap<string, Member>
): Attribution {
	const byKind: Attribution['byKind'] =
		byKindRaw === 'employee' || byKindRaw === 'system' || byKindRaw === 'unknown'
			? byKindRaw
			: 'unknown';
	const byName = byKind === 'employee' && byId ? membersById.get(byId)?.name : undefined;
	return {
		...(at ? { at } : {}),
		byKind,
		...(byKind === 'employee' && byId ? { byId } : {}),
		...(byName ? { byName } : {})
	};
}

function mapRole(record: WireRoleRecord, membersById: ReadonlyMap<string, Member>): Role {
	return {
		id: record.id,
		...(record.key ? { key: record.key } : {}),
		schemaVersion: 1,
		revision: record.revision,
		name: record.name,
		bodyMarkdown: record.body,
		created: mapAttribution(
			record.created_at,
			record.created_by_kind,
			record.created_by_id,
			membersById
		),
		updated: mapAttribution(
			record.updated_at,
			record.updated_by_kind,
			record.updated_by_id,
			membersById
		)
	};
}

function isActiveMember(m: Member): boolean {
	return !(m.kind === 'agent' && m.agent?.status === 'offboarded');
}

function memberTitle(m: Member): string | undefined {
	return m.agent?.title || m.title_in_team || undefined;
}

function isOwnerViewer(snapshot: OrgSnapshot, org: OrganizationView | undefined): boolean {
	if (!org?.employee_id) return false;
	return snapshot.members.some(
		(m) => m.id === org.employee_id && m.kind === 'person' && m.owner === true
	);
}

function fieldForRoleError(err: AppError): RoleField | undefined {
	if (err.code === 'already_exists' && /\bkey\b/i.test(err.message)) return 'key';
	if (err.code === 'invalid_name') {
		return /\bkey\b/i.test(err.message) ? 'key' : 'name';
	}
	return undefined;
}

function withField(err: AppError): AppError & { field?: RoleField } {
	const field = fieldForRoleError(err);
	return field ? { ...err, field } : err;
}

function isUnknownOutcomeReply(reply: GatewayReply): boolean {
	if (reply.status === 504) return true;
	const root =
		reply.body && typeof reply.body === 'object'
			? (reply.body as Record<string, unknown>)
			: undefined;
	return root?.outcome === 'unknown';
}

function extractConflictLatest(err: unknown, expectedId: string): Role | undefined {
	if (!err || typeof err !== 'object' || !('latest' in err)) return undefined;
	const latest = (err as { latest?: Role }).latest;
	return latest && latest.id === expectedId ? latest : undefined;
}

function buildFallbackRole(
	ack: { id: string; revision: number },
	desired: SubmittedRolePayload,
	created: Attribution = { byKind: 'unknown' }
): Role {
	return {
		id: ack.id,
		...(desired.key ? { key: desired.key } : {}),
		schemaVersion: 1,
		revision: ack.revision,
		name: desired.name,
		bodyMarkdown: desired.bodyMarkdown,
		created,
		updated: { byKind: 'unknown' }
	};
}

function isTerminalUpdateRefusal(code: string | undefined): boolean {
	return code === 'not_authorized' || code === 'conflict' || code === 'not_found';
}

function shouldPreserveAttempt(reply: GatewayReply): boolean {
	return (
		isUnknownOutcomeReply(reply) ||
		reply.status >= 500 ||
		reply.status === 401 ||
		reply.status === 429
	);
}

function toNotDispatchedAppError(err: NotDispatchedError): AppError {
	if (/exceeds its limit/i.test(err.message)) {
		return {
			source: 'api',
			code: 'too_large',
			message: 'This role is too large to save. Shorten the text and try again.'
		};
	}
	return {
		source: 'api',
		code: 'invalid_request',
		message: err.message
	};
}

function deleteIfCurrent<T>(map: Map<string, T>, key: string | undefined, expected: T): void {
	if (key && map.get(key) === expected) map.delete(key);
}

/**
 * Production HTTP adapter for Organization settings › Roles (`FC-108`, `TASK-41`).
 * Talks directly to `/api/v1/organizations/{org}/roles*` and fails closed when unavailable.
 */
export function createHttpRolesApi({
	http,
	orgApi,
	sessionScope,
	sessionGeneration,
	newEntityId = newStableId,
	newCommandId = newStableId
}: {
	http: GatewayHttpTransport;
	orgApi: Pick<OrgApi, 'view'>;
	/** Stable caller identity (`instance_id:account_id`) so pending retry IDs never cross accounts. */
	sessionScope?: () => string | undefined;
	/** Monotonic session generation bumped on sign-out / account switch (unchanged across same-account reauth). */
	sessionGeneration?: () => number;
	newEntityId?: () => string;
	newCommandId?: () => string;
}): HttpRolesApi {
	// Retain idempotent IDs and originally submitted payloads scoped by caller + logical save
	// (`attemptId`) across uncertain/transient outcomes (504, 5xx, network drop, 401 reauth, 429,
	// malformed ack). Even if the user edits fields after a 504, the original attempt is resolved
	// first so an already-committed create is updated in place rather than duplicated.
	const pendingCreates = new Map<string, PendingCreate>();
	const pendingUpdates = new Map<string, PendingUpdate>();

	function currentSessionToken(): SessionToken {
		return {
			caller: sessionScope?.() ?? '',
			generation: sessionGeneration?.() ?? 0
		};
	}

	function isSameSession(session: SessionToken): boolean {
		return (
			(sessionGeneration?.() ?? 0) === session.generation &&
			(sessionScope?.() ?? '') === session.caller
		);
	}

	function assertSameSession(session: SessionToken): void {
		if (!isSameSession(session)) throw SESSION_CHANGED_ERROR;
	}

	async function readGateway(path: string, fallbackMessage: string): Promise<GatewayReply> {
		try {
			return await http.read(path);
		} catch (err) {
			if (err instanceof Error && err.message === 'Session changed') throw SESSION_CHANGED_ERROR;
			throw { source: 'api', code: 'unavailable', message: fallbackMessage } satisfies AppError;
		}
	}

	async function submitGateway(
		path: string,
		body: unknown,
		fallbackMessage: string
	): Promise<GatewayReply> {
		try {
			return await http.submit(path, body);
		} catch (err) {
			if (err instanceof NotDispatchedError) throw err;
			if (err instanceof Error && err.message === 'Session changed') throw SESSION_CHANGED_ERROR;
			throw { source: 'api', code: 'unavailable', message: fallbackMessage } satisfies AppError;
		}
	}

	async function dispatchAttempt<T extends { dispatched?: boolean; committedRevision?: number }>(
		path: string,
		body: unknown,
		pending: Map<string, T>,
		attempt: T,
		attemptKey: string | undefined,
		session: SessionToken
	): Promise<GatewayReply> {
		assertSameSession(session);
		try {
			const reply = await submitGateway(path, body, UNCERTAIN_OUTCOME_MESSAGE);
			attempt.dispatched = true;
			assertSameSession(session);
			return reply;
		} catch (err) {
			if (err instanceof NotDispatchedError) {
				if (!attempt.dispatched && attempt.committedRevision === undefined) {
					deleteIfCurrent(pending, attemptKey, attempt);
				}
				throw toNotDispatchedAppError(err);
			}
			if (err !== SESSION_CHANGED_ERROR) {
				attempt.dispatched = true;
			}
			assertSameSession(session);
			throw err;
		}
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

	async function fetchRoleList(organizationId: string): Promise<WireRoleRecord[]> {
		const fallback = "Couldn't load the roles.";
		const reply = await readGateway(
			`/api/v1/organizations/${encodeURIComponent(organizationId)}/roles`,
			fallback
		);
		const result = unwrapGatewayResult(reply, fallback);
		if (!Array.isArray(result.roles)) {
			throw { source: 'api', code: 'unavailable', message: fallback } satisfies AppError;
		}
		return result.roles.map((item) => readRoleRecord(item, fallback));
	}

	async function fetchRoleRecord(organizationId: string, roleId: string): Promise<WireRoleRecord> {
		const fallback = "Couldn't load this role.";
		const reply = await readGateway(
			`/api/v1/organizations/${encodeURIComponent(organizationId)}/roles/${encodeURIComponent(roleId)}`,
			fallback
		);
		const result = unwrapGatewayResult(reply, fallback);
		return readRoleRecordWithId(result, roleId, fallback);
	}

	async function fetchAuthoritativeRole(
		organizationId: string,
		roleId: string
	): Promise<Role | undefined> {
		try {
			const [record, view] = await Promise.all([
				fetchRoleRecord(organizationId, roleId),
				orgApi.view(organizationId).catch(() => undefined)
			]);
			const membersById = new Map((view?.snapshot.members ?? []).map((m) => [m.id, m]));
			return mapRole(record, membersById);
		} catch {
			return undefined;
		}
	}

	async function submitRoleCreate(
		organizationId: string,
		attempt: PendingCreate,
		attemptKey: string | undefined,
		session: SessionToken,
		fallback: string
	) {
		const reply = await dispatchAttempt(
			`/api/v1/organizations/${encodeURIComponent(organizationId)}/roles`,
			{
				command_id: attempt.commandId,
				role_id: attempt.roleId,
				name: attempt.submitted.name,
				body: attempt.submitted.bodyMarkdown,
				...(attempt.submitted.key ? { key: attempt.submitted.key } : {})
			},
			pendingCreates,
			attempt,
			attemptKey,
			session
		);
		if (isUnknownOutcomeReply(reply)) {
			throw {
				source: 'api',
				code: 'unavailable',
				message: UNCERTAIN_OUTCOME_MESSAGE
			} satisfies AppError;
		}
		if (reply.status !== 200) {
			if (!shouldPreserveAttempt(reply)) deleteIfCurrent(pendingCreates, attemptKey, attempt);
			const err = readGatewayAppError(reply.body, gatewayStatusCode(reply.status), fallback);
			throw withField(err);
		}
		const result = unwrapGatewayResult(reply, fallback);
		return readRoleEntityAck(result, attempt.roleId, fallback);
	}

	async function submitRoleUpdate(
		organizationId: string,
		roleId: string,
		expectedRevision: number,
		attempt: PendingUpdate,
		attemptKey: string | undefined,
		session: SessionToken,
		fallback: string
	) {
		const reply = await dispatchAttempt(
			`/api/v1/organizations/${encodeURIComponent(organizationId)}/roles/update`,
			{
				command_id: attempt.commandId,
				role_id: roleId,
				expected_version: expectedRevision,
				name: attempt.submitted.name,
				body: attempt.submitted.bodyMarkdown,
				...(attempt.submitted.key
					? { key: attempt.submitted.key, clear_key: false }
					: { clear_key: true })
			},
			pendingUpdates,
			attempt,
			attemptKey,
			session
		);
		if (isUnknownOutcomeReply(reply)) {
			throw {
				source: 'api',
				code: 'unavailable',
				message: UNCERTAIN_OUTCOME_MESSAGE
			} satisfies AppError;
		}
		if (reply.status !== 200) {
			if (!shouldPreserveAttempt(reply)) deleteIfCurrent(pendingUpdates, attemptKey, attempt);
			const err = readGatewayAppError(reply.body, gatewayStatusCode(reply.status), fallback);
			if (err.code === 'conflict') {
				assertSameSession(session);
				const latest = await fetchAuthoritativeRole(organizationId, roleId);
				assertSameSession(session);
				if (latest) {
					throw { ...err, message: CONFLICT_MESSAGE, latest };
				}
			}
			throw withField(err);
		}
		const result = unwrapGatewayResult(reply, fallback);
		return readRoleEntityAck(result, roleId, fallback);
	}

	async function resolveChangedPendingUpdate(
		organizationId: string,
		roleId: string,
		edit: RoleEdit,
		desired: SubmittedRolePayload,
		existing: PendingUpdate,
		attemptKey: string,
		session: SessionToken,
		fallback: string
	): Promise<{ resolved: Role } | { retryFresh: true }> {
		if (existing.committedRevision === undefined) {
			try {
				const replayed = await submitRoleUpdate(
					organizationId,
					roleId,
					edit.expectedRevision,
					existing,
					attemptKey,
					session,
					fallback
				);
				existing.committedRevision = replayed.revision;
			} catch (err) {
				assertSameSession(session);
				if (pendingUpdates.get(attemptKey) === existing) throw err;
				if (isTerminalUpdateRefusal((err as AppError | undefined)?.code)) throw err;
				return { retryFresh: true };
			}
		}
		assertSameSession(session);
		const followUp = await updateRoleInSession(
			organizationId,
			roleId,
			{
				...desired,
				expectedRevision: existing.committedRevision,
				attemptId: `${edit.attemptId}:rev:${existing.committedRevision}`
			},
			session
		);
		deleteIfCurrent(pendingUpdates, attemptKey, existing);
		return { resolved: followUp };
	}

	async function updateRoleInSession(
		organizationId: string,
		roleId: string,
		edit: RoleEdit,
		session: SessionToken
	): Promise<Role> {
		assertSameSession(session);
		const fallback = 'Could not save the role.';
		const desired = toSubmittedPayload(edit);
		const attemptKey = edit.attemptId
			? JSON.stringify([
					session.caller,
					organizationId,
					roleId,
					edit.expectedRevision,
					edit.attemptId
				])
			: undefined;

		let existing = attemptKey ? pendingUpdates.get(attemptKey) : undefined;
		if (
			attemptKey &&
			existing &&
			(existing.committedRevision !== undefined || !samePayload(existing.submitted, desired))
		) {
			const outcome = await resolveChangedPendingUpdate(
				organizationId,
				roleId,
				edit,
				desired,
				existing,
				attemptKey,
				session,
				fallback
			);
			if ('resolved' in outcome) return outcome.resolved;
			existing = undefined;
		}

		assertSameSession(session);
		const attempt: PendingUpdate = existing ?? {
			caller: session.caller,
			commandId: newCommandId(),
			submitted: desired
		};
		if (attemptKey) pendingUpdates.set(attemptKey, attempt);

		const ack = await submitRoleUpdate(
			organizationId,
			roleId,
			edit.expectedRevision,
			attempt,
			attemptKey,
			session,
			fallback
		);
		deleteIfCurrent(pendingUpdates, attemptKey, attempt);
		assertSameSession(session);
		const authoritative = await fetchAuthoritativeRole(organizationId, ack.id);
		assertSameSession(session);
		if (authoritative && authoritative.revision === ack.revision) return authoritative;
		return buildFallbackRole(ack, desired, authoritative?.created);
	}

	async function updateRole(organizationId: string, roleId: string, edit: RoleEdit): Promise<Role> {
		return updateRoleInSession(organizationId, roleId, edit, currentSessionToken());
	}

	async function resolveChangedPendingCreate(
		organizationId: string,
		draft: RoleDraft,
		desired: SubmittedRolePayload,
		existing: PendingCreate,
		attemptKey: string,
		session: SessionToken,
		fallback: string
	): Promise<{ resolved: Role } | { retryFresh: true }> {
		if (existing.committedRevision === undefined) {
			try {
				const replayed = await submitRoleCreate(
					organizationId,
					existing,
					attemptKey,
					session,
					fallback
				);
				existing.committedRevision = replayed.revision;
			} catch (err) {
				assertSameSession(session);
				if (pendingCreates.get(attemptKey) === existing) throw err;
				if ((err as AppError | undefined)?.code === 'not_authorized') throw err;
				return { retryFresh: true };
			}
		}
		assertSameSession(session);
		try {
			const updated = await updateRoleInSession(
				organizationId,
				existing.roleId,
				{
					...desired,
					expectedRevision: existing.committedRevision,
					attemptId: `${draft.attemptId}:followup`
				},
				session
			);
			deleteIfCurrent(pendingCreates, attemptKey, existing);
			return { resolved: updated };
		} catch (err) {
			assertSameSession(session);
			const latest = extractConflictLatest(err, existing.roleId);
			if (latest) existing.committedRevision = latest.revision;
			throw err;
		}
	}

	return Object.freeze<HttpRolesApi>({
		clearCaller(caller?: string) {
			if (caller === undefined) {
				pendingCreates.clear();
				pendingUpdates.clear();
				return;
			}
			for (const [key, entry] of pendingCreates) {
				if (entry.caller === caller) pendingCreates.delete(key);
			}
			for (const [key, entry] of pendingUpdates) {
				if (entry.caller === caller) pendingUpdates.delete(key);
			}
		},

		async list(organizationId: string): Promise<RolesOverview> {
			const [rolesRes, viewRes, orgRes] = await Promise.allSettled([
				fetchRoleList(organizationId),
				orgApi.view(organizationId),
				readOrgView(organizationId)
			]);
			if (rolesRes.status === 'rejected') throw rolesRes.reason;
			if (viewRes.status === 'rejected') throw viewRes.reason;
			const snapshot = viewRes.value.snapshot;
			const org = orgRes.status === 'fulfilled' ? orgRes.value : undefined;
			const counts = new Map<string, number>();
			for (const member of snapshot.members) {
				if (!member.role_id || !isActiveMember(member)) continue;
				counts.set(member.role_id, (counts.get(member.role_id) ?? 0) + 1);
			}
			const roles: RoleSummary[] = rolesRes.value.map((r) => ({
				id: r.id,
				...(r.key ? { key: r.key } : {}),
				name: r.name,
				revision: r.revision,
				employeeCount: counts.get(r.id) ?? 0
			}));
			return {
				roles,
				viewer: { canEdit: isOwnerViewer(snapshot, org) },
				// Live HR role drafting has no backend RPC yet: fail closed rather than offering a broken button.
				hr: { canRun: false }
			};
		},

		async role(organizationId: string, roleId: string): Promise<RoleDetail> {
			const [roleRes, viewRes, orgRes] = await Promise.allSettled([
				fetchRoleRecord(organizationId, roleId),
				orgApi.view(organizationId),
				readOrgView(organizationId)
			]);
			if (roleRes.status === 'rejected') throw roleRes.reason;
			if (viewRes.status === 'rejected') throw viewRes.reason;
			const snapshot = viewRes.value.snapshot;
			const org = orgRes.status === 'fulfilled' ? orgRes.value : undefined;
			const membersById = new Map(snapshot.members.map((m) => [m.id, m]));
			const role = mapRole(roleRes.value, membersById);
			const employees: RoleEmployee[] = snapshot.members
				.filter(
					(m) =>
						m.role_id === role.id &&
						isActiveMember(m) &&
						(m.kind === 'agent' || m.kind === 'person')
				)
				.map((m) => {
					const title = memberTitle(m);
					return {
						id: m.id,
						name: m.name,
						kind: m.kind as 'agent' | 'person',
						...(title ? { title } : {})
					};
				})
				.toSorted(
					(a, b) =>
						a.name.localeCompare(b.name, undefined, { sensitivity: 'base' }) ||
						a.id.localeCompare(b.id)
				);
			return {
				role,
				employees,
				history: [
					{
						revision: role.revision,
						name: role.name,
						bodyMarkdown: role.bodyMarkdown,
						updated: role.updated
					}
				],
				viewer: { canEdit: isOwnerViewer(snapshot, org) }
			};
		},

		async create(organizationId: string, draft: RoleDraft): Promise<Role> {
			const fallback = 'Could not create the role.';
			const session = currentSessionToken();
			const desired = toSubmittedPayload(draft);
			const attemptKey = draft.attemptId
				? JSON.stringify([session.caller, organizationId, draft.attemptId])
				: undefined;

			let existing = attemptKey ? pendingCreates.get(attemptKey) : undefined;
			if (
				attemptKey &&
				existing &&
				(existing.committedRevision !== undefined || !samePayload(existing.submitted, desired))
			) {
				const outcome = await resolveChangedPendingCreate(
					organizationId,
					draft,
					desired,
					existing,
					attemptKey,
					session,
					fallback
				);
				if ('resolved' in outcome) return outcome.resolved;
				existing = undefined;
			}

			assertSameSession(session);
			const attempt: PendingCreate = existing ?? {
				caller: session.caller,
				commandId: newCommandId(),
				roleId: newEntityId(),
				submitted: desired
			};
			if (attemptKey) pendingCreates.set(attemptKey, attempt);

			const ack = await submitRoleCreate(organizationId, attempt, attemptKey, session, fallback);
			deleteIfCurrent(pendingCreates, attemptKey, attempt);
			assertSameSession(session);
			const authoritative = await fetchAuthoritativeRole(organizationId, ack.id);
			assertSameSession(session);
			if (authoritative && authoritative.revision === ack.revision) return authoritative;
			return buildFallbackRole(ack, desired);
		},

		update: updateRole,

		async askHrToDraft(): Promise<RoleDraft> {
			throw {
				source: 'api',
				code: 'not_ready',
				message: HR_CANT_RUN
			} satisfies AppError;
		}
	});
}
