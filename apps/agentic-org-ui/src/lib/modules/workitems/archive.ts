// Auto-archive helpers for Work queues and member/project/team task views (TASK-131).
// Threshold is organization-wide (default 10 days); "Show archived" is strictly browser-session-scoped
// (sessionStorage) so fresh sessions always open clean.
import { isOpen } from './present';
import type { TaskRow } from './types';

export const SESSION_SHOW_ARCHIVED_KEY = 'agentic-org.ui.workShowArchived';

const MS_PER_DAY = 86_400_000;

export function isArchivedTimestamp(
	closedOrUpdatedAt: string | undefined,
	archiveAfterDays: number,
	nowMs = Date.now()
): boolean {
	if (archiveAfterDays <= 0 || !closedOrUpdatedAt) return false;
	const ts = Date.parse(closedOrUpdatedAt);
	if (!Number.isFinite(ts)) return false;
	return nowMs - ts >= archiveAfterDays * MS_PER_DAY;
}

export function isArchivedTaskRow(
	row: Pick<TaskRow, 'status' | 'statusAt' | 'updatedAt'>,
	archiveAfterDays: number,
	nowMs = Date.now()
): boolean {
	if (isOpen(row.status)) return false;
	return isArchivedTimestamp(row.statusAt || row.updatedAt, archiveAfterDays, nowMs);
}

export function readSessionShowArchived(): boolean {
	try {
		if (typeof window === 'undefined' || !window.sessionStorage) return false;
		return window.sessionStorage.getItem(SESSION_SHOW_ARCHIVED_KEY) === '1';
	} catch {
		return false;
	}
}

export function writeSessionShowArchived(enabled: boolean): void {
	try {
		if (typeof window === 'undefined' || !window.sessionStorage) return;
		if (enabled) {
			window.sessionStorage.setItem(SESSION_SHOW_ARCHIVED_KEY, '1');
		} else {
			window.sessionStorage.removeItem(SESSION_SHOW_ARCHIVED_KEY);
		}
	} catch {
		// Ignore private mode / storage quota errors.
	}
}
