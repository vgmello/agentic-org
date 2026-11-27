import { describe, expect, it } from 'vitest';
import type { ProjectTeam, WorkItemSummary } from '$lib/proto';
import type { InboxItem } from './inbox-types';
import { classifyProjectHealth, deriveProjectHealth, isProjectOverdue } from './project-health';

function makeWorkItem(overrides: Partial<WorkItemSummary> = {}): WorkItemSummary {
	return {
		id: 'item-1',
		kind: 'task',
		project_id: 'proj-1',
		short_id: 'ORG-1',
		title: 'Ship work',
		status: 'in_progress',
		created_by: 'owner',
		created_at: '2026-11-27T10:00:00Z',
		updated_at: '2026-11-27T10:00:00Z',
		severity_confirmed: false,
		reopen_count: 0,
		fixes: [],
		fixed_by: [],
		checklist: { total: 0, done: 0, required_left: 0 },
		done_anyway: false,
		owner_office_id: 'hq',
		generation: 1,
		version: 1,
		deleted: false,
		...overrides
	};
}

function makeInbox(overrides: Partial<InboxItem> = {}): InboxItem {
	return {
		id: 'in-1',
		kind: 'approval',
		source: 'Release',
		title: 'Approve change',
		who: { id: 'rowan', name: 'Rowan', kind: 'agent' },
		project: { id: 'proj-1', name: 'Office federation' },
		why: 'Ready for review',
		created_at: '2026-11-27T10:00:00Z',
		actions: ['approve', 'deny'],
		state: 'open',
		...overrides
	};
}

const TEAM_LINK: ProjectTeam[] = [
	{ id: 'pt-1', project_id: 'proj-1', team_id: 'team-1', version: 1 }
];

describe('shared project-health classifier', () => {
	const NOW = Date.parse('2026-11-27T12:00:00Z');

	it('returns On track when no risk signals exist', () => {
		expect(
			deriveProjectHealth({
				projectId: 'proj-1',
				workItems: [makeWorkItem({ status: 'in_progress' })],
				inboxItems: [],
				projectTeams: TEAM_LINK,
				due: '2026-12-05',
				nowMs: NOW
			})
		).toEqual({ tone: 'ok', label: 'On track', statusPillClass: 't-green' });
	});

	it('returns At risk for overdue incomplete projects, single blocker, scoped request, >=2 inbox items, or no team', () => {
		expect(
			deriveProjectHealth({
				projectId: 'proj-1',
				workItems: [makeWorkItem({ status: 'in_progress' })],
				inboxItems: [],
				projectTeams: TEAM_LINK,
				due: '2026-11-20',
				nowMs: NOW
			})
		).toEqual({ tone: 'warn', label: 'At risk', statusPillClass: 't-amber' });

		// Completed project past due date is NOT overdue.
		expect(isProjectOverdue('2026-11-20', 2, 2, NOW)).toBe(false);

		expect(
			deriveProjectHealth({
				projectId: 'proj-1',
				workItems: [makeWorkItem({ status: 'blocked' })],
				inboxItems: [],
				projectTeams: TEAM_LINK,
				nowMs: NOW
			}).tone
		).toBe('warn');

		expect(
			deriveProjectHealth({
				projectId: 'proj-1',
				workItems: [makeWorkItem({ kind: 'request', status: 'scoped' })],
				inboxItems: [],
				projectTeams: TEAM_LINK,
				nowMs: NOW
			}).tone
		).toBe('warn');

		expect(
			deriveProjectHealth({
				projectId: 'proj-1',
				workItems: [],
				inboxItems: [makeInbox({ id: 'a' }), makeInbox({ id: 'b' })],
				projectTeams: TEAM_LINK,
				nowMs: NOW
			}).tone
		).toBe('warn');

		expect(
			deriveProjectHealth({
				projectId: 'proj-1',
				workItems: [],
				inboxItems: [],
				projectTeams: [],
				nowMs: NOW
			}).tone
		).toBe('warn');
	});

	it('returns Off track for critical/major open bugs, >=2 blocked items, or problem + blocked', () => {
		expect(
			deriveProjectHealth({
				projectId: 'proj-1',
				workItems: [makeWorkItem({ kind: 'bug', status: 'new', severity: 'critical' })],
				inboxItems: [],
				projectTeams: TEAM_LINK
			})
		).toEqual({ tone: 'bad', label: 'Off track', statusPillClass: 't-red' });

		expect(
			deriveProjectHealth({
				projectId: 'proj-1',
				workItems: [
					makeWorkItem({ id: 'a', status: 'blocked' }),
					makeWorkItem({ id: 'b', status: 'blocked' })
				],
				inboxItems: [],
				projectTeams: TEAM_LINK
			}).tone
		).toBe('bad');

		expect(
			deriveProjectHealth({
				projectId: 'proj-1',
				workItems: [makeWorkItem({ status: 'blocked' })],
				inboxItems: [makeInbox({ kind: 'problem' })],
				projectTeams: TEAM_LINK
			}).tone
		).toBe('bad');

		expect(
			classifyProjectHealth({
				criticalBugs: 0,
				blockedCount: 0,
				problemCount: 0,
				isOverdue: false,
				scopedReqsCount: 0,
				openInboxCount: 0,
				unassignedTeam: false
			})
		).toEqual({ tone: 'ok', label: 'On track', statusPillClass: 't-green' });
	});
});
