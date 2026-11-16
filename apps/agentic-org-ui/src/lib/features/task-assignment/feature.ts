import type {
	createOperationClient,
	OperationTransport,
	Operation
} from '$lib/platform/http/client';
import type { createJsonHttp } from '$lib/platform/http/json-http';
import { createAssignmentTransport, createTaskReads } from './gateway';
import {
	completion,
	id,
	pair,
	publicRef,
	revision,
	type Completion,
	type Pair,
	type Scope
} from './wire';

type Options = {
	scope: Scope;
	http: ReturnType<typeof createJsonHttp>;
	read: Parameters<typeof createTaskReads>[0];
	operations: (transport: OperationTransport) => ReturnType<typeof createOperationClient>;
	storage: Pick<Storage, 'getItem' | 'setItem' | 'removeItem'>;
};
/** Public references only. Never persist intent, source content, password, cookie or proof here. */
export function createTaskFeature({ scope, http, read, operations, storage }: Options) {
	const bound = Object.freeze({ ...scope });
	for (const value of Object.values(bound)) id(value);
	function forTask(task: string) {
		id(task);
		const client = operations(createAssignmentTransport(http, bound, task));
		const key = `org-selected-operation:${JSON.stringify([bound.instanceId, bound.accountId, bound.organizationId, bound.officeId, task])}`;
		const decode = (v: unknown): Completion => completion(v, bound, task);
		return Object.freeze({
			prepare(expectedRevision: string, desired: Pair) {
				if (storage.getItem(key))
					throw new Error('Reconcile and acknowledge the original operation first.');
				const operation = client.prepare({
					kind: 'work',
					method: 'task.set_assignment',
					scope: bound,
					params: {
						task_id: task,
						command: { expected_revision: revision(expectedRevision), ...pair(desired) }
					},
					decode
				});
				const reference = publicRef({ kind: 'work', id: operation.id, scope: bound, taskId: task });
				// Storage failure prevents dispatch. Reload resumes lookup, never the command.
				storage.setItem(key, JSON.stringify(reference));
				return operation;
			},
			resume() {
				const raw = storage.getItem(key);
				if (!raw) return undefined;
				const ref = publicRef(JSON.parse(raw));
				if (
					Object.keys(bound).some(
						(key) => ref.scope[key as keyof Scope] !== bound[key as keyof Scope]
					) ||
					ref.taskId !== task
				)
					throw new Error('Operation belongs to a different account or task.');
				return client.resume(ref, decode);
			},
			/** Explicitly acknowledge a terminal result before preparing a fresh edit. */
			acknowledge(operation: Operation<Completion>) {
				const raw = storage.getItem(key);
				if (
					!raw ||
					publicRef(JSON.parse(raw)).id !== operation.id ||
					!['completed', 'rejected', 'not_submitted'].includes(operation.state.status)
				)
					throw new Error('The original outcome must be resolved first.');
				storage.removeItem(key);
			}
		});
	}
	return Object.freeze({ scope: bound, reads: createTaskReads(read, bound), forTask });
}
export type TaskFeature = ReturnType<typeof createTaskFeature>;
