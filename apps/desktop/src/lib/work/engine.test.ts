import { describe, expect, it } from 'vitest';
import { timeBlock } from './time';
import { applyWork, emptyWork, itemOf, summaries, type WorkCall, type WorkContext, type WorkState } from './engine';

const ctx: WorkContext = {
	now: '2026-11-08T10:00:00.000Z',
	actor: 'owner',
	projects: [
		{ id: 'p1', teamIds: ['web'] },
		{ id: 'old', archived: true, teamIds: [] }
	],
	members: ['owner', 'sam']
};
const id = (n: number) => `01JC00000000000000000000${String(n).padStart(2, '0')}`;
let change = 0;
const cid = () => id(50 + ++change);
const create = (state: WorkState, n: number, extra: object = {}) =>
	applyWork(state, { method: 'task.create', change_id: cid(), item_id: id(n), project_id: 'p1', title: `Task ${n}`, ...extra } as WorkCall, ctx);
const refused = (fn: () => unknown) => {
	try {
		fn();
	} catch (e) {
		return e as { source: string; code: string; message: string };
	}
	throw new Error('expected a refusal');
};

describe('creating tasks', () => {
	it('numbers tasks per project from 1, starts them To do at normal priority and never reuses a number', () => {
		let s = emptyWork();
		s = create(s, 1).state;
		s = create(s, 2).state;
		expect(summaries(s, 'p1').map((t) => [t.number, t.status, t.priority])).toEqual([
			[1, 'todo', 'normal'],
			[2, 'todo', 'normal']
		]);
		s = applyWork(s, { method: 'work.delete', change_id: cid(), item_id: id(2), expected_version: 1 }, ctx).state;
		expect(summaries(s, 'p1')).toHaveLength(1);
		expect(summaries(create(s, 3).state, 'p1').at(-1)?.number).toBe(3);
	});

	it('refuses bad titles, unknown or archived projects, other projects teams and unknown assignees', () => {
		const s = emptyWork();
		expect(refused(() => create(s, 1, { title: '   ' })).code).toBe('invalid_request');
		expect(refused(() => create(s, 1, { title: 'a\nb' })).code).toBe('invalid_request');
		expect(refused(() => create(s, 1, { title: 'x'.repeat(121) })).code).toBe('invalid_request');
		expect(refused(() => create(s, 1, { project_id: 'nope' })).code).toBe('not_found');
		expect(refused(() => create(s, 1, { project_id: 'old' })).code).toBe('invalid_request');
		expect(refused(() => create(s, 1, { team_id: 'design' })).code).toBe('invalid_request');
		expect(refused(() => create(s, 1, { assignee_id: 'ghost' })).code).toBe('unknown_recipient');
		expect(refused(() => create(create(s, 1).state, 1)).code).toBe('already_exists');
	});

	it('a repeated change id gets the same answer and changes nothing; reused for another change it is refused', () => {
		const call: WorkCall = { method: 'task.create', change_id: 'c-1', item_id: id(1), project_id: 'p1', title: 'One' };
		const first = applyWork(emptyWork(), call, ctx);
		const again = applyWork(first.state, call, ctx);
		expect(again.accepted).toEqual(first.accepted);
		expect(summaries(again.state)).toHaveLength(1);
		expect(refused(() => applyWork(first.state, { ...call, title: 'Other' }, ctx)).code).toBe('invalid_request');
	});
});

