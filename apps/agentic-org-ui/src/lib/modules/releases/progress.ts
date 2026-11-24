// Release & milestone completion helpers (organization-data 02 §Completion of milestones and releases).
// A task is finished when its status is done or cancelled (cancelled tasks count as finished).
import type { ReleaseDetail, ReleaseMilestone } from './types';

function completionPct(finished: number, total: number, hasEmptyMilestone = false): number {
	if (total <= 0 || finished <= 0) return 0;
	if (finished >= total) return hasEmptyMilestone ? 99 : 100;
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
	let hasEmptyMilestone = false;
	for (const m of milestones) {
		const stats = computeReleaseMilestoneStats(m);
		if (stats.total === 0) hasEmptyMilestone = true;
		finished += stats.finished;
		total += stats.total;
	}
	return completionPct(finished, total, hasEmptyMilestone);
}

/**
 * Computes release readiness across the deduplicated union of milestone tasks and explicit
 * manifest tasks (`organization-data 02 §Completion of milestones and releases`).
 *
 * Milestone rows already aggregate every task assigned to each milestone (`tasksDone`,
 * `tasksOpen`, `tasksCancelled`). Explicit `detail.tasks` entries with no `milestoneRef`
 * (or referencing a milestone not already counted in `detail.milestones`) are added once.
 */
export function computeReleaseDetailReadiness(detail: ReleaseDetail): {
	finished: number;
	total: number;
	pct: number;
	scopeLabel: 'Release readiness';
} | null {
	let finished = 0;
	let total = 0;
	let hasEmptyMilestone = false;
	const countedMilestoneRefs = new Set<string>();

	for (const m of detail.milestones) {
		const stats = computeReleaseMilestoneStats(m);
		if (stats.total === 0) {
			hasEmptyMilestone = true;
		} else {
			countedMilestoneRefs.add(`M-${m.number}`.toUpperCase());
		}
		finished += stats.finished;
		total += stats.total;
	}

	for (const t of detail.tasks) {
		const ref = t.milestoneRef?.trim().toUpperCase();
		if (ref && countedMilestoneRefs.has(ref)) continue;
		total += 1;
		if (isReleaseTaskFinished(t.status)) finished += 1;
	}

	if (total <= 0) return null;

	return {
		finished,
		total,
		pct: completionPct(finished, total, hasEmptyMilestone),
		scopeLabel: 'Release readiness'
	};
}
