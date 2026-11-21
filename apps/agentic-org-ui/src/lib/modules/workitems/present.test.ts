import { describe, expect, it } from 'vitest';
import {
	activityKindLabel,
	assignmentText,
	boardColumns,
	byPriorityThenAge,
	bySeverityThenPriority,
	formatDuration,
	groupRows,
	isRef,
	moved,
	neighbourColumn,
	normalizeRef,
	preciseDuration,
	rowSpeech,
	severityCounts,
	splitSteps,
	stepError,
	timeSplit,
	titleError,
	typedRef
} from './present';
import type { TaskRow } from './types';

function makeRow(overrides: Partial<TaskRow> = {}): TaskRow {
	return {
		id: 't-1',
		ref: 'T-A42',
		type: 'task',
		title: 'Split checkout',
		status: 'todo',
		priority: 'normal',
		severity: null,
		effort: null,
		project: { id: 'beacon', name: 'Beacon', number: 8 },
		team: { id: 'web', name: 'Web' },
		assignedTeam: null,
		assignee: null,
		reporter: { id: 'you', name: 'Alex Kim', kind: 'person' },
		createdAt: '2026-11-10T10:00:00Z',
		updatedAt: '2026-11-10T10:00:00Z',
		statusAt: '2026-11-10T10:00:00Z',
		checklist: { done: 0, total: 0, requiredLeft: 0 },
		blockedReason: null,
		reopenCount: 0,
		needsMe: null,
		budget: null,
		...overrides
	};
}

