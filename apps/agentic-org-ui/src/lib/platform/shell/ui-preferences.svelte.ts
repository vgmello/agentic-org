import { SvelteURLSearchParams } from 'svelte/reactivity';

export type UiTheme = 'ocean' | 'classic' | 'report' | 'report-dark';
export type UiColorScheme = 'dark' | 'light';

export const UI_THEMES: readonly {
	id: UiTheme;
	label: string;
	shortLabel: string;
	description: string;
	scheme: UiColorScheme;
}[] = [
	{
		id: 'ocean',
		label: 'Ocean',
		shortLabel: 'Ocean',
		description: 'Deep ocean canvas with subtle translucent frosted panels and sky accents.',
		scheme: 'dark'
	},
	{
		id: 'classic',
		label: 'Classic',
		shortLabel: 'Classic',
		description: 'Original v0 slate and indigo palette, flat hairline borders, no shadows or blur.',
		scheme: 'dark'
	},
	{
		id: 'report',
		label: 'Report',
		shortLabel: 'Report',
		description:
			'Modern 2D executive layout on a crisp white canvas with ink and steel-blue accents.',
		scheme: 'light'
	},
	{
		id: 'report-dark',
		label: 'Report Night',
		shortLabel: 'Night',
		description:
			'Modern 2D executive layout paired with Classic slate surfaces and indigo accents.',
		scheme: 'dark'
	}
] as const;

export const STORAGE_KEYS = {
	theme: 'agentic-org.ui.theme',
	navCollapsed: 'agentic-org.ui.navCollapsed',
	favorites: 'agentic-org.ui.favoriteProjects',
	singleKey: 'agentic-org.ui.singleKey'
} as const;

const VALID_THEMES = new Set<UiTheme>(UI_THEMES.map((t) => t.id));
const THEME_SCHEMES = new Map<UiTheme, UiColorScheme>(UI_THEMES.map((t) => [t.id, t.scheme]));

export function isUiTheme(value: unknown): value is UiTheme {
	return typeof value === 'string' && VALID_THEMES.has(value as UiTheme);
}

/**
 * Migrate retired theme IDs cleanly: `'light'` maps to `'report'`.
 */
export function normalizeUiTheme(value: unknown): UiTheme | undefined {
	if (value === 'light') return 'report';
	return isUiTheme(value) ? value : undefined;
}

export function isLightTheme(theme: UiTheme): boolean {
	return THEME_SCHEMES.get(theme) === 'light';
}

export interface StorageLike {
	getItem(key: string): string | null;
	setItem(key: string, value: string): void;
}

export interface UiPrefBootstrap {
	theme: UiTheme;
	navCollapsed: boolean;
	favoriteProjects: string[];
	singleKeyShortcuts: boolean;
}

/**
 * Pure parser for URL query params + storage so preference precedence
 * is deterministic and unit-tested without a browser.
 */
export function resolveInitialPreferences({
	search = '',
	storage
}: {
	search?: string;
	storage?: StorageLike;
} = {}): UiPrefBootstrap {
	// Local one-shot query parsing, never mutated or read reactively.
	const params = new SvelteURLSearchParams(search);
	const urlTheme = normalizeUiTheme(params.get('theme'));

	let storedThemeRaw: string | null = null;
	let storedCollapsed: string | null = null;
	let storedFavs: string | null = null;
	let storedSingleKey: string | null = null;
	try {
		storedThemeRaw = storage?.getItem(STORAGE_KEYS.theme) ?? null;
		storedCollapsed = storage?.getItem(STORAGE_KEYS.navCollapsed) ?? null;
		storedFavs = storage?.getItem(STORAGE_KEYS.favorites) ?? null;
		storedSingleKey = storage?.getItem(STORAGE_KEYS.singleKey) ?? null;
	} catch {
		// Private browsing or blocked storage: fall back safely to defaults.
	}

	const storedTheme = normalizeUiTheme(storedThemeRaw);
	const theme: UiTheme = urlTheme ?? storedTheme ?? 'ocean';

	let favoriteProjects: string[] = [];
	if (storedFavs) {
		try {
			const parsed: unknown = JSON.parse(storedFavs);
			if (Array.isArray(parsed)) {
				favoriteProjects = parsed.filter((v): v is string => typeof v === 'string');
			}
		} catch {
			favoriteProjects = [];
		}
	}

	return {
		theme,
		navCollapsed: storedCollapsed === '1',
		favoriteProjects,
		singleKeyShortcuts: storedSingleKey !== '0'
	};
}

