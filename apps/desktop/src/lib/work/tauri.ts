import { invoke } from '@tauri-apps/api/core';
import type { WorkAccepted, WorkItem, WorkItemSummary, WorkPosition } from '$lib/proto';
import type { WorkApi } from './api';

let queue: Promise<unknown> = Promise.resolve();

/** The Rust shell work calls (its mock office applies the same rules as the browser mock: see test-vectors/work-commands.json). */
export const tauriWork: WorkApi = {
	snapshot: (organizationId, projectId, closedSince) =>
		invoke<{ position: WorkPosition; items: WorkItemSummary[] }>('work_snapshot', { organizationId, projectId, closedSince }),
	get: (organizationId, itemId) => invoke<WorkItem>('work_get', { organizationId, itemId }),
	command(organizationId, call) {
		// One at a time, in the order sent: the shell runs each call on its own thread, which keeps no order, so a
		// change that depends on the one before it must not overtake it.
		const answer = queue.then(() => invoke<WorkAccepted>('work_command', { organizationId, call }));
		queue = answer.catch(() => undefined);
		return answer;
	}
};
