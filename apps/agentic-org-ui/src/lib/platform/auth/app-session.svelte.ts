import { createOperationClock, formatUuidV7 } from '../http/clock';
import { createAuthlessJsonHttp, createGatewayPublicHttp, createJsonHttp } from '../http/json-http';
import type { AccessView } from '../http/wire';
import { createSessionController, type SessionState } from './session-controller';
import { createSessionGateway } from './session-gateway';
import { createSessionStore } from './session-store';

export type AppSessionState = SessionState | { kind: 'loading' } | { kind: 'unreachable' };
type Controller = ReturnType<typeof createSessionController>;

const LIMITS = {
	timeoutMs: 15_000,
	maxRequestBytes: 256 * 1024,
	maxResponseBytes: 1024 * 1024
} as const;

/**
 * The browser composition of the session controller: same-origin HTTP, IndexedDB, Web Locks
 * and BroadcastChannel (ADR-0013, ADR-0033, ADR-0034, auth/README.md). Views only; the proof
 * stays in the controller.
 */
function compose() {
	let state = $state<AppSessionState>({ kind: 'loading' });
	let access = $state<AccessView>({ auth_enabled: true, listen_scheme: 'unknown' });
	let controller: Controller | undefined;
	let booted: Promise<void> | undefined;
	let operationClock: ReturnType<typeof createOperationClock> | undefined;
	function get(): Controller {
		if (controller) return controller;
		const options = { origin: location.origin, ...LIMITS };
		const http = createJsonHttp(options);
		const authlessHttp = createAuthlessJsonHttp(options);
		controller = createSessionController({
			store: createSessionStore(indexedDB),
			gateway: createSessionGateway(createGatewayPublicHttp(options), http, authlessHttp),
			http,
			authlessHttp,
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
			},
			accessChanged: (next) => {
				access = next;
			}
		});
		return controller;
	}
	return {
		get state() {
			return state;
		},
		get access() {
			return access;
		},
		get authEnabled() {
			return access.auth_enabled;
		},
		get listenScheme() {
			return access.listen_scheme;
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
