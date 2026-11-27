import { describe, expect, it, vi } from 'vitest';
import { ExternalAuthController } from './external-auth-controller.svelte';
import type {
	ExternalAuthApi,
	ExternalAuthError,
	ExternalSessionView,
	MemberExternalSessionsView
} from './external-auth-types';

const ORG_A = '0193a5c0-0000-7000-8000-000000000001';
const MEM_A = '0193a5c0-0000-7000-8000-000000000010';
const MEM_B = '0193a5c0-0000-7000-8000-000000000011';
const APPROVER_ID = '0193a5c0-0000-7000-8000-000000000002';

function sampleSessions(memberId: string, label: string): MemberExternalSessionsView {
	return {
		employee_id: memberId,
		active: {
			session_id: `0193a5c0-0000-7000-8000-00000000010${label.length % 10}`,
			employee_id: memberId,
			workspace_label: `ws-${label}`,
			pubkey_fingerprint: '0011223344556677',
			approved_by: APPROVER_ID,
			approved_at: '2026-11-27T08:00:00Z',
			expires_at: '2026-12-04T08:00:00Z',
			active: true,
			took_over_managed: false
		},
		pending: [],
		history: []
	};
}

function deferred<T>() {
	let resolve!: (value: T) => void;
	let reject!: (reason?: unknown) => void;
	const promise = new Promise<T>((res, rej) => {
		resolve = res;
		reject = rej;
	});
	return { promise, resolve, reject };
}

function createStubApi(overrides: Partial<ExternalAuthApi> = {}): ExternalAuthApi {
	return {
		pending: vi.fn().mockResolvedValue([]),
		memberSessions: vi.fn().mockResolvedValue(sampleSessions(MEM_A, 'default')),
		approve: vi.fn(),
		renew: vi.fn(),
		revoke: vi.fn(),
		...overrides
	};
}