describe('changing a task', () => {
	const one = () => create(emptyWork(), 1).state;

	it('update checks the version, needs a change and refuses a value together with its clear', () => {
		const s = one();
		const ok = applyWork(s, { method: 'task.update', change_id: cid(), item_id: id(1), expected_version: 1, title: 'Renamed', priority: 'high' }, ctx);
		expect(ok.accepted.items[0]).toMatchObject({ title: 'Renamed', priority: 'high', version: 2 });
		expect(refused(() => applyWork(ok.state, { method: 'task.update', change_id: cid(), item_id: id(1), expected_version: 1, title: 'Late' }, ctx)).code).toBe('conflict');
		expect(refused(() => applyWork(s, { method: 'task.update', change_id: cid(), item_id: id(1), expected_version: 1 }, ctx)).code).toBe('invalid_request');
		expect(refused(() => applyWork(s, { method: 'task.update', change_id: cid(), item_id: id(1), expected_version: 1, description: 'x', clear_description: true }, ctx)).code).toBe('invalid_request');
	});

	it('assigns and unassigns', () => {
		const a = applyWork(one(), { method: 'task.assign', change_id: cid(), item_id: id(1), expected_version: 1, assignee_id: 'sam' }, ctx);
		expect(a.accepted.items[0].assignee_id).toBe('sam');
		const b = applyWork(a.state, { method: 'task.assign', change_id: cid(), item_id: id(1), expected_version: 2 }, ctx);
		expect(b.accepted.items[0].assignee_id).toBeUndefined();
	});

	const status = (s: WorkState, v: number, extra: object) =>
		applyWork(s, { method: 'task.set_status', change_id: cid(), item_id: id(1), expected_version: v, ...extra } as WorkCall, ctx);

	it('follows the state machine: blocked needs a reason, a reason only goes with blocked, Done can only be reopened', () => {
		let s = one();
		expect(refused(() => status(s, 1, { status: 'blocked' })).code).toBe('invalid_request');
		expect(refused(() => status(s, 1, { status: 'in_progress', blocked_reason: 'x' })).code).toBe('invalid_request');
		s = status(s, 1, { status: 'blocked', blocked_reason: 'Waiting on design' }).state;
		expect(summaries(s)[0]).toMatchObject({ status: 'blocked', blocked_reason: 'Waiting on design' });
		s = status(s, 2, { status: 'in_progress' }).state;
		expect(summaries(s)[0].blocked_reason).toBeUndefined();
		s = status(s, 3, { status: 'done' }).state;
		expect(summaries(s)[0]).toMatchObject({ status: 'done', resolution: 'completed' });
		expect(refused(() => status(s, 4, { status: 'blocked', blocked_reason: 'x' })).code).toBe('invalid_request');
		s = status(s, 4, { status: 'todo' }).state;
		expect(summaries(s)[0]).toMatchObject({ status: 'todo', resolution: undefined, reopen_count: 1 });
	});

	it('Done is guarded by required steps unless done anyway is given a reason; cancelled skips the guard', () => {
		let s = applyWork(
			one(),
			{ method: 'checklist.add', change_id: cid(), item_id: id(1), steps: [{ step_id: id(60), text: 'Write tests', required: true }, { step_id: id(61), text: 'Polish', required: false }] },
			ctx
		).state;
		const err = refused(() => status(s, 2, { status: 'done' }));
		expect(err.code).toBe('checklist_incomplete');
		expect(err.message).toContain('Write tests');
		expect(err.message).not.toContain('Polish');
		const anyway = status(s, 2, { status: 'done', anyway_reason: 'Covered by the integration test' });
		expect(anyway.accepted.items[0]).toMatchObject({ status: 'done', done_anyway: true });
		expect(itemOf(anyway.state, id(1), ctx.now).done_anyway_detail).toEqual({ reason: 'Covered by the integration test', steps: [id(60)] });
		// Reopening clears it.
		expect(status(anyway.state, 3, { status: 'todo' }).accepted.items[0].done_anyway).toBe(false);
		expect(status(s, 2, { status: 'done', resolution: 'cancelled' }).accepted.items[0].resolution).toBe('cancelled');
		s = applyWork(s, { method: 'checklist.check', change_id: cid(), step_id: id(60), done: true }, ctx).state;
		expect(status(s, 3, { status: 'done' }).accepted.items[0].status).toBe('done');
	});
});

