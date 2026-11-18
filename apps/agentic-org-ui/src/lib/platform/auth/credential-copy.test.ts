import { describe, expect, it } from 'vitest';
import { NotDispatchedError } from '../http/errors';
import { WireDecodeError } from '../http/wire';
import {
	credentialCopy,
	describeCredentialFailure,
	rateLimitExpired,
	HQ_ERROR_COPY,
	LINK_USED_COPY,
	MISMATCH_COPY,
	OUT_OF_DATE_COPY,
	rateLimitCopy,
	UNREACHABLE_COPY
} from './credential-copy';
import { UpdateRequiredError } from './session-controller';
import { SessionRequestError } from './session-gateway';

// The table in the A3 plan §Credential rules (SIGN-21 / UX §4a), row by row.
describe('B6 credential copy table', () => {
	it.each([
		['login', 'required', 'sign-in', 'Enter your username.'],
		['login', 'required', 'register', 'Enter a username.'],
		['password', 'required', 'sign-in', 'Enter your password.'],
		['login', 'format', 'register', 'Use 1 to 64 characters, without spaces at the start or end.'],
		['login', 'taken', 'register', 'That username is taken. Choose another.'],
		['password', 'too_short', 'register', 'Use at least 12 characters.'],
		['password', 'mismatch', 'register', "The passwords don't match. Enter them again."]
	] as const)('%s/%s (%s)', (field, rule, context, copy) => {
		expect(credentialCopy(field, rule, context)).toBe(copy);
	});
	it('SIGN-4 copy is fixed', () => {
		expect(MISMATCH_COPY).toBe("That username and password don't match. Try again.");
	});
	it.each([
		[undefined, 'Too many sign-in attempts. Wait a few minutes, then try again.'],
		[0, 'Too many sign-in attempts. Wait a few minutes, then try again.'],
		[1, 'Too many sign-in attempts. Try again in 1 minute.'],
		[60_000, 'Too many sign-in attempts. Try again in 1 minute.'],
		[60_001, 'Too many sign-in attempts. Try again in 2 minutes.'],
		[900_000, 'Too many sign-in attempts. Try again in 15 minutes.']
	])('SIGN-6 retry %s ms', (ms, copy) => {
		expect(rateLimitCopy(ms)).toBe(copy);
	});
});

const refusal = (status: number, error: Record<string, unknown>) =>
	new SessionRequestError(status, { code: 'x', message: 'server words', ...error } as never);

describe('B6 failure mapping', () => {
	it('maps every refusal class to its page state without server text', () => {
		expect(describeCredentialFailure(refusal(401, { code: 'not_authorized' }), 0)).toEqual({ kind: 'mismatch', message: MISMATCH_COPY });
		expect(describeCredentialFailure(refusal(429, { code: 'rate_limited', retry_after_ms: 90_000 }), 1_000)).toEqual({
			kind: 'rate_limited', message: 'Too many sign-in attempts. Try again in 2 minutes.', retryUntilMs: 91_000
		});
		expect(describeCredentialFailure(refusal(400, { code: 'invalid_request', field: 'login', rule: 'taken' }), 0, 'register')).toMatchObject({
			kind: 'field', field: 'login', rule: 'taken', message: 'That username is taken. Choose another.'
		});
		expect(describeCredentialFailure(refusal(404, { code: 'not_found' }), 0, 'register')).toEqual({ kind: 'link_used', message: LINK_USED_COPY });
		expect(describeCredentialFailure(refusal(503, { code: 'unavailable' }), 0)).toEqual({ kind: 'unreachable', message: UNREACHABLE_COPY });
		expect(describeCredentialFailure(refusal(500, { code: 'internal' }), 0)).toEqual({ kind: 'hq_error', message: HQ_ERROR_COPY });
		expect(describeCredentialFailure(new WireDecodeError(), 0).kind).toBe('hq_error');
		expect(describeCredentialFailure(new NotDispatchedError(), 0).kind).toBe('hq_error');
		expect(describeCredentialFailure(new TypeError('Failed to fetch'), 0)).toEqual({ kind: 'unreachable', message: UNREACHABLE_COPY });
		expect(describeCredentialFailure(new UpdateRequiredError(), 0)).toEqual({ kind: 'out_of_date', message: OUT_OF_DATE_COPY });
		for (const status of [400, 401, 404, 429, 500, 503])
			expect(JSON.stringify(describeCredentialFailure(refusal(status, {}), 0))).not.toContain('server words');
	});
	it('SIGN-7 unreachable copy names the status command', () => {
		expect(UNREACHABLE_COPY).toContain('agentic-org service status --name <instance>');
	});
});

describe('B6 rate-limit expiry', () => {
	it('ends a rate-limit failure exactly when its retry time passes', () => {
		const failure = describeCredentialFailure(refusal(429, { code: 'rate_limited', retry_after_ms: 60_000 }), 1_000);
		expect(failure).toMatchObject({ kind: 'rate_limited', retryUntilMs: 61_000 });
		expect(rateLimitExpired(failure, 60_999)).toBe(false);
		expect(rateLimitExpired(failure, 61_000)).toBe(true);
	});
	it('never ends a failure without a retry time or of another kind', () => {
		expect(rateLimitExpired({ kind: 'rate_limited', message: 'x' }, Number.MAX_SAFE_INTEGER)).toBe(false);
		expect(rateLimitExpired({ kind: 'mismatch', message: 'x' }, Number.MAX_SAFE_INTEGER)).toBe(false);
		expect(rateLimitExpired(undefined, 0)).toBe(false);
	});
});
