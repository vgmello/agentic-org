import { describe, expect, it, vi } from 'vitest';
import { createSignOutFlow } from './sign-out-flow.svelte';

describe('sign-out flow session generation isolation (SIGN-19, SIGN-20, SIGN-29)', () => {
	it('ignores a terminal 401 signOut rejection once the session has already ended', async () => {
		let signedIn = true;
		let generation = 1;
		const endedListeners: (() => void)[] = [];
		const endSession = () => {
			signedIn = false;
			generation++;
			for (const listener of endedListeners) listener();
		};

		const flow = createSignOutFlow({
			isSignedIn: () => signedIn,
			sessionGeneration: () => generation,
			onSessionEnded: (listener) => {
				endedListeners.push(listener);
				return () => {};
			},
			signOut: vi.fn(async () => {
				// Mirrors session-controller.ts:641: terminal 401 ends the session before rejecting.
				endSession();
				throw new Error('Session request failed');
			}),
			anyUnsaved: async () => false
		});

		await flow.request();
		expect(flow.failed).toBe(false);
		expect(flow.confirming).toBe(false);
		expect(flow.signingOut).toBe(false);
	});

	it('ignores late draft-query completions after the originating session ends and a new session signs in', async () => {
		let signedIn = true;
		let generation = 1;
		const endedListeners: (() => void)[] = [];
		let resolveUnsaved!: (value: boolean) => void;
		const signOut = vi.fn(async () => {});

		const flow = createSignOutFlow({
			isSignedIn: () => signedIn,
			sessionGeneration: () => generation,
			onSessionEnded: (listener) => {
				endedListeners.push(listener);
				return () => {};
			},
			signOut,
			anyUnsaved: () =>
				new Promise<boolean>((resolve) => {
					resolveUnsaved = resolve;
				})
		});

		const pending = flow.request();
		// Another tab signs out and then a new session signs in before the draft query resolves.
		signedIn = false;
		generation++;
		for (const listener of endedListeners) listener();
		signedIn = true;

		resolveUnsaved(true);
		await pending;
		expect(flow.confirming).toBe(false);
		expect(signOut).not.toHaveBeenCalled();
	});

	it('keeps failed=true on a transient 503 when the session remains active (SIGN-29)', async () => {
		const flow = createSignOutFlow({
			isSignedIn: () => true,
			sessionGeneration: () => 1,
			onSessionEnded: () => () => {},
			signOut: vi.fn(async () => {
				throw new Error('503 unavailable');
			}),
			anyUnsaved: async () => false
		});

		await flow.request();
		expect(flow.failed).toBe(true);
		flow.dismissFailure();
		expect(flow.failed).toBe(false);
	});
});