describe('checklists', () => {
	const withSteps = () =>
		applyWork(create(emptyWork(), 1).state, { method: 'checklist.add', change_id: cid(), item_id: id(1), steps: [{ step_id: id(60), text: 'One', required: true }] }, ctx).state;

	it('counts steps, records who ticked and when, and ticking again changes nothing', () => {
		let s = withSteps();
		expect(summaries(s)[0].checklist).toEqual({ total: 1, done: 0, required_left: 1 });
		s = applyWork(s, { method: 'checklist.check', change_id: cid(), step_id: id(60), done: true }, ctx).state;
		const version = summaries(s)[0].version;
		expect(itemOf(s, id(1), ctx.now).checklist[0]).toMatchObject({ done: true, checked_by: 'owner', checked_at: ctx.now });
		s = applyWork(s, { method: 'checklist.check', change_id: cid(), step_id: id(60), done: true }, ctx).state;
		expect(summaries(s)[0].version).toBe(version);
		expect(summaries(s)[0].checklist).toEqual({ total: 1, done: 1, required_left: 0 });
	});

	it('adds after a step, edits with the steps version, removes, and refuses duplicates and too many', () => {
		let s = withSteps();
		s = applyWork(s, { method: 'checklist.add', change_id: cid(), item_id: id(1), steps: [{ step_id: id(61), text: 'Two', required: false }], after_step_id: id(60) }, ctx).state;
		expect(itemOf(s, id(1), ctx.now).checklist.map((c) => c.text)).toEqual(['One', 'Two']);
		expect(refused(() => applyWork(s, { method: 'checklist.add', change_id: cid(), item_id: id(1), steps: [{ step_id: id(61), text: 'Dup', required: false }] }, ctx)).code).toBe('already_exists');
		s = applyWork(s, { method: 'checklist.edit', change_id: cid(), step_id: id(61), expected_version: 1, text: 'Second', required: true }, ctx).state;
		expect(refused(() => applyWork(s, { method: 'checklist.edit', change_id: cid(), step_id: id(61), expected_version: 1, text: 'Late' }, ctx)).code).toBe('conflict');
		s = applyWork(s, { method: 'checklist.remove', change_id: cid(), step_id: id(61), expected_version: 2 }, ctx).state;
		expect(itemOf(s, id(1), ctx.now).checklist).toHaveLength(1);
		const many = Array.from({ length: 101 }, (_, i) => ({ step_id: `s${i}`, text: 't', required: false }));
		expect(refused(() => applyWork(emptyWork(), { method: 'task.create', change_id: cid(), item_id: id(9), project_id: 'p1', title: 'Big', checklist: many }, ctx)).code).toBe('invalid_request');
	});
});

describe('Done and time', () => {
	const at = (now: string) => ({ ...ctx, now });
	it('lead time stops at Done, cycle time starts at the first start, and Done only lists what closed since the cutoff', () => {
		let s = applyWork(emptyWork(), { method: 'task.create', change_id: cid(), item_id: id(1), project_id: 'p1', title: 'One' }, at('2026-11-01T00:00:00.000Z')).state;
		s = applyWork(s, { method: 'task.set_status', change_id: cid(), item_id: id(1), expected_version: 1, status: 'in_progress' }, at('2026-11-01T01:00:00.000Z')).state;
		s = applyWork(s, { method: 'task.set_status', change_id: cid(), item_id: id(1), expected_version: 2, status: 'done' }, at('2026-11-01T03:00:00.000Z')).state;
		// Later edits and the passing days change neither figure.
		s = applyWork(s, { method: 'task.update', change_id: cid(), item_id: id(1), expected_version: 3, title: 'One!' }, at('2026-11-07T00:00:00.000Z')).state;
		const t = itemOf(s, id(1), '2026-11-08T00:00:00.000Z').time;
		expect([t.lead_ms, t.cycle_ms, t.open]).toEqual([3 * 3600_000, 2 * 3600_000, false]);
		expect(timeBlock(t)).toBe("Lead time 3 h · waiting on you 0 · blocked 0 · the rest 3 h (working time isn't tracked yet)");
		expect(timeBlock({ ...t, open: true, lead_ms: 28 * 3600_000, waiting_on_owner_ms: 3 * 3600_000 })).toBe("Lead time 1 d 4 h so far · waiting on you 3 h · blocked 0 · the rest 1 d 1 h (working time isn't tracked yet)");
		expect(summaries(s, 'p1', '2026-11-01T02:00:00.000Z')).toHaveLength(1);
		expect(summaries(s, 'p1', '2026-11-01T04:00:00.000Z')).toHaveLength(0);
		expect(summaries(s, 'p1')).toHaveLength(1);
	});
	it('refuses done anyway without a real reason', () => {
		let s = create(emptyWork(), 1).state;
		s = applyWork(s, { method: 'checklist.add', change_id: cid(), item_id: id(1), steps: [{ step_id: id(60), text: 'Must', required: true }] }, ctx).state;
		const e = refused(() => applyWork(s, { method: 'task.set_status', change_id: cid(), item_id: id(1), expected_version: 2, status: 'done', anyway_reason: '   ' }, ctx));
		expect(e.code).toBe('invalid_request');
	});
});
