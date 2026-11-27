import { describe, expect, it } from 'vitest';
import type { ProjectTeam, WorkItemSummary } from '$lib/proto';
import type { InboxItem } from '$lib/platform/workspace/inbox-types';
import { deriveProjectHealth } from './project-health';

function makeWorkItem(overrides: Partial<WorkItemSummary>): WorkItemSummary {
	return {
		id: 'w-1',
		key: 'T-1',
		kind: 'task',
		title: 'Sample work',
		status: 'todo',
		project_id: 'proj-a',
		created_at: '2026-11-27T00:00:00Z',
		updated_at: '2026-11-27T00:00:00Z',
		version: 1,
		...overrides
	} as WorkItemSummary;
}

function makeInbox(overrides: Partial<InboxItem>): InboxItem {
	return {
		id: 'in-1',
		kind: 'approval',
		state: 'open',
		title: 'Approve release',
		summary: '',
		from: { id: 'ada', name: 'Ada', kind: 'agent' },
		project: { id: 'proj-a', name: 'Project A' },
		createdAt: '2026-11-27T00:00:00Z',
		urgent: false,
		...overrides
	} as InboxItem;
}

const assignedTeam: ProjectTeam[] = [
	{ id: 'pt-1', project_id: 'proj-a', team_id: 'team-1', version: 1 }
];

describe('deriveProjectHealth', () => {
	it('returns ok / On track when no blockers, bugs, or attention spikes exist', () => {
		expect(deriveProjectHealth('proj-a', [makeWorkItem({})], [], assignedTeam)).toEqual({
			tone: 'ok',
			label: 'On track'
		});
	});

	it('returns warn / At risk for a single blocked item, scoped request, 2+ inbox items, or unassigned team', () => {
		expect(
			deriveProjectHealth('proj-a', [makeWorkItem({ status: 'blocked' })], [], assignedTeam)
		).toEqual({ tone: 'warn', label: 'At risk' });

		expect(
			deriveProjectHealth(
				'proj-a',
				[makeWorkItem({ kind: 'request', status: 'scoped' })],
				[],
				assignedTeam
			)
		).toEqual({ tone: 'warn', label: 'At risk' });

		expect(
			deriveProjectHealth(
				'proj-a',
				[],
				[makeInbox({ id: 'in-1' }), makeInbox({ id: 'in-2' })],
				assignedTeam
			)
		).toEqual({ tone: 'warn', label: 'At risk' });

		expect(deriveProjectHealth('proj-a', [], [], [])).toEqual({
			tone: 'warn',
			label: 'At risk'
		});
	});

	it('returns bad / Off track for open critical/major bugs, 2+ blocked tasks, or problem + blocked', () => {
		expect(
			deriveProjectHealth(
				'proj-a',
				[makeWorkItem({ kind: 'bug', status: 'todo', severity: 'critical' })],
				[],
				assignedTeam
			)
		).toEqual({ tone: 'bad', label: 'Off track' });

		expect(
			deriveProjectHealth(
				'proj-a',
				[
					makeWorkItem({ id: 'w-1', status: 'blocked' }),
					makeWorkItem({ id: 'w-2', status: 'blocked' })
				],
				[],
				assignedTeam
			)
		).toEqual({ tone: 'bad', label: 'Off track' });

		expect(
			deriveProjectHealth(
				'proj-a',
				[makeWorkItem({ status: 'blocked' })],
				[makeInbox({ kind: 'problem' })],
				assignedTeam
			)
		).toEqual({ tone: 'bad', label: 'Off track' });
	});

	it('ignores deleted items, closed bugs, and handled inbox entries', () => {
		expect(
			deriveProjectHealth(
				'proj-a',
				[
					makeWorkItem({ kind: 'bug', status: 'closed', severity: 'critical' }),
					makeWorkItem({ status: 'blocked', deleted: true })
				],
				[makeInbox({ state: 'handled', kind: 'problem' })],
				assignedTeam
			)
		).toEqual({ tone: 'ok', label: 'On track' });
	});
});
