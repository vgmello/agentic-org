import { describe, expect, it } from 'vitest';
import { liveState } from './live-state.svelte';

describe('liveState store', () => {
	it('tracks per-organization pause and loaded timestamps cleanly', () => {
		liveState.paused = { 'org-1': true };
		liveState.loadedAt = { 'org-1': new Date('2026-11-27T00:00:00Z') };
		expect(liveState.paused['org-1']).toBe(true);
		expect(liveState.loadedAt['org-1']?.toISOString()).toBe('2026-11-27T00:00:00.000Z');
		liveState.paused = {};
		liveState.loadedAt = {};
		expect(Object.keys(liveState.paused)).toHaveLength(0);
	});
});
