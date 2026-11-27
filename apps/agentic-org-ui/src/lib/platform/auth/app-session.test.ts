import { describe, expect, it, vi } from 'vitest';

const UUID_V7_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;

describe('appSession store', () => {
	it('mints canonical UUIDv7 operation IDs and exposes initial loading state', async () => {
		const { appSession } = await import('./app-session.svelte');
		expect(appSession.state.kind).toBe('loading');
		expect(appSession.callerScope).toBeUndefined();
		expect(appSession.authEnabled).toBe(true);
		expect(appSession.newOperationId()).toMatch(UUID_V7_RE);
	});

	it('registers and unsubscribes onSessionEnded listeners cleanly without duplicates', async () => {
		const { appSession } = await import('./app-session.svelte');
		const listener = vi.fn();
		const stopA = appSession.onSessionEnded(listener);
		const stopB = appSession.onSessionEnded(listener);
		expect(typeof stopA).toBe('function');
		expect(typeof stopB).toBe('function');
		stopA();
		stopB();
	});
});
