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
 * 1. Prefers the authoritative `detail.progress` (`release_progress`) aggregate when supplied.
 * 2. When milestones exist alongside `detail.tasks`, deduplicates strictly by stable `milestoneId`
 *    (never by release-scoped `M-<n>` refs, which can collide across releases).
 * 3. If a manifest task carries only `milestoneRef` without `milestoneId` while milestones exist,
 *    membership cannot be deduplicated reliably, so the helper falls back honestly to the
 *    milestone subset labeled `'Milestone readiness'`.
 */
export function computeReleaseDetailReadiness(detail: ReleaseDetail): {
	finished: number;
	total: number;
	pct: number;
	scopeLabel: 'Release readiness' | 'Milestone readiness';
} | null {
	if (detail.progress && detail.progress.total > 0) {
		const { finished, total, isDone } = detail.progress;
		const pct = isDone === false && finished >= total ? 99 : completionPct(finished, total);
		return {
			finished,
			total,
			pct,
			scopeLabel: 'Release readiness'
		};
	}

	let milestoneFinished = 0;
	let milestoneTotal = 0;
	let hasEmptyMilestone = false;
	const countedMilestoneIds = new Set<string>();

	for (const m of detail.milestones) {
		const stats = computeReleaseMilestoneStats(m);
		if (stats.total === 0) {
			hasEmptyMilestone = true;
		} else {
			countedMilestoneIds.add(m.id);
		}
		milestoneFinished += stats.finished;
		milestoneTotal += stats.total;
	}

	// If milestones exist and any manifest task carries only a release-scoped `milestoneRef`
	// without a stable `milestoneId`, we cannot prove whether it belongs to this release's milestone
	// or another release's milestone — report the milestone subset honestly instead of guessing.
	const hasAmbiguousMilestoneRef =
		detail.milestones.length > 0 &&
		detail.tasks.some((t) => Boolean(t.milestoneRef?.trim()) && !t.milestoneId?.trim());

	if (hasAmbiguousMilestoneRef) {
		if (milestoneTotal <= 0) return null;
		return {
			finished: milestoneFinished,
			total: milestoneTotal,
			pct: completionPct(milestoneFinished, milestoneTotal, hasEmptyMilestone),
			scopeLabel: 'Milestone readiness'
		};
	}

	let finished = milestoneFinished;
	let total = milestoneTotal;

	for (const t of detail.tasks) {
		const msId = t.milestoneId?.trim();
		if (msId && countedMilestoneIds.has(msId)) continue;
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
