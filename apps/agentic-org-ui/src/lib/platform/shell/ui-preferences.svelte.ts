import { SvelteURLSearchParams } from 'svelte/reactivity';

export type UiTheme =
	'ocean' | 'classic' | 'graphite' | 'graphite-light' | 'cozy' | 'report' | 'report-dark';
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
		id: 'graphite',
		label: 'Graphite',
		shortLabel: 'Graphite',
		description:
			'Matte dark graphite surfaces, compact density, crisp hairline borders, and neutral charcoal accents.',
		scheme: 'dark'
	},
	{
		id: 'graphite-light',
		label: 'Graphite Light',
		shortLabel: 'G. Light',
		description:
			'Compact matte zinc light theme with white panels, crisp hairlines, monochrome selection, and high-contrast ink buttons.',
		scheme: 'light'
	},
	{
		id: 'cozy',
		label: 'Cozy Glass',
		shortLabel: 'Cozy',
		description:
			'Warm espresso frosted glass over an ambient studio room with emerald and teal accents.',
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
	cozyHighContrast: 'agentic-org.ui.cozyHighContrast',
	navCollapsed: 'agentic-org.ui.navCollapsed',
	favorites: 'agentic-org.ui.favoriteProjects',
	singleKey: 'agentic-org.ui.singleKey',
	assemblyDividers: 'agentic-org.ui.assemblyDividers',
	showTessPill: 'agentic-org.ui.showTessPill',
	archiveAfterDays: 'agentic-org.ui.archiveAfterDays',
	workspaceNav: 'agentic-org.ui.workspaceNav'
} as const;

export const WORKSPACE_NAV_IDS = ['work', 'chat', 'activity', 'projects', 'insights'] as const;
export type WorkspaceNavId = (typeof WORKSPACE_NAV_IDS)[number];
export const DEFAULT_PINNED_WORKSPACE_NAV: readonly WorkspaceNavId[] = [
	'work',
	'chat',
	'activity',
	'projects'
];
const VALID_WORKSPACE_NAV = new Set<string>(WORKSPACE_NAV_IDS);

/** Pure parser/normalizer for persisted pinned Workspace nav items (always keeps >= 1 valid item). */
export function normalizeWorkspaceNav(raw: unknown): WorkspaceNavId[] {
	if (!Array.isArray(raw)) return [...DEFAULT_PINNED_WORKSPACE_NAV];
	const out: WorkspaceNavId[] = [];
	for (const entry of raw) {
		if (typeof entry === 'string' && VALID_WORKSPACE_NAV.has(entry)) {
			const id = entry as WorkspaceNavId;
			if (!out.includes(id)) out.push(id);
		}
	}
	return out.length > 0 ? out : [...DEFAULT_PINNED_WORKSPACE_NAV];
}

/** Pure reorder/pin transition for Workspace sidebar items (drag-and-drop & Alt+ArrowUp/Down). */
export function moveWorkspaceNavItem(
	current: readonly WorkspaceNavId[],
	id: WorkspaceNavId,
	beforeId: WorkspaceNavId | null,
	zone: 'pinned' | 'more'
): WorkspaceNavId[] {
	const next = current.filter((item) => item !== id);
	if (zone === 'more') {
		return next.length > 0 ? next : [...current];
	}
	const insertAt = beforeId ? next.indexOf(beforeId) : -1;
	if (insertAt >= 0) next.splice(insertAt, 0, id);
	else next.push(id);
	return next;
}

export function toggleWorkspaceNavPin(
	current: readonly WorkspaceNavId[],
	id: WorkspaceNavId
): WorkspaceNavId[] {
	if (current.includes(id)) {
		return current.length > 1 ? current.filter((item) => item !== id) : [...current];
	}
	return [...current, id];
}

export const ARCHIVE_AFTER_DAYS_OPTIONS = [0, 3, 7, 10, 14, 30] as const;
export type ArchiveAfterDays = (typeof ARCHIVE_AFTER_DAYS_OPTIONS)[number];

