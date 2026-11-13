import { createJsonHttp, createGatewayPublicHttp } from '../src/lib/platform/http/json-http';
import { createSessionGateway } from '../src/lib/platform/auth/session-gateway';
import { createSessionStore } from '../src/lib/platform/auth/session-store';
import { createSessionController, type SessionState } from '../src/lib/platform/auth/session-controller';

/** Browser-only test composition: the real auth boundary, with a test-owned HTTP server. */
export function createHarness() {
	const options = { origin: location.origin, timeoutMs: 5000, maxRequestBytes: 4096, maxResponseBytes: 16384 };
	const http = createJsonHttp(options);
	const store = createSessionStore(indexedDB);
	const channel = new BroadcastChannel('org-session');
	const states: SessionState[] = [];
	const broadcasts: unknown[] = [];
	channel.addEventListener('message', event => broadcasts.push(event.data));
	const controller = createSessionController({ store, http, channel, locks: navigator.locks,
		gateway: createSessionGateway(createGatewayPublicHttp(options), http), now: Date.now,
		changed: state => states.push(state) });
	return { store, controller, states, broadcasts, dispose() { controller.dispose(); channel.close(); } };
}
export type Harness = ReturnType<typeof createHarness>;
