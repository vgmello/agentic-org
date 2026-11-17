// Fake host integrations for the POC: a task board (`task` capability) and project facts
// (`project` capability). A real host implements the same contracts from office data.

import type { Facts } from './engine';
import type { FactSource, Runner } from './runtime';

export const TASK_STATUSES = ['todo', 'in_progress', 'in_review', 'done'] as const;
export type TaskStatus = (typeof TASK_STATUSES)[number];

export interface HostState {
	tasks: Record<string, { status: TaskStatus }>;
	project: { standards_defined: boolean };
}

export const emptyHost = (): HostState => ({
	tasks: { 'T-1': { status: 'todo' } },
	project: { standards_defined: false }
});

/** Facts for a run's subject, only for the capabilities the host provides. */
export function factSource(host: HostState, capabilities: string[]): FactSource {
	return (run) => {
		const facts: Facts = {};
		if (capabilities.includes('task')) {
			const id = run.subject.task ?? '';
			facts.task = { id, status: host.tasks[id]?.status ?? '' };
		}
		if (capabilities.includes('project')) facts.project = { ...host.project };
		return facts;
	};
}

/** A person, agent or script moves a task. With the `task` capability the runner hears about it. */
export function moveTask(host: HostState, runner: Runner, id: string, to: TaskStatus): void {
	const task = host.tasks[id];
	if (!task) throw new Error(`unknown task ${id}`);
	const from = task.status;
	if (from === to) return;
	task.status = to;
	if (runner.flow.capabilities.includes('task')) runner.deliver({ type: 'task.status_changed', payload: { from, to } }, { task: id });
}
