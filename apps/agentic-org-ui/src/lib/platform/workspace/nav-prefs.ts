/**
 * Pure helpers for sidebar Workspace and Organization nav pin/reorder preferences.
 * Kept free of browser storage and domain imports so `ui-preferences.svelte.ts` stays lean
 * and every transition is deterministic and unit-tested.
 */

export const WORKSPACE_NAV_IDS = ['work', 'chat', 'activity', 'projects', 'insights'] as const;
export type WorkspaceNavId = (typeof WORKSPACE_NAV_IDS)[number];

/** All five workspace views pinned by default (Prototype Release c773480f1). */
export const DEFAULT_PINNED_WORKSPACE_NAV: readonly WorkspaceNavId[] = [
	'work',
	'chat',
	'activity',
	'projects',
	'insights'
];

export const ORG_NAV_IDS = ['teams', 'settings', 'processes'] as const;
export type OrgNavId = (typeof ORG_NAV_IDS)[number];

/** Teams and Settings pinned by default; Processes sits under More (Prototype Release c773480f1). */
export const DEFAULT_PINNED_ORG_NAV: readonly OrgNavId[] = ['teams', 'settings'];

const VALID_WORKSPACE_NAV = new Set<string>(WORKSPACE_NAV_IDS);
const VALID_ORG_NAV = new Set<string>(ORG_NAV_IDS);

function normalizePinnedList<T extends string>(
	raw: unknown,
	valid: ReadonlySet<string>,
	fallback: readonly T[]
): T[] {
	if (!Array.isArray(raw)) return [...fallback];
	const out: T[] = [];
	for (const entry of raw) {
		if (typeof entry === 'string' && valid.has(entry)) {
			const id = entry as T;
			if (!out.includes(id)) out.push(id);
		}
	}
	return out.length > 0 ? out : [...fallback];
}

function movePinnedItem<T extends string>(
	current: readonly T[],
	id: T,
	beforeId: T | null,
	zone: 'pinned' | 'more'
): T[] {
	const next = current.filter((item) => item !== id);
	if (zone === 'more') {
		return next.length > 0 ? next : [...current];
	}
	const insertAt = beforeId ? next.indexOf(beforeId) : -1;
	if (insertAt >= 0) next.splice(insertAt, 0, id);
	else next.push(id);
	return next;
}

function togglePinnedItem<T extends string>(current: readonly T[], id: T): T[] {
	if (current.includes(id)) {
		return current.length > 1 ? current.filter((item) => item !== id) : [...current];
	}
	return [...current, id];
}

/** Pure parser/normalizer for persisted pinned Workspace nav items (always keeps >= 1 valid item). */
export function normalizeWorkspaceNav(raw: unknown): WorkspaceNavId[] {
	return normalizePinnedList(raw, VALID_WORKSPACE_NAV, DEFAULT_PINNED_WORKSPACE_NAV);
}

/** Pure reorder/pin transition for Workspace sidebar items (drag-and-drop & Alt+ArrowUp/Down). */
export function moveWorkspaceNavItem(
	current: readonly WorkspaceNavId[],
	id: WorkspaceNavId,
	beforeId: WorkspaceNavId | null,
	zone: 'pinned' | 'more'
): WorkspaceNavId[] {
	return movePinnedItem(current, id, beforeId, zone);
}

export function toggleWorkspaceNavPin(
	current: readonly WorkspaceNavId[],
	id: WorkspaceNavId
): WorkspaceNavId[] {
	return togglePinnedItem(current, id);
}

/** Pure parser/normalizer for persisted pinned Organization nav items (always keeps >= 1 valid item). */
export function normalizeOrgNav(raw: unknown): OrgNavId[] {
	return normalizePinnedList(raw, VALID_ORG_NAV, DEFAULT_PINNED_ORG_NAV);
}

/** Pure reorder/pin transition for Organization sidebar items (drag-and-drop & Alt+ArrowUp/Down). */
export function moveOrgNavItem(
	current: readonly OrgNavId[],
	id: OrgNavId,
	beforeId: OrgNavId | null,
	zone: 'pinned' | 'more'
): OrgNavId[] {
	return movePinnedItem(current, id, beforeId, zone);
}

export function toggleOrgNavPin(current: readonly OrgNavId[], id: OrgNavId): OrgNavId[] {
	return togglePinnedItem(current, id);
}
