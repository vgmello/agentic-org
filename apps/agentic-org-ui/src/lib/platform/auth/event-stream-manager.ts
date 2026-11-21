import { NotDispatchedError } from '../http/errors';
import {
	EventStreamError,
	type createEventStreamClient,
	type EventStreamHandle,
	type EventStreamHandlers,
	type Subscription
} from '../http/events';
import { WireDecodeError } from '../http/wire';
import type { SessionRecord } from './session-record';

const INITIAL_BACKOFF_MS = 1_000;
const MAX_BACKOFF_MS = 30_000;

export interface EventStreamAuthContext {
	isSignedIn: () => boolean;
	isDisposed: () => boolean;
	authEnabled: () => boolean;
	currentRecord: () => SessionRecord | undefined;
	recoverRotatedProof: (
		expected: SessionRecord,
		sessionReason?: 'session_ended' | 'unknown'
	) => Promise<string | undefined>;
}

function withSavedCursors(
	subscriptions: readonly Subscription[],
	cursors: ReadonlyMap<number, string>
): Subscription[] {
	return subscriptions.map((sub, i) => {
		const cursor = cursors.get(i);
		return cursor !== undefined ? { ...sub, cursor } : sub;
	});
}

function captureCursors(
	subscriptions: readonly Subscription[],
	handle: EventStreamHandle,
	cursors: Map<number, string>
) {
	subscriptions.forEach((sub, i) => {
		const next = handle.cursor(sub.scope, sub.feed);
		if (next) cursors.set(i, next);
	});
}

async function emitResync(subscriptions: readonly Subscription[], handlers: EventStreamHandlers) {
	const first = subscriptions[0];
	if (!first || !handlers.onResync) return;
	await handlers.onResync({
		type: 'resync',
		scope: first.scope,
		feed: first.feed,
		reason: 'snapshot_required'
	});
}

/**
 * Manages active SSE event streams for a session controller: tracks open handles so sign-out,
 * session end, and disposal abort every stream, and reconnects transient drops with saved cursors.
 */
export function createEventStreamManager(
	eventStream: ReturnType<typeof createEventStreamClient> | undefined,
	auth: EventStreamAuthContext,
	schedule: (fn: () => void, ms: number) => ReturnType<typeof setTimeout> = setTimeout,
	cancelSchedule: (id: ReturnType<typeof setTimeout>) => void = clearTimeout
) {
	const activeStops = new Set<() => void>();

	function openManaged(
		subscriptions: readonly Subscription[],
		handlers: EventStreamHandlers
	): () => void {
		if (!eventStream || auth.isDisposed() || !auth.isSignedIn() || subscriptions.length === 0) {
			return () => {};
		}
		if (!auth.currentRecord() && auth.authEnabled()) return () => {};

		let stopped = false;
		let attempt = 0;
		let reconnecting = false;
		let handle: EventStreamHandle | undefined;
		let timer: ReturnType<typeof setTimeout> | undefined;
		const cursors = new Map<number, string>();

		const stop = () => {
			if (stopped) return;
			stopped = true;
			activeStops.delete(stop);
			if (timer !== undefined) {
				cancelSchedule(timer);
				timer = undefined;
			}
			handle?.close();
			handle = undefined;
		};

		const queueReconnect = (immediate = false) => {
			if (stopped || auth.isDisposed() || !auth.isSignedIn()) return;
			reconnecting = true;
			const delay = immediate
				? 0
				: Math.min(INITIAL_BACKOFF_MS * 2 ** Math.min(attempt, 5), MAX_BACKOFF_MS);
			attempt++;
			timer = schedule(() => {
				timer = undefined;
				connect();
			}, delay);
		};

		const handleFailure = async (error: unknown, expected: SessionRecord | undefined) => {
			if (stopped || auth.isDisposed() || !auth.isSignedIn()) return;
			if (error instanceof WireDecodeError || error instanceof NotDispatchedError) return;
			if (error instanceof EventStreamError && error.status === 401) {
				if (!expected) return;
				const rotated = await auth.recoverRotatedProof(
					expected,
					error.gatewayError?.session_reason
				);
				if (rotated) queueReconnect(true);
				return;
			}
			if (error instanceof EventStreamError && error.status >= 400 && error.status < 500) return;
			queueReconnect(false);
		};

		const connect = () => {
			if (stopped || auth.isDisposed() || !auth.isSignedIn()) return;
			const expected = auth.currentRecord();
			if (!expected && auth.authEnabled()) return;
			const wasReconnect = reconnecting;
			const wrappedHandlers: EventStreamHandlers = {
				...handlers,
				onReady: async (frame) => {
					attempt = 0;
					reconnecting = false;
					await handlers.onReady?.(frame);
					if (wasReconnect) await emitResync(subscriptions, handlers);
				}
			};
			try {
				const nextHandle = eventStream.open({
					subscriptions: withSavedCursors(subscriptions, cursors),
					...(expected ? { proof: expected.proof } : { authless: true }),
					handlers: wrappedHandlers
				});
				handle = nextHandle;
				void nextHandle.closed
					.then((result) => {
						captureCursors(subscriptions, nextHandle, cursors);
						if (handle === nextHandle) handle = undefined;
						if (stopped || result.reason === 'session_ended' || result.reason === 'aborted') return;
						queueReconnect(false);
					})
					.catch((error: unknown) => {
						captureCursors(subscriptions, nextHandle, cursors);
						if (handle === nextHandle) handle = undefined;
						void handleFailure(error, expected);
					});
			} catch {
				queueReconnect(false);
			}
		};

		activeStops.add(stop);
		connect();
		return stop;
	}

	return Object.freeze({
		subscribe: openManaged,
		closeAll() {
			for (const stop of [...activeStops]) stop();
		}
	});
}
