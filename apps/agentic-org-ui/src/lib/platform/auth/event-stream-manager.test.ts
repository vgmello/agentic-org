import { describe, expect, it, vi } from 'vitest';
import {
	EventStreamError,
	type createEventStreamClient,
	type EventStreamOpenRequest,
	type EventStreamResult,
	type Subscription
} from '../http/events';
import { createEventStreamManager } from './event-stream-manager';
import type { SessionRecord } from './session-record';

const SCOPE = { kind: 'organization' as const, instance_id: 'inst-1', org_id: 'org-1' };
const SUBS: Subscription[] = [{ scope: SCOPE, feed: 'messaging' }];
const PROOF_1 = 'a'.repeat(64);
const PROOF_2 = 'b'.repeat(64);
const RECORD_1: SessionRecord = {
	instance_id: 'inst-1',
	account_id: 'acc-1',
	login: 'owner',
	proof: PROOF_1,
	rotated_at_ms: 100
};

function deferred<T>() {
	let resolve!: (value: T) => void;
	let reject!: (reason?: unknown) => void;
	const promise = new Promise<T>((res, rej) => {
		resolve = res;
		reject = rej;
	});
	return { promise, resolve, reject };
}

describe('createEventStreamManager', () => {
	it('closes all active streams when closeAll is called (sign-out/ended/dispose)', () => {
		const closeA = vi.fn();
		const closeB = vi.fn();
		const open = vi
			.fn()
			.mockReturnValueOnce({
				closed: deferred<EventStreamResult>().promise,
				close: closeA,
				cursor: () => undefined
			})
			.mockReturnValueOnce({
				closed: deferred<EventStreamResult>().promise,
				close: closeB,
				cursor: () => undefined
			});
		const manager = createEventStreamManager(
			{ open } as unknown as ReturnType<typeof createEventStreamClient>,
			{
				isSignedIn: () => true,
				isDisposed: () => false,
				authEnabled: () => true,
				currentRecord: () => RECORD_1,
				recoverRotatedProof: vi.fn()
			}
		);

		manager.subscribe(SUBS, {});
		manager.subscribe(SUBS, {});
		manager.closeAll();

		expect(closeA).toHaveBeenCalledTimes(1);
		expect(closeB).toHaveBeenCalledTimes(1);
	});

	it('reconnects on ended with saved cursor and emits onResync after reconnect ready', async () => {
		const first = deferred<EventStreamResult>();
		const second = deferred<EventStreamResult>();
		const calls: EventStreamOpenRequest[] = [];
		const open = vi.fn((req: EventStreamOpenRequest) => {
			calls.push(req);
			const idx = calls.length;
			return {
				closed: idx === 1 ? first.promise : second.promise,
				close: vi.fn(),
				cursor: () => (idx === 1 ? 'cur-10' : 'cur-20')
			};
		});

		const scheduled: (() => void)[] = [];
		const manager = createEventStreamManager(
			{ open } as unknown as ReturnType<typeof createEventStreamClient>,
			{
				isSignedIn: () => true,
				isDisposed: () => false,
				authEnabled: () => true,
				currentRecord: () => RECORD_1,
				recoverRotatedProof: vi.fn()
			},
			(fn) => {
				scheduled.push(fn);
				return scheduled.length as unknown as ReturnType<typeof setTimeout>;
			},
			vi.fn()
		);

		const onResync = vi.fn();
		const stop = manager.subscribe(SUBS, { onResync });

		first.resolve({ reason: 'ended', cursors: new Map() });
		await Promise.resolve();
		await Promise.resolve();
		expect(scheduled).toHaveLength(1);

		scheduled[0]();
		expect(calls).toHaveLength(2);
		expect(calls[1].subscriptions[0].cursor).toBe('cur-10');

		await calls[1].handlers?.onReady?.({
			type: 'ready',
			subscriptions: [{ scope: SCOPE, feed: 'messaging', cursor: 'cur-10' }]
		});
		expect(onResync).toHaveBeenCalledTimes(1);
		stop();
	});

	it('recovers rotated proof on 401 and does not reconnect on session_ended', async () => {
		let current: SessionRecord = RECORD_1;
		const first = deferred<EventStreamResult>();
		const second = deferred<EventStreamResult>();
		const calls: EventStreamOpenRequest[] = [];
		const open = vi.fn((req: EventStreamOpenRequest) => {
			calls.push(req);
			return {
				closed: calls.length === 1 ? first.promise : second.promise,
				close: vi.fn(),
				cursor: () => undefined
			};
		});

		const scheduled: (() => void)[] = [];
		const manager = createEventStreamManager(
			{ open } as unknown as ReturnType<typeof createEventStreamClient>,
			{
				isSignedIn: () => true,
				isDisposed: () => false,
				authEnabled: () => true,
				currentRecord: () => current,
				recoverRotatedProof: vi.fn(async () => {
					current = { ...RECORD_1, proof: PROOF_2 };
					return PROOF_2;
				})
			},
			(fn) => {
				scheduled.push(fn);
				return scheduled.length as unknown as ReturnType<typeof setTimeout>;
			},
			vi.fn()
		);

		manager.subscribe(SUBS, {});
		first.reject(new EventStreamError(401));
		await new Promise((resolve) => setTimeout(resolve, 0));
		expect(scheduled).toHaveLength(1);

		scheduled[0]();
		expect(calls).toHaveLength(2);
		expect(calls[1].proof).toBe(PROOF_2);

		second.resolve({ reason: 'session_ended', cursors: new Map() });
		await new Promise((resolve) => setTimeout(resolve, 0));
		expect(scheduled).toHaveLength(1);
	});
});
