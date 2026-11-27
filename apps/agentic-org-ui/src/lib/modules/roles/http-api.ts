import type { AppError } from '$lib/proto';
import {
	gatewayStatusCode,
	readGatewayAppError,
	unwrapGatewayResult,
	type GatewayHttpTransport
} from '$lib/platform/http/gateway-errors';
import type { OrgApi } from '$lib/modules/organization/api';
import { newStableId } from '$lib/modules/organization/commands';
import { createAttemptKey, createRoleGatewayHelper, updateAttemptKey } from './http-attempts';
import {
	buildFallbackRole,
	canReuseCommitted,
	CONFLICT_MESSAGE,
	deleteIfCurrent,
	isActiveMember,
	isOwnerViewer,
	isPreCommitValidationRefusal,
	isTerminalUpdateRefusal,
	isUnknownOutcomeReply,
	mapRole,
	memberTitle,
	samePayload,
	shouldPreserveAttempt,
	toSubmittedPayload,
	UNCERTAIN_OUTCOME_MESSAGE,
	withField,
	type PendingCreate,
	type PendingUpdate,
	type SessionToken,
	type SubmittedRolePayload
} from './http-mappers';
import { HR_CANT_RUN } from './present';
import {
	readRoleEntityAck,
	readRoleRecord,
	readRoleRecordWithId,
	type WireRoleRecord
} from './wire';
import type {
	Role,
	RoleDetail,
	RoleDraft,
	RoleEdit,
	RoleEmployee,
	RoleSummary,
	RolesApi,
	RolesOverview
} from './types';

export interface HttpRolesApi extends RolesApi {
	/** Drops abandoned pending retry state for `caller` (or all callers when omitted) on sign-out. */
	clearCaller(caller?: string): void;
	/** Reclaims settled detail-evicted entries for a discarded draft `attemptId`. */
	releaseEvictedAttempt(attemptId: string): void;
}