export function normalizeArchiveAfterDays(value: unknown): ArchiveAfterDays {
	const num = typeof value === 'number' ? value : Number(value);
	if (Number.isInteger(num) && (ARCHIVE_AFTER_DAYS_OPTIONS as readonly number[]).includes(num)) {
		return num as ArchiveAfterDays;
	}
	return 10;
}

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
	cozyHighContrast: boolean;
	navCollapsed: boolean;
	favoriteProjects: string[];
	singleKeyShortcuts: boolean;
	assemblyDividers: boolean;
	showTessPill: boolean;
	archiveAfterDays: ArchiveAfterDays;
	workspaceNav: WorkspaceNavId[];
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
	let storedCozyContrast: string | null = null;
	let storedCollapsed: string | null = null;
	let storedFavs: string | null = null;
	let storedSingleKey: string | null = null;
	let storedAssemblyDividers: string | null = null;
	let storedShowTessPill: string | null = null;
	let storedArchiveDays: string | null = null;
	let storedWorkspaceNav: string | null = null;
	try {
		storedThemeRaw = storage?.getItem(STORAGE_KEYS.theme) ?? null;
		storedCozyContrast = storage?.getItem(STORAGE_KEYS.cozyHighContrast) ?? null;
		storedCollapsed = storage?.getItem(STORAGE_KEYS.navCollapsed) ?? null;
		storedFavs = storage?.getItem(STORAGE_KEYS.favorites) ?? null;
		storedSingleKey = storage?.getItem(STORAGE_KEYS.singleKey) ?? null;
		storedAssemblyDividers = storage?.getItem(STORAGE_KEYS.assemblyDividers) ?? null;
		storedShowTessPill = storage?.getItem(STORAGE_KEYS.showTessPill) ?? null;
		storedArchiveDays = storage?.getItem(STORAGE_KEYS.archiveAfterDays) ?? null;
		storedWorkspaceNav = storage?.getItem(STORAGE_KEYS.workspaceNav) ?? null;
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

	let workspaceNav: WorkspaceNavId[] = [...DEFAULT_PINNED_WORKSPACE_NAV];
	if (storedWorkspaceNav) {
		try {
			workspaceNav = normalizeWorkspaceNav(JSON.parse(storedWorkspaceNav));
		} catch {
			workspaceNav = [...DEFAULT_PINNED_WORKSPACE_NAV];
		}
	}

	return {
		theme,
		cozyHighContrast: storedCozyContrast === '1',
		navCollapsed: storedCollapsed === '1',
		favoriteProjects,
		singleKeyShortcuts: storedSingleKey !== '0',
		assemblyDividers: storedAssemblyDividers === '1',
		showTessPill: storedShowTessPill === '1',
		archiveAfterDays:
			storedArchiveDays !== null ? normalizeArchiveAfterDays(storedArchiveDays) : 10,
		workspaceNav
	};
}

/** Apply active theme attributes to the document root cleanly (zero inline scripts). */
export function applyUiAttributes(
	root: HTMLElement | undefined,
	theme: UiTheme,
	cozyHighContrast = false,
	assemblyDividers = false
): void {
	if (!root) return;
	root.dataset.uiMode = 'executive';
	root.dataset.uiTheme = theme;
	if (cozyHighContrast) {
		root.dataset.cozyContrast = 'high';
	} else {
		delete root.dataset.cozyContrast;
	}
	if (assemblyDividers) {
		root.dataset.assemblyDividers = '';
	} else {
		delete root.dataset.assemblyDividers;
	}
	const isLight = isLightTheme(theme);
	root.classList.toggle('dark', !isLight);
	root.classList.toggle('light', isLight);
}

function safeLocalStorage(): StorageLike | undefined {
	if (typeof window === 'undefined') return undefined;
	try {
		return window.localStorage;
	} catch {
		return undefined;
	}
}

