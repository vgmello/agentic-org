import { describe, expect, it } from 'vitest';
import {
	STORAGE_KEYS,
	UI_THEMES,
	isLightTheme,
	isUiTheme,
	normalizeUiTheme,
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

describe('ui preferences and 4-theme selection', () => {
	it('defaults to ocean theme with shortcuts enabled when no overrides exist', () => {
		const resolved = resolveInitialPreferences();
		expect(resolved.theme).toBe('ocean');
		expect(resolved.navCollapsed).toBe(false);
		expect(resolved.singleKeyShortcuts).toBe(true);
	});

	it('reads saved theme from storage across all 4 v6 themes', () => {
		for (const id of ['ocean', 'classic', 'report', 'report-dark'] as const) {
			const resolved = resolveInitialPreferences({
				storage: memoryStorage({ [STORAGE_KEYS.theme]: id })
			});
			expect(resolved.theme).toBe(id);
		}
	});

	it('gives URL query params priority over storage for all 4 v6 themes', () => {
		const storage = memoryStorage({ [STORAGE_KEYS.theme]: 'ocean' });
		expect(resolveInitialPreferences({ search: '?theme=report', storage }).theme).toBe('report');
		expect(resolveInitialPreferences({ search: '?theme=report-dark', storage }).theme).toBe(
			'report-dark'
		);
		expect(resolveInitialPreferences({ search: '?theme=classic', storage }).theme).toBe('classic');
	});

	it('auto-migrates persisted or URL light theme cleanly to report', () => {
		const fromStorage = resolveInitialPreferences({
			storage: memoryStorage({ [STORAGE_KEYS.theme]: 'light' })
		});
		expect(fromStorage.theme).toBe('report');

		const fromUrl = resolveInitialPreferences({
			search: '?theme=light',
			storage: memoryStorage({ [STORAGE_KEYS.theme]: 'ocean' })
		});
		expect(fromUrl.theme).toBe('report');
		expect(normalizeUiTheme('light')).toBe('report');
	});

	it('ignores invalid theme values and malformed favorites JSON safely', () => {
		const storage = memoryStorage({
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

	it('validates the 4 v6 theme ids and color schemes strictly', () => {
		expect(UI_THEMES.map((t) => t.id)).toEqual(['ocean', 'classic', 'report', 'report-dark']);
		expect(isUiTheme('ocean')).toBe(true);
		expect(isUiTheme('classic')).toBe(true);
		expect(isUiTheme('report')).toBe(true);
		expect(isUiTheme('report-dark')).toBe(true);
		expect(isUiTheme('light')).toBe(false);
		expect(isUiTheme('dark')).toBe(false);
		expect(isUiTheme(null)).toBe(false);

		expect(isLightTheme('report')).toBe(true);
		expect(isLightTheme('report-dark')).toBe(false);
		expect(isLightTheme('ocean')).toBe(false);
		expect(isLightTheme('classic')).toBe(false);
	});
});
