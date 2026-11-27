import { describe, expect, it, vi } from 'vitest';
import type { GatewayHttpTransport, GatewayReply } from '$lib/platform/http/gateway-errors';
import {
	buildMemberNameMap,
	canUseExternalSignIn,
	formatApprovedSessionToast,
	formatLastApproved,
	resolveExternalAuthActor,
	type ExternalSessionView,
	type PendingExternalAuthView
} from '$lib/platform/workspace/external-auth';
import type { Member } from '$lib/proto';
import { createHttpExternalAuthApi, externalAuthApi } from './external-auth-http';
import {
	codePrefixMismatch,
	extractCodeLookupPrefix,
	familiarityMeta,
	formatApprovalCodeAssist,
	formatExternalAuthErrorMessage,
	formatExternalAuthTime,
	formatHandle,
	formatRetrySeconds,
	formatTimeRemaining,
	isExpiringSoon,
	normalizeHandle,
	resolvePendingApprovalTarget,
	validateApprovalCode
} from './external-auth-present';
import { createApprovalSubmissionGuard } from './features/external-auth/approve-guard';

const ORG = '0193a5c0-0000-7000-8000-000000000001';
const APPROVER = '0193a5c0-0000-7000-8000-000000000002';
const MEMBER = '0193a5c0-0000-7000-8000-0000000000aa';
const REQ_1 = '0193a5c0-0000-7000-8000-000000000201';
const OFFICE_HQ = '0193a5c0-0000-7000-8000-000000000301';
const SESS_1 = '0193a5c0-0000-7000-8000-000000000401';
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
						request_id: REQ_1,
						employee_id: MEMBER,
						handle: '@kai',
						office_id: OFFICE_HQ,
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
						last_approved_by: APPROVER,
						last_approved_at: '2026-11-20T09:00:00Z'
					},
					{
						request_id: REQ_1,
						employee_id: MEMBER,
						office_id: OFFICE_HQ,
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
			request_id: REQ_1,
			handle: 'kai',
			code_prefix: 'K7P4-••••',
			workspace_familiarity: 'returning_new_key',
			managed_turn_active: true,
			last_approved_by: APPROVER
		});
	});

	it('decodes memberSessions with active lease, pending, and deduplicated history keys', async () => {
		const read = vi.fn().mockResolvedValue(
			ok({
				employee_id: MEMBER,
				active: {
					session_id: SESS_1,
					employee_id: MEMBER,
					handle: '@@kai',
					workspace_label: 'laptop:~/repo',
					pubkey_fingerprint: '0011223344556677',
					approved_by: APPROVER,
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
						session_id: SESS_1,
						actor_id: APPROVER,
						workspace_label: 'laptop:~/repo'
					},
					{
						at: '2026-11-27T08:00:00Z',
						event: 'approved',
						session_id: SESS_1,
						actor_id: APPROVER,
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
			session_id: SESS_1,
			handle: 'kai',
			took_over_managed: true
		});
		expect(res.history).toHaveLength(2);
		expect(res.history[0].id).not.toBe(res.history[1].id);
	});

	it('submits exact deny_unknown_fields bodies for approve, renew (+7d), and revoke', async () => {
		const SESS_NEW = '0193a5c0-0000-7000-8000-000000000402';
		const submit = vi
			.fn()
			.mockResolvedValueOnce(
				ok({
					session_id: SESS_NEW,
					employee_id: MEMBER,
					workspace_label: 'laptop:~/repo',
					pubkey_fingerprint: 'aabbccdd11223344',
					approved_by: APPROVER,
					approved_at: '2026-11-27T10:00:00Z',
					expires_at: '2026-12-04T10:00:00Z',
					active: true,
					took_over_managed: true
				})
			)
			.mockResolvedValueOnce(
				ok({
					session_id: SESS_NEW,
					employee_id: MEMBER,
					workspace_label: 'laptop:~/repo',
					pubkey_fingerprint: 'aabbccdd11223344',
					approved_by: APPROVER,
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
					session_id: SESS_NEW
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
		expect(approved.session_id).toBe(SESS_NEW);

		const renewed = await api.renew(
			ORG,
			MEMBER,
			{ session_id: SESS_NEW, ttl_seconds: 604_800 },
			ac.signal
		);
		expect(submit).toHaveBeenNthCalledWith(
			2,
			`/api/v1/organizations/${ORG}/members/${MEMBER}/external-sessions/renew`,
			{
				command_id: CMD_ID,
				session_id: SESS_NEW,
				ttl_seconds: 604_800
			},
			ac.signal
		);
		expect(renewed.expires_at).toBe('2026-12-11T10:00:00Z');

		const revoked = await api.revoke(ORG, MEMBER, { session_id: SESS_NEW }, ac.signal);
		expect(submit).toHaveBeenNthCalledWith(
			3,
			`/api/v1/organizations/${ORG}/members/${MEMBER}/external-sessions/revoke`,
			{
				command_id: CMD_ID,
				session_id: SESS_NEW
			},
			ac.signal
		);
		expect(revoked).toEqual({ employee_id: MEMBER, revoked: true, session_id: SESS_NEW });

		// L8: Omitted `revoked` field fails closed to `false` instead of assuming `true`.
		const submitOmitted = vi.fn().mockResolvedValueOnce(ok({ employee_id: MEMBER }));
		const apiOmitted = createHttpExternalAuthApi({
			http: { read: vi.fn(), submit: submitOmitted }
		});
		await expect(apiOmitted.revoke(ORG, MEMBER)).resolves.toEqual({
			employee_id: MEMBER,
			revoked: false
		});
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
	it('formats & validates Crockford codes (including pasted CLI commands), normalizes @handles, and formats UTC times deterministically', () => {
		expect(formatApprovalCodeAssist('k7p4 9m2x')).toBe('K7P4-9M2X');
		expect(formatApprovalCodeAssist('   k7p4  -  9m2x   ')).toBe('K7P4-9M2X');
		// L10: Extracts the last Crockford approval token when pasting a full CLI command line.
		expect(formatApprovalCodeAssist('agentic-org agent approve K7P4-9M2X')).toBe('K7P4-9M2X');
		expect(formatApprovalCodeAssist('agentic-org agent approve k7p4 9m2x')).toBe('K7P4-9M2X');
		expect(formatApprovalCodeAssist('agentic-org --org 0193a5c0 agent approve K7P4-9M2X')).toBe(
			'K7P4-9M2X'
		);
		expect(validateApprovalCode('  o1l2   3456  ')).toEqual({ ok: true, code: '0112-3456' });
		expect(validateApprovalCode('o1l2-3456')).toEqual({ ok: true, code: '0112-3456' });
		expect(validateApprovalCode('U123-4567').ok).toBe(false);
		expect(validateApprovalCode('K7P4').ok).toBe(false);
		expect(extractCodeLookupPrefix('o1l2-9m2x')).toBe('0112');
		expect(extractCodeLookupPrefix('u1234567')).toBeUndefined();
		expect(codePrefixMismatch('K7P4-9M2X', 'K7P4-••••')).toBe(false);
		expect(codePrefixMismatch('ABCD-9M2X', 'K7P4-••••')).toBe(true);
		expect(normalizeHandle('@@kai')).toBe('kai');
		expect(formatHandle('@kai')).toBe('@kai');
		expect(formatHandle('   ')).toBeUndefined();
		// M4: Deterministic 24-hour formatting under explicit UTC timezone + invalid date guard.
		expect(formatExternalAuthTime('not-a-timestamp', 'UTC')).toBe('Unknown time');
		expect(formatExternalAuthTime('2026-11-27T14:05:00Z', 'UTC')).toContain('14:05');
	});

	it('resolves pending requests strictly for the viewed agent, blocks cross-agent mismatches, and builds session toasts (M1)', () => {
		const KAI_ID = '0193a5c0-0000-7000-8000-000000000010';
		const ADA_ID = '0193a5c0-0000-7000-8000-000000000011';
		const ALEX_ID = '0193a5c0-0000-7000-8000-000000000002';
		const basePending = (overrides: Partial<PendingExternalAuthView>): PendingExternalAuthView => ({
			request_id: '0193a5c0-0000-7000-8000-000000000201',
			employee_id: KAI_ID,
			handle: 'kai',
			office_id: '0193a5c0-0000-7000-8000-000000000301',
			office_name: 'HQ Mac',
			workspace_label: 'studio:~/app',
			pubkey_fingerprint: '1111222233334444',
			managed_turn_active: false,
			requested_at: '2026-11-27T10:00:00Z',
			expires_at: '2026-11-27T10:10:00Z',
			code_prefix: 'ABCD-••••',
			workspace_familiarity: 'first_time',
			...overrides
		});
		const req1 = basePending({
			request_id: '0193a5c0-0000-7000-8000-000000000201',
			code_prefix: 'ABCD-••••'
		});
		const req2 = basePending({
			request_id: '0193a5c0-0000-7000-8000-000000000202',
			code_prefix: 'K7P4-••••',
			managed_turn_active: true,
			workspace_familiarity: 'returning_known_key'
		});
		const otherAgentReq = basePending({
			request_id: '0193a5c0-0000-7000-8000-000000000203',
			employee_id: ADA_ID,
			handle: 'ada',
			code_prefix: 'EFGH-••••'
		});

		// Enter-code mode starts without preselecting pendingList[0], then matches dynamically on 4-char prefix.
		expect(
			resolvePendingApprovalTarget({
				rawCode: 'K7',
				candidates: [req1, req2, otherAgentReq],
				expectedEmployeeId: KAI_ID
			})
		).toEqual({ status: 'awaiting_prefix', display: undefined, matched: undefined });

		const matched = resolvePendingApprovalTarget({
			rawCode: 'k7p4-9m2x',
			candidates: [req1, req2, otherAgentReq],
			expectedEmployeeId: KAI_ID
		});
		expect(matched).toEqual({ status: 'matched', display: req2, matched: req2 });

		// Cross-agent prefix (`EFGH` belongs to Ada) is blocked when viewing Kai.
		expect(
			resolvePendingApprovalTarget({
				rawCode: 'EFGH-1234',
				candidates: [req1, req2, otherAgentReq],
				expectedEmployeeId: KAI_ID
			}).status
		).toBe('mismatch');

		// Explicit pending card blocks mismatched prefix even if another request exists for the agent.
		expect(
			resolvePendingApprovalTarget({
				rawCode: 'K7P4-9M2X',
				explicitPending: req1,
				candidates: [req1, req2],
				expectedEmployeeId: KAI_ID
			}).status
		).toBe('mismatch');

		// Empty candidate list blocks blind approval.
		expect(
			resolvePendingApprovalTarget({
				rawCode: 'K7P4-9M2X',
				candidates: [],
				expectedEmployeeId: KAI_ID
			}).status
		).toBe('no_pending');

		// Toast uses returned session's employee_id and warns if it disagrees with expectedEmployeeId.
		const membersById = buildMemberNameMap([
			{ id: KAI_ID, name: 'Kai Lindqvist', handle: 'kai' },
			{ id: ADA_ID, name: 'Ada Lovelace', handle: 'ada' }
		]);
		const session: ExternalSessionView = {
			session_id: '0193a5c0-0000-7000-8000-000000000401',
			employee_id: ADA_ID,
			handle: 'ada',
			workspace_label: 'studio:~/ada-ws',
			pubkey_fingerprint: '9999aaaabbbbcccc',
			approved_by: ALEX_ID,
			approved_at: '2026-11-27T10:02:00Z',
			expires_at: '2026-11-28T10:02:00Z',
			active: true,
			took_over_managed: false
		};
		expect(
			formatApprovedSessionToast(session, {
				expectedEmployeeId: ADA_ID,
				expectedAgentName: 'Ada Lovelace',
				membersById
			})
		).toMatchObject({
			kind: 'success',
			title: 'Approved external sign-in for Ada Lovelace.'
		});
		expect(
			formatApprovedSessionToast(session, {
				expectedEmployeeId: KAI_ID,
				expectedAgentName: 'Kai Lindqvist',
				membersById
			})
		).toMatchObject({
			kind: 'warning',
			title: 'Approved external sign-in for Ada Lovelace (different agent than Kai Lindqvist).'
		});
	});

	it('blocks built-in HR and Tess agents from external sign-in (M2) and resolves approver UUIDs safely (M3)', () => {
		const HR_ID = '0193a5c0-0000-7000-8000-000000000020';
		const TESS_ID = '0193a5c0-0000-7000-8000-000000000021';
		const KAI_ID = '0193a5c0-0000-7000-8000-000000000010';
		const ALEX_ID = '0193a5c0-0000-7000-8000-000000000002';
		const UNKNOWN_ID = '0193a5c0-0000-7000-8000-000000000099';

		const hrMember: Member = {
			id: HR_ID,
			kind: 'agent',
			name: 'HR',
			handle: 'hr',
			builtin_key: 'hr',
			version: 1
		};
		const tessMember: Member = {
			id: TESS_ID,
			kind: 'agent',
			name: 'Tess',
			handle: 'tess',
			builtin_key: 'chief_of_staff',
			version: 1
		};
		const customAgent: Member = {
			id: KAI_ID,
			kind: 'agent',
			name: 'Kai Lindqvist',
			handle: 'kai',
			version: 1
		};

		const tessLegacyRole: Member = {
			id: TESS_ID,
			kind: 'agent',
			name: 'Tess',
			agent: {
				role: 'chief_of_staff',
				title: 'Chief of Staff',
				messaging_name: 'tess',
				runtime: { other: 'builtin' },
				status: 'active'
			},
			version: 1
		};
		expect(canUseExternalSignIn(hrMember)).toBe(false);
		expect(canUseExternalSignIn(tessMember)).toBe(false);
		expect(canUseExternalSignIn(tessLegacyRole)).toBe(false);
		expect(canUseExternalSignIn(undefined, { kind: 'agent', handle: 'tess-cos' })).toBe(false);
		expect(canUseExternalSignIn(undefined, { kind: 'agent', handle: 'hr' })).toBe(false);
		expect(canUseExternalSignIn(customAgent)).toBe(true);

		const membersById = buildMemberNameMap([{ id: ALEX_ID, name: 'Alex Rivera', handle: 'alex' }]);
		expect(resolveExternalAuthActor(ALEX_ID, membersById)).toBe('Alex Rivera');
		expect(resolveExternalAuthActor(UNKNOWN_ID, membersById)).toBe('Unknown member');
		expect(resolveExternalAuthActor(undefined, membersById)).toBeUndefined();
		const lastKnown = formatLastApproved(ALEX_ID, '2026-11-20T09:00:00Z', membersById);
		expect(lastKnown).toContain('Last approved by Alex Rivera');
		expect(lastKnown).not.toContain(ALEX_ID);
		const lastUnknown = formatLastApproved(UNKNOWN_ID, '2026-11-20T09:00:00Z', membersById);
		expect(lastUnknown).toContain('Last approved by Unknown member');
		expect(lastUnknown).not.toContain(UNKNOWN_ID);
	});

	it('aborts and rejects stale dialog approval submissions after close, request switch, or unmount (M4)', () => {
		const guard = createApprovalSubmissionGuard();
		const org1 = '0193a5c0-0000-7000-8000-000000000001';
		const req1 = '0193a5c0-0000-7000-8000-000000000201';
		const req2 = '0193a5c0-0000-7000-8000-000000000202';

		// Close (`cancel`) aborts in-flight submission and marks it stale.
		const t1 = guard.begin(org1, req1);
		expect(guard.isCurrent(t1, org1, req1)).toBe(true);
		guard.cancel();
		expect(t1.abort.signal.aborted).toBe(true);
		expect(guard.isCurrent(t1, org1, req1)).toBe(false);

		// Request switch marks in-flight submission stale.
		const t2 = guard.begin(org1, req1);
		expect(guard.isCurrent(t2, org1, req2)).toBe(false);

		// Unmount (`destroy`) aborts in-flight submission and marks all subsequent checks stale.
		guard.destroy();
		expect(t2.abort.signal.aborted).toBe(true);
		expect(guard.isCurrent(t2, org1, req1)).toBe(false);
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
