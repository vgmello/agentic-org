import { createJsonHttp, createGatewayPublicHttp } from '$lib/platform/http/json-http';
import { createOperationClock } from '$lib/platform/http/clock';
import { createSessionGateway } from '$lib/platform/auth/session-gateway';
import { createSessionController, type SessionState } from '$lib/platform/auth/session-controller';
import { createSessionStore } from '$lib/platform/auth/session-store';
import { createTaskFeature } from './feature';
import type { Scope } from './wire';
/** Called only on browser mount. Every request uses the configured production origin. */
export function createBrowserTasks(changed: (state: SessionState) => void) {
	if (!navigator.locks || !globalThis.indexedDB || !globalThis.BroadcastChannel)
		throw new Error('This browser cannot protect the shared session.');
	const options = {
		origin: location.origin,
		timeoutMs: 15000,
		maxRequestBytes: 1024 * 1024,
		maxResponseBytes: 1024 * 1024
	};
	const http = createJsonHttp(options),
		channel = new BroadcastChannel('org-session'),
		clock = createOperationClock();
	let state: SessionState = { kind: 'signed-out' };
	const controller = createSessionController({
		http,
		store: createSessionStore(indexedDB),
		gateway: createSessionGateway(createGatewayPublicHttp(options), http),
		locks: navigator.locks,
		channel,
		now: Date.now,
		changed(next) {
			state = next;
			if (next.kind === 'signed-in') clock.synchronize(next.session.server_time_ms);
			changed(next);
		}
	});
	return Object.freeze({
		boot: controller.boot,
		signIn: controller.signIn,
		signOut: controller.signOut,
		refresh: () => controller.refresh(Date.now()),
		select(organizationId: string, officeId: string) {
			if (
				state.kind !== 'signed-in' ||
				state.reauth ||
				!state.session.organizations.some(
					(o) => o.org_id === organizationId && o.office_ids.includes(officeId)
				)
			)
				throw new Error('Select a linked office after signing in.');
			const scope: Scope = {
				instanceId: state.session.instance_id,
				accountId: state.session.account.id,
				organizationId,
				officeId
			};
			return createTaskFeature({
				scope,
				http,
				read: controller.read,
				storage: sessionStorage,
				operations: (transport) => controller.operations(transport, clock)
			});
		},
		dispose() {
			controller.dispose();
			channel.close();
		}
	});
}
export type BrowserTasks = ReturnType<typeof createBrowserTasks>;
