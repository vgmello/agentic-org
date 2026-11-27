import type { ItemStatus, WorkItemSummary } from '$lib/proto';
import { areaCapability } from '$lib/platform/workspace/capabilities';
import { workApi } from '$lib/modules/work/client';
import { workItemsApi } from './api';
import type { TaskRow } from './types';

function summaryStatus(row: TaskRow): ItemStatus {
	if (row.type === 'bug') {
		if (row.status === 'done' || row.status === 'cancelled') return 'closed';
		if (row.awaitingVerification) return 'fixed';
		if (row.needsMe?.source === 'critical') return 'new';
		return row.status;
	}
	if (row.type === 'request') {
		if (row.status === 'done') return 'approved';
		if (row.status === 'cancelled') return row.merged ? 'merged' : 'declined';
		if (row.needsMe?.source === 'approval') return 'scoped';
		return row.status;
	}
	return row.status === 'cancelled' ? 'closed' : row.status;
}

function rowToSummary(row: TaskRow): WorkItemSummary {
	const numMatch = /\d+$/.exec(row.ref);
	return {
		id: row.id,
		short_id: row.ref,
		kind: row.type,
		number: numMatch ? Number(numMatch[0]) : 0,
		title: row.title,
		status: summaryStatus(row),
		...(row.type === 'task' && row.status === 'cancelled' ? { resolution: 'cancelled' } : {}),
		priority: row.priority,
		severity: row.severity ?? undefined,
		project_id: row.project?.id,
		assignee_id: row.assignee?.id,
		created_by: row.reporter.id,
		created_at: row.createdAt,
		updated_at: row.updatedAt,
		blocked_reason: row.blockedReason ?? undefined,
		severity_confirmed: row.type === 'bug',
		fixes: [],
		fixed_by: [],
		checklist: {
			total: row.checklist.total,
			done: row.checklist.done,
			required_left: row.checklist.requiredLeft
		},
		done_anyway: false,
		owner_office_id: '',
		generation: 1,
		reopen_count: row.reopenCount,
		version: row.revision,
		deleted: false
	};
}

export async function loadShellWorkItems(
	organizationId: string,
	signal?: AbortSignal
): Promise<WorkItemSummary[]> {
	if (areaCapability('/work').available) {
		try {
			const queue = await workItemsApi.queue(organizationId, { kind: 'organization' }, signal);
			return queue.rows.map(rowToSummary);
		} catch (err) {
			if (signal?.aborted) throw err;
			// Fall back to legacy workApi when workItemsApi is unavailable.
		}
	}
	return (await workApi.snapshot(organizationId, undefined, undefined, signal)).items;
}
