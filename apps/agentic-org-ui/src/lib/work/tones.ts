// Semantic tone mapping for Work items (tasks, bugs, requests) across all 4 themes.
// Every tone maps to the shared --tone-* / .t-green / .t-amber / .t-red token ramps.
import type { Priority, Severity, WorkItemSummary } from '$lib/proto';

export type WorkTone = 'ok' | 'warn' | 'bad' | 'accent' | 'neutral';

/** Semantic tone for a work item's status pill. */
export function statusTone(kind: WorkItemSummary['kind'], status: string): WorkTone {
	if (kind === 'bug') {
		if (status === 'fixed' || status === 'closed') return 'ok';
		if (status === 'in_progress' || status === 'triaged') return 'accent';
		if (status === 'new') return 'warn';
		return 'neutral';
	}
	if (kind === 'request') {
		if (status === 'approved') return 'ok';
		if (status === 'scoped') return 'warn';
		if (status === 'routed' || status === 'scoping') return 'accent';
		return 'neutral';
	}
	if (status === 'done') return 'ok';
	if (status === 'blocked') return 'bad';
	if (status === 'in_progress') return 'accent';
	return 'neutral';
}

/** Semantic tone for a task's priority pill. */
export function priorityTone(priority: Priority | undefined): WorkTone {
	if (priority === 'urgent') return 'bad';
	if (priority === 'high') return 'warn';
	return 'neutral';
}

/** Semantic tone for a bug's severity pill. */
export function severityTone(severity: Severity | undefined): WorkTone {
	if (severity === 'critical') return 'bad';
	if (severity === 'major') return 'warn';
	return 'neutral';
}
