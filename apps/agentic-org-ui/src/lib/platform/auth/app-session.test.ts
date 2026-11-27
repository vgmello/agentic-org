import { afterEach, describe, expect, it, vi } from 'vitest';
import type { SessionState } from './session-controller';

let capturedChanged: ((next: SessionState) => void) | undefined;

vi.mock('./session-controller', () => ({
	createSessionController: vi.fn((opts: { changed: (next: SessionState) => void }) => {
		capturedChanged = opts.changed;
		return {
			boot: vi.fn().mockResolvedValue(undefined)
		};
	})
}));
vi.mock('./session-store', () => ({
	createSessionStore: vi.fn(() => ({}))
}));
vi.mock('./session-gateway', () => ({
	createSessionGateway: vi.fn(() => ({}))
}));
vi.mock('../http/json-http', () => ({
	createJsonHttp: vi.fn(() => ({})),
	createAuthlessJsonHttp: vi.fn(() => ({})),
	createGatewayPublicHttp: vi.fn(() => ({}))
}));
vi.mock('../http/events', () => ({
	createEventStreamClient: vi.fn(() => ({}))
}));

const UUID_V7_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;

function signedIn(instanceId: string, accountId: string): SessionState {
	return {
		kind: 'signed-in',
		session: {
			instance_id: instanceId,
			account: { id: accountId, name: accountId },
			admin: true,
			organizations: []
		}
	} as unknown as SessionState;
}

afterEach(() => {
	vi.unstubAllGlobals();
});

describe('appSession store', () => {
	it('mints canonical UUIDv7 operation IDs and fires onSessionEnded on account switch and sign-out only', async () => {
		vi.stubGlobal('location', { origin: 'http://127.0.0.1:5173' });
		vi.stubGlobal('indexedDB', {});
		vi.stubGlobal('navigator', { locks: {} });
		vi.stubGlobal(
			'BroadcastChannel',
			class FakeBroadcastChannel {
				close() {}
			}
		);

		const { appSession } = await import('./app-session.svelte');
		expect(appSession.state.kind).toBe('loading');
		expect(appSession.callerScope).toBeUndefined();
		expect(appSession.authEnabled).toBe(true);
		expect(appSession.newOperationId()).toMatch(UUID_V7_RE);

		const ended = vi.fn();
		const stopA = appSession.onSessionEnded(ended);
		const stopB = appSession.onSessionEnded(ended);

		appSession.controller();
		expect(capturedChanged).toBeTypeOf('function');
		const baseGen = appSession.sessionGeneration;

		// Initial sign-in does not trigger onSessionEnded.
		capturedChanged!(signedIn('inst-1', 'acct-1'));
		expect(appSession.callerScope).toBe('inst-1:acct-1');
		expect(ended).not.toHaveBeenCalled();
		expect(appSession.sessionGeneration).toBe(baseGen);

		// Same-account reauth / refresh does not clear session state.
		capturedChanged!(signedIn('inst-1', 'acct-1'));
		expect(ended).not.toHaveBeenCalled();
		expect(appSession.sessionGeneration).toBe(baseGen);

		// Account switch fires onSessionEnded once (no duplicate registration) and increments sessionGeneration.
		capturedChanged!(signedIn('inst-1', 'acct-2'));
		expect(appSession.callerScope).toBe('inst-1:acct-2');
		expect(ended).toHaveBeenCalledTimes(1);
		expect(ended).toHaveBeenLastCalledWith('inst-1:acct-1');
		expect(appSession.sessionGeneration).toBe(baseGen + 1);

		// Sign-out fires onSessionEnded for the departing caller and increments sessionGeneration.
		capturedChanged!({ kind: 'signed-out', reason: 'signed-out' } as SessionState);
		expect(appSession.callerScope).toBeUndefined();
		expect(ended).toHaveBeenCalledTimes(2);
		expect(ended).toHaveBeenLastCalledWith('inst-1:acct-2');
		expect(appSession.sessionGeneration).toBe(baseGen + 2);

		stopA();
		stopB();

		// Unsubscribed listener does not fire on subsequent transitions.
		capturedChanged!(signedIn('inst-1', 'acct-3'));
		capturedChanged!({ kind: 'signed-out', reason: 'signed-out' } as SessionState);
		expect(ended).toHaveBeenCalledTimes(2);
	});
});
