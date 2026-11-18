import { createGatewayPublicHttp, createJsonHttp } from '../http/json-http';
import { createSessionController, type SessionState } from './session-controller';
import { createSessionGateway } from './session-gateway';
import { createSessionStore } from './session-store';

export type AppSessionState = SessionState | { kind: 'loading' } | { kind: 'unreachable' };
type Controller = ReturnType<typeof createSessionController>;

const LIMITS = { timeoutMs: 15_000, maxRequestBytes: 16 * 1024, maxResponseBytes: 1024 * 1024 } as const;

/**
 * The browser composition of the session controller: same-origin HTTP, IndexedDB, Web Locks
 * and BroadcastChannel (ADR-0013, auth/README.md). Views only; the proof stays in the controller.
 */
function compose() {
	let state = $state<AppSessionState>({ kind: 'loading' });
	let controller: Controller | undefined;
	let booted: Promise<void> | undefined;
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
			}
		});
		return controller;
	}
	return {
		get state() {
			return state;
		},
		controller: get,
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
