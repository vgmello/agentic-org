import { createOperationClock, formatUuidV7 } from '../http/clock';
import { createGatewayPublicHttp, createJsonHttp } from '../http/json-http';
import { createSessionController, type SessionState } from './session-controller';
import { createSessionGateway } from './session-gateway';
import { createSessionStore } from './session-store';

export type AppSessionState = SessionState | { kind: 'loading' } | { kind: 'unreachable' };
type Controller = ReturnType<typeof createSessionController>;

const LIMITS = {
	timeoutMs: 15_000,
	maxRequestBytes: 16 * 1024,
	maxResponseBytes: 1024 * 1024
} as const;

/**
 * The browser composition of the session controller: same-origin HTTP, IndexedDB, Web Locks
 * and BroadcastChannel (ADR-0013, auth/README.md). Views only; the proof stays in the controller.
 */
function compose() {
	let state = $state<AppSessionState>({ kind: 'loading' });
	let controller: Controller | undefined;
	let booted: Promise<void> | undefined;
	let operationClock: ReturnType<typeof createOperationClock> | undefined;
	function get(): Controller {
		if (controller) return controller;
		const options = { origin: location.origin, ...LIMITS };
		const http = createJsonHttp(options);
		controller = createSessionController({
			store: createSessionStore(indexedDB),
			gateway: createSessionGateway(createGatewayPublicHttp(options), http),
			http,
			locks: navigator.locks,
			channel: new BroadcastChannel('org-session'),
			now: () => Date.now(),
			changed: (next) => {
				state = next;
				if (next.kind === 'signed-in' && typeof next.session.server_time_ms === 'number') {
					const clock = createOperationClock();
					clock.synchronize(next.session.server_time_ms);
					operationClock = clock;
				} else {
					operationClock = undefined;
				}
			}
		});
		return controller;
	}
	return {
		get state() {
			return state;
		},
		controller: get,
		/** Mints a canonical lowercase UUIDv7 calibrated immediately on sign-in/refresh. */
		newOperationId(): string {
			if (operationClock) return operationClock.newId();
			return formatUuidV7(Date.now(), globalThis.crypto.getRandomValues(new Uint8Array(10)));
		},
		/** Boots once per page load; later callers share the result. A failed boot can be retried. */
		boot(retry = false): Promise<void> {
			if (!booted || retry) {
				state = { kind: 'loading' };
				booted = get()
					.boot()
					.catch(() => {
						state = { kind: 'unreachable' };
					});
			}
			return booted;
		}
	};
}

export const appSession = compose();
