import { describe, expect, it, vi } from 'vitest';

const store = new Map<unknown, unknown>();
vi.mock('svelte', () => ({
	setContext: (key: unknown, value: unknown) => {
		store.set(key, value);
		return value;
	},
	getContext: (key: unknown) => store.get(key)
}));

import { getMenuReturn, setMenuReturn } from './dropdown-menu-return.svelte';

describe('dropdown-menu return context', () => {
	it('initializes and returns mutable trigger + outsideClose state', () => {
		const state = setMenuReturn();
		expect(state).toEqual({ trigger: null, outsideClose: false });
		state.outsideClose = true;
		expect(getMenuReturn()?.outsideClose).toBe(true);
	});
});
