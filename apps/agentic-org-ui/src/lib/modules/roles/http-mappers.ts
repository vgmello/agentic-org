import type { AppError, Member, OrgSnapshot } from '$lib/proto';
import type { NotDispatchedError } from '$lib/platform/http/errors';
import type { GatewayReply } from '$lib/platform/http/gateway-errors';
import type { OrganizationView } from '$lib/platform/organizations/readers';
import type { WireRoleRecord } from './wire';
import type { Attribution, Role, RoleDraft, RoleField } from './types';

export const CONFLICT_MESSAGE =
	"This role changed while you were editing. Here's the newer version; your changes are kept.";
export const UNCERTAIN_OUTCOME_MESSAGE = 'The outcome of this request is not confirmed yet.';
export const SESSION_CHANGED_ERROR: AppError = {
	source: 'api',
	code: 'not_authorized',
	message: 'Your session changed. Sign in and try again.'
};

export type SessionToken = {
	caller: string;
	generation: number;
};

export type SubmittedRolePayload = {
	name: string;
	key?: string;
	bodyMarkdown: string;
};

export type PendingCreate = {
	caller: string;
	attemptId?: string;
	commandId: string;
	roleId: string;
	submitted: SubmittedRolePayload;
	dispatched?: boolean;
	mayHaveCommitted?: boolean;
	committedRevision?: number;
	hasUnresolvedFollowUp?: boolean;
	conflicted?: boolean;
	detailsEvicted?: boolean;
	resolved?: Role;
};

export type PendingUpdate = {
	caller: string;
	attemptId?: string;
	commandId: string;
	submitted: SubmittedRolePayload;
	dispatched?: boolean;
	mayHaveCommitted?: boolean;
	committedRevision?: number;
	hasUnresolvedFollowUp?: boolean;
	conflicted?: boolean;
	detailsEvicted?: boolean;
	resolved?: Role;
};

export function samePayload(a: SubmittedRolePayload, b: SubmittedRolePayload): boolean {
	return (
		a.name === b.name &&
		(a.key ?? undefined) === (b.key ?? undefined) &&
		a.bodyMarkdown === b.bodyMarkdown
	);
}

export function toSubmittedPayload(draft: RoleDraft): SubmittedRolePayload {
	return {
		name: draft.name,
		...(draft.key ? { key: draft.key } : {}),
		bodyMarkdown: draft.bodyMarkdown
	};
}

export function mapAttribution(
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

export function mapRole(record: WireRoleRecord, membersById: ReadonlyMap<string, Member>): Role {
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

export function isActiveMember(m: Member): boolean {
	return !(m.kind === 'agent' && m.agent?.status === 'offboarded');
}

export function memberTitle(m: Member): string | undefined {
	return m.agent?.title || m.title_in_team || undefined;
}

export function isOwnerViewer(snapshot: OrgSnapshot, org: OrganizationView | undefined): boolean {
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

export function withField(err: AppError): AppError & { field?: RoleField } {
	const field = fieldForRoleError(err);
	return field ? { ...err, field } : err;
}

export function isUnknownOutcomeReply(reply: GatewayReply): boolean {
	if (reply.status === 504) return true;
	const root =
		reply.body && typeof reply.body === 'object'
			? (reply.body as Record<string, unknown>)
			: undefined;
	return root?.outcome === 'unknown';
}

export function isPreCommitValidationRefusal(code: string | undefined): boolean {
	return (
		code === 'too_large' ||
		code === 'invalid_request' ||
		code === 'invalid_name' ||
		code === 'already_exists' ||
		code === 'not_authorized'
	);
}

export function buildFallbackRole(
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

export function isTerminalUpdateRefusal(code: string | undefined): boolean {
	return code === 'not_authorized' || code === 'conflict' || code === 'not_found';
}

export function shouldPreserveAttempt(reply: GatewayReply, mayHaveCommitted: boolean): boolean {
	if (reply.status === 401) return mayHaveCommitted;
	return isUnknownOutcomeReply(reply) || reply.status >= 500 || reply.status === 429;
}

export function toNotDispatchedAppError(err: NotDispatchedError): AppError {
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

export function deleteIfCurrent<T>(
	map: Map<string, T>,
	key: string | undefined,
	expected: T
): void {
	if (key && map.get(key) === expected) map.delete(key);
}

export function canReuseCommitted(
	existing:
		| {
				submitted: SubmittedRolePayload;
				committedRevision?: number;
				hasUnresolvedFollowUp?: boolean;
				conflicted?: boolean;
		  }
		| undefined,
	desired: SubmittedRolePayload
): boolean {
	return Boolean(
		existing &&
		existing.committedRevision !== undefined &&
		!existing.hasUnresolvedFollowUp &&
		!existing.conflicted &&
		samePayload(existing.submitted, desired)
	);
}
