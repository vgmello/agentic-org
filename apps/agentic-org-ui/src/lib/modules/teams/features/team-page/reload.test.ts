import { describe, expect, it, vi } from 'vitest';
import type { Resource } from '$lib/components/resource.svelte';
import { createChangeReloader, reloadOnChange } from './reload.svelte';

describe('team-page reloadOnChange', () => {
	it('skips initial load count and reloads whenever loads increments', () => {
		const reload = vi.fn(async () => {});
		const resource = { reload } as unknown as Resource<unknown>;
		const sync = createChangeReloader(resource, 1);

		expect(sync(1)).toBe(false);
		expect(reload).not.toHaveBeenCalled();

		expect(sync(2)).toBe(true);
		expect(reload).toHaveBeenCalledTimes(1);

		expect(sync(2)).toBe(false);
		expect(sync(3)).toBe(true);
		expect(reload).toHaveBeenCalledTimes(2);
		expect(typeof reloadOnChange).toBe('function');
	});
});