describe('workitems present helpers', () => {
	it('normalizes and validates task references across all types', () => {
		expect(normalizeRef(' t-a42 ')).toBe('T-A42');
		expect(isRef('T-A42')).toBe(true);
		expect(isRef('t-b17')).toBe(true);
		expect(isRef('T-14')).toBe(true);
		expect(isRef('B-1')).toBe(false);
		expect(isRef('nope')).toBe(false);
		expect(typedRef({ type: 'bug', ref: 'T-A42' })).toBe('Bug T-A42');
	});

	it('sorts columns by priority then age, and bugs by severity then priority', () => {
		const lowOld = makeRow({ ref: 'T-A1', priority: 'low', createdAt: '2026-11-01T00:00:00Z' });
		const urgentNew = makeRow({
			ref: 'T-A2',
			priority: 'urgent',
			createdAt: '2026-11-15T00:00:00Z'
		});
		const highOld = makeRow({ ref: 'T-A3', priority: 'high', createdAt: '2026-11-02T00:00:00Z' });
		expect([lowOld, urgentNew, highOld].sort(byPriorityThenAge).map((r) => r.ref)).toEqual([
			'T-A2',
			'T-A3',
			'T-A1'
		]);

		const unset = makeRow({ ref: 'T-B1', type: 'bug', severity: null, priority: 'urgent' });
		const crit = makeRow({ ref: 'T-B2', type: 'bug', severity: 'critical', priority: 'low' });
		const major = makeRow({ ref: 'T-B3', type: 'bug', severity: 'major', priority: 'high' });
		expect([unset, major, crit].sort(bySeverityThenPriority).map((r) => r.ref)).toEqual([
			'T-B2',
			'T-B3',
			'T-B1'
		]);
	});

	it('keeps Done column to last 7 days unless allDone is requested', () => {
		const now = new Date('2026-11-20T12:00:00Z');
		const recent = makeRow({ ref: 'T-A1', status: 'done', statusAt: '2026-11-18T12:00:00Z' });
		const old = makeRow({ ref: 'T-A2', status: 'done', statusAt: '2026-11-01T12:00:00Z' });
		const cancelled = makeRow({ ref: 'T-A3', status: 'cancelled' });

		const defaultCols = boardColumns([recent, old, cancelled], { allDone: false, now });
		const doneCol = defaultCols.find((c) => c.column === 'done')!;
		expect(doneCol.rows.map((r) => r.ref)).toEqual(['T-A1']);
		expect(doneCol.hiddenDone).toBe(1);

		const allCols = boardColumns([recent, old, cancelled], { allDone: true, now });
		expect(allCols.find((c) => c.column === 'done')!.rows.map((r) => r.ref)).toEqual([
			'T-A1',
			'T-A2'
		]);
		expect(neighbourColumn('todo', -1)).toBeUndefined();
		expect(neighbourColumn('todo', 1)).toBe('in_progress');
		expect(neighbourColumn('cancelled', 1)).toBeUndefined();
	});

	it('groups bugs by assigned team with Unassigned team last and counts unset severities', () => {
		const rows = [
			makeRow({
				ref: 'T-A1',
				type: 'bug',
				severity: 'critical',
				assignedTeam: { id: 'web', name: 'Web' }
			}),
			makeRow({
				ref: 'T-A2',
				type: 'bug',
				severity: null,
				assignedTeam: null
			}),
			makeRow({
				ref: 'T-A3',
				type: 'bug',
				severity: 'major',
				status: 'done',
				assignedTeam: { id: 'web', name: 'Web' }
			})
		];
		const groups = groupRows(rows, 'assigned_team', bySeverityThenPriority);
		expect(groups.map((g) => g.label)).toEqual(['Web', 'Unassigned team']);
		expect(severityCounts(rows)).toBe('1 critical · 1 not set');
	});

	it('formats shared durations per TIME-4 and splits lead time into waiting, blocked and the rest', () => {
		expect(formatDuration(0)).toBe('0 min');
		expect(formatDuration(59 * 60 + 59)).toBe('59 min');
		expect(formatDuration(47 * 3600 + 59 * 60)).toBe('47 h');
		expect(formatDuration(48 * 3600)).toBe('2.0 d');
		expect(formatDuration(126 * 3600)).toBe('5.2 d');
		expect(preciseDuration(28 * 3600 + 180)).toBe('1 day 4 hours 3 minutes');

		const split = timeSplit({
			createdAt: '2026-11-18T00:00:00Z',
			startedAt: '2026-11-18T01:00:00Z',
			startUnavailable: false,
			open: true,
			leadSeconds: 28 * 3600,
			cycleSeconds: 27 * 3600,
			waitingOnYouSeconds: 3 * 3600,
			blockedSeconds: 0
		});
		expect(split?.lead).toBe('28 h');
		expect(split?.soFar).toBe(true);
		expect(split?.parts.map((p) => `${p.label} ${p.value}`)).toEqual([
			'waiting on you 3 h',
			'blocked 0 min',
			'the rest 25 h'
		]);
	});

	it('formats assignment text, screen-reader row speech, checklist validation and unknown events', () => {
		const row = makeRow({
			type: 'bug',
			ref: 'T-A42',
			title: 'Pay button wraps',
			severity: 'major',
			priority: 'high',
			team: { id: 'web', name: 'Web' },
			assignedTeam: { id: 'platform', name: 'Platform' },
			assignee: { id: 'noor', name: 'Noor Haddad', kind: 'agent' }
		});
		expect(assignmentText(row)).toBe('Platform · Noor Haddad');
		expect(assignmentText({ assignedTeam: null, assignee: null })).toBe('Unassigned');
		expect(rowSpeech(row)).toContain('Bug T-A42, Pay button wraps, severity major, priority high');
		expect(rowSpeech(row)).toContain('assignee Noor Haddad, agent');

		expect(splitSteps('Step one\n\n Step two \n')).toEqual(['Step one', 'Step two']);
		expect(stepError([])).toBe('Write the step first.');
		expect(stepError(['x'.repeat(201)])).toContain('at most 200 characters');
		expect(moved(['a', 'b', 'c'], 2, 0)).toEqual(['c', 'a', 'b']);
		expect(titleError('')).toBe('Write a title.');
		expect(titleError('a'.repeat(121))).toContain('at most 120');
		expect(activityKindLabel('status')).toBe('status');
		expect(activityKindLabel('future_v9_event')).toBe('unknown');
	});
});
