import { describe, expect, it } from 'vitest';
import type { WorkItemSummary } from '$lib/proto';
import { checklistWords, columns, lastWeek, movesFrom, priorityWord, taskRef } from './tasks';

const task = (n: number, over: Partial<WorkItemSummary> = {}): WorkItemSummary => ({
	id: `id${n}`,
	kind: 'task',
	project_id: 'p',
	number: n,
	short_id: `s${n}`,
	title: `T${n}`,
	status: 'todo',
	priority: 'normal',
	created_by: 'owner',
	created_at: `2026-11-0${n}T10:00:00.000Z`,
	updated_at: `2026-11-0${n}T10:00:00.000Z`,
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
const NOW = '2026-11-08T12:00:00.000Z';

describe('TSK-1 the board columns', () => {
	it('has To do, In progress, Blocked and Done, and puts each task in its column', () => {
		const cols = columns([task(1, { status: 'done' }), task(2), task(3, { status: 'blocked' })]);
		expect(cols.map((c) => c.label)).toEqual(['To do', 'In progress', 'Blocked', 'Done']);
		expect(cols.map((c) => c.tasks.map((t) => t.number))).toEqual([[2], [], [3], [1]]);
	});
	it('asks for the last 7 days of Done as a cutoff', () => {
		expect(lastWeek(new Date(NOW))).toBe('2026-11-01T12:00:00.000Z');
	});
});

describe('TSK-4 order and TSK-10 priority words', () => {
	it('sorts Urgent first, then the oldest', () => {
		const tasks = [task(3, { priority: 'low' }), task(2), task(1), task(4, { priority: 'urgent' })];
		expect(columns(tasks)[0].tasks.map((t) => t.number)).toEqual([4, 1, 2, 3]);
	});
	it('says a priority only when it is not Normal', () => {
		expect(priorityWord('normal')).toBeUndefined();
		expect(priorityWord(undefined)).toBeUndefined();
		expect(priorityWord('urgent')).toBe('Urgent');
		expect(priorityWord('low')).toBe('Low');
	});
});

describe('references, moves and checklists', () => {
	it('writes T-12, or Pending with the short id before HQ numbers it', () => {
		expect(taskRef(task(12))).toBe('T-12');
		expect(taskRef(task(12), 'chk')).toBe('chk-T-12');
		expect(taskRef(task(1, { number: undefined, short_id: '3k9f2x' }))).toBe('Pending · 3k9f2x');
	});
	it('offers the moves the state machine allows', () => {
		expect(movesFrom('todo')).toEqual(['in_progress', 'blocked', 'done']);
		expect(movesFrom('done')).toEqual(['todo', 'in_progress']);
	});
	it('counts steps in words', () => {
		expect(checklistWords(task(1))).toBe('');
		expect(checklistWords(task(1, { checklist: { total: 1, done: 0, required_left: 0 } }))).toBe('0 of 1 step');
		expect(checklistWords(task(1, { checklist: { total: 5, done: 2, required_left: 1 } }))).toBe('2 of 5 steps');
	});
});
