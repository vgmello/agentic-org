import { describe, expect, it } from 'vitest';
import {
	STORAGE_KEYS,
	isUiTheme,
	resolveInitialPreferences,
	type StorageLike
} from './ui-preferences.svelte';

function memoryStorage(initial: Record<string, string> = {}): StorageLike {
	const map = new Map<string, string>(Object.entries(initial));
	return {
		getItem: (key) => map.get(key) ?? null,
		setItem: (key, value) => void map.set(key, value)
	};
}

describe('ui preferences and executive UI feature flag', () => {
	it('defaults to legacy mode (executiveUi off) and classic theme when no overrides exist', () => {
		const resolved = resolveInitialPreferences();
		expect(resolved.executiveUi).toBe(false);
		expect(resolved.theme).toBe('classic');
		expect(resolved.navCollapsed).toBe(false);
		expect(resolved.singleKeyShortcuts).toBe(true);
	});

	it('switches default theme to ocean when executiveUi is enabled via storage or env', () => {
		const viaStorage = resolveInitialPreferences({
			storage: memoryStorage({ [STORAGE_KEYS.executiveUi]: '1' })
		});
		expect(viaStorage.executiveUi).toBe(true);
		expect(viaStorage.theme).toBe('ocean');

		const viaEnv = resolveInitialPreferences({ envExecutiveDefault: true });
		expect(viaEnv.executiveUi).toBe(true);
		expect(viaEnv.theme).toBe('ocean');
	});

	it('gives URL query params priority over storage for quick team testing', () => {
		const storage = memoryStorage({
			[STORAGE_KEYS.executiveUi]: '0',
			[STORAGE_KEYS.theme]: 'ocean'
		});
		const on = resolveInitialPreferences({ search: '?ui=v2&theme=light', storage });
		expect(on.executiveUi).toBe(true);
		expect(on.theme).toBe('light');

		const off = resolveInitialPreferences({
			search: '?ui=v1&theme=classic',
			storage: memoryStorage({ [STORAGE_KEYS.executiveUi]: '1' })
		});
		expect(off.executiveUi).toBe(false);
		expect(off.theme).toBe('classic');
	});

	it('ignores invalid theme values and malformed favorites JSON safely', () => {
		const storage = memoryStorage({
			[STORAGE_KEYS.executiveUi]: '1',
			[STORAGE_KEYS.theme]: 'neon-invalid',
			[STORAGE_KEYS.favorites]: '{not json',
			[STORAGE_KEYS.navCollapsed]: '1',
			[STORAGE_KEYS.singleKey]: '0'
		});
		const resolved = resolveInitialPreferences({ search: '?theme=bogus', storage });
		expect(resolved.theme).toBe('ocean');
		expect(resolved.favoriteProjects).toEqual([]);
		expect(resolved.navCollapsed).toBe(true);
		expect(resolved.singleKeyShortcuts).toBe(false);
	});

	it('validates theme ids strictly', () => {
		expect(isUiTheme('ocean')).toBe(true);
		expect(isUiTheme('light')).toBe(true);
		expect(isUiTheme('classic')).toBe(true);
		expect(isUiTheme('dark')).toBe(false);
		expect(isUiTheme(null)).toBe(false);
	});
});
