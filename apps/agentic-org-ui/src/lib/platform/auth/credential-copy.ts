import { NotDispatchedError } from '../http/errors';
import { WireDecodeError } from '../http/wire';
import type { CredentialField, CredentialRule } from './credential-rules';
import { UpdateRequiredError } from './session-controller';
import { SessionRequestError } from './session-gateway';

/** UI copy for credential refusals (plan §Credential rules; UX §4a.2, §4a.3, SIGN-3/4/6/7/21). */
export type CopyContext = 'sign-in' | 'register';

export function credentialCopy(field: CredentialField, rule: CredentialRule, context: CopyContext = 'sign-in'): string {
	if (field === 'login') {
		if (rule === 'required') return context === 'register' ? 'Enter a username.' : 'Enter your username.';
		if (rule === 'taken') return 'That username is taken. Choose another.';
		return 'Use 1 to 64 characters, without spaces at the start or end.';
	}
	if (rule === 'required') return 'Enter your password.';
	if (rule === 'too_short') return 'Use at least 12 characters.';
	if (rule === 'too_long') return 'That password is too long. Use at most 1024 bytes.';
	if (rule === 'mismatch') return "The passwords don't match. Enter them again.";
	return 'Use at least 12 characters.';
}

export const MISMATCH_COPY = "That username and password don't match. Try again.";
export const UNREACHABLE_COPY =
	"Can't reach HQ. Check that the instance is running on the HQ machine (agentic-org service status --name <instance>), then try again.";
export const HQ_ERROR_COPY =
	"HQ couldn't sign you in because of a problem on the HQ machine. Try again. If it keeps happening, run agentic-org doctor --name <instance> there.";
export const OUT_OF_DATE_COPY = 'This page is out of date. Reload to get the current version.';
export const LINK_USED_COPY = 'This link has expired or was already used.';

/** Whole minutes, rounded up, "1 minute" when one is left (§4a.3). */
export function rateLimitCopy(remainingMs: number | undefined): string {
	if (remainingMs === undefined || !Number.isFinite(remainingMs) || remainingMs <= 0)
		return 'Too many sign-in attempts. Wait a few minutes, then try again.';
	const minutes = Math.max(1, Math.ceil(remainingMs / 60_000));
	return `Too many sign-in attempts. Try again in ${minutes} ${minutes === 1 ? 'minute' : 'minutes'}.`;
}

export type CredentialFailure =
	| { kind: 'field'; field: CredentialField; rule: CredentialRule; message: string }
	| { kind: 'mismatch'; message: string }
	| { kind: 'rate_limited'; message: string; retryUntilMs?: number }
	| { kind: 'link_used'; message: string }
	| { kind: 'unreachable'; message: string }
	| { kind: 'hq_error'; message: string }
	| { kind: 'out_of_date'; message: string };

/**
 * SIGN-6: a rate-limit refusal with a retry time ends when that time passes, so the page clears the
 * message together with re-enabling the button instead of showing a countdown that contradicts it.
 */
export function rateLimitExpired(failure: CredentialFailure | undefined, nowMs: number): boolean {
	return failure?.kind === 'rate_limited' && failure.retryUntilMs !== undefined && nowMs >= failure.retryUntilMs;
}

/** Maps any sign-in/registration failure to its page state. Server message text is never shown. */
export function describeCredentialFailure(error: unknown, nowMs: number, context: CopyContext = 'sign-in'): CredentialFailure {
	if (error instanceof UpdateRequiredError) return { kind: 'out_of_date', message: OUT_OF_DATE_COPY };
	if (error instanceof SessionRequestError) {
		if (error.status === 429) {
			const retryUntilMs = error.retryAfterMs === undefined ? undefined : nowMs + error.retryAfterMs;
			return { kind: 'rate_limited', message: rateLimitCopy(error.retryAfterMs), ...(retryUntilMs === undefined ? {} : { retryUntilMs }) };
		}
		if (error.status === 400 && error.field && error.rule && error.field !== 'unknown' && error.rule !== 'unknown')
			return { kind: 'field', field: error.field, rule: error.rule, message: credentialCopy(error.field, error.rule, context) };
		if (error.status === 401 && context === 'sign-in') return { kind: 'mismatch', message: MISMATCH_COPY };
		if (error.status === 404 && context === 'register') return { kind: 'link_used', message: LINK_USED_COPY };
		// The web process answers 503 when it cannot reach the host: HQ is not reachable.
		if (error.status === 503) return { kind: 'unreachable', message: UNREACHABLE_COPY };
		return { kind: 'hq_error', message: HQ_ERROR_COPY };
	}
	if (error instanceof WireDecodeError || error instanceof NotDispatchedError) return { kind: 'hq_error', message: HQ_ERROR_COPY };
	// fetch rejections, timeouts and dropped connections: nothing usable came back.
	return { kind: 'unreachable', message: UNREACHABLE_COPY };
}
