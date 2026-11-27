import { describe, expect, it } from 'vitest';
import {
	DEFAULT_PINNED_WORKSPACE_NAV,
	STORAGE_KEYS,
	UI_THEMES,
	applyUiAttributes,
	isLightTheme,
	isUiTheme,
	moveWorkspaceNavItem,
	normalizeUiTheme,
	normalizeWorkspaceNav,
	resolveInitialPreferences,
	toggleWorkspaceNavPin,
	uiPrefs,
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

describe('ui preferences and 7-theme selection', () => {
	it('defaults to ocean theme with shortcuts enabled, assembly dividers off, and cozy high contrast off', () => {
		const resolved = resolveInitialPreferences();
		expect(resolved.theme).toBe('ocean');
		expect(resolved.cozyHighContrast).toBe(false);
		expect(resolved.navCollapsed).toBe(false);
		expect(resolved.singleKeyShortcuts).toBe(true);
		expect(resolved.assemblyDividers).toBe(false);
	});

	it('reads persisted assemblyDividers flag from storage', () => {
		const enabled = resolveInitialPreferences({
			storage: memoryStorage({ [STORAGE_KEYS.assemblyDividers]: '1' })
		});
		expect(enabled.assemblyDividers).toBe(true);

		const disabled = resolveInitialPreferences({
			storage: memoryStorage({ [STORAGE_KEYS.assemblyDividers]: '0' })
		});
		expect(disabled.assemblyDividers).toBe(false);
	});

	it('reads saved theme from storage across all 7 v6 themes', () => {
		for (const id of [
			'ocean',
			'classic',
			'graphite',
			'graphite-light',
			'cozy',
			'report',
			'report-dark'
		] as const) {
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

	it('gives URL query params priority over storage for all 7 themes', () => {
		const storage = memoryStorage({ [STORAGE_KEYS.theme]: 'ocean' });
		expect(resolveInitialPreferences({ search: '?theme=graphite', storage }).theme).toBe(
			'graphite'
		);
		expect(resolveInitialPreferences({ search: '?theme=graphite-light', storage }).theme).toBe(
			'graphite-light'
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

	it('validates the 7 theme ids and color schemes strictly', () => {
		expect(UI_THEMES.map((t) => t.id)).toEqual([
			'ocean',
			'classic',
			'graphite',
			'graphite-light',
			'cozy',
			'report',
			'report-dark'
		]);
		expect(isUiTheme('ocean')).toBe(true);
		expect(isUiTheme('classic')).toBe(true);
		expect(isUiTheme('graphite')).toBe(true);
		expect(isUiTheme('graphite-light')).toBe(true);
		expect(isUiTheme('cozy')).toBe(true);
		expect(isUiTheme('report')).toBe(true);
		expect(isUiTheme('report-dark')).toBe(true);
		expect(isUiTheme('light')).toBe(false);
		expect(isUiTheme('dark')).toBe(false);
		expect(isUiTheme(null)).toBe(false);

		expect(isLightTheme('report')).toBe(true);
		expect(isLightTheme('graphite-light')).toBe(true);
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

		applyUiAttributes(root as unknown as HTMLElement, 'graphite-light', false);
		expect(root.dataset.uiTheme).toBe('graphite-light');
		expect(root.classes.has('light')).toBe(true);
		expect(root.classes.has('dark')).toBe(false);

		applyUiAttributes(root as unknown as HTMLElement, 'graphite', false, true);
		expect(root.dataset.uiTheme).toBe('graphite');
		expect(root.dataset.assemblyDividers).toBe('');
		expect(root.classes.has('dark')).toBe(true);
		expect(root.classes.has('light')).toBe(false);

		applyUiAttributes(root as unknown as HTMLElement, 'graphite', false, false);
		expect('assemblyDividers' in root.dataset).toBe(false);
	});

	it('hydrates defaults and URL overrides safely when window.localStorage getter throws SecurityError', () => {
		const hadWindow = 'window' in globalThis;
		const originalWindow = (globalThis as { window?: unknown }).window;
		const fakeWindow = { location: { search: '' } };
		Object.defineProperty(fakeWindow, 'localStorage', {
			configurable: true,
			get() {
				throw new DOMException('blocked', 'SecurityError');
			}
		});
		Object.defineProperty(globalThis, 'window', {
			configurable: true,
			writable: true,
			value: fakeWindow
		});
		try {
			expect(() => uiPrefs.hydrate('')).not.toThrow();
			expect(uiPrefs.theme).toBe('ocean');
			expect(uiPrefs.isReport).toBe(false);
			expect(uiPrefs.cozyHighContrast).toBe(false);

			expect(() => uiPrefs.hydrate('?theme=report')).not.toThrow();
			expect(uiPrefs.isReport).toBe(true);
			expect(() => uiPrefs.hydrate('?theme=report-dark')).not.toThrow();
			expect(uiPrefs.isReport).toBe(true);

			expect(() => uiPrefs.hydrate('?theme=graphite')).not.toThrow();
			expect(uiPrefs.theme).toBe('graphite');
			expect(uiPrefs.isReport).toBe(false);
			expect(() => uiPrefs.setCozyHighContrast(true)).not.toThrow();
			expect(uiPrefs.cozyHighContrast).toBe(true);
			uiPrefs.setCozyHighContrast(false);
			uiPrefs.setTheme('ocean');
		} finally {
			if (hadWindow) {
				Object.defineProperty(globalThis, 'window', {
					configurable: true,
					writable: true,
					value: originalWindow
				});
			} else {
				delete (globalThis as { window?: unknown }).window;
			}
		}
	});

	it('persists, normalizes, reorders, and pins/unpins Workspace nav items safely', () => {
		expect(resolveInitialPreferences().workspaceNav).toEqual([...DEFAULT_PINNED_WORKSPACE_NAV]);
		expect(normalizeWorkspaceNav(['insights', 'work', 'bogus', 'work'])).toEqual([
			'insights',
			'work'
		]);
		expect(normalizeWorkspaceNav([])).toEqual([...DEFAULT_PINNED_WORKSPACE_NAV]);

		const reordered = moveWorkspaceNavItem(
			['work', 'chat', 'activity', 'projects'],
			'projects',
			'chat',
			'pinned'
		);
		expect(reordered).toEqual(['work', 'projects', 'chat', 'activity']);

		const movedToMore = moveWorkspaceNavItem(reordered, 'chat', null, 'more');
		expect(movedToMore).toEqual(['work', 'projects', 'activity']);

		const keptLast = moveWorkspaceNavItem(['work'], 'work', null, 'more');
		expect(keptLast).toEqual(['work']);

		const pinned = toggleWorkspaceNavPin(['work', 'chat'], 'insights');
		expect(pinned).toEqual(['work', 'chat', 'insights']);
		expect(toggleWorkspaceNavPin(['work'], 'work')).toEqual(['work']);

		const stored = resolveInitialPreferences({
			storage: memoryStorage({ [STORAGE_KEYS.workspaceNav]: JSON.stringify(['chat', 'insights']) })
		});
		expect(stored.workspaceNav).toEqual(['chat', 'insights']);
	});

	it('clears org-scoped favoriteProjects from memory and localStorage via clearFavorites()', () => {
		const removed: string[] = [];
		const map = new Map<string, string>();
		const prevWindow = globalThis.window;
		Object.defineProperty(globalThis, 'window', {
			configurable: true,
			value: {
				localStorage: {
					getItem: (k: string) => map.get(k) ?? null,
					setItem: (k: string, v: string) => void map.set(k, v),
					removeItem: (k: string) => {
						removed.push(k);
						map.delete(k);
					}
				}
			}
		});
		try {
			uiPrefs.toggleFavorite('proj-1');
			expect(uiPrefs.favoriteProjects).toContain('proj-1');
			expect(map.get(STORAGE_KEYS.favorites)).toContain('proj-1');

			uiPrefs.clearFavorites();
			expect(uiPrefs.favoriteProjects).toEqual([]);
			expect(removed).toContain(STORAGE_KEYS.favorites);
			expect(map.has(STORAGE_KEYS.favorites)).toBe(false);
		} finally {
			Object.defineProperty(globalThis, 'window', { configurable: true, value: prevWindow });
		}
	});
});