describe('ExternalAuthController', () => {
	it('forwards AbortSignal, cancels superseded load(), and discards stale success and error replies', async () => {
		const first = deferred<MemberExternalSessionsView>();
		const second = deferred<MemberExternalSessionsView>();
		const third = deferred<MemberExternalSessionsView>();
		const signals: (AbortSignal | undefined)[] = [];
		const memberSessions = vi
			.fn<ExternalAuthApi['memberSessions']>()
			.mockImplementationOnce((_org, _mem, opts) => {
				signals.push(opts?.signal);
				return first.promise;
			})
			.mockImplementationOnce((_org, _mem, opts) => {
				signals.push(opts?.signal);
				return second.promise;
			})
			.mockImplementationOnce((_org, _mem, opts) => {
				signals.push(opts?.signal);
				return third.promise;
			});

		const controller = new ExternalAuthController({
			api: createStubApi({ memberSessions }),
			onSessionEnded: () => () => {}
		});

		const p1 = controller.load(ORG_A, MEM_A);
		expect(signals[0]?.aborted).toBe(false);

		const p2 = controller.load(ORG_A, MEM_B);
		expect(signals[0]?.aborted).toBe(true);
		expect(signals[1]?.aborted).toBe(false);

		second.resolve(sampleSessions(MEM_B, 'fresh'));
		await p2;
		expect(controller.status).toBe('ready');
		expect(controller.data?.employee_id).toBe(MEM_B);
		expect(controller.reloading).toBe(false);

		// Late resolution of superseded request 1 must NOT overwrite fresher MEM_B state.
		first.resolve(sampleSessions(MEM_A, 'stale'));
		await p1;
		expect(controller.data?.employee_id).toBe(MEM_B);

		// Late rejection of a superseded request must NOT flip status to error.
		const p3 = controller.load(ORG_A, MEM_B, { silent: true });
		controller.dispose();
		expect(signals[2]?.aborted).toBe(true);
		third.reject({ source: 'api', code: 'unavailable', message: 'Late failure' });
		await p3;
		expect(controller.status).toBe('ready');
		expect(controller.error).toBeUndefined();
		expect(controller.reloading).toBe(false);
	});

	it('clears state and aborts in-flight loads when onSessionEnded fires', async () => {
		let fireSessionEnded: (() => void) | undefined;
		const unsubscribe = vi.fn();
		const inflight = deferred<MemberExternalSessionsView>();
		let capturedSignal: AbortSignal | undefined;

		const api = createStubApi({
			memberSessions: vi
				.fn<ExternalAuthApi['memberSessions']>()
				.mockResolvedValueOnce(sampleSessions(MEM_A, 'initial'))
				.mockImplementationOnce((_org, _mem, opts) => {
					capturedSignal = opts?.signal;
					return inflight.promise;
				})
		});

		const controller = new ExternalAuthController({
			api,
			onSessionEnded: (listener) => {
				fireSessionEnded = listener;
				return unsubscribe;
			}
		});

		await controller.load(ORG_A, MEM_A);
		expect(controller.status).toBe('ready');
		expect(controller.data?.active?.workspace_label).toBe('ws-initial');

		const reloadPromise = controller.reload();
		expect(capturedSignal?.aborted).toBe(false);

		fireSessionEnded?.();
		expect(capturedSignal?.aborted).toBe(true);
		expect(controller.status).toBe('idle');
		expect(controller.data).toBeUndefined();
		expect(controller.error).toBeUndefined();
		expect(controller.actionError).toBeUndefined();
		expect(controller.reloading).toBe(false);

		inflight.resolve(sampleSessions(MEM_A, 'after-signout'));
		await reloadPromise;
		expect(controller.status).toBe('idle');
		expect(controller.data).toBeUndefined();

		controller.dispose();
		expect(unsubscribe).toHaveBeenCalledTimes(1);
	});

	it('maps 403 to unauthorized and surfaces 409/429 action errors while resetting busyAction in finally', async () => {
		const forbidden: ExternalAuthError = {
			source: 'api',
			code: 'not_authorized',
			status: 403,
			message: 'Human lead required.'
		};
		const conflict: ExternalAuthError = {
			source: 'api',
			code: 'conflict',
			status: 409,
			message: 'Managed turn running.'
		};
		const rateLimited: ExternalAuthError = {
			source: 'api',
			code: 'rate_limited',
			status: 429,
			message: 'Too many attempts.',
			retry_after_ms: 30_000
		};
		const renewedSession: ExternalSessionView = {
			session_id: '0193a5c0-0000-7000-8000-000000000101',
			employee_id: MEM_A,
			workspace_label: 'ws-1',
			pubkey_fingerprint: '0011223344556677',
			approved_by: APPROVER_ID,
			approved_at: '2026-11-27T08:00:00Z',
			expires_at: '2026-12-04T08:00:00Z',
			active: true,
			took_over_managed: false
		};

		const memberSessions = vi
			.fn<ExternalAuthApi['memberSessions']>()
			.mockRejectedValueOnce(forbidden)
			.mockResolvedValue(sampleSessions(MEM_A, 'ok'));
		const renew = vi
			.fn<ExternalAuthApi['renew']>()
			.mockRejectedValueOnce(conflict)
			.mockRejectedValueOnce(rateLimited)
			.mockResolvedValueOnce(renewedSession);
		const revoke = vi.fn<ExternalAuthApi['revoke']>().mockResolvedValueOnce({
			employee_id: MEM_A,
			revoked: true,
			session_id: '0193a5c0-0000-7000-8000-000000000101'
		});

		const controller = new ExternalAuthController({
			api: createStubApi({ memberSessions, renew, revoke }),
			onSessionEnded: () => () => {}
		});

		await controller.load(ORG_A, MEM_A);
		expect(controller.status).toBe('unauthorized');
		expect(controller.error?.code).toBe('not_authorized');

		await controller.load(ORG_A, MEM_A);
		expect(controller.status).toBe('ready');

		await expect(
			controller.renew(ORG_A, MEM_A, { session_id: '0193a5c0-0000-7000-8000-000000000101' })
		).rejects.toMatchObject({
			code: 'conflict',
			status: 409
		});
		expect(controller.busyAction).toBeUndefined();
		expect(controller.actionError).toMatchObject({ code: 'conflict', status: 409 });

		await expect(
			controller.renew(ORG_A, MEM_A, { session_id: '0193a5c0-0000-7000-8000-000000000101' })
		).rejects.toMatchObject({
			code: 'rate_limited',
			status: 429,
			retry_after_ms: 30_000
		});
		expect(controller.busyAction).toBeUndefined();
		expect(controller.actionError).toMatchObject({
			code: 'rate_limited',
			retry_after_ms: 30_000
		});

		const renewed = await controller.renew(ORG_A, MEM_A, {
			session_id: '0193a5c0-0000-7000-8000-000000000101',
			ttl_seconds: 604_800
		});
		expect(renewed.session_id).toBe('0193a5c0-0000-7000-8000-000000000101');
		expect(controller.busyAction).toBeUndefined();
		expect(controller.actionError).toBeUndefined();

		const revoked = await controller.revoke(ORG_A, MEM_A, {
			session_id: '0193a5c0-0000-7000-8000-000000000101'
		});
		expect(revoked.revoked).toBe(true);
		expect(controller.busyAction).toBeUndefined();
		expect(memberSessions).toHaveBeenCalledTimes(4);

		controller.dispose();
	});

	it('aborts in-flight mutations, resets busyAction, and drops late outcomes on target switch (H3)', async () => {
		const renewGate = deferred<ExternalSessionView>();
		const revokeGate = deferred<{ employee_id: string; revoked: boolean; session_id?: string }>();
		const signals: AbortSignal[] = [];

		const memberSessions = vi
			.fn<ExternalAuthApi['memberSessions']>()
			.mockImplementation(async (_orgId, memberId) => sampleSessions(memberId, `ws-${memberId}`));
		const renew = vi.fn<ExternalAuthApi['renew']>((_orgId, _memId, _input, signal) => {
			if (signal) signals.push(signal);
			return renewGate.promise;
		});
		const revoke = vi.fn<ExternalAuthApi['revoke']>((_orgId, _memId, _input, signal) => {
			if (signal) signals.push(signal);
			return revokeGate.promise;
		});

		const controller = new ExternalAuthController({
			api: createStubApi({ memberSessions, renew, revoke }),
			onSessionEnded: () => () => {}
		});

		await controller.load(ORG_A, MEM_A);
		const callsBeforeSwitch = memberSessions.mock.calls.length;

		// Start a renew on MEM_A, then switch to MEM_B before it settles.
		const renewPromise = controller.renew(ORG_A, MEM_A, { session_id: 'sess-a' });
		expect(controller.busyAction).toBe('renew');

		await controller.load(ORG_A, MEM_B);
		expect(signals[0]?.aborted).toBe(true);
		expect(controller.busyAction).toBeUndefined();
		expect(controller.data?.employee_id).toBe(MEM_B);

		// Late rejection from MEM_A must NOT pollute MEM_B's actionError.
		renewGate.reject({ source: 'api', code: 'unavailable', message: 'Late MEM_A failure' });
		await expect(renewPromise).rejects.toMatchObject({ message: 'Late MEM_A failure' });
		expect(controller.actionError).toBeUndefined();
		expect(controller.data?.employee_id).toBe(MEM_B);
		expect(memberSessions).toHaveBeenCalledTimes(callsBeforeSwitch + 1);

		// Start a revoke on MEM_B, then switch orgs before it resolves.
		const revokePromise = controller.revoke(ORG_A, MEM_B, { session_id: 'sess-b' });
		expect(controller.busyAction).toBe('revoke');

		await controller.load('0193a5c0-0000-7000-8000-000000000002', MEM_A);
		expect(signals[1]?.aborted).toBe(true);
		expect(controller.busyAction).toBeUndefined();

		const callsAfterOrgSwitch = memberSessions.mock.calls.length;
		// Late resolution from ORG_A/MEM_B must NOT trigger a reload for the old target.
		revokeGate.resolve({ employee_id: MEM_B, revoked: true, session_id: 'sess-b' });
		await expect(revokePromise).resolves.toMatchObject({ revoked: true });
		expect(memberSessions).toHaveBeenCalledTimes(callsAfterOrgSwitch);

		controller.dispose();
	});

	it('honors reload: false and recordActionError: false for modal-owned revoke (M3)', async () => {
		const memberSessions = vi
			.fn<ExternalAuthApi['memberSessions']>()
			.mockResolvedValue(sampleSessions(MEM_A, 'ws-a'));
		const revoke = vi
			.fn<ExternalAuthApi['revoke']>()
			.mockRejectedValueOnce({
				source: 'api',
				code: 'not_authorized',
				status: 403,
				message: 'Owner only.'
			})
			.mockResolvedValueOnce({ employee_id: MEM_A, revoked: true, session_id: 'sess-1' });

		const controller = new ExternalAuthController({
			api: createStubApi({ memberSessions, revoke }),
			onSessionEnded: () => () => {}
		});

		await controller.load(ORG_A, MEM_A);
		expect(memberSessions).toHaveBeenCalledTimes(1);

		await expect(
			controller.revoke(
				ORG_A,
				MEM_A,
				{ session_id: 'sess-1' },
				{ reload: false, recordActionError: false }
			)
		).rejects.toMatchObject({ code: 'not_authorized' });
		// Error stays inside the confirmation modal instead of leaking into the page banner.
		expect(controller.actionError).toBeUndefined();

		await expect(
			controller.revoke(
				ORG_A,
				MEM_A,
				{ session_id: 'sess-1' },
				{ reload: false, recordActionError: false }
			)
		).resolves.toMatchObject({ revoked: true });
		// Caller (onChanged) owns the single post-revoke refresh.
		expect(memberSessions).toHaveBeenCalledTimes(1);

		controller.dispose();
	});

	it('rethrows original AbortError DOMException on aborted renew/revoke without setting actionError', async () => {
		const abortErr = new DOMException('Aborted', 'AbortError');
		const renew = vi.fn<ExternalAuthApi['renew']>((_orgId, _memId, _input, signal) => {
			return new Promise<ExternalSessionView>((_resolve, reject) => {
				signal?.addEventListener('abort', () => reject(abortErr), { once: true });
			});
		});
		const revoke = vi.fn<ExternalAuthApi['revoke']>((_orgId, _memId, _input, signal) => {
			return new Promise<{ employee_id: string; revoked: boolean }>((_resolve, reject) => {
				signal?.addEventListener('abort', () => reject(abortErr), { once: true });
			});
		});
		let fireSessionEnded: (() => void) | undefined;
		const controller = new ExternalAuthController({
			api: createStubApi({ renew, revoke }),
			onSessionEnded: (listener) => {
				fireSessionEnded = listener;
				return () => {};
			}
		});

		await controller.load(ORG_A, MEM_A);
		const renewPromise = controller.renew(ORG_A, MEM_A, { session_id: 'sess-1' });
		fireSessionEnded?.();
		await expect(renewPromise).rejects.toBe(abortErr);
		expect(controller.actionError).toBeUndefined();
		expect(controller.busyAction).toBeUndefined();

		await controller.load(ORG_A, MEM_A);
		const revokePromise = controller.revoke(ORG_A, MEM_A, { session_id: 'sess-1' });
		controller.dispose();
		await expect(revokePromise).rejects.toBe(abortErr);
		expect(controller.actionError).toBeUndefined();
		expect(controller.busyAction).toBeUndefined();
	});
});
