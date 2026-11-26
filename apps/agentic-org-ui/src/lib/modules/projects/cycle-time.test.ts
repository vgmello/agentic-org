import { describe, expect, it } from 'vitest';
import type { WorkItemSummary } from '$lib/proto';
import { computeProjectCycleTime } from './cycle-time';

function item(overrides: Partial<WorkItemSummary>): WorkItemSummary {
	return {
		id: 'w-1',
		short_id: 'T-1',
		kind: 'task',
		title: 'Task',
		status: 'done',
		priority: 'normal',
		project_id: 'proj-1',
		created_at: '2026-11-24T00:00:00Z',
		updated_at: '2026-11-25T00:00:00Z',
		...overrides
	} as WorkItemSummary;
}

describe('computeProjectCycleTime', () => {
	const now = Date.parse('2026-11-26T12:00:00Z');

	it('returns honest empty labels when no closed tasks exist', () => {
		const res = computeProjectCycleTime([item({ status: 'in_progress' })], 'proj-1', now);
		expect(res.closedCount).toBe(0);
		expect(res.closedPerDayLabel).toBe('—');
		expect(res.medianCycleLabel).toBe('No closed tasks yet');
	});

	it('computes 7d throughput, median cycle time, and prior-week delta from real closed items', () => {
		const res = computeProjectCycleTime(
			[
				item({
					id: 'w-1',
					created_at: '2026-11-24T00:00:00Z',
					updated_at: '2026-11-25T00:00:00Z'
				}),
				item({
					id: 'w-2',
					created_at: '2026-11-23T00:00:00Z',
					updated_at: '2026-11-25T00:00:00Z'
				}),
				item({
					id: 'w-old',
					created_at: '2026-11-15T00:00:00Z',
					updated_at: '2026-11-16T00:00:00Z'
				})
			],
			'proj-1',
			now
		);
		expect(res.closedCount).toBe(3);
		expect(res.closedLast7Days).toBe(2);
		expect(res.closedPerDayLabel).toBe('0.3/day');
		expect(res.medianCycleLabel).toBe('1.0d median');
		expect(res.deltaLabel).toBe('+1 vs prior 7d');
		expect(res.deltaTone).toBe('ok');
	});
});
