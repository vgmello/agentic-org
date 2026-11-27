import { describe, expect, it } from 'vitest';
import { createScopedSearchHost } from './scoped-search.svelte';

describe('createScopedSearchHost controller', () => {
	it('ignores stale cleanup from a superseded registration', () => {
		const host = createScopedSearchHost();
		let a = '';
		let b = '';
		const releaseA = host.register({
			getValue: () => a,
			setValue: (v) => (a = v),
			label: 'Search A',
			placeholder: 'A'
		});
		const releaseB = host.register({
			getValue: () => b,
			setValue: (v) => (b = v),
			label: 'Search B',
			placeholder: 'B'
		});

		expect(host.active?.label).toBe('Search B');
		releaseA();
		expect(host.active?.label).toBe('Search B');
		releaseB();
		expect(host.active).toBeUndefined();
	});
});
