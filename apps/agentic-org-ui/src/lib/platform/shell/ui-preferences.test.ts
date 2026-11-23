import { describe, expect, it } from 'vitest';
import {
	STORAGE_KEYS,
	UI_THEMES,
	applyUiAttributes,
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

function fakeRoot(): {
	dataset: Record<string, string | undefined>;
	classes: Set<string>;
	classList: { toggle(name: string, force?: boolean): boolean };
} {
	const classes = new Set<string>();
	return {
		dataset: {},
		classes,
		classList: {
			toggle(name: string, force?: boolean) {
				const shouldAdd = force ?? !classes.has(name);
				if (shouldAdd) classes.add(name);
				else classes.delete(name);
				return shouldAdd;
			}
		}
	};
}

describe('ui preferences and 6-theme selection', () => {
	it('defaults to ocean theme with shortcuts enabled and cozy high contrast off', () => {
		const resolved = resolveInitialPreferences();
		expect(resolved.theme).toBe('ocean');
		expect(resolved.cozyHighContrast).toBe(false);
		expect(resolved.navCollapsed).toBe(false);
		expect(resolved.singleKeyShortcuts).toBe(true);
	});

	it('reads saved theme from storage across all 6 v6 themes', () => {
		for (const id of ['ocean', 'classic', 'graphite', 'cozy', 'report', 'report-dark'] as const) {
			const resolved = resolveInitialPreferences({
				storage: memoryStorage({ [STORAGE_KEYS.theme]: id })
			});
			expect(resolved.theme).toBe(id);
		}
	});

	it('reads persisted cozyHighContrast flag from storage', () => {
		const enabled = resolveInitialPreferences({
			storage: memoryStorage({
				[STORAGE_KEYS.theme]: 'cozy',
				[STORAGE_KEYS.cozyHighContrast]: '1'
			})
		});
		expect(enabled.theme).toBe('cozy');
		expect(enabled.cozyHighContrast).toBe(true);

		const disabled = resolveInitialPreferences({
			storage: memoryStorage({
				[STORAGE_KEYS.theme]: 'cozy',
				[STORAGE_KEYS.cozyHighContrast]: '0'
			})
		});
		expect(disabled.cozyHighContrast).toBe(false);
	});

	it('gives URL query params priority over storage for all 6 themes', () => {
		const storage = memoryStorage({ [STORAGE_KEYS.theme]: 'ocean' });
		expect(resolveInitialPreferences({ search: '?theme=graphite', storage }).theme).toBe(
			'graphite'
		);
		expect(resolveInitialPreferences({ search: '?theme=cozy', storage }).theme).toBe('cozy');
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
			[STORAGE_KEYS.singleKey]: '0',
			[STORAGE_KEYS.cozyHighContrast]: 'bogus'
		});
		const resolved = resolveInitialPreferences({ search: '?theme=bogus', storage });
		expect(resolved.theme).toBe('ocean');
		expect(resolved.cozyHighContrast).toBe(false);
		expect(resolved.favoriteProjects).toEqual([]);
		expect(resolved.navCollapsed).toBe(true);
		expect(resolved.singleKeyShortcuts).toBe(false);
	});

	it('validates the 6 theme ids and color schemes strictly', () => {
		expect(UI_THEMES.map((t) => t.id)).toEqual([
			'ocean',
			'classic',
			'graphite',
			'cozy',
			'report',
			'report-dark'
		]);
		expect(isUiTheme('ocean')).toBe(true);
		expect(isUiTheme('classic')).toBe(true);
		expect(isUiTheme('graphite')).toBe(true);
		expect(isUiTheme('cozy')).toBe(true);
		expect(isUiTheme('report')).toBe(true);
		expect(isUiTheme('report-dark')).toBe(true);
		expect(isUiTheme('light')).toBe(false);
		expect(isUiTheme('dark')).toBe(false);
		expect(isUiTheme(null)).toBe(false);

		expect(isLightTheme('report')).toBe(true);
		expect(isLightTheme('report-dark')).toBe(false);
		expect(isLightTheme('ocean')).toBe(false);
		expect(isLightTheme('classic')).toBe(false);
		expect(isLightTheme('graphite')).toBe(false);
		expect(isLightTheme('cozy')).toBe(false);
	});

	it('syncs data-ui-theme, color scheme classes, and data-cozy-contrast on documentElement', () => {
		const root = fakeRoot();
		applyUiAttributes(root as unknown as HTMLElement, 'cozy', true);
		expect(root.dataset.uiMode).toBe('executive');
		expect(root.dataset.uiTheme).toBe('cozy');
		expect(root.dataset.cozyContrast).toBe('high');
		expect(root.classes.has('dark')).toBe(true);
		expect(root.classes.has('light')).toBe(false);

		applyUiAttributes(root as unknown as HTMLElement, 'cozy', false);
		expect('cozyContrast' in root.dataset).toBe(false);

		applyUiAttributes(root as unknown as HTMLElement, 'report', false);
		expect(root.dataset.uiTheme).toBe('report');
		expect(root.classes.has('light')).toBe(true);
		expect(root.classes.has('dark')).toBe(false);

		applyUiAttributes(root as unknown as HTMLElement, 'graphite', false);
		expect(root.dataset.uiTheme).toBe('graphite');
		expect(root.classes.has('dark')).toBe(true);
		expect(root.classes.has('light')).toBe(false);
	});
});
