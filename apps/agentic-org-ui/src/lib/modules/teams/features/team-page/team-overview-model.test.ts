import { describe, expect, it } from 'vitest';
import type { TeamDetail, TeamProjectRow } from '../../types';
import type { ActivityEntry, WorkRow } from '$lib/modules/members';
import {
	TEAM_OVERVIEW_MIN_TIMING_SAMPLES,
	computeMedian,
	deriveTeamOverview,
	formatDurationSeconds,
	type TeamCompletionRecord
} from './team-overview-model';

const NOW_MS = Date.UTC(2026, 10, 24, 15, 0, 0); // 2026-11-24T15:00:00Z

const DETAIL: TeamDetail = {
	team: {
		id: 'web',
		name: 'Web',
		ref: 'TEAM-2',
		lead: { id: 'noor', name: 'Noor Haddad' },
		people: 1,
		agents: 1,
		projects: [{ id: 'beacon', name: 'Beacon', role: 'owner' }],
		maxMembers: 6,
		status: 'active'
	},
	path: [{ id: 'eng', name: 'Engineering', ref: 'TEAM-1' }],
	purpose: 'Owns the customer checkout experience.',
	members: [
		{ id: 'noor', name: 'Noor Haddad', kind: 'person', handle: 'noor', lead: true },
		{ id: 'mira', name: 'Mira Castillo', kind: 'agent', handle: 'mira', lead: false }
	]
};

const WORK_ROWS: WorkRow[] = [
	{
		ref: 'T-A40',
		title: 'Split checkout',
		project: 'Beacon',
		assigneeId: 'sam',
		assignee: 'Sam Okafor',
		state: 'in_progress'
	},
	{
		ref: 'T-A42',
		title: 'Pay button wraps',
		project: 'Beacon',
		assigneeId: 'noor',
		assignee: 'Noor Haddad',
		state: 'in_review'
	},
	{
		ref: 'T-A44',
		title: 'Retry policy',
		project: 'Beacon',
		assigneeId: 'mira',
		assignee: 'Mira Castillo',
		state: 'blocked'
	},
	{ ref: 'T-A49', title: 'Unassigned backlog', project: 'Beacon', state: 'todo' },
	{
		ref: 'T-A31',
		title: 'Old done task',
		project: 'Beacon',
		assigneeId: 'noor',
		assignee: 'Noor Haddad',
		state: 'done'
	}
];

const PROJECTS: TeamProjectRow[] = [
	{
		id: 'beacon',
		name: 'Beacon',
		key: 'P-8',
		role: 'owner',
		openWork: 4,
		tasks: { todo: 1, inProgress: 2, blocked: 1, done: 1, cancelled: 0 },
		bugs: { critical: 0, major: 0, minor: 0, trivial: 0, unset: 0 }
	}
];

const ACTIVITY: ActivityEntry[] = [
	{ id: 'a1', at: '2026-11-20T10:00:00Z', actor: 'Noor Haddad', kind: 'work', text: 'oldest' },
	{ id: 'a2', at: '2026-11-24T12:00:00Z', actor: 'Mira Castillo', kind: 'work', text: 'newest' },
	{ id: 'a3', at: '2026-11-23T12:00:00Z', actor: 'Noor Haddad', kind: 'work', text: 'second' },
	{ id: 'a4', at: '2026-11-22T12:00:00Z', actor: 'Mira Castillo', kind: 'work', text: 'third' }
];

