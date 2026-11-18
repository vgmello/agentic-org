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
});
