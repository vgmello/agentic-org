import { describe, expect, it, vi } from 'vitest';
import type { GatewayHttpTransport, GatewayReply } from '$lib/platform/http/gateway-errors';
import { createHttpExternalAuthApi, externalAuthApi } from './external-auth-http';
import {
	codePrefixMismatch,
	familiarityMeta,
	formatApprovalCodeAssist,
	formatExternalAuthErrorMessage,
	formatHandle,
	formatRetrySeconds,
	formatTimeRemaining,
	isExpiringSoon,
	normalizeHandle,
	validateApprovalCode
} from './external-auth-present';

const ORG = '0193a5c0-0000-7000-8000-000000000001';
const MEMBER = '0193a5c0-0000-7000-8000-0000000000aa';
const CMD_ID = '0193a5c0-0000-7000-8000-0000000000ff';

function ok(result: unknown): GatewayReply {
	return { status: 200, body: { result } };
}

describe('createHttpExternalAuthApi (live BYOA wire contract)', () => {
	it('decodes pending requests, masks unsafe prefixes, deduplicates by request_id, and forwards query + signal', async () => {
		const read = vi.fn().mockResolvedValue(
			ok({
				pending: [
					{
						request_id: 'req-1',
						employee_id: MEMBER,
						handle: '@kai',
						office_id: 'off-hq',
						office_name: 'HQ Mac',
						hostname: 'macbook-pro.local',
						workspace_path: '/Users/dev/project',
						workspace_label: 'macbook-pro:~/project',
						pubkey_fingerprint: 'a1b2c3d4e5f60718',
						managed_turn_active: true,
						requested_at: '2026-11-27T10:00:00Z',
						expires_at: '2026-11-27T10:10:00Z',
						code_prefix: 'k7p4-LEAK',
						workspace_familiarity: 'returning_new_key',
						last_approved_by: 'Alex Rivera',
						last_approved_at: '2026-11-20T09:00:00Z'
					},
					{
						request_id: 'req-1',
						employee_id: MEMBER,
						office_id: 'off-hq',
						office_name: 'HQ Mac',
						workspace_label: 'dup',
						pubkey_fingerprint: 'deadbeef00000000',
						managed_turn_active: false,
						requested_at: '2026-11-27T10:00:00Z',
						expires_at: '2026-11-27T10:10:00Z',
						code_prefix: 'ABCD-••••',
						workspace_familiarity: 'first_time'
					}
				]
			})
		);
		const http: GatewayHttpTransport = { read, submit: vi.fn() };
		const api = createHttpExternalAuthApi({ http });
		const ac = new AbortController();

		const pending = await api.pending(ORG, { memberId: MEMBER, signal: ac.signal });

		expect(read).toHaveBeenCalledWith(
			`/api/v1/organizations/${ORG}/external-auth/pending?member_id=${MEMBER}`,
			ac.signal
		);
		expect(pending).toHaveLength(1);
		expect(pending[0]).toMatchObject({
			request_id: 'req-1',
			handle: 'kai',
			code_prefix: 'K7P4-••••',
			workspace_familiarity: 'returning_new_key',
			managed_turn_active: true,
			last_approved_by: 'Alex Rivera'
		});
	});

	it('decodes memberSessions with active lease, pending, and deduplicated history keys', async () => {
		const read = vi.fn().mockResolvedValue(
			ok({
				employee_id: MEMBER,
				active: {
					session_id: 'sess-1',
					employee_id: MEMBER,
					handle: '@@kai',
					workspace_label: 'laptop:~/repo',
					pubkey_fingerprint: '0011223344556677',
					approved_by: 'Alex Rivera',
					approved_at: '2026-11-27T08:00:00Z',
					expires_at: '2026-11-28T08:00:00Z',
					active: true,
					took_over_managed: true
				},
				pending: [],
				history: [
					{
						at: '2026-11-27T08:00:00Z',
						event: 'approved',
						session_id: 'sess-1',
						actor_id: 'Alex Rivera',
						workspace_label: 'laptop:~/repo'
					},
					{
						at: '2026-11-27T08:00:00Z',
						event: 'approved',
						session_id: 'sess-1',
						actor_id: 'Alex Rivera',
						workspace_label: 'laptop:~/repo'
					}
				]
			})
		);
		const api = createHttpExternalAuthApi({ http: { read, submit: vi.fn() } });
		const res = await api.memberSessions(ORG, MEMBER, { limit: 500 });

		expect(read).toHaveBeenCalledWith(
			`/api/v1/organizations/${ORG}/members/${MEMBER}/external-sessions?limit=200`
		);
		expect(res.active).toMatchObject({
			session_id: 'sess-1',
			handle: 'kai',
			took_over_managed: true
		});
		expect(res.history).toHaveLength(2);
		expect(res.history[0].id).not.toBe(res.history[1].id);
	});

	it('submits exact deny_unknown_fields bodies for approve, renew (+7d), and revoke', async () => {
		const submit = vi
			.fn()
			.mockResolvedValueOnce(
				ok({
					session_id: 'sess-new',
					employee_id: MEMBER,
					workspace_label: 'laptop:~/repo',
					pubkey_fingerprint: 'aabbccdd11223344',
					approved_by: 'Alex Rivera',
					approved_at: '2026-11-27T10:00:00Z',
					expires_at: '2026-12-04T10:00:00Z',
					active: true,
					took_over_managed: true
				})
			)
			.mockResolvedValueOnce(
				ok({
					session_id: 'sess-new',
					employee_id: MEMBER,
					workspace_label: 'laptop:~/repo',
					pubkey_fingerprint: 'aabbccdd11223344',
					approved_by: 'Alex Rivera',
					approved_at: '2026-11-27T10:00:00Z',
					expires_at: '2026-12-11T10:00:00Z',
					active: true,
					took_over_managed: false
				})
			)
			.mockResolvedValueOnce(
				ok({
					employee_id: MEMBER,
					revoked: true,
					session_id: 'sess-new'
				})
			);
		const api = createHttpExternalAuthApi({
			http: { read: vi.fn(), submit },
			newCommandId: () => CMD_ID
		});
		const ac = new AbortController();

		const approved = await api.approve(
			ORG,
			{ code: 'K7P4-9M2X', ttl_seconds: 604_800, takeover: true },
			ac.signal
		);
		expect(submit).toHaveBeenNthCalledWith(
			1,
			`/api/v1/organizations/${ORG}/external-auth/approve`,
			{
				code: 'K7P4-9M2X',
				command_id: CMD_ID,
				ttl_seconds: 604_800,
				takeover: true
			},
			ac.signal
		);
		expect(approved.session_id).toBe('sess-new');

		const renewed = await api.renew(
			ORG,
			MEMBER,
			{ session_id: 'sess-new', ttl_seconds: 604_800 },
			ac.signal
		);
		expect(submit).toHaveBeenNthCalledWith(
			2,
			`/api/v1/organizations/${ORG}/members/${MEMBER}/external-sessions/renew`,
			{
				command_id: CMD_ID,
				session_id: 'sess-new',
				ttl_seconds: 604_800
			},
			ac.signal
		);
		expect(renewed.expires_at).toBe('2026-12-11T10:00:00Z');

		const revoked = await api.revoke(ORG, MEMBER, { session_id: 'sess-new' }, ac.signal);
		expect(submit).toHaveBeenNthCalledWith(
			3,
			`/api/v1/organizations/${ORG}/members/${MEMBER}/external-sessions/revoke`,
			{
				command_id: CMD_ID,
				session_id: 'sess-new'
			},
			ac.signal
		);
		expect(revoked).toEqual({ employee_id: MEMBER, revoked: true, session_id: 'sess-new' });
	});

	it('decodes 429 rate_limited + retry_after_ms, 409 conflict, 410 expired, 403 owner_protected, and fails closed when signed out', async () => {
		const submit = vi
			.fn()
			.mockResolvedValueOnce({
				status: 429,
				body: {
					error: {
						code: 'rate_limited',
						message: 'Too many attempts.',
						retry_after_ms: 42_100
					}
				}
			})
			.mockResolvedValueOnce({
				status: 409,
				body: {
					error: {
						code: 'conflict',
						message: 'Live managed turn is active.'
					}
				}
			})
			.mockResolvedValueOnce({
				status: 410,
				body: {
					error: {
						code: 'expired',
						message: 'Approval code has expired.'
					}
				}
			});
		const read = vi.fn().mockResolvedValueOnce({
			status: 403,
			body: { error: { code: 'owner_protected', message: 'Not authorized.' } }
		});
		const api = createHttpExternalAuthApi({ http: { read, submit } });

		await expect(api.approve(ORG, { code: 'K7P4-9M2X' })).rejects.toMatchObject({
			source: 'api',
			code: 'rate_limited',
			status: 429,
			retry_after_ms: 42_100
		});
		await expect(api.approve(ORG, { code: 'K7P4-9M2X' })).rejects.toMatchObject({
			source: 'api',
			code: 'conflict',
			status: 409
		});
		await expect(api.approve(ORG, { code: 'K7P4-9M2X' })).rejects.toMatchObject({
			source: 'api',
			code: 'expired',
			status: 410
		});
		await expect(api.memberSessions(ORG, MEMBER)).rejects.toMatchObject({
			source: 'api',
			code: 'owner_protected',
			status: 403
		});

		await expect(externalAuthApi.pending(ORG)).rejects.toMatchObject({
			source: 'api',
			code: 'unavailable'
		});
		await expect(externalAuthApi.memberSessions(ORG, MEMBER)).rejects.toMatchObject({
			source: 'api',
			code: 'unavailable'
		});
		await expect(externalAuthApi.approve(ORG, { code: 'K7P4-9M2X' })).rejects.toMatchObject({
			source: 'api',
			code: 'unavailable'
		});
		await expect(externalAuthApi.renew(ORG, MEMBER)).rejects.toMatchObject({
			source: 'api',
			code: 'unavailable'
		});
		await expect(externalAuthApi.revoke(ORG, MEMBER)).rejects.toMatchObject({
			source: 'api',
			code: 'unavailable'
		});
	});
});

