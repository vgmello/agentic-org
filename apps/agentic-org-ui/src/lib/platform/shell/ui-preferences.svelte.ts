export type UiTheme = 'ocean' | 'light' | 'classic';

export const UI_THEMES: readonly {
	id: UiTheme;
	label: string;
	shortLabel: string;
	description: string;
}[] = [
	{
		id: 'ocean',
		label: 'Ocean (Frosted)',
		shortLabel: 'Ocean',
		description: 'Deep ocean canvas with subtle translucent frosted panels and sky accents.'
	},
	{
		id: 'light',
		label: 'Light (Daylight)',
		shortLabel: 'Light',
		description: 'Crisp daylight workspace with white elevated surfaces and high-contrast slate ink.'
	},
	{
		id: 'classic',
		label: 'Classic (v0 Flat)',
		shortLabel: 'Classic',
		description: 'Original v0 slate and indigo palette, flat hairline borders, no shadows or blur.'
	}
] as const;

export const STORAGE_KEYS = {
	executiveUi: 'agentic-org.ui.executive',
	theme: 'agentic-org.ui.theme',
	navCollapsed: 'agentic-org.ui.navCollapsed',
	favorites: 'agentic-org.ui.favoriteProjects',
	singleKey: 'agentic-org.ui.singleKey'
} as const;

const VALID_THEMES = new Set<UiTheme>(['ocean', 'light', 'classic']);

export function isUiTheme(value: unknown): value is UiTheme {
	return typeof value === 'string' && VALID_THEMES.has(value as UiTheme);
}

export interface StorageLike {
	getItem(key: string): string | null;
	setItem(key: string, value: string): void;
}

export interface UiPrefBootstrap {
	executiveUi: boolean;
	theme: UiTheme;
	navCollapsed: boolean;
	favoriteProjects: string[];
	singleKeyShortcuts: boolean;
}

/**
 * Pure parser for URL query params + storage + build env, so preference precedence
 * is deterministic and unit-tested without a browser.
 */
export function resolveInitialPreferences({
	search = '',
	storage,
	envExecutiveDefault = false
}: {
	search?: string;
	storage?: StorageLike;
	envExecutiveDefault?: boolean;
} = {}): UiPrefBootstrap {
	// Local one-shot query parsing, never mutated or read reactively.
	// eslint-disable-next-line svelte/prefer-svelte-reactivity
	const params = new URLSearchParams(search);
	const urlUi = params.get('ui');
	const urlTheme = params.get('theme');

	let storedExec: string | null = null;
	let storedTheme: string | null = null;
	let storedCollapsed: string | null = null;
	let storedFavs: string | null = null;
	let storedSingleKey: string | null = null;
	try {
		storedExec = storage?.getItem(STORAGE_KEYS.executiveUi) ?? null;
		storedTheme = storage?.getItem(STORAGE_KEYS.theme) ?? null;
		storedCollapsed = storage?.getItem(STORAGE_KEYS.navCollapsed) ?? null;
		storedFavs = storage?.getItem(STORAGE_KEYS.favorites) ?? null;
		storedSingleKey = storage?.getItem(STORAGE_KEYS.singleKey) ?? null;
	} catch {
		// Private browsing or blocked storage: fall back safely to defaults.
	}

	const executiveUi =
		urlUi === 'v2' || urlUi === '1' || urlUi === 'true'
			? true
			: urlUi === 'v1' || urlUi === '0' || urlUi === 'false'
				? false
				: storedExec === '1'
					? true
					: storedExec === '0'
						? false
						: envExecutiveDefault;

	const defaultTheme: UiTheme = executiveUi ? 'ocean' : 'classic';
	const theme: UiTheme = isUiTheme(urlTheme)
		? urlTheme
		: isUiTheme(storedTheme)
			? storedTheme
			: defaultTheme;

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
		executiveUi,
		theme,
		navCollapsed: storedCollapsed === '1',
		favoriteProjects,
		singleKeyShortcuts: storedSingleKey !== '0'
	};
}

/** Apply active theme and UI mode attributes to the document root cleanly (zero inline scripts). */
export function applyUiAttributes(
	root: HTMLElement | undefined,
	executiveUi: boolean,
	theme: UiTheme
): void {
	if (!root) return;
	const effectiveTheme: UiTheme = executiveUi ? theme : theme === 'light' ? 'light' : 'classic';
	root.dataset.uiMode = executiveUi ? 'executive' : 'legacy';
	root.dataset.uiTheme = effectiveTheme;
	const isLight = effectiveTheme === 'light';
	root.classList.toggle('dark', !isLight);
	root.classList.toggle('light', isLight);
}

class UiPreferencesStore {
	executiveUi = $state(false);
	theme = $state<UiTheme>('ocean');
	navCollapsed = $state(false);
	favoriteProjects = $state<string[]>([]);
	singleKeyShortcuts = $state(true);
	#hydrated = false;

	hydrate(search = typeof window !== 'undefined' ? window.location.search : ''): void {
		const envDefault =
			typeof import.meta !== 'undefined' &&
			(import.meta.env as Record<string, string | undefined>)?.PUBLIC_EXECUTIVE_UI === '1';
		const storage = typeof window !== 'undefined' ? window.localStorage : undefined;
		const resolved = resolveInitialPreferences({
			search,
			storage,
			envExecutiveDefault: envDefault
		});
		this.executiveUi = resolved.executiveUi;
		this.theme = resolved.theme;
		this.navCollapsed = resolved.navCollapsed;
		this.favoriteProjects = resolved.favoriteProjects;
		this.singleKeyShortcuts = resolved.singleKeyShortcuts;
		this.#hydrated = true;

		// Persist explicit URL overrides so navigation preserves the chosen flag/theme.
		const params = new URLSearchParams(search);
		if (params.has('ui')) this.#save(STORAGE_KEYS.executiveUi, this.executiveUi ? '1' : '0');
		if (params.has('theme') && isUiTheme(params.get('theme'))) {
			this.#save(STORAGE_KEYS.theme, this.theme);
		}
		this.syncDom();
	}

	get hydrated(): boolean {
		return this.#hydrated;
	}

	setExecutiveUi(enabled: boolean): void {
		this.executiveUi = enabled;
		this.#save(STORAGE_KEYS.executiveUi, enabled ? '1' : '0');
		// If turning Executive UI on while still on untouched default, keep user's chosen theme.
		this.syncDom();
	}

	setTheme(next: UiTheme): void {
		if (!isUiTheme(next)) return;
		this.theme = next;
		this.#save(STORAGE_KEYS.theme, next);
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
		applyUiAttributes(document.documentElement, this.executiveUi, this.theme);
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
