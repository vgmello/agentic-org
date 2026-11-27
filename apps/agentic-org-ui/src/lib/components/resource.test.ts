import { describe, expect, it } from 'vitest';
import { unavailableCall } from '$lib/platform/unavailable-api';
import { Resource } from './resource.svelte';

/** A load whose answers the test releases one at a time, in any order. */
function controlled<T>() {
	const pending: { resolve: (v: T) => void; reject: (e: unknown) => void }[] = [];
	const load = () => new Promise<T>((resolve, reject) => pending.push({ resolve, reject }));
	return { load, pending };
}
const settle = () => new Promise((r) => setTimeout(r, 0));

describe('Resource', () => {
	it('loads when created and becomes ready', async () => {
		const c = controlled<string[]>();
		const r = new Resource(c.load);
		expect(r.state).toBe('loading');
		expect(r.reloading).toBe(true);
		c.pending[0].resolve(['a']);
		await settle();
		expect(r.state).toBe('ready');
		expect(r.data).toEqual(['a']);
		expect(r.reloading).toBe(false);
	});

	it('keeps the shown data while a reload is in flight', async () => {
		const c = controlled<string[]>();
		const r = new Resource(c.load);
		c.pending[0].resolve(['a']);
		await settle();
		void r.reload();
		expect(r.state).toBe('ready');
		expect(r.data).toEqual(['a']);
		expect(r.reloading).toBe(true);
	});

	it('drops the old data when a reload fails, and clears the message on the next success', async () => {
		const c = controlled<string[]>();
		const r = new Resource(c.load);
		c.pending[0].resolve(['a']);
		await settle();
		void r.reload();
		c.pending[1].reject(
			Object.assign(new Error('Office stopped'), { source: 'api', code: 'office_unavailable' })
		);
		await settle();
		expect(r.state).toBe('error');
		expect(r.data).toBeUndefined();
		expect(r.message).toBe('Office stopped');
		void r.reload();
		c.pending[2].resolve(['b']);
		await settle();
		expect(r.state).toBe('ready');
		expect(r.message).toBe('');
	});

	it('tells unavailable (the production adapter) apart from an error', async () => {
		const r = new Resource(unavailableCall);
		await settle();
		expect(r.state).toBe('unavailable');
	});

	it('marks an office unsupported answer as unsupported rather than a retryable error', async () => {
		const r = new Resource(async () => {
			throw {
				source: 'api',
				code: 'unsupported',
				message: 'Update the office to read this view.'
			};
		});
		await settle();
		expect(r.state).toBe('unsupported');
		expect(r.data).toBeUndefined();
		expect(r.message).toBe('Update the office to read this view.');
	});

	it('allows retry when a read fails temporarily with an unavailable code', async () => {
		const c = controlled<string[]>();
		const r = new Resource(c.load);
		c.pending[0].resolve(['old']);
		await settle();

		const refresh = r.reload();
		c.pending[1].reject({ source: 'api', code: 'unavailable', message: "HQ didn't answer." });
		await refresh;
		expect(r.state).toBe('error');
		expect(r.data).toBeUndefined();
		expect(r.message).toBe("HQ didn't answer.");

		const retry = r.reload();
		c.pending[2].resolve(['new']);
		await retry;
		expect(r.state).toBe('ready');
		expect(r.data).toEqual(['new']);
	});

	it('shows the latest request even when an older one answers last', async () => {
		const c = controlled<string>();
		const r = new Resource(c.load);
		void r.reload();
		c.pending[1].resolve('new');
		await settle();
		c.pending[0].resolve('old');
		await settle();
		expect(r.data).toBe('new');
		expect(r.reloading).toBe(false);
	});

	it('ignores an older request that fails after a newer one succeeded', async () => {
		const c = controlled<string>();
		const r = new Resource(c.load);
		void r.reload();
		c.pending[1].resolve('new');
		await settle();
		c.pending[0].reject(new Error('late failure'));
		await settle();
		expect(r.state).toBe('ready');
		expect(r.data).toBe('new');
		expect(r.message).toBe('');
		expect(r.reloading).toBe(false);
	});

	it('passes an AbortSignal and aborts the previous in-flight load on reload and dispose', async () => {
		const signals: AbortSignal[] = [];
		const pending: { resolve: (v: string) => void; reject: (e: unknown) => void }[] = [];
		const r = new Resource<string>((signal) => {
			if (signal) signals.push(signal);
			return new Promise((resolve, reject) => pending.push({ resolve, reject }));
		});

		expect(signals).toHaveLength(1);
		expect(signals[0].aborted).toBe(false);

		void r.reload();
		expect(signals).toHaveLength(2);
		expect(signals[0].aborted).toBe(true);
		expect(signals[1].aborted).toBe(false);

		r.dispose();
		expect(signals[1].aborted).toBe(true);
		expect(r.reloading).toBe(false);

		pending[1].resolve('after-dispose');
		await settle();
		expect(r.data).toBeUndefined();
		expect(r.state).toBe('loading');

		await r.reload();
		expect(signals).toHaveLength(2);
	});
});
