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
		assignee: 'Sam Okafor',
		state: 'in_progress'
	},
	{
		ref: 'T-A42',
		title: 'Pay button wraps',
		project: 'Beacon',
		assignee: 'Noor Haddad',
		state: 'in_review'
	},
	{
		ref: 'T-A44',
		title: 'Retry policy',
		project: 'Beacon',
		assignee: 'Mira Castillo',
		state: 'blocked'
	},
	{ ref: 'T-A49', title: 'Unassigned backlog', project: 'Beacon', state: 'todo' },
	{
		ref: 'T-A31',
		title: 'Old done task',
		project: 'Beacon',
		assignee: 'Noor Haddad',
		state: 'done'
	}
];

const PROJECTS: TeamProjectRow[] = [
	{ id: 'beacon', name: 'Beacon', key: 'P-8', role: 'owner', openWork: 4 }
];

const ACTIVITY: ActivityEntry[] = [
	{ id: 'a1', at: '2026-11-20T10:00:00Z', actor: 'Noor Haddad', kind: 'work', text: 'oldest' },
	{ id: 'a2', at: '2026-11-24T12:00:00Z', actor: 'Mira Castillo', kind: 'work', text: 'newest' },
	{ id: 'a3', at: '2026-11-23T12:00:00Z', actor: 'Noor Haddad', kind: 'work', text: 'second' },
	{ id: 'a4', at: '2026-11-22T12:00:00Z', actor: 'Mira Castillo', kind: 'work', text: 'third' }
];

describe('deriveTeamOverview', () => {
	it('reconciles open work, blocked count, status order (Blocked first), and alphabetical contributors when completion transitions are unavailable', () => {
		const model = deriveTeamOverview({
			detail: DETAIL,
			workRows: WORK_ROWS,
			projects: PROJECTS,
			activity: ACTIVITY,
			completions: null,
			memberIdByName: { 'Sam Okafor': 'sam' },
			nowMs: NOW_MS
		});

		expect(model.windowLabel).toBe('Oct 28–Nov 24');
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

		// Blocked appears first in open-work status breakdown and sums strictly to openTotal.
		expect(model.openByStatus.map((s) => s.state)).toEqual([
			'blocked',
			'in_progress',
			'in_review',
			'todo'
		]);
		expect(model.openByStatus.reduce((sum, s) => sum + s.count, 0)).toBe(model.openTotal);

		// Alphabetical contributor order and strict open/blocked reconciliation with unassigned items.
		expect(model.contributors.map((c) => c.name)).toEqual([
			'Mira Castillo',
			'Noor Haddad',
			'Sam Okafor'
		]);
		expect(model.contributors.every((c) => c.completed === null)).toBe(true);
		const sumOpen = model.contributors.reduce((s, c) => s + c.open, 0) + model.unassignedOpen;
		const sumBlocked =
			model.contributors.reduce((s, c) => s + c.blocked, 0) + model.unassignedBlocked;
		expect(sumOpen).toBe(model.openTotal);
		expect(sumBlocked).toBe(model.blockedTotal);
		expect(model.unassignedOpen).toBe(1);

		// Recent activity takes top 3 newest first.
		expect(model.recentActivity.map((a) => a.id)).toEqual(['a2', 'a3', 'a4']);
	});

	it('withholds timing medians below 5 completions and shows them at >= 5 completions with strict bucket reconciliation', () => {
		const fourCompletions: TeamCompletionRecord[] = [
			{
				completedAt: '2026-11-24T10:00:00Z',
				kind: 'task',
				contributorId: 'noor',
				cycleSeconds: 7200,
				approvalWaitSeconds: 1800
			},
			{
				completedAt: '2026-11-20T10:00:00Z',
				kind: 'bug',
				contributorId: 'mira',
				cycleSeconds: 3600,
				approvalWaitSeconds: 600
			},
			{
				completedAt: '2026-11-12T10:00:00Z',
				kind: 'task',
				contributorId: 'noor',
				cycleSeconds: 10800,
				approvalWaitSeconds: 3600
			},
			{
				completedAt: '2026-11-01T10:00:00Z',
				kind: 'bug',
				contributorId: 'mira',
				cycleSeconds: 14400,
				approvalWaitSeconds: 1200
			}
		];

		const belowThreshold = deriveTeamOverview({
			detail: DETAIL,
			workRows: [],
			completions: fourCompletions,
			nowMs: NOW_MS
		});
		expect(belowThreshold.completedTotal).toBe(4);
		expect(belowThreshold.kpis.find((k) => k.id === 'cycle_time')).toEqual({
			id: 'cycle_time',
			label: 'Median cycle time',
			value: '—',
			note: `At least ${TEAM_OVERVIEW_MIN_TIMING_SAMPLES} completions needed`
		});
		expect(belowThreshold.kpis.find((k) => k.id === 'approval_wait')?.value).toBe('—');

		const fiveCompletions: TeamCompletionRecord[] = [
			...fourCompletions,
			{
				completedAt: '2026-10-29T10:00:00Z',
				kind: 'task',
				contributorId: 'mira',
				cycleSeconds: 18000,
				approvalWaitSeconds: 2400
			},
			// Outside 28-day window — must be excluded from KPIs, weekly buckets, and contributors.
			{
				completedAt: '2026-10-20T10:00:00Z',
				kind: 'task',
				contributorId: 'noor',
				cycleSeconds: 999999,
				approvalWaitSeconds: 999999
			}
		];

		const atThreshold = deriveTeamOverview({
			detail: DETAIL,
			workRows: [],
			completions: fiveCompletions,
			nowMs: NOW_MS
		});
		expect(atThreshold.completedTotal).toBe(5);
		expect(atThreshold.weekly).toHaveLength(4);
		expect(atThreshold.weekly.reduce((sum, b) => sum + b.total, 0)).toBe(5);
		expect(atThreshold.kpis.find((k) => k.id === 'cycle_time')?.value).toBe('3h');
		expect(atThreshold.kpis.find((k) => k.id === 'approval_wait')?.value).toBe('30m');
		expect(atThreshold.contributors.map((c) => [c.name, c.completed])).toEqual([
			['Mira Castillo', 3],
			['Noor Haddad', 2]
		]);
	});

	it('computes medians and formats durations cleanly', () => {
		expect(computeMedian([])).toBeNull();
		expect(computeMedian([10, 30, 20])).toBe(20);
		expect(computeMedian([10, 20, 30, 40])).toBe(25);
		expect(formatDurationSeconds(30)).toBe('<1m');
		expect(formatDurationSeconds(1800)).toBe('30m');
		expect(formatDurationSeconds(5400)).toBe('1.5h');
		expect(formatDurationSeconds(172800)).toBe('2d');
	});
});