/** Apply active theme attributes to the document root cleanly (zero inline scripts). */
export function applyUiAttributes(root: HTMLElement | undefined, theme: UiTheme): void {
	if (!root) return;
	root.dataset.uiMode = 'executive';
	root.dataset.uiTheme = theme;
	const isLight = isLightTheme(theme);
	root.classList.toggle('dark', !isLight);
	root.classList.toggle('light', isLight);
}

class UiPreferencesStore {
	theme = $state<UiTheme>('ocean');
	navCollapsed = $state(false);
	favoriteProjects = $state<string[]>([]);
	singleKeyShortcuts = $state(true);
	#hydrated = false;

	hydrate(search = typeof window !== 'undefined' ? window.location.search : ''): void {
		const storage = typeof window !== 'undefined' ? window.localStorage : undefined;
		const resolved = resolveInitialPreferences({ search, storage });
		this.theme = resolved.theme;
		this.navCollapsed = resolved.navCollapsed;
		this.favoriteProjects = resolved.favoriteProjects;
		this.singleKeyShortcuts = resolved.singleKeyShortcuts;
		this.#hydrated = true;

		// Persist explicit URL overrides and auto-migrate persisted 'light' -> 'report'.
		const params = new SvelteURLSearchParams(search);
		const urlTheme = normalizeUiTheme(params.get('theme'));
		const storedThemeRaw = (() => {
			try {
				return storage?.getItem(STORAGE_KEYS.theme) ?? null;
			} catch {
				return null;
			}
		})();
		const storedTheme = normalizeUiTheme(storedThemeRaw);
		if (urlTheme) {
			this.#save(STORAGE_KEYS.theme, this.theme);
		} else if (storedThemeRaw === 'light' && storedTheme === 'report') {
			this.#save(STORAGE_KEYS.theme, 'report');
		}
		this.syncDom();
	}

	get hydrated(): boolean {
		return this.#hydrated;
	}

	setTheme(next: UiTheme): void {
		const normalized = normalizeUiTheme(next);
		if (!normalized) return;
		this.theme = normalized;
		this.#save(STORAGE_KEYS.theme, normalized);
		this.syncDom();
	}

	toggleNavCollapsed(): void {
		this.navCollapsed = !this.navCollapsed;
		this.#save(STORAGE_KEYS.navCollapsed, this.navCollapsed ? '1' : '0');
	}

	setSingleKeyShortcuts(enabled: boolean): void {
		this.singleKeyShortcuts = enabled;
		this.#save(STORAGE_KEYS.singleKey, enabled ? '1' : '0');
	}

	isFavorite(projectId: string): boolean {
		return this.favoriteProjects.includes(projectId);
	}

	toggleFavorite(projectId: string): void {
		if (this.favoriteProjects.includes(projectId)) {
			this.favoriteProjects = this.favoriteProjects.filter((id) => id !== projectId);
		} else {
			this.favoriteProjects = [...this.favoriteProjects, projectId];
		}
		this.#save(STORAGE_KEYS.favorites, JSON.stringify(this.favoriteProjects));
	}

	syncDom(): void {
		if (typeof document === 'undefined') return;
		applyUiAttributes(document.documentElement, this.theme);
	}

	#save(key: string, value: string): void {
		if (typeof window === 'undefined') return;
		try {
			window.localStorage.setItem(key, value);
		} catch {
			// Ignore quota / private mode failures.
		}
	}
}

export const uiPrefs = new UiPreferencesStore();
