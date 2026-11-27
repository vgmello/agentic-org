// Pure presentation helpers for the compact Work list view (ported from poc/v6.html).
import type { WorkStatusRingTone } from '$lib/components/pills';
import type { Priority, WorkItemSummary } from '$lib/proto';
import { BUG_STATUS_LABEL } from './bugs';
import type { WorkTab } from './saved';
import { STATUS_LABEL } from './tasks';

export type { WorkStatusRingTone };
export type PriorityLevel = 'high' | 'medium' | 'low';
export type PriorityMeterTone = 'strong' | 'neutral';
export type ProjectDotTone = 'accent' | 'ok' | 'warn' | 'queued' | 'bad';

export interface PriorityMeter {
	level: PriorityLevel;
	label: string;
	tone: PriorityMeterTone;
}

export interface WorkStatusGroup {
	status: string;
	label: string;
	tone: WorkStatusRingTone;
	items: WorkItemSummary[];
}

const TASK_GROUP_ORDER = ['blocked', 'in_progress', 'todo', 'done'] as const;
const BUG_GROUP_ORDER = ['new', 'in_progress', 'triaged', 'fixed', 'closed'] as const;
const DOT_TONES: readonly ProjectDotTone[] = ['accent', 'ok', 'warn', 'queued', 'bad'];

/** Map a work item's status to its compact status-ring visual tone. */
export function workStatusTone(kind: WorkItemSummary['kind'], status: string): WorkStatusRingTone {
	if (status === 'in_progress' || status === 'scoping') return 'active';
	if (status === 'done' || status === 'fixed' || status === 'approved') return 'done';
	if (status === 'blocked' || status === 'declined') return 'blocked';
	if (status === 'triaged' || status === 'scoped' || status === 'routed') return 'review';
	if (kind === 'bug' && status === 'new') return 'active';
	return 'neutral';
}

/** Whether one work item currently waits on the owner (mirrors `needsYou` in `saved.ts`). */
export function needsOwnerAttention(item: WorkItemSummary, me: string): boolean {
	if (item.deleted) return false;
	if (item.kind === 'bug') return item.status === 'new' || item.status === 'fixed';
	if (item.kind === 'task') return item.status === 'blocked' && item.assignee_id === me;
	if (item.kind === 'request') return item.status === 'scoped';
	return false;
}

/** 3-bar priority meter metadata for explicit non-Normal priorities using neutral tones (TSK-10 reserves amber for Needs you). */
export function priorityMeter(priority: Priority | undefined): PriorityMeter | undefined {
	if (priority === 'urgent') return { level: 'high', label: 'Urgent priority', tone: 'strong' };
	if (priority === 'high') return { level: 'high', label: 'High priority', tone: 'neutral' };
	if (priority === 'low') return { level: 'low', label: 'Low priority', tone: 'neutral' };
	return undefined;
}

/** Deterministic semantic dot tone for a project/team pill (bound to theme `--tone-*` variables). */
export function projectDotTone(projectId: string | undefined): ProjectDotTone {
	if (!projectId) return 'accent';
	let hash = 0;
	for (let i = 0; i < projectId.length; i++) hash = (hash * 31 + projectId.charCodeAt(i)) >>> 0;
	return DOT_TONES[hash % DOT_TONES.length];
}

/** Status display label for a task or bug item. */
export function workStatusLabel(kind: WorkItemSummary['kind'], status: string): string {
	if (kind === 'bug') return BUG_STATUS_LABEL[status] ?? status;
	return STATUS_LABEL[status as keyof typeof STATUS_LABEL] ?? status;
}

/** Group work items into ordered status buckets, omitting empty buckets. */
export function groupWorkByStatus(
	items: readonly WorkItemSummary[],
	tab: Exclude<WorkTab, 'requests'>
): WorkStatusGroup[] {
	const kind = tab === 'bugs' ? 'bug' : 'task';
	const order: readonly string[] = tab === 'bugs' ? BUG_GROUP_ORDER : TASK_GROUP_ORDER;
	const buckets = new Map<string, WorkItemSummary[]>();
	for (const status of order) buckets.set(status, []);
	for (const item of items) {
		const list = buckets.get(item.status);
		if (list) list.push(item);
		else buckets.set(item.status, [item]);
	}
	const out: WorkStatusGroup[] = [];
	for (const [status, grouped] of buckets) {
		if (grouped.length === 0) continue;
		out.push({
			status,
			label: workStatusLabel(kind, status),
			tone: workStatusTone(kind, status),
			items: grouped
		});
	}
	return out;
}

/** Up to two initials for a compact assignee chip. */
export function assigneeInitials(name: string): string {
	const parts = name.trim().split(/\s+/).filter(Boolean);
	if (parts.length === 0) return '?';
	if (parts.length === 1) return parts[0].slice(0, 2).toUpperCase();
	return `${parts[0][0]}${parts[parts.length - 1][0]}`.toUpperCase();
}
