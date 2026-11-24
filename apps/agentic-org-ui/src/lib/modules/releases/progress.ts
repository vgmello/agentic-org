// Release & milestone completion helpers (organization-data 02 §Completion of milestones and releases).
// A task is finished when its status is done or cancelled (cancelled tasks count as finished).
import type { ReleaseDetail, ReleaseMilestone } from './types';

function completionPct(finished: number, total: number): number {
	if (total <= 0 || finished <= 0) return 0;
	if (finished >= total) return 100;
	return Math.min(99, Math.round((finished * 100) / total));
}

export function isReleaseTaskFinished(status: string): boolean {
	const normalized = status.trim().toLowerCase();
	return (
		normalized === 'done' ||
		normalized === 'closed' ||
		normalized === 'cancelled' ||
		normalized === 'canceled' ||
		normalized === 'completed'
	);
}

export function computeReleaseMilestoneStats(milestone: ReleaseMilestone): {
	finished: number;
	total: number;
	pct: number;
} {
	const finished = milestone.tasksDone + milestone.tasksCancelled;
	const total = finished + milestone.tasksOpen;
	return {
		finished,
		total,
		pct: completionPct(finished, total)
	};
}

export function computeReleaseMilestonesAggregatePct(
	milestones: readonly ReleaseMilestone[]
): number {
	let finished = 0;
	let total = 0;
	for (const m of milestones) {
		const stats = computeReleaseMilestoneStats(m);
		finished += stats.finished;
		total += stats.total;
	}
	return completionPct(finished, total);
}

export function computeReleaseDetailReadiness(detail: ReleaseDetail): {
	merged: number;
	total: number;
	pct: number;
	scopeLabel: 'Release readiness';
} | null {
	if (detail.tasks.length > 0) {
		const merged = detail.tasks.filter((t) => isReleaseTaskFinished(t.status)).length;
		const total = detail.tasks.length;
		return {
			merged,
			total,
			pct: completionPct(merged, total),
			scopeLabel: 'Release readiness'
		};
	}
	let finished = 0;
	let total = 0;
	for (const m of detail.milestones) {
		const stats = computeReleaseMilestoneStats(m);
		finished += stats.finished;
		total += stats.total;
	}
	if (total > 0) {
		return {
			merged: finished,
			total,
			pct: completionPct(finished, total),
			scopeLabel: 'Release readiness'
		};
	}
	return null;
}
