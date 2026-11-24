// Pure presentation, progress calculation, grouping, slug, and retirement validation helpers
// for the Project Features screens (features-ux §0–§3, FTR-1..14).
import type {
	FeatureDetail,
	FeatureProgress,
	FeatureRow,
	FeatureStatus,
	FeatureTaskItem
} from './types';

export const featuresHref = (projectId: string) =>
	`/projects/${encodeURIComponent(projectId)}/features`;

export const featureHref = (projectId: string, featureId: string) =>
	`${featuresHref(projectId)}/${encodeURIComponent(featureId)}`;

export const FEATURE_STATUS_WORD: Record<FeatureStatus, string> = {
	planned: 'Planned',
	in_progress: 'In progress',
	live: 'Live',
	retired: 'Retired'
};

export const FEATURE_STATUS_TONE: Record<FeatureStatus, 'neutral' | 'info' | 'ok' | 'owner'> = {
	planned: 'neutral',
	in_progress: 'info',
	live: 'ok',
	retired: 'neutral'
};

export const FEATURE_TEMPLATE = `## Purpose

## How it works

## Rules

## Known limitations

## Non-goals
`;

/** `F-6` inside the project, or `"Pending number"` while HQ hasn't minted a number yet (FTR-13). */
export function featureRef(f: Pick<FeatureRow, 'number'>): string {
	return f.number === null ? 'Pending number' : `F-${f.number}`;
}

/** `P-8/F-6` outside the project (features-ux Decision 1). */
export function qualifiedFeatureRef(f: Pick<FeatureRow, 'projectNumber' | 'number'>): string {
	return f.number === null ? `P-${f.projectNumber}/Pending` : `P-${f.projectNumber}/F-${f.number}`;
}

/** Derived public slug `<project-slug>/<local-slug>` (features-ux §1). */
export function featurePublicSlug(f: Pick<FeatureRow, 'projectSlug' | 'localSlug'>): string {
	return `${f.projectSlug}/${f.localSlug}`;
}

export function slugifyFeature(title: string): string {
	return title
		.normalize('NFKD')
		.replace(/[\u0300-\u036f]/g, '')
		.toLowerCase()
		.replace(/[^a-z0-9]+/g, '-')
		.replace(/^-+|-+$/g, '')
		.slice(0, 63)
		.replace(/-+$/g, '');
}

export function checkFeatureSlug(slug: string): string | undefined {
	const s = slug.trim();
	if (!s) return 'Enter a local slug such as quick-capture.';
	if (s.length > 63 || !/^[a-z0-9]+(-[a-z0-9]+)*$/.test(s)) {
		return 'Use lowercase letters, digits and single hyphens, starting and ending with a letter or digit.';
	}
	return undefined;
}

/**
 * Derived task progress (FTR-10): cancelled tasks are excluded from the denominator,
 * and an empty eligible set never implies 100% delivery.
 */
export function featureProgressStats(progress: FeatureProgress): {
	eligible: number;
	done: number;
	percent: number;
	label: string;
	cancelledNote: string | null;
} {
	const eligible = progress.done + progress.open + progress.blocked;
	const percent = eligible === 0 ? 0 : Math.round((progress.done / eligible) * 100);
	const label =
		eligible === 0
			? 'No tasks yet'
			: `${progress.done} of ${eligible} ${eligible === 1 ? 'task' : 'tasks'} completed`;
	const cancelledNote =
		progress.cancelled > 0 ? `${progress.cancelled} cancelled (excluded from progress)` : null;
	return { eligible, done: progress.done, percent, label, cancelledNote };
}

export interface FeaturesFilter {
	statuses: Set<FeatureStatus>;
	query: string;
}

export function defaultStatusFilters(): Set<FeatureStatus> {
	// Retired appears only when selected in the status filter (FTR-10).
	return new Set<FeatureStatus>(['in_progress', 'planned', 'live']);
}

export function matchesFeatureFilter(row: FeatureRow, filter: FeaturesFilter): boolean {
	if (!filter.statuses.has(row.status)) return false;
	const q = filter.query.trim().toLowerCase();
	if (!q) return true;
	return [featureRef(row), row.title, row.localSlug, featurePublicSlug(row), row.id].some((s) =>
		s.toLowerCase().includes(q)
	);
}

export interface FeatureGroup {
	status: FeatureStatus;
	label: string;
	rows: FeatureRow[];
}

const GROUP_ORDER: FeatureStatus[] = ['in_progress', 'planned', 'live', 'retired'];

export function groupFeatures(rows: readonly FeatureRow[]): FeatureGroup[] {
	const buckets = new Map<FeatureStatus, FeatureRow[]>();
	for (const r of rows) {
		const list = buckets.get(r.status) ?? [];
		list.push(r);
		buckets.set(r.status, list);
	}
	return GROUP_ORDER.flatMap((status) => {
		const list = buckets.get(status);
		return list && list.length > 0
			? [{ status, label: FEATURE_STATUS_WORD[status], rows: list }]
			: [];
	});
}

/**
 * Validates retirement inputs (FTR-8): requires `retired_reason` and forbids self-replacement
 * or replacement cycles within the project.
 */
export function validateRetirement(
	reason: string,
	replacementId: string | null,
	featureId: string,
	allFeatures: readonly Pick<FeatureRow, 'id' | 'replacedBy'>[]
): string | undefined {
	if (!reason.trim()) return 'Enter a reason for retiring this feature.';
	if (!replacementId) return undefined;
	if (replacementId === featureId) return 'A feature cannot replace itself.';
	const byId = new Map(allFeatures.map((f) => [f.id, f]));
	if (!byId.has(replacementId)) return 'Replacement must be another feature in this project.';
	const visited = new Set<string>([featureId]);
	let cursor: string | null = replacementId;
	while (cursor) {
		if (visited.has(cursor)) {
			return 'Choosing that replacement would create a replacement cycle.';
		}
		visited.add(cursor);
		cursor = byId.get(cursor)?.replacedBy?.id ?? null;
	}
	return undefined;
}

export const TASK_STATE_ORDER: FeatureTaskItem['status'][] = [
	'in_progress',
	'blocked',
	'todo',
	'done',
	'cancelled'
];

export const TASK_STATE_LABEL: Record<FeatureTaskItem['status'], string> = {
	in_progress: 'In progress',
	blocked: 'Blocked',
	todo: 'To do',
	done: 'Done',
	cancelled: 'Cancelled'
};

export function groupFeatureTasks(
	tasks: readonly FeatureTaskItem[]
): { status: FeatureTaskItem['status']; label: string; items: FeatureTaskItem[] }[] {
	const buckets = new Map<FeatureTaskItem['status'], FeatureTaskItem[]>();
	for (const t of tasks) {
		const list = buckets.get(t.status) ?? [];
		list.push(t);
		buckets.set(t.status, list);
	}
	return TASK_STATE_ORDER.flatMap((status) => {
		const items = buckets.get(status);
		return items && items.length > 0 ? [{ status, label: TASK_STATE_LABEL[status], items }] : [];
	});
}

export function formatMicroUsd(microUsd: number): string {
	const dollars = microUsd / 1_000_000;
	return dollars < 0.01 && dollars > 0 ? '< $0.01' : `$${dollars.toFixed(2)}`;
}

export function conflictLatestFeature(e: unknown): FeatureDetail | undefined {
	if (!e || typeof e !== 'object' || (e as { code?: unknown }).code !== 'conflict') {
		return undefined;
	}
	const latest = (e as { latest?: unknown }).latest;
	return latest && typeof latest === 'object' && 'revision' in latest
		? (latest as FeatureDetail)
		: undefined;
}
