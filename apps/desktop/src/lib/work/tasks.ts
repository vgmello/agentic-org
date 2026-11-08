// What the task board says and decides, as pure functions (work-items-ux §9; TSK-1..TSK-10).
import type { Priority, TaskStatus, WorkItemSummary } from '$lib/proto';
import { TASK_STATUSES } from '$lib/proto';

export const STATUS_LABEL: Record<TaskStatus, string> = { todo: 'To do', in_progress: 'In progress', blocked: 'Blocked', done: 'Done' };
export const PRIORITIES: { id: Exclude<Priority, 'unknown'>; label: string }[] = [
	{ id: 'urgent', label: 'Urgent' },
	{ id: 'high', label: 'High' },
	{ id: 'normal', label: 'Normal' },
	{ id: 'low', label: 'Low' }
];
const RANK: Record<string, number> = { urgent: 0, high: 1, normal: 2, low: 3 };

/** "T-12", or "Pending · 3k9f2x" while HQ has not numbered it. */
export const taskRef = (t: WorkItemSummary): string => (t.number ? `T-${t.number}` : `Pending · ${t.short_id}`);

/** Priority is shown only when it isn't Normal (TSK-10): its word, or nothing. */
export const priorityWord = (p: Priority | undefined): string | undefined => (p && p !== 'normal' && p !== 'unknown' ? (PRIORITIES.find((x) => x.id === p)?.label ?? undefined) : undefined);

/** Urgent first, then oldest first (TSK-4). */
export const byPriorityThenAge = (a: WorkItemSummary, b: WorkItemSummary): number =>
	(RANK[a.priority ?? 'normal'] ?? 2) - (RANK[b.priority ?? 'normal'] ?? 2) || a.created_at.localeCompare(b.created_at) || (a.number ?? 0) - (b.number ?? 0);

export const DAY_MS = 24 * 60 * 60 * 1000;

export interface Column {
	status: TaskStatus;
	label: string;
	tasks: WorkItemSummary[];
}

/** The board's four columns. The Done column holds what the board asked for: the last 7 days, or all (TSK-1, `work.snapshot { closed_since }`). */
export const columns = (tasks: WorkItemSummary[]): Column[] =>
	TASK_STATUSES.map((status) => ({ status, label: STATUS_LABEL[status], tasks: tasks.filter((t) => t.kind === 'task' && t.status === status).sort(byPriorityThenAge) }));

/** The cutoff for "the last 7 days" of Done. */
export const lastWeek = (now: Date = new Date()): string => new Date(now.getTime() - 7 * DAY_MS).toISOString();

/** Where a task can be moved from where it is (the state machine, design §1.10). */
export const movesFrom = (status: WorkItemSummary['status']): TaskStatus[] =>
	status === 'done' ? ['todo', 'in_progress'] : TASK_STATUSES.filter((s) => s !== status);

/** "2 of 5 steps" for a checklist, empty when there is none. */
export function checklistWords(t: WorkItemSummary): string {
	const { total, done } = t.checklist;
	return total === 0 ? '' : `${done} of ${total} ${total === 1 ? 'step' : 'steps'}`;
}