describe('external-auth-present helpers', () => {
	it('formats & validates Crockford codes, normalizes @handles, and checks prefix mismatch', () => {
		expect(formatApprovalCodeAssist('k7p4 9m2x')).toBe('K7P4-9M2X');
		expect(validateApprovalCode('o1l2-3456')).toEqual({ ok: true, code: '0112-3456' });
		expect(validateApprovalCode('U123-4567').ok).toBe(false);
		expect(validateApprovalCode('K7P4').ok).toBe(false);
		expect(codePrefixMismatch('K7P4-9M2X', 'K7P4-••••')).toBe(false);
		expect(codePrefixMismatch('ABCD-9M2X', 'K7P4-••••')).toBe(true);
		expect(normalizeHandle('@@kai')).toBe('kai');
		expect(formatHandle('@kai')).toBe('@kai');
		expect(formatHandle('   ')).toBeUndefined();
	});

	it('classifies familiarity badges, expiring-soon (<24h), countdowns, and 429 retry seconds', () => {
		expect(familiarityMeta('first_time').label).toBe('First-time workspace');
		expect(familiarityMeta('returning_new_key').label).toBe('Returning workspace (new key)');
		expect(familiarityMeta('returning_known_key').label).toBe('Returning workspace (known key)');

		const now = Date.parse('2026-11-27T12:00:00Z');
		expect(isExpiringSoon('2026-11-28T06:00:00Z', now)).toBe(true);
		expect(isExpiringSoon('2026-11-29T12:00:00Z', now)).toBe(false);
		expect(isExpiringSoon('2026-11-27T11:00:00Z', now)).toBe(false);
		expect(formatTimeRemaining('2026-11-27T12:09:30Z', now)).toBe('9 min left');
		expect(formatTimeRemaining('2026-11-29T15:00:00Z', now)).toBe('2 d 3 h left');
		expect(formatTimeRemaining('2026-11-27T11:59:00Z', now)).toBe('Expired');

		expect(formatRetrySeconds(42_100)).toBe(43);
		expect(
			formatExternalAuthErrorMessage({
				source: 'api',
				code: 'rate_limited',
				message: 'Too many attempts.',
				retry_after_ms: 42_100
			})
		).toBe('Too many attempts. Try again in 43 s.');
	});
});
