import { isItemDone } from '$lib/format';
import type { WorkItemSummary } from '$lib/proto';

export interface ProjectCycleTimeSummary {
	closedCount: number;
	closedLast7Days: number;
	closedPerDayLabel: string;
	medianCycleLabel: string;
	deltaLabel?: string;
	deltaTone?: 'ok' | 'warn' | 'muted';
}

const DAY_MS = 24 * 60 * 60 * 1000;

function formatDurationMs(ms: number): string {
	const mins = Math.max(1, Math.round(ms / 60_000));
	if (mins < 60) return `${mins}m`;
	const hrs = mins / 60;
	if (hrs < 24) return `${hrs < 10 ? hrs.toFixed(1) : Math.round(hrs)}h`;
	const days = hrs / 24;
	return `${days < 10 ? days.toFixed(1) : Math.round(days)}d`;
}

function isClosedItem(item: WorkItemSummary): boolean {
	return !item.deleted && isItemDone(item);
}

function medianOf(values: number[]): number | undefined {
	if (values.length === 0) return undefined;
	const sorted = [...values].sort((a, b) => a - b);
	const mid = Math.floor(sorted.length / 2);
	return sorted.length % 2 === 1 ? sorted[mid] : (sorted[mid - 1] + sorted[mid]) / 2;
}

function summarizeWindowDelta(
	last7: number,
	prev7: number
): { deltaLabel?: string; deltaTone?: 'ok' | 'warn' | 'muted' } {
	if (last7 === 0 && prev7 === 0) return {};
	const diff = last7 - prev7;
	if (diff > 0) return { deltaLabel: `+${diff} vs prior 7d`, deltaTone: 'ok' };
	if (diff < 0) return { deltaLabel: `${diff} vs prior 7d`, deltaTone: 'warn' };
	return { deltaLabel: 'Steady vs prior 7d', deltaTone: 'muted' };
}

function formatRate(last7: number): string {
	if (last7 === 0) return '0/day';
	const rate = last7 / 7;
	return `${rate < 10 ? rate.toFixed(1) : Math.round(rate)}/day`;
}

function collectClosedMetrics(closed: readonly WorkItemSummary[], nowMs: number) {
	const sevenDaysAgo = nowMs - 7 * DAY_MS;
	const fourteenDaysAgo = nowMs - 14 * DAY_MS;
	let last7 = 0;
	let prev7 = 0;
	const durations: number[] = [];

	for (const item of closed) {
		const updated = Date.parse(item.updated_at);
		if (!Number.isNaN(updated)) {
			if (updated >= sevenDaysAgo) last7 += 1;
			else if (updated >= fourteenDaysAgo) prev7 += 1;
		}
		const created = Date.parse(item.created_at);
		if (!Number.isNaN(created) && !Number.isNaN(updated) && updated >= created) {
			durations.push(updated - created);
		}
	}
	return { last7, prev7, durations };
}

/**
 * Computes real 7-day throughput and median open-to-closed cycle time for a project
 * strictly from live WorkItemSummary records. Returns honest empty strings when none are closed.
 */
export function computeProjectCycleTime(
	items: readonly WorkItemSummary[],
	projectId: string,
	nowMs = Date.now()
): ProjectCycleTimeSummary {
	const closed = items.filter((item) => item.project_id === projectId && isClosedItem(item));
	if (closed.length === 0) {
		return {
			closedCount: 0,
			closedLast7Days: 0,
			closedPerDayLabel: '—',
			medianCycleLabel: 'No closed tasks yet'
		};
	}

	const { last7, prev7, durations } = collectClosedMetrics(closed, nowMs);
	const medianMs = medianOf(durations);
	const delta = summarizeWindowDelta(last7, prev7);

	return {
		closedCount: closed.length,
		closedLast7Days: last7,
		closedPerDayLabel: formatRate(last7),
		medianCycleLabel:
			medianMs !== undefined ? `${formatDurationMs(medianMs)} median` : `${closed.length} closed`,
		...delta
	};
}
