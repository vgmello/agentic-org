import { readJsonObject, readResponse } from '../http/wire';
import type { AccessView, SessionGateway, SessionView } from './session-gateway';
import type { SessionRecord } from './session-record';

/** The API and this page disagree on the contract version: the page must reload (§4a.3 "Page out of date"). */
export class UpdateRequiredError extends Error {
	constructor() {
		super('Gateway update required');
		this.name = 'UpdateRequiredError';
	}
}

export type SessionState =
	| { kind: 'signed-out'; reason?: 'ended' | 'signed-out' | 'changed-account'; login?: string }
	| { kind: 'update-required' }
	| {
			kind: 'signed-in';
			session: SessionView;
			reauth?: 'session_ended' | 'unknown';
			/** Present only when local HQ serves plain HTTP without authentication (ADR-0034). */
			authless?: true;
	  };

export type ControllerGateway = Pick<
	SessionGateway,
	'version' | 'signIn' | 'register' | 'get' | 'refresh' | 'signOut'
> &
	Partial<
		Pick<
			SessionGateway,
			| 'access'
			| 'getLocal'
			| 'enableAuth'
			| 'enableTls'
			| 'decideCliApproval'
			| 'registerSignIn'
			| 'inspectLink'
			| 'setPassword'
			| 'changePassword'
		>
	>;

export const DEFAULT_ACCESS: AccessView = Object.freeze({
	auth_enabled: true,
	listen_scheme: 'unknown'
});

/**
 * Sign-out refusals that prove the host no longer holds this browser's session. Only an
 * unauthenticated 401 does: a 503 may come before dispatch, and the host deletes the stored
 * record first, so an unavailable reply leaves the session in place (ADR-0013, SIGN-29).
 */
export const SIGN_OUT_ENDED: ReadonlySet<number> = new Set([401]);

export type SessionSendRequest = { path: string; signal?: AbortSignal } & (
	{ method: 'GET' } | { method: 'POST' | 'PATCH'; body: unknown }
);

export const sameAccount = (a: SessionRecord, b: SessionRecord) =>
	a.instance_id === b.instance_id && a.account_id === b.account_id;

export const matches = (view: SessionView, record: SessionRecord) =>
	view.instance_id === record.instance_id && view.account.id === record.account_id;

export function readSessionErrorReason(body: unknown): 'session_ended' | 'unknown' | undefined {
	try {
		const parsed = readResponse(body, readJsonObject);
		if ('error' in parsed) return parsed.error.session_reason;
	} catch {
		/* Untrusted error payload. */
	}
	return undefined;
}