class UiPreferencesStore {
	theme = $state<UiTheme>('ocean');
	cozyHighContrast = $state(false);
	navCollapsed = $state(false);
	favoriteProjects = $state<string[]>([]);
	singleKeyShortcuts = $state(true);
	assemblyDividers = $state(false);
	showTessPill = $state(false);
	archiveAfterDays = $state<ArchiveAfterDays>(10);
	workspaceNav = $state<WorkspaceNavId[]>([...DEFAULT_PINNED_WORKSPACE_NAV]);
	#hydrated = false;

	hydrate(search = typeof window !== 'undefined' ? window.location.search : ''): void {
		const storage = safeLocalStorage();
		const resolved = resolveInitialPreferences({ search, storage });
		this.theme = resolved.theme;
		this.cozyHighContrast = resolved.cozyHighContrast;
		this.navCollapsed = resolved.navCollapsed;
		this.favoriteProjects = resolved.favoriteProjects;
		this.singleKeyShortcuts = resolved.singleKeyShortcuts;
		this.assemblyDividers = resolved.assemblyDividers;
		this.showTessPill = resolved.showTessPill;
		this.archiveAfterDays = resolved.archiveAfterDays;
		this.workspaceNav = resolved.workspaceNav;
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

	setCozyHighContrast(enabled: boolean): void {
		this.cozyHighContrast = enabled;
		this.#save(STORAGE_KEYS.cozyHighContrast, enabled ? '1' : '0');
		this.syncDom();
	}

	toggleCozyHighContrast(): void {
		this.setCozyHighContrast(!this.cozyHighContrast);
	}

	toggleNavCollapsed(): void {
		this.navCollapsed = !this.navCollapsed;
		this.#save(STORAGE_KEYS.navCollapsed, this.navCollapsed ? '1' : '0');
	}

	setSingleKeyShortcuts(enabled: boolean): void {
		this.singleKeyShortcuts = enabled;
		this.#save(STORAGE_KEYS.singleKey, enabled ? '1' : '0');
	}

	setAssemblyDividers(enabled: boolean): void {
		this.assemblyDividers = enabled;
		this.#save(STORAGE_KEYS.assemblyDividers, enabled ? '1' : '0');
		this.syncDom();
	}

	toggleAssemblyDividers(): void {
		this.setAssemblyDividers(!this.assemblyDividers);
	}

	setShowTessPill(enabled: boolean): void {
		this.showTessPill = enabled;
		this.#save(STORAGE_KEYS.showTessPill, enabled ? '1' : '0');
	}

	toggleShowTessPill(): void {
		this.setShowTessPill(!this.showTessPill);
	}

	setArchiveAfterDays(days: ArchiveAfterDays): void {
		const normalized = normalizeArchiveAfterDays(days);
		this.archiveAfterDays = normalized;
		this.#save(STORAGE_KEYS.archiveAfterDays, String(normalized));
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

	moveWorkspaceItem(
		id: WorkspaceNavId,
		beforeId: WorkspaceNavId | null,
		zone: 'pinned' | 'more'
	): void {
		this.workspaceNav = moveWorkspaceNavItem(this.workspaceNav, id, beforeId, zone);
		this.#save(STORAGE_KEYS.workspaceNav, JSON.stringify(this.workspaceNav));
	}

	toggleWorkspacePin(id: WorkspaceNavId): void {
		this.workspaceNav = toggleWorkspaceNavPin(this.workspaceNav, id);
		this.#save(STORAGE_KEYS.workspaceNav, JSON.stringify(this.workspaceNav));
	}

	syncDom(): void {
		if (typeof document === 'undefined') return;
		applyUiAttributes(
			document.documentElement,
			this.theme,
			this.cozyHighContrast,
			this.assemblyDividers
		);
	}

	#save(key: string, value: string): void {
		const storage = safeLocalStorage();
		if (!storage) return;
		try {
			storage.setItem(key, value);
		} catch {
			// Ignore quota / private mode failures.
		}
	}
}

export const uiPrefs = new UiPreferencesStore();
