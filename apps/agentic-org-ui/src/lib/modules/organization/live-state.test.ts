import { describe, expect, it, vi } from 'vitest';

const { sessionEndedListeners } = vi.hoisted(() => ({
	sessionEndedListeners: [] as ((caller: string | undefined) => void)[]
}));

vi.mock('$lib/platform/auth/app-session.svelte', () => ({
	appSession: {
		onSessionEnded: (listener: (caller: string | undefined) => void) => {
			sessionEndedListeners.push(listener);
			return () => {};
		}
	}
}));

import { liveState } from './live-state.svelte';

describe('liveState store', () => {
	it('tracks per-organization pause/loadedAt and clears both on appSession.onSessionEnded', () => {
		liveState.paused = { 'org-1': true };
		liveState.loadedAt = { 'org-1': new Date('2026-11-27T00:00:00Z') };
		expect(liveState.paused['org-1']).toBe(true);
		expect(liveState.loadedAt['org-1']?.toISOString()).toBe('2026-11-27T00:00:00.000Z');

		expect(sessionEndedListeners.length).toBeGreaterThan(0);
		for (const listener of sessionEndedListeners) listener('inst-1:acct-1');

		expect(Object.keys(liveState.paused)).toHaveLength(0);
		expect(Object.keys(liveState.loadedAt)).toHaveLength(0);
	});
});