describe('deriveTeamOverview', () => {
	it('reconciles open work, blocked count, status order (Blocked first), real project progress, and alphabetical contributors when completion transitions are unavailable', () => {
		const model = deriveTeamOverview({
			detail: DETAIL,
			workRows: WORK_ROWS,
			workState: 'ready',
			projects: PROJECTS,
			activity: ACTIVITY,
			completions: null,
			memberNameById: { sam: 'Sam Okafor' },
			nowMs: NOW_MS
		});

		expect(model.windowLabel).toBe('Oct 28–Nov 24');
		expect(model.workAvailable).toBe(true);
		expect(model.completionsAvailable).toBe(false);
		expect(model.completedTotal).toBeNull();
		expect(model.kpis.find((k) => k.id === 'completed')?.value).toBe('—');
		expect(model.kpis.find((k) => k.id === 'cycle_time')?.value).toBe('—');
		expect(model.kpis.find((k) => k.id === 'approval_wait')?.value).toBe('—');
		expect(model.kpis.find((k) => k.id === 'blocked')).toEqual({
			id: 'blocked',
			label: 'Blocked now',
			value: '1',
			note: 'Of 4 open items'
		});

		// Blocked first, then In progress, In review, To do
		expect(model.openByStatus.map((r) => [r.state, r.count])).toEqual([
			['blocked', 1],
			['in_progress', 1],
			['in_review', 1],
			['todo', 1]
		]);

		// Active project progress and health computed from real TeamProjectRow counts
		expect(model.activeProjects).toHaveLength(1);
		expect(model.activeProjects[0]).toMatchObject({
			id: 'beacon',
			healthLabel: 'Needs attention',
			tone: 'warn',
			progressPct: 20,
			doneTasks: 1,
			totalTasks: 5
		});

		// Contributors sorted A-Z with stable IDs and unassigned reconciled
		expect(model.contributors).toEqual([
			{
				key: 'id:mira',
				memberId: 'mira',
				name: 'Mira Castillo',
				completed: null,
				open: 1,
				blocked: 1,
				unresolvedAttribution: false
			},
			{
				key: 'id:noor',
				memberId: 'noor',
				name: 'Noor Haddad',
				completed: null,
				open: 1,
				blocked: 0,
				unresolvedAttribution: false
			},
			{
				key: 'id:sam',
				memberId: 'sam',
				name: 'Sam Okafor',
				completed: null,
				open: 1,
				blocked: 0,
				unresolvedAttribution: false
			}
		]);
		expect(model.unassignedOpen).toBe(1);
		expect(model.unassignedBlocked).toBe(0);
		expect(model.unresolvedAttributionOpen).toBe(0);

		// Top 3 newest activity entries
		expect(model.recentActivity.map((e) => e.id)).toEqual(['a2', 'a3', 'a4']);
	});

	it('never presents unavailable or loading work/project data as zeros or Active health', () => {
		const unavailableModel = deriveTeamOverview({
			detail: DETAIL,
			workRows: undefined,
			workState: 'unavailable',
			projects: [{ id: 'checkout', name: 'Checkout', key: 'P-1', role: 'owner' }],
			activity: undefined,
			completions: null,
			nowMs: NOW_MS
		});

		expect(unavailableModel.workAvailable).toBe(false);
		expect(unavailableModel.openTotal).toBeNull();
		expect(unavailableModel.blockedTotal).toBeNull();
		expect(unavailableModel.kpis.find((k) => k.id === 'blocked')).toEqual({
			id: 'blocked',
			label: 'Blocked now',
			value: '—',
			note: 'Open work not served yet'
		});
		expect(unavailableModel.activeProjects[0]).toMatchObject({
			id: 'checkout',
			healthLabel: null,
			tone: null,
			progressPct: null,
			progressText: "Progress and health aren't served by this server yet."
		});

		const loadingModel = deriveTeamOverview({
			detail: DETAIL,
			workRows: undefined,
			workState: 'loading',
			nowMs: NOW_MS
		});
		expect(loadingModel.kpis.find((k) => k.id === 'blocked')?.value).toBe('—');
		expect(loadingModel.kpis.find((k) => k.id === 'blocked')?.note).toBe('Loading open work…');

		// Partial project data (task counts served, bug counts absent): show task progress without fabricating "On track".
		const partialProjectModel = deriveTeamOverview({
			detail: DETAIL,
			workRows: [],
			workState: 'ready',
			projects: [
				{
					id: 'partial',
					name: 'Partial',
					role: 'owner',
					tasks: { todo: 0, inProgress: 0, blocked: 0, done: 2, cancelled: 0 }
				}
			],
			nowMs: NOW_MS
		});
		expect(partialProjectModel.activeProjects[0]).toMatchObject({
			id: 'partial',
			healthLabel: null,
			tone: null,
			progressPct: 100,
			progressText: '2 of 2 tasks done',
			doneTasks: 2,
			totalTasks: 2
		});
	});

	it('separates duplicate display names by stable assignee ID and protects against prototype keys and name-only rows', () => {
		const model = deriveTeamOverview({
			detail: DETAIL,
			workRows: [
				{
					ref: 'T-1',
					title: 'Alex A task',
					assigneeId: 'alex-1',
					assignee: 'Alex',
					state: 'in_progress'
				},
				{
					ref: 'T-2',
					title: 'Alex B task',
					assigneeId: 'alex-2',
					assignee: 'Alex',
					state: 'blocked'
				},
				{
					ref: 'T-3',
					title: 'Prototype-named assignee',
					assignee: 'Constructor',
					state: 'todo'
				},
				{
					ref: 'T-4',
					title: 'Name-only Noor without stable ID',
					assignee: 'Noor Haddad',
					state: 'in_review'
				}
			],
			workState: 'ready',
			memberNameById: { 'alex-1': 'Alex', 'alex-2': 'Alex' },
			nowMs: NOW_MS
		});

		const alexRows = model.contributors.filter((c) => c.name === 'Alex');
		expect(alexRows).toHaveLength(2);
		expect(alexRows.map((c) => c.memberId)).toEqual(['alex-1', 'alex-2']);
		expect(alexRows[0]).toMatchObject({ open: 1, blocked: 0, unresolvedAttribution: false });
		expect(alexRows[1]).toMatchObject({ open: 1, blocked: 1, unresolvedAttribution: false });

		const ctorRow = model.contributors.find((c) => c.name === 'Constructor');
		expect(ctorRow).toEqual({
			key: 'unresolved:constructor',
			memberId: undefined,
			name: 'Constructor',
			completed: null,
			open: 1,
			blocked: 0,
			unresolvedAttribution: true
		});

		const nameOnlyNoor = model.contributors.find((c) => c.name === 'Noor Haddad');
		expect(nameOnlyNoor?.memberId).toBeUndefined();
		expect(nameOnlyNoor?.unresolvedAttribution).toBe(true);
		expect(model.unresolvedAttributionOpen).toBe(2);
	});

	it('enforces the >= 5 completion threshold for timing medians and buckets completions into 4x7d UTC weeks', () => {
		const fourCompletions: TeamCompletionRecord[] = [
			{
				completedAt: '2026-11-24T09:00:00Z',
				kind: 'task',
				contributorId: 'noor',
				cycleSeconds: 3600,
				approvalWaitSeconds: 1800
			},
			{
				completedAt: '2026-11-23T09:00:00Z',
				kind: 'bug',
				contributorId: 'mira',
				cycleSeconds: 7200,
				approvalWaitSeconds: 600
			},
			{
				completedAt: '2026-11-15T09:00:00Z',
				kind: 'task',
				contributorId: 'noor',
				cycleSeconds: 10800,
				approvalWaitSeconds: 1200
			},
			{
				completedAt: '2026-11-01T09:00:00Z',
				kind: 'task',
				contributorId: 'noor',
				cycleSeconds: 14400,
				approvalWaitSeconds: 2400
			}
		];

		const sparse = deriveTeamOverview({
			detail: DETAIL,
			workRows: [],
			workState: 'ready',
			completions: fourCompletions,
			nowMs: NOW_MS
		});
		expect(sparse.completedTotal).toBe(4);
		expect(sparse.kpis.find((k) => k.id === 'cycle_time')?.value).toBe('—');
		expect(sparse.kpis.find((k) => k.id === 'cycle_time')?.note).toBe(
			`At least ${TEAM_OVERVIEW_MIN_TIMING_SAMPLES} completions needed`
		);
		expect(sparse.kpis.find((k) => k.id === 'approval_wait')?.value).toBe('—');

		const fiveCompletions: TeamCompletionRecord[] = [
			...fourCompletions,
			{
				completedAt: '2026-11-20T12:00:00Z',
				kind: 'bug',
				contributorId: 'mira',
				cycleSeconds: 5400,
				approvalWaitSeconds: 900
			},
			// Outside 28-day window -> excluded
			{
				completedAt: '2026-10-20T12:00:00Z',
				kind: 'task',
				contributorId: 'noor',
				cycleSeconds: 99999,
				approvalWaitSeconds: 99999
			}
		];

		const full = deriveTeamOverview({
			detail: DETAIL,
			workRows: [],
			workState: 'ready',
			completions: fiveCompletions,
			nowMs: NOW_MS
		});
		expect(full.completedTotal).toBe(5);
		expect(full.weekly.reduce((sum, b) => sum + b.total, 0)).toBe(5);
		expect(full.kpis.find((k) => k.id === 'cycle_time')?.value).toBe('2h');
		expect(full.kpis.find((k) => k.id === 'approval_wait')?.value).toBe('20m');
	});
});

describe('math & formatting helpers', () => {
	it('computes odd and even medians cleanly', () => {
		expect(computeMedian([])).toBeNull();
		expect(computeMedian([10, 30, 20])).toBe(20);
		expect(computeMedian([10, 40, 20, 30])).toBe(25);
	});

	it('formats short and long durations cleanly', () => {
		expect(formatDurationSeconds(30)).toBe('<1m');
		expect(formatDurationSeconds(900)).toBe('15m');
		expect(formatDurationSeconds(5400)).toBe('1.5h');
		expect(formatDurationSeconds(7200)).toBe('2h');
		expect(formatDurationSeconds(3 * 86400)).toBe('3d');
	});
});
