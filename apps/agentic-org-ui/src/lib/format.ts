import type { WorkItemSummary } from '$lib/proto';

/** Up to two capital letters for an avatar: the first letters of the first and last word. */
export function initialsOf(name: string): string {
	const parts = name.trim().split(/\s+/).filter(Boolean);
	if (parts.length === 0) return '?';
	const first = [...parts[0]][0];
	const last = parts.length > 1 ? [...parts[parts.length - 1]][0] : '';
	return (first + last).toUpperCase();
}

/** Two-letter badge initials: first two letters for single-word names, first+last for multi-word names. */
export function badgeInitials(name: string): string {
	const parts = name.trim().split(/\s+/).filter(Boolean);
	if (parts.length === 0) return '?';
	if (parts.length === 1) return parts[0].slice(0, 2).toUpperCase();
	return (parts[0][0] + parts[parts.length - 1][0]).toUpperCase();
}

/** Concise relative timestamp label (`Just now`, `12m ago`, `3h ago`, `2d ago`). */
export function relativeAge(iso: string, nowMs: number): string {
	const ts = Date.parse(iso);
	if (Number.isNaN(ts)) return 'recently';
	const diffMin = Math.max(0, Math.round((nowMs - ts) / 60_000));
	if (diffMin < 2) return 'Just now';
	if (diffMin < 60) return `${diffMin}m ago`;
	const diffH = Math.round(diffMin / 60);
	if (diffH < 24) return `${diffH}h ago`;
	const diffD = Math.round(diffH / 24);
	return `${diffD}d ago`;
}

/** Whether a work item has reached its completed state across tasks, bugs, and requests. */
export function isItemDone(item: WorkItemSummary): boolean {
	if (item.deleted) return false;
	return (
		(item.kind === 'task' && item.status === 'done') ||
		(item.kind === 'bug' && item.status === 'closed') ||
		(item.kind === 'request' && item.status === 'approved')
	);
}
