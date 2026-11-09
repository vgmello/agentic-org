import { describe, expect, it } from 'vitest';
import type { WorkItemSummary } from '$lib/proto';
import { bugRef, closedWord, filterBugs, groupByArea, similarBugs, severityWord } from './bugs';

const bug = (n: number, over: Partial<WorkItemSummary> = {}): WorkItemSummary => ({
	id: `b${n}`,
	kind: 'bug',
	project_id: 'p',
	number: n,
	short_id: `s${n}`,
	title: `Bug ${n}`,
	status: 'new',
	priority: 'normal',
	created_by: 'owner',
	created_at: `2026-11-0${n}T10:00:00.000Z`,
	updated_at: `2026-11-0${n}T10:00:00.000Z`,
	area_id: 'gen',
	severity: 'major',
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
	...over
});

describe('words', () => {
	it('writes B-7 or Pending, severity and how a bug closed', () => {
		expect(bugRef(bug(7))).toBe('B-7');
		expect(bugRef(bug(7), 'chk')).toBe('chk-B-7');
		expect(bugRef(bug(1, { number: undefined, short_id: 'abc123' }), 'chk')).toBe('Pending · abc123');
		expect(bugRef(bug(1, { number: undefined, short_id: 'abc123' }))).toBe('Pending · abc123');
		expect(severityWord('critical')).toBe('Critical');
		expect(severityWord(undefined)).toBe('Severity not set');
		expect(closedWord(bug(1, { resolution: 'verified' }))).toBe('Verified');
		expect(closedWord(bug(1, { resolution: 'wont_fix' }))).toBe("Won't fix");
		expect(closedWord(bug(1, { resolution: 'duplicate', duplicate_of: 'b2' }), bug(2))).toBe('Duplicate of B-2');
	});
});

describe('filters and groups', () => {
	const all = [bug(1), bug(2, { status: 'triaged' }), bug(3, { status: 'fixed' }), bug(4, { status: 'closed', resolution: 'verified' })];
	it('filters by where the bug is in its life', () => {
		expect(filterBugs(all, 'open').map((b) => b.number)).toEqual([1, 2, 3]);
		expect(filterBugs(all, 'needs_triage').map((b) => b.number)).toEqual([1]);
		expect(filterBugs(all, 'fixed').map((b) => b.number)).toEqual([3]);
		expect(filterBugs(all, 'closed').map((b) => b.number)).toEqual([4]);
		expect(filterBugs(all, 'all')).toHaveLength(4);
	});
	it('groups by area with General first, most severe first, and puts an unknown area under General', () => {
		const areas = [{ id: 'pay', name: 'Payments' }, { id: 'gen', name: 'General', general: true }];
		const groups = groupByArea([bug(1, { severity: 'minor' }), bug(2, { severity: 'critical' }), bug(3, { area_id: 'pay' }), bug(4, { area_id: 'gone' })], areas);
		expect(groups.map((g) => [g.name, g.bugs.map((b) => b.number)])).toEqual([['General', [2, 4, 1]], ['Payments', [3]]]);
	});
});

describe('BUG-4 similar bugs', () => {
	it('suggests open bugs that share a word, best match first, and not closed ones', () => {
		const list = [bug(1, { title: 'Upload fails on large files' }), bug(2, { title: 'Upload button misaligned' }), bug(3, { title: 'Upload fails', status: 'closed' }), bug(4, { title: 'Login broken' })];
		expect(similarBugs(list, 'Upload fails').map((b) => b.number)).toEqual([1, 2]);
		expect(similarBugs(list, 'a b')).toEqual([]);
	});
});