export function createHttpRolesApi({
	http,
	orgApi,
	sessionScope,
	sessionGeneration,
	isAttemptRetained,
	newEntityId = () => newStableId(),
	newCommandId = () => newStableId()
}: {
	http: GatewayHttpTransport;
	orgApi: Pick<OrgApi, 'view'>;
	/** Stable caller identity (`instance_id:account_id`) so pending retry IDs never cross accounts. */
	sessionScope?: () => string | undefined;
	/** Monotonic session generation bumped on sign-out / account switch (unchanged across same-account reauth). */
	sessionGeneration?: () => number;
	/** Reports whether an editor draft still holds `attemptId`; retained attempts keep entity identity past detail eviction. */
	isAttemptRetained?: (attemptId: string) => boolean;
	newEntityId?: () => string;
	newCommandId?: () => string;
}): HttpRolesApi {
	const pendingCreates = new Map<string, PendingCreate>();
	const pendingUpdates = new Map<string, PendingUpdate>();
	const inFlightKeys = new Map<string, Promise<unknown>>();
	const {
		currentSessionToken,
		assertSameSession,
		withAttemptLock,
		readGateway,
		dispatchAttempt,
		readOrgView
	} = createRoleGatewayHelper({
		http,
		inFlightKeys,
		sessionScope,
		sessionGeneration,
		isAttemptRetained
	});

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
			attempt.mayHaveCommitted = true;
			throw {
				source: 'api',
				code: 'unavailable',
				message: UNCERTAIN_OUTCOME_MESSAGE
			} satisfies AppError;
		}
		if (reply.status !== 200) {
			if (reply.status >= 500) attempt.mayHaveCommitted = true;
			if (!shouldPreserveAttempt(reply, Boolean(attempt.mayHaveCommitted))) {
				deleteIfCurrent(pendingCreates, attemptKey, attempt);
			}
			const err = readGatewayAppError(reply.body, gatewayStatusCode(reply.status), fallback);
			throw withField(err);
		}
		attempt.mayHaveCommitted = true;
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
			attempt.mayHaveCommitted = true;
			throw {
				source: 'api',
				code: 'unavailable',
				message: UNCERTAIN_OUTCOME_MESSAGE
			} satisfies AppError;
		}
		if (reply.status !== 200) {
			if (reply.status >= 500) attempt.mayHaveCommitted = true;
			if (!shouldPreserveAttempt(reply, Boolean(attempt.mayHaveCommitted))) {
				deleteIfCurrent(pendingUpdates, attemptKey, attempt);
			}
			const err = readGatewayAppError(reply.body, gatewayStatusCode(reply.status), fallback);
			if (err.code === 'conflict') {
				assertSameSession(session);
				const latest = await fetchAuthoritativeRole(organizationId, roleId);
				assertSameSession(session);
				if (latest) throw { ...err, message: CONFLICT_MESSAGE, latest };
			}
			throw withField(err);
		}
		attempt.mayHaveCommitted = true;
		const result = unwrapGatewayResult(reply, fallback);
		return readRoleEntityAck(result, roleId, fallback);
	}

	async function resolveCommittedMatch(
		organizationId: string,
		roleId: string,
		existing: {
			submitted: SubmittedRolePayload;
			committedRevision?: number;
			detailsEvicted?: boolean;
			resolved?: Role;
		},
		session: SessionToken
	): Promise<Role> {
		if (existing.resolved) return existing.resolved;
		assertSameSession(session);
		const revision = existing.committedRevision!;
		const authoritative = await fetchAuthoritativeRole(organizationId, roleId);
		assertSameSession(session);
		const role =
			authoritative && authoritative.revision === revision
				? authoritative
				: buildFallbackRole({ id: roleId, revision }, existing.submitted, authoritative?.created);
		existing.resolved = role;
		existing.detailsEvicted = false;
		return role;
	}

	async function runFollowUpUpdate(
		organizationId: string,
		roleId: string,
		baseAttemptId: string | undefined,
		desired: SubmittedRolePayload,
		existing: {
			submitted: SubmittedRolePayload;
			committedRevision?: number;
			hasUnresolvedFollowUp?: boolean;
			conflicted?: boolean;
			detailsEvicted?: boolean;
			resolved?: Role;
		},
		session: SessionToken
	): Promise<Role> {
		assertSameSession(session);
		const expectedRevision = existing.committedRevision!;
		const followUpAttemptId = `${baseAttemptId}:rev:${expectedRevision}`;
		const followUpKey = updateAttemptKey(
			session.caller,
			organizationId,
			roleId,
			expectedRevision,
			followUpAttemptId
		);
		const prevResolved = existing.resolved;
		existing.resolved = undefined;
		existing.hasUnresolvedFollowUp = true;
		try {
			const updated = await updateRoleInSession(
				organizationId,
				roleId,
				{ ...desired, expectedRevision, attemptId: followUpAttemptId },
				session
			);
			existing.submitted = desired;
			existing.committedRevision = updated.revision;
			existing.hasUnresolvedFollowUp = false;
			existing.conflicted = false;
			existing.detailsEvicted = false;
			existing.resolved = updated;
			return updated;
		} catch (err) {
			assertSameSession(session);
			const code = (err as AppError | undefined)?.code;
			if (code === 'conflict') {
				existing.conflicted = true;
				existing.hasUnresolvedFollowUp = false;
			} else if (
				isPreCommitValidationRefusal(code) &&
				(!followUpKey || !pendingUpdates.has(followUpKey))
			) {
				existing.hasUnresolvedFollowUp = false;
				if (prevResolved && existing.committedRevision === prevResolved.revision) {
					existing.resolved = prevResolved;
				}
			}
			throw err;
		}
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
		const followUp = await runFollowUpUpdate(
			organizationId,
			roleId,
			edit.attemptId,
			desired,
			existing,
			session
		);
		return { resolved: followUp };
	}

	async function runInitialUpdate(
		organizationId: string,
		roleId: string,
		expectedRevision: number,
		desired: SubmittedRolePayload,
		attempt: PendingUpdate,
		attemptKey: string | undefined,
		session: SessionToken,
		fallback: string
	): Promise<Role> {
		const ack = await submitRoleUpdate(
			organizationId,
			roleId,
			expectedRevision,
			attempt,
			attemptKey,
			session,
			fallback
		);
		attempt.committedRevision = ack.revision;
		assertSameSession(session);
		const authoritative = await fetchAuthoritativeRole(organizationId, ack.id);
		assertSameSession(session);
		const role =
			authoritative && authoritative.revision === ack.revision
				? authoritative
				: buildFallbackRole(ack, desired, authoritative?.created);
		attempt.detailsEvicted = false;
		attempt.resolved = role;
		return role;
	}

	function updateRoleInSession(
		organizationId: string,
		roleId: string,
		edit: RoleEdit,
		session: SessionToken
	): Promise<Role> {
		const fallback = 'Could not save the role.';
		const desired = toSubmittedPayload(edit);
		const attemptKey = updateAttemptKey(
			session.caller,
			organizationId,
			roleId,
			edit.expectedRevision,
			edit.attemptId
		);
		return withAttemptLock(attemptKey, pendingUpdates, session, async () => {
			assertSameSession(session);
			let existing = attemptKey ? pendingUpdates.get(attemptKey) : undefined;
			if (existing && canReuseCommitted(existing, desired)) {
				return resolveCommittedMatch(organizationId, roleId, existing, session);
			}
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
				attemptId: edit.attemptId,
				commandId: newCommandId(),
				submitted: desired
			};
			if (attemptKey) pendingUpdates.set(attemptKey, attempt);
			return runInitialUpdate(
				organizationId,
				roleId,
				edit.expectedRevision,
				desired,
				attempt,
				attemptKey,
				session,
				fallback
			);
		});
	}

	async function runInitialCreate(
		organizationId: string,
		desired: SubmittedRolePayload,
		attempt: PendingCreate,
		attemptKey: string | undefined,
		session: SessionToken,
		fallback: string
	): Promise<Role> {
		const ack = await submitRoleCreate(organizationId, attempt, attemptKey, session, fallback);
		attempt.committedRevision = ack.revision;
		assertSameSession(session);
		const authoritative = await fetchAuthoritativeRole(organizationId, ack.id);
		assertSameSession(session);
		const role =
			authoritative && authoritative.revision === ack.revision
				? authoritative
				: buildFallbackRole(ack, desired);
		attempt.detailsEvicted = false;
		attempt.resolved = role;
		return role;
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
		const updated = await runFollowUpUpdate(
			organizationId,
			existing.roleId,
			draft.attemptId,
			desired,
			existing,
			session
		);
		return { resolved: updated };
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

		releaseEvictedAttempt(attemptId: string) {
			for (const map of [pendingCreates, pendingUpdates]) {
				for (const [key, entry] of map) {
					if (
						entry.attemptId === attemptId &&
						!inFlightKeys.has(key) &&
						entry.detailsEvicted &&
						entry.committedRevision !== undefined &&
						!entry.hasUnresolvedFollowUp
					) {
						map.delete(key);
					}
				}
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

		create(organizationId: string, draft: RoleDraft): Promise<Role> {
			const fallback = 'Could not create the role.';
			const session = currentSessionToken();
			const desired = toSubmittedPayload(draft);
			const attemptKey = createAttemptKey(session.caller, organizationId, draft.attemptId);
			return withAttemptLock(attemptKey, pendingCreates, session, async () => {
				assertSameSession(session);
				let existing = attemptKey ? pendingCreates.get(attemptKey) : undefined;
				if (existing && canReuseCommitted(existing, desired)) {
					return resolveCommittedMatch(organizationId, existing.roleId, existing, session);
				}
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
					attemptId: draft.attemptId,
					commandId: newCommandId(),
					roleId: newEntityId(),
					submitted: desired
				};
				if (attemptKey) pendingCreates.set(attemptKey, attempt);
				return runInitialCreate(organizationId, desired, attempt, attemptKey, session, fallback);
			});
		},

		update(organizationId: string, roleId: string, edit: RoleEdit): Promise<Role> {
			return updateRoleInSession(organizationId, roleId, edit, currentSessionToken());
		},

		async askHrToDraft(): Promise<RoleDraft> {
			throw { source: 'api', code: 'not_ready', message: HR_CANT_RUN } satisfies AppError;
		}
	});
}
