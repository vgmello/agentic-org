import { describe, expect, it } from 'vitest';
import { activityOf, applyWork, emptyWork, itemOf, summaries, type WorkCall, type WorkContext, type WorkState } from './engine';
import type { RequestOutcome } from '$lib/proto';

const ctx: WorkContext = {
	now: '2026-11-08T10:00:00.000Z',
	actor: 'owner',
	projects: [
		{ id: 'p1', teamIds: ['web'] },
		{ id: 'r', teamIds: [] },
		{ id: 'old', archived: true, teamIds: [] }
	],
	members: ['owner', 'eden', 'ada']
};
const id = (n: number) => `01JC00000000000000000000${String(n).padStart(2, '0')}`;
let change = 0;
const cid = () => id(60 + ++change);
const run = (state: WorkState, call: object, as = ctx) => applyWork(state, { change_id: cid(), ...call } as WorkCall, as);
const refused = (fn: () => unknown) => {
	try {
		fn();
	} catch (e) {
		return e as { source: string; code: string; message: string };
	}
	throw new Error('expected a refusal');
};
const created = (state: WorkState, n: number, extra: object = {}) => run(state, { method: 'request.create', item_id: id(n), text: `Request ${n}`, ...extra }).state;
const at = (state: WorkState, n: number) => state.items[id(n)];
/** The notes on a request: (who, what) of each comment in its activity. */
const notes = (state: WorkState, n: number) => activityOf(state, id(n)).flatMap((e) => (e.change.type === 'comment' ? [[e.actor_id, e.change.body]] : []));
const v = (state: WorkState, n: number) => at(state, n).summary.version;
const outcome = (extra: Partial<RequestOutcome> = {}): RequestOutcome => ({
	docs: [],
	draft_tasks: [
		{ draft_id: id(80), title: 'First' },
		{ draft_id: id(81), title: 'Second', priority: 'high', assignee_id: 'ada' }
	],
	...extra
});
const scoped = (state: WorkState, n: number, extra: Partial<RequestOutcome> = {}) => {
	let s = run(state, { method: 'request.accept', item_id: id(n), expected_version: v(state, n) }).state;
	s = run(s, { method: 'request.scope', item_id: id(n), expected_version: v(s, n), outcome: outcome(extra) }).state;
	return s;
};
const approvedTasks = (n = 2) => Array.from({ length: n }, (_, i) => ({ item_id: id(90 + i), link_id: id(70 + i), title: `Task ${i + 1}` }));

describe('REQ creating requests', () => {
	it('numbers requests across the organization from 1, never reuses a number, and a bare capture is New', () => {
		let s = created(emptyWork(), 1);
		s = created(s, 2, { project_id: 'p1' });
		expect(summaries(s).map((r) => [r.number, r.status, r.kind, r.assignee_id])).toEqual([
			[1, 'new', 'request', undefined],
			[2, 'new', 'request', undefined]
		]);
		s = run(s, { method: 'work.delete', item_id: id(2), expected_version: 1 }).state;
		expect(at(created(s, 3), 3).summary.number).toBe(3);
	});

	it('keeps the original words and, untitled, takes the first line cut to 120 characters', () => {
		const long = `${'x'.repeat(130)}\nsecond line`;
		const r = at(created(emptyWork(), 1, { text: long }), 1);
		expect(r.summary.title).toBe('x'.repeat(120));
		expect(itemOf(created(emptyWork(), 1, { text: long }), id(1), ctx.now).text).toBe(long);
		expect(at(created(emptyWork(), 1, { text: 'Hello\nWorld', title: 'Mine' }), 1).summary.title).toBe('Mine');
	});

	it('cuts an untitled title at the first line break, and refuses control characters in the words and one-line fields', () => {
		expect(at(created(emptyWork(), 1, { text: 'abc\ndef\tghi' }), 1).summary.title).toBe('abc');
		for (const text of ['abc\rdef', 'abc\u0000def', 'abc\u0007']) expect(refused(() => created(emptyWork(), 1, { text })).code).toBe('invalid_request');
		for (const title of ['x\u0007y', 'x\ty']) expect(refused(() => created(emptyWork(), 1, { title })).code).toBe('invalid_request');
	});

	it('follows the office text rules: Windows line ends read as new lines, and separators, bidi and C1 controls are refused', () => {
		expect(itemOf(created(emptyWork(), 1, { text: 'a\r\nb' }), id(1), ctx.now).text).toBe('a\nb');
		expect(at(created(emptyWork(), 1, { text: 'a\tb\nmore' }), 1).summary.title).toBe('a b');
		for (const bad of ['a\u2028b', 'a\u202eb', 'a\u0085b']) expect(refused(() => created(emptyWork(), 1, { title: bad })).code).toBe('invalid_request');
		for (const bad of ['a\u202eb', 'a\u0085b']) expect(refused(() => created(emptyWork(), 1, { text: bad })).code).toBe('invalid_request');
		// A line separator is fine inside a long text, as in the office.
		expect(() => created(emptyWork(), 1, { text: 'a\u2028b' })).not.toThrow();
	});

	it('the default title reads a line or paragraph separator as a space; an explicit one is refused', () => {
		expect(at(created(emptyWork(), 1, { text: 'a\u2028b\u2029c rest' }), 1).summary.title).toBe('a b c rest');
	});

	it('refuses the bidirectional marks and isolates, limits, and over-long ids', () => {
		for (const mark of ['\u200e', '\u200f', '\u202a', '\u2066', '\u2069']) expect(refused(() => created(emptyWork(), 1, { title: `a${mark}b` })).code).toBe('invalid_request');
		expect(refused(() => created(emptyWork(), 1, { item_id: 'x'.repeat(129) })).code).toBe('invalid_request');
		expect(() => created(emptyWork(), 1, { item_id: 'x'.repeat(128) })).not.toThrow();
		expect(refused(() => scoped(created(emptyWork(), 1), 1, { estimate: { effort: 'x'.repeat(201) } })).code).toBe('invalid_request');
		expect(() => scoped(created(emptyWork(), 1), 1, { estimate: { effort: 'x'.repeat(200) } })).not.toThrow();
		expect(refused(() => scoped(created(emptyWork(), 1), 1, { feature: { type: 'bogus', feature_id: 'f' } as unknown as RequestOutcome['feature'] })).code).toBe('invalid_request');
	});

	it('a deleted request is not there, and taking a routed request back forgets who suggested the route', () => {
		const s = created(emptyWork(), 1, { scoper_id: 'eden', routed_by: 'copilot', route_reason: 'designer' });
		const back = run(s, { method: 'request.route', item_id: id(1), expected_version: 1 }).state;
		expect(itemOf(back, id(1), ctx.now)).toMatchObject({ route_reason: undefined, routed_by: undefined });
		const gone = run(back, { method: 'work.delete', item_id: id(1), expected_version: 2 }).state;
		expect(refused(() => run(gone, { method: 'request.accept', item_id: id(1), expected_version: 3 })).code).toBe('not_found');
	});

	it('a task checklist step whose text is not text is refused, not a crash', () => {
		const add = (steps: unknown) => run(run(emptyWork(), { method: 'task.create', item_id: id(1), project_id: 'p1', title: 'T' }).state, { method: 'checklist.add', item_id: id(1), steps });
		expect(refused(() => add([{ step_id: 'a', text: 5, required: false }])).code).toBe('invalid_request');
	});

	it('keeps accepting the checklists tasks always took: no required flag, any step id, and only wrong shapes refused', () => {
		const add = (steps: unknown) => run(run(emptyWork(), { method: 'task.create', item_id: id(1), project_id: 'p1', title: 'T' }).state, { method: 'checklist.add', item_id: id(1), steps });
		expect(() => add([{ step_id: '', text: 'No flag' }])).not.toThrow();
		expect(() => add([{ step_id: 'x'.repeat(129), text: 'Long id', required: false }])).not.toThrow();
		for (const bad of ['abc', [null], [{ step_id: 5, text: 'x', required: true }], [{ step_id: 'a', text: 'x', required: 'yes' }]]) expect(refused(() => add(bad)).code).toBe('invalid_request');
	});

	it('trims as the office does: a trailing NEL goes, a leading byte order mark stays', () => {
		expect(at(created(emptyWork(), 1, { text: 'abc\u0085' }), 1).summary.title).toBe('abc');
		expect(at(created(emptyWork(), 1, { title: 'abc\u0085' }), 1).summary.title).toBe('abc');
		expect(at(created(emptyWork(), 1, { title: '\ufeffabc' }), 1).summary.title).toBe('\ufeffabc');
		expect(itemOf(created(emptyWork(), 1, { text: '\ufeffabc' }), id(1), ctx.now).text).toBe('\ufeffabc');
	});

	it('a decline reason and the steps an approval makes are one line, in the office rule; a step text must be text', () => {
		const s = scoped(created(emptyWork(), 1, { project_id: 'p1' }), 1);
		expect(refused(() => run(s, { method: 'request.decline', item_id: id(1), expected_version: v(s, 1), reason: 'a\nb' })).code).toBe('invalid_request');
		const approve = (step: object) => () => run(s, { method: 'request.approve', item_id: id(1), expected_version: v(s, 1), tasks: [{ ...approvedTasks(1)[0], checklist: [{ step_id: id(34), required: false, ...step }] }] });
		expect(refused(approve({ text: 'a\tb' })).code).toBe('invalid_request');
		expect(refused(approve({ text: 'a\u202eb' })).code).toBe('invalid_request');
		expect(refused(approve({ text: 5 })).code).toBe('invalid_request');
		expect(approve({ text: 'fine' })).not.toThrow();
	});

	it('leaves the text rules of tasks and bugs as the Rust twin has them', () => {
		const s = run(emptyWork(), { method: 'task.create', item_id: id(1), project_id: 'p1', title: 'a\tb', description: 'x\r\ny' }).state;
		expect([at(s, 1).summary.title, at(s, 1).description]).toEqual(['a\tb', 'x\r\ny']);
	});

	it('refuses fields of the wrong type with a refusal, not a crash', () => {
		const s = created(emptyWork(), 1);
		for (const extra of [{ title: 5 }, { route_reason: 5, scoper_id: 'eden' }, { text: 5 }]) expect(refused(() => created(emptyWork(), 2, extra)).code).toBe('invalid_request');
		expect(refused(() => run(s, { method: 'request.update', item_id: id(1), expected_version: 1, title: 5 })).code).toBe('invalid_request');
		expect(refused(() => run(s, { method: 'request.decline', item_id: id(1), expected_version: 1, reason: 5 })).code).toBe('invalid_request');
		expect(refused(() => scoped(s, 1, { estimate: { effort: 5 as unknown as string } })).code).toBe('invalid_request');
		expect(refused(() => scoped(s, 1, { draft_tasks: [{ draft_id: 'a', title: 5 as unknown as string }] })).code).toBe('invalid_request');
		expect(refused(() => scoped(s, 1, { draft_tasks: [{ draft_id: 'a', title: 'T', description: null as unknown as string }] })).code).toBe('invalid_request');
	});

	it('forgets who suggested a route when nobody was named', () => {
		expect(itemOf(created(emptyWork(), 1, { routed_by: 'rule' }), id(1), ctx.now).routed_by).toBeUndefined();
	});

	it('routes at once when a scoper is named, with the reason and who suggested it', () => {
		const s = created(emptyWork(), 1, { scoper_id: 'eden', route_reason: 'product designer: a new user-facing flow', routed_by: 'rule' });
		const full = itemOf(s, id(1), ctx.now);
		expect([full.summary.status, full.summary.assignee_id, full.route_reason, full.routed_by]).toEqual(['routed', 'eden', 'product designer: a new user-facing flow', 'rule']);
	});

	it('refuses empty or oversized words, bad titles, kinds, unknown or archived projects, unknown scopers, a reason with no scoper, and a reused id', () => {
		const s = emptyWork();
		expect(refused(() => created(s, 1, { text: '   ' })).code).toBe('invalid_request');
		expect(refused(() => created(s, 1, { text: 'x'.repeat(20_001) })).code).toBe('invalid_request');
		expect(refused(() => created(s, 1, { text: 'x'.repeat(20_000) }) && created(s, 1, { title: 'a\nb' })).code).toBe('invalid_request');
		expect(refused(() => created(s, 1, { request_kind: 'bug' })).code).toBe('invalid_request');
		expect(refused(() => created(s, 1, { routed_by: 'nobody' })).code).toBe('invalid_request');
		expect(refused(() => created(s, 1, { project_id: 'nope' })).code).toBe('not_found');
		expect(refused(() => created(s, 1, { project_id: 'old' })).code).toBe('invalid_request');
		expect(refused(() => created(s, 1, { scoper_id: 'ghost' })).code).toBe('unknown_recipient');
		expect(refused(() => created(s, 1, { route_reason: 'why' })).code).toBe('invalid_request');
		expect(refused(() => created(s, 1, { scoper_id: 'eden', route_reason: 'x'.repeat(501) })).code).toBe('invalid_request');
		expect(refused(() => created(created(s, 1), 1)).code).toBe('already_exists');
	});

	it('a failed call changes nothing', () => {
		const s = created(emptyWork(), 1);
		const before = JSON.stringify(s);
		refused(() => created(s, 2, { project_id: 'nope' }));
		expect(JSON.stringify(s)).toBe(before);
	});
});

describe('REQ the lifecycle', () => {
	it('New → Routed → scoping once the agent accepts → Scoped, and rerouting makes it Routed again for the new agent', () => {
		let s = created(emptyWork(), 1);
		s = run(s, { method: 'request.route', item_id: id(1), expected_version: v(s, 1), scoper_id: 'eden', route_reason: 'designer', routed_by: 'copilot' }).state;
		expect([at(s, 1).summary.status, at(s, 1).summary.assignee_id]).toEqual(['routed', 'eden']);
		s = run(s, { method: 'request.accept', item_id: id(1), expected_version: v(s, 1) }, { ...ctx, actor: 'eden' }).state;
		expect([at(s, 1).summary.status, at(s, 1).summary.assignee_id]).toEqual(['scoping', 'eden']);
		s = run(s, { method: 'request.route', item_id: id(1), expected_version: v(s, 1), scoper_id: 'ada', route_reason: 'mostly the office protocol' }).state;
		expect([at(s, 1).summary.status, at(s, 1).summary.assignee_id, at(s, 1).route_reason]).toEqual(['routed', 'ada', 'mostly the office protocol']);
		s = run(s, { method: 'request.route', item_id: id(1), expected_version: v(s, 1) }).state;
		expect([at(s, 1).summary.status, at(s, 1).summary.assignee_id, at(s, 1).route_reason, at(s, 1).routed_by]).toEqual(['new', undefined, undefined, undefined]);
	});

	it('an unrouted request can be accepted by the caller, who becomes its scoper', () => {
		let s = created(emptyWork(), 1);
		s = run(s, { method: 'request.accept', item_id: id(1), expected_version: 1 }, { ...ctx, actor: 'ada' }).state;
		expect([at(s, 1).summary.status, at(s, 1).summary.assignee_id]).toEqual(['scoping', 'ada']);
	});

	it('scoping records the outcome and moves to Scoped; asking for changes sends it back to scoping with the note kept', () => {
		let s = scoped(created(emptyWork(), 1, { scoper_id: 'eden' }), 1, { estimate: { effort: '3 days', cost_usd_cents_min: 4000, cost_usd_cents_max: 7000 }, feature: { type: 'new', feature_id: id(40) } });
		const full = itemOf(s, id(1), ctx.now);
		expect(full.summary.status).toBe('scoped');
		expect(full.outcome?.draft_tasks.map((d) => d.title)).toEqual(['First', 'Second']);
		expect(full.outcome?.estimate).toEqual({ effort: '3 days', cost_usd_cents_min: 4000, cost_usd_cents_max: 7000 });
		s = run(s, { method: 'request.ask_changes', item_id: id(1), expected_version: v(s, 1), comment_id: id(30), note: 'Split the second task.' }).state;
		expect(at(s, 1).summary.status).toBe('scoping');
		expect(activityOf(s, id(1)).at(-1)).toMatchObject({ actor_id: 'owner', change: { type: 'status_changed', from: 'scoped', to: 'scoping', reason: 'Split the second task.' } });
	});

	it('only moves that the state allows: each wrong state is refused with the state named', () => {
		const fresh = created(emptyWork(), 1);
		const call = (s: WorkState, method: string, extra: object = {}) => run(s, { method, item_id: id(1), expected_version: v(s, 1), ...extra }).state;
		expect(refused(() => call(fresh, 'request.scope', { outcome: outcome() })).message).toContain('new');
		expect(refused(() => call(fresh, 'request.ask_changes', { comment_id: id(30), note: 'n' })).code).toBe('invalid_request');
		expect(refused(() => call(fresh, 'request.approve', { tasks: approvedTasks() })).code).toBe('invalid_request');
		const sc = scoped(fresh, 1);
		expect(refused(() => call(sc, 'request.accept')).code).toBe('invalid_request');
		expect(refused(() => call(sc, 'request.route', { scoper_id: 'eden' })).code).toBe('invalid_request');
		const declined = call(fresh, 'request.decline', { reason: 'Not now' });
		for (const m of ['request.accept', 'request.route', 'request.update', 'request.decline']) expect(refused(() => call(declined, m, { scoper_id: 'eden', reason: 'x', title: 'y' })).code).toBe('invalid_request');
	});

	it('closed requests (approved, declined, merged) take no further move, and each call keeps to its source states', () => {
		const approved = run(scoped(created(emptyWork(), 1, { project_id: 'p1' }), 1), { method: 'request.approve', item_id: id(1), expected_version: 3, tasks: approvedTasks(1) }).state;
		const merged = run(created(created(emptyWork(), 1), 2), { method: 'request.merge', item_id: id(1), expected_version: 1, into: id(2), comment_id: id(30) }).state;
		for (const [closed, n] of [[approved, 1], [merged, 1]] as const) {
			const calls: [string, object][] = [['request.route', { scoper_id: 'eden' }], ['request.accept', {}], ['request.update', { title: 'x' }], ['request.decline', { reason: 'r' }], ['request.merge', { into: id(2), comment_id: id(32) }], ['request.ask_changes', { comment_id: id(33), note: 'n' }], ['request.scope', { outcome: outcome() }]];
			for (const [method, extra] of calls) expect(refused(() => run(closed, { method, item_id: id(n), expected_version: v(closed, n), ...extra })).code).toBe('invalid_request');
		}
		const inScoping = run(created(emptyWork(), 1, { scoper_id: 'eden' }), { method: 'request.accept', item_id: id(1), expected_version: 1 }).state;
		expect(refused(() => run(inScoping, { method: 'request.accept', item_id: id(1), expected_version: 2 })).code).toBe('invalid_request');
		expect(refused(() => run(inScoping, { method: 'request.ask_changes', item_id: id(1), expected_version: 2, comment_id: id(33), note: 'n' })).code).toBe('invalid_request');
		const routed = created(emptyWork(), 1, { scoper_id: 'eden' });
		expect(refused(() => run(routed, { method: 'request.ask_changes', item_id: id(1), expected_version: 1, comment_id: id(33), note: 'n' })).code).toBe('invalid_request');
		const done = scoped(created(emptyWork(), 1), 1);
		expect(refused(() => run(done, { method: 'request.scope', item_id: id(1), expected_version: v(done, 1), outcome: outcome() })).code).toBe('invalid_request');
		expect(refused(() => run(done, { method: 'request.ask_changes', item_id: id(1), expected_version: v(done, 1), comment_id: id(33), note: '  ' })).code).toBe('invalid_request');
	});

	it('an old version is a conflict, one above the current is invalid, and a missing item is not found', () => {
		const s = created(emptyWork(), 1);
		const moved = run(s, { method: 'request.update', item_id: id(1), expected_version: 1, title: 'Again' }).state;
		expect(refused(() => run(moved, { method: 'request.accept', item_id: id(1), expected_version: 1 })).code).toBe('conflict');
		expect(refused(() => run(s, { method: 'request.accept', item_id: id(1), expected_version: 7 })).code).toBe('invalid_request');
		expect(refused(() => run(s, { method: 'request.accept', item_id: 'nope', expected_version: 1 })).code).toBe('not_found');
	});

	it('taking a request back to Not routed is only for one that is routed or scoping', () => {
		const fresh = created(emptyWork(), 1);
		expect(refused(() => run(fresh, { method: 'request.route', item_id: id(1), expected_version: 1 })).code).toBe('invalid_request');
		expect(v(fresh, 1)).toBe(1);
	});

	it('the merge note is the caller\'s, not the merged request creator\'s', () => {
		let s = run(emptyWork(), { method: 'request.create', item_id: id(1), text: 'Eden words' }, { ...ctx, actor: 'eden' }).state;
		s = created(s, 2);
		s = run(s, { method: 'request.merge', item_id: id(1), expected_version: 1, into: id(2), comment_id: id(30) }, { ...ctx, actor: 'ada' }).state;
		expect(notes(s, 2)).toEqual([['ada', 'Eden words']]);
	});

	it('a request that ended stops its clock: its time figures stay where it ended', () => {
		const ended = (call: object, setup: (s: WorkState) => WorkState = (s) => s) => {
			const s = run(setup(created(created(emptyWork(), 1, { project_id: 'p1' }), 2)), { item_id: id(1), expected_version: 1, ...call }).state;
			const at1 = itemOf(s, id(1), '2026-11-08T11:00:00.000Z').time;
			const later = itemOf(s, id(1), '2027-11-08T11:00:00.000Z').time;
			return [at1.open, at1.lead_ms, later.lead_ms, later.rest_ms];
		};
		expect(ended({ method: 'request.decline', reason: 'no' })).toEqual([false, 0, 0, 0]);
		expect(ended({ method: 'request.merge', into: id(2), comment_id: id(30) })).toEqual([false, 0, 0, 0]);
		const ready = (s: WorkState) => scoped(s, 1);
		const approved = run(ready(created(emptyWork(), 1, { project_id: 'p1' })), { method: 'request.approve', item_id: id(1), expected_version: 3, tasks: approvedTasks(1) }).state;
		expect(itemOf(approved, id(1), '2027-11-08T11:00:00.000Z').time).toMatchObject({ open: false, lead_ms: 0 });
	});

	it('every way a request ends leaves a closed-since snapshot', () => {
		const base = created(created(created(emptyWork(), 1, { project_id: 'p1' }), 2), 3);
		const declined = run(base, { method: 'request.decline', item_id: id(2), expected_version: 1, reason: 'no' }).state;
		const merged = run(declined, { method: 'request.merge', item_id: id(3), expected_version: 1, into: id(1), comment_id: id(30) }).state;
		const ready = scoped(merged, 1);
		const approved = run(ready, { method: 'request.approve', item_id: id(1), expected_version: v(ready, 1), tasks: approvedTasks(1) }).state;
		expect(summaries(approved, undefined, '2027-01-01T00:00:00.000Z').filter((r) => r.kind === 'request')).toEqual([]);
		expect(summaries(approved).filter((r) => r.kind === 'request')).toHaveLength(3);
	});

	it('requests that ended are closed in time figures and in a closed-since snapshot', () => {
		const declined = run(created(emptyWork(), 1), { method: 'request.decline', item_id: id(1), expected_version: 1, reason: 'no' }).state;
		const open = created(declined, 2);
		expect(itemOf(declined, id(1), ctx.now).time.open).toBe(false);
		expect(itemOf(open, id(2), ctx.now).time.open).toBe(true);
		expect(summaries(open, undefined, '2026-11-09T00:00:00.000Z').map((r) => r.id)).toEqual([id(2)]);
		expect(summaries(open).map((r) => r.id)).toEqual([id(1), id(2)]);
	});

	it('a request call on a task or a bug is refused, and a task or checklist call on a request is too', () => {
		let s = run(created(emptyWork(), 1), { method: 'task.create', item_id: id(2), project_id: 'p1', title: 'T' }).state;
		s = run(s, { method: 'bug.report', item_id: id(3), project_id: 'p1', title: 'B', what_happened: 'It broke', severity: 'minor' }).state;
		expect(at(s, 3).summary.status).toBe('new');
		for (const n of [2, 3]) expect(refused(() => run(s, { method: 'request.accept', item_id: id(n), expected_version: 1 })).message).toMatch(/not a request/);
		expect(refused(() => run(s, { method: 'task.update', item_id: id(1), expected_version: 1, title: 'x' })).code).toBe('invalid_request');
		expect(refused(() => run(s, { method: 'checklist.add', item_id: id(1), steps: [{ step_id: id(33), text: 'Step', required: false }] })).message).toBe("A request doesn't have a checklist.");
	});

	it('a stale version is refused before the wrong state', () => {
		let s = created(emptyWork(), 1, { scoper_id: 'eden' });
		s = run(s, { method: 'request.accept', item_id: id(1), expected_version: 1 }, { ...ctx, actor: 'eden' }).state;
		const done = run(s, { method: 'request.decline', item_id: id(1), expected_version: 2, reason: 'no' }).state;
		// Declined (a wrong state for each call) and stale at once: the stale version answers.
		for (const [method, extra] of [['request.accept', {}], ['request.decline', { reason: 'x' }], ['request.merge', { into: id(1), comment_id: id(30) }], ['request.approve', { tasks: approvedTasks() }]] as const)
			expect(refused(() => run(done, { method, item_id: id(1), expected_version: 1, ...extra })).code).toBe('conflict');
	});

	it('replaying a change id gives the same answer and creates or approves nothing twice', () => {
		const create: WorkCall = { method: 'request.create', change_id: 'c-create', item_id: id(1), text: 'Once', project_id: 'p1' };
		const first = applyWork(emptyWork(), create, ctx);
		const again = applyWork(first.state, create, ctx);
		expect(again.accepted).toEqual(first.accepted);
		expect(summaries(again.state)).toHaveLength(1);
		const ready = scoped(first.state, 1);
		const approve: WorkCall = { method: 'request.approve', change_id: 'c-approve', item_id: id(1), expected_version: v(ready, 1), tasks: approvedTasks(1) };
		const done = applyWork(ready, approve, ctx);
		const twice = applyWork(done.state, approve, ctx);
		expect(twice.accepted).toEqual(done.accepted);
		expect(summaries(twice.state, 'p1').filter((t) => t.kind === 'task')).toHaveLength(1);
	});
});

describe('REQ ids that are special words', () => {
	it('an id like __proto__ is refused and one like constructor finds nothing, never a crash', () => {
		const s = created(emptyWork(), 1);
		for (const bad of ['__proto__']) expect(refused(() => created(s, 2, { item_id: bad })).code).toBe('invalid_request');
		for (const name of ['constructor', 'toString', 'hasOwnProperty']) {
			for (const method of ['request.accept', 'request.decline', 'request.update']) expect(refused(() => run(s, { method, item_id: name, expected_version: 1, reason: 'r', title: 't' })).code).toBe('not_found');
			expect(refused(() => run(s, { method: 'request.merge', item_id: id(1), expected_version: 1, into: name, comment_id: id(30) })).code).toBe('not_found');
			expect(refused(() => run(s, { method: 'work.delete', item_id: name, expected_version: 1 })).code).toBe('not_found');
			expect(summaries(created(s, 2, { item_id: name })).map((r) => r.id)).toContain(name);
		}
		const scopedFirst = scoped(s, 1, { feature: { type: 'new', feature_id: 'f' } });
		expect(refused(() => run(scopedFirst, { method: 'request.approve', item_id: id(1), expected_version: v(scopedFirst, 1), tasks: [{ item_id: '__proto__', link_id: id(70), title: 'T' }] })).code).toBe('invalid_request');
		expect(Object.getPrototypeOf(scopedFirst.items)).toBe(Object.prototype);
	});

	it('a change id such as constructor is a change id like any other', () => {
		const first = run(emptyWork(), { method: 'request.create', change_id: 'constructor', item_id: id(1), text: 'One' });
		expect(summaries(first.state)).toHaveLength(1);
		expect(run(first.state, { method: 'request.create', change_id: 'toString', item_id: id(2), text: 'Two' }).state.seq).toBe(2);
		expect(refused(() => run(emptyWork(), { method: 'request.create', change_id: '__proto__', item_id: id(1), text: 'One' })).code).toBe('invalid_request');
	});

	it('request numbers do not share a counter with a project that happens to be called r', () => {
		let s = created(emptyWork(), 1);
		s = run(s, { method: 'task.create', item_id: id(2), project_id: 'r', title: 'T' }).state;
		expect(at(s, 2).summary.number).toBe(1);
		expect(at(created(s, 3), 3).summary.number).toBe(2);
	});
});

describe('REQ the outcome', () => {
	const sc = (extra: Partial<RequestOutcome>) => () => scoped(created(emptyWork(), 1), 1, extra);
	it('refuses more than 50 drafts, repeated draft ids, bad titles, priorities and unknown assignees', () => {
		const draft = (i: number) => ({ draft_id: id(i % 100), title: 'T' });
		expect(refused(sc({ draft_tasks: Array.from({ length: 51 }, (_, i) => ({ draft_id: `d${i}`, title: 'T' })) })).code).toBe('invalid_request');
		expect(() => sc({ draft_tasks: Array.from({ length: 50 }, (_, i) => ({ draft_id: `d${i}`, title: 'T' })) })()).not.toThrow();
		expect(refused(sc({ draft_tasks: [draft(1), draft(1)] })).code).toBe('already_exists');
		expect(refused(sc({ draft_tasks: [{ draft_id: 'a', title: '' }] })).code).toBe('invalid_request');
		expect(refused(sc({ draft_tasks: [{ draft_id: 'a', title: 'T', priority: 'asap' as 'low' }] })).code).toBe('invalid_request');
		expect(refused(sc({ draft_tasks: [{ draft_id: 'a', title: 'T', assignee_id: 'ghost' }] })).code).toBe('unknown_recipient');
		expect(refused(sc({ draft_tasks: [{ draft_id: 'a', title: 'T', description: 'x'.repeat(2001) }] })).code).toBe('invalid_request');
	});

	it('refuses an estimate with the range upside down or a fractional or negative cost', () => {
		expect(refused(sc({ estimate: { cost_usd_cents_min: 700, cost_usd_cents_max: 100 } })).code).toBe('invalid_request');
		expect(refused(sc({ estimate: { cost_usd_cents_min: 1.5 } })).code).toBe('invalid_request');
		expect(refused(sc({ estimate: { cost_usd_cents_max: -1 } })).code).toBe('invalid_request');
		expect(() => sc({ estimate: { cost_usd_cents_min: 5, cost_usd_cents_max: 5 } })()).not.toThrow();
	});

	it('refuses docs of unknown projects and a feature of an unknown kind', () => {
		expect(refused(sc({ docs: [{ project_id: 'nope', doc_key: 'spec' }] })).code).toBe('not_found');
		expect(refused(sc({ feature: { type: 'unknown' } })).code).toBe('invalid_request');
	});

	it('keeps what it knows, trimmed, and nothing the caller added', () => {
		const dirty = { draft_tasks: [{ draft_id: 'a', title: '  Padded  ', junk: 'x'.repeat(100) }], docs: [{ project_id: 'p1', doc_key: 'spec', extra: 1 }], estimate: { effort: ' 3 days ', extra: 1 }, feature: { type: 'extends', feature_id: 'f', extra: 1 }, extra: 'top' } as unknown as RequestOutcome;
		const full = itemOf(scoped(created(emptyWork(), 1), 1, dirty), id(1), ctx.now);
		expect(full.outcome).toEqual({ docs: [{ project_id: 'p1', doc_key: 'spec' }], draft_tasks: [{ draft_id: 'a', title: 'Padded' }], estimate: { effort: '3 days' }, feature: { type: 'extends', feature_id: 'f' } });
	});

	it('does not keep the callers own objects', () => {
		const given = outcome();
		const s = scoped(created(emptyWork(), 1), 1, given);
		given.draft_tasks[0].title = 'Changed later';
		expect(at(s, 1).outcome?.draft_tasks[0].title).toBe('First');
	});

	it('refuses malformed shapes with a refusal, not a crash, and more than 50 docs', () => {
		const bad = (o: unknown) => sc(o as Partial<RequestOutcome>);
		expect(refused(bad({ docs: 5 })).code).toBe('invalid_request');
		expect(refused(bad({ draft_tasks: [null] })).code).toBe('invalid_request');
		expect(refused(bad({ draft_tasks: 'x' })).code).toBe('invalid_request');
		expect(refused(bad({ estimate: 3 })).code).toBe('invalid_request');
		expect(refused(bad({ estimate: { cost_usd_cents_min: null } })).code).toBe('invalid_request');
		expect(refused(bad({ feature: 'x' })).code).toBe('invalid_request');
		expect(refused(bad({ docs: Array.from({ length: 51 }, (_, i) => ({ project_id: 'p1', doc_key: `d${i}` })) })).code).toBe('invalid_request');
		const accepted = run(created(emptyWork(), 1), { method: 'request.accept', item_id: id(1), expected_version: 1 }).state;
		expect(refused(() => run(accepted, { method: 'request.scope', item_id: id(1), expected_version: 2, outcome: null })).code).toBe('invalid_request');
		const s = scoped(created(emptyWork(), 1, { project_id: 'p1' }), 1);
		expect(refused(() => run(s, { method: 'request.approve', item_id: id(1), expected_version: v(s, 1), tasks: [null] })).code).toBe('invalid_request');
		for (const checklist of ['abc', [null], [{ step_id: 5, text: 'x', required: true }], [{ step_id: id(34), text: 'x', required: 'yes' }]])
			expect(refused(() => run(s, { method: 'request.approve', item_id: id(1), expected_version: v(s, 1), tasks: [{ ...approvedTasks(1)[0], checklist }] })).code).toBe('invalid_request');
	});

	it('links a doc of an archived project: archiving only stops new items', () => {
		expect(() => sc({ docs: [{ project_id: 'old', doc_key: 'spec' }] })()).not.toThrow();
	});

	it('checks a draft team against the request project once it has one', () => {
		const s = created(emptyWork(), 1, { project_id: 'p1' });
		const bad = () => run(run(s, { method: 'request.accept', item_id: id(1), expected_version: 1 }).state, { method: 'request.scope', item_id: id(1), expected_version: 2, outcome: outcome({ draft_tasks: [{ draft_id: 'a', title: 'T', team_id: 'design' }] }) });
		expect(refused(bad).code).toBe('invalid_request');
	});
});

describe('REQ approving', () => {
	it('creates every task To do in the request project, linked "from" the request and on the feature, in one call', () => {
		let s = scoped(created(emptyWork(), 1, { project_id: 'p1' }), 1, { feature: { type: 'new', feature_id: id(40) } });
		const done = run(s, { method: 'request.approve', item_id: id(1), expected_version: v(s, 1), tasks: [...approvedTasks(1), { item_id: id(92), link_id: id(72), title: 'Second', assignee_id: 'ada', priority: 'high', team_id: 'web', description: 'Details', definition_of_done: 'Shipped', checklist: [{ step_id: id(34), text: 'A step', required: true }] }] });
		s = done.state;
		expect(at(s, 1).summary.status).toBe('approved');
		const tasks = summaries(s, 'p1').filter((t) => t.kind === 'task');
		expect(tasks.map((t) => [t.number, t.status, t.title, t.feature_id, t.assignee_id, t.priority])).toEqual([
			[1, 'todo', 'Task 1', id(40), undefined, 'normal'],
			[2, 'todo', 'Second', id(40), 'ada', 'high']
		]);
		expect(refused(() => run(s, { method: 'request.approve', item_id: id(1), expected_version: v(s, 1), tasks: approvedTasks(1) })).code).toBe('invalid_request');
		const rich = itemOf(s, id(92), ctx.now);
		expect([rich.summary.team_id, rich.description, rich.definition_of_done, rich.checklist.map((c) => c.text), rich.summary.checklist.required_left]).toEqual(['web', 'Details', 'Shipped', ['A step'], 1]);
		expect(itemOf(s, id(90), ctx.now).links.map((l) => [l.kind, l.to])).toEqual([['from_request', { type: 'item', item_id: id(1) }]]);
		expect(itemOf(s, id(1), ctx.now).backlinks.map((l) => l.from_item_id)).toEqual([id(90), id(92)]);
		// The answer names the request and every task it made.
		expect(done.accepted.items.map((i) => i.id)).toEqual([id(1), id(90), id(92)]);
		// Tasks made by an approval can't have their origin link removed.
		expect(refused(() => run(s, { method: 'work.unlink', link_id: id(70) })).code).toBe('invalid_request');
	});

	it('a request without a project can not be approved until it has one', () => {
		let s = scoped(created(emptyWork(), 1), 1);
		const noProject = refused(() => run(s, { method: 'request.approve', item_id: id(1), expected_version: v(s, 1), tasks: approvedTasks() }));
		expect([noProject.code, noProject.message]).toEqual(['invalid_request', 'Pick a project before approving: the tasks need one.']);
		s = run(s, { method: 'request.update', item_id: id(1), expected_version: v(s, 1), project_id: 'p1' }).state;
		s = run(s, { method: 'request.approve', item_id: id(1), expected_version: v(s, 1), tasks: approvedTasks() }).state;
		expect(at(s, 1).summary.status).toBe('approved');
		expect(summaries(s, 'p1').filter((t) => t.kind === 'task')).toHaveLength(2);
	});

	it('creates all or none: one bad task, a reused id or link, or 0 or 51 tasks leave it Scoped with nothing made', () => {
		const s = scoped(created(emptyWork(), 1, { project_id: 'p1' }), 1);
		const go = (tasks: object[]) => () => run(s, { method: 'request.approve', item_id: id(1), expected_version: v(s, 1), tasks });
		const bad = (extra: object) => go([...approvedTasks(1), { item_id: id(95), link_id: id(75), title: 'Bad', ...extra }]);
		expect(refused(bad({ title: '' })).code).toBe('invalid_request');
		expect(refused(bad({ assignee_id: 'ghost' })).code).toBe('unknown_recipient');
		expect(refused(bad({ team_id: 'design' })).code).toBe('invalid_request');
		expect(refused(bad({ priority: 'asap' })).code).toBe('invalid_request');
		expect(refused(bad({ item_id: id(90) })).code).toBe('already_exists');
		expect(refused(bad({ link_id: id(70) })).code).toBe('already_exists');
		expect(refused(go([])).code).toBe('invalid_request');
		expect(refused(go(Array.from({ length: 51 }, (_, i) => ({ item_id: `t${i}`, link_id: `l${i}`, title: 'T' })))).code).toBe('invalid_request');
		expect(at(s, 1).summary.status).toBe('scoped');
		expect(summaries(s, 'p1').filter((t) => t.kind === 'task')).toHaveLength(0);
	});

	it('is refused when the project has been archived since, and a task count restarts from the project own sequence', () => {
		const s = scoped(created(emptyWork(), 1, { project_id: 'p1' }), 1);
		const archived: WorkContext = { ...ctx, projects: [{ id: 'p1', teamIds: ['web'], archived: true }] };
		expect(refused(() => run(s, { method: 'request.approve', item_id: id(1), expected_version: v(s, 1), tasks: approvedTasks() }, archived)).code).toBe('invalid_request');
		const withTask = run(s, { method: 'task.create', item_id: id(3), project_id: 'p1', title: 'Earlier' }).state;
		const after = run(withTask, { method: 'request.approve', item_id: id(1), expected_version: v(withTask, 1), tasks: approvedTasks(1) }).state;
		expect(at(after, 90).summary.number).toBe(2);
	});

	it('a task that extends a feature does not need the outcome to name one', () => {
		const s = scoped(created(emptyWork(), 1, { project_id: 'p1' }), 1);
		const after = run(s, { method: 'request.approve', item_id: id(1), expected_version: v(s, 1), tasks: approvedTasks(1) }).state;
		expect(at(after, 90).summary.feature_id).toBeUndefined();
	});
});

describe('REQ declining and merging', () => {
	it('declining needs a reason and ends the request from any open state', () => {
		for (const prepare of [(s: WorkState) => s, (s: WorkState) => run(s, { method: 'request.route', item_id: id(1), expected_version: 1, scoper_id: 'eden' }).state, (s: WorkState) => scoped(s, 1)]) {
			const s = prepare(created(emptyWork(), 1));
			expect(refused(() => run(s, { method: 'request.decline', item_id: id(1), expected_version: v(s, 1), reason: '  ' })).code).toBe('invalid_request');
			expect(refused(() => run(s, { method: 'request.decline', item_id: id(1), expected_version: v(s, 1), reason: 'x'.repeat(1001) })).code).toBe('invalid_request');
			const done = run(s, { method: 'request.decline', item_id: id(1), expected_version: v(s, 1), reason: 'Not now' }).state;
			expect([at(done, 1).summary.status, itemOf(done, id(1), ctx.now).decline_reason]).toEqual(['declined', 'Not now']);
		}
	});

	it('merging closes this request into another open one, which quotes its words and gains its creator as a watcher', () => {
		let s = run(emptyWork(), { method: 'request.create', item_id: id(1), text: 'Dark mode please' }, { ...ctx, actor: 'eden' }).state;
		s = created(s, 2, { text: 'Dark mode for the docs reader' });
		s = run(s, { method: 'request.merge', item_id: id(1), expected_version: v(s, 1), into: id(2), comment_id: id(31) }, { ...ctx, actor: 'ada' }).state;
		expect([at(s, 1).summary.status, itemOf(s, id(1), ctx.now).merged_into]).toEqual(['merged', id(2)]);
		expect(notes(s, 2).map(([, body]) => body)).toEqual(['Dark mode please']);
		expect(at(s, 2).watchers).toEqual(['owner', 'eden']);
		expect(at(s, 2).summary.version).toBe(2);
	});

	it('cuts a long quoted request and says where it continues', () => {
		let s = created(created(emptyWork(), 1, { text: 'y'.repeat(12_000) }), 2);
		s = run(s, { method: 'request.merge', item_id: id(1), expected_version: v(s, 1), into: id(2), comment_id: id(31) }).state;
		const body = notes(s, 2)[0][1] as string;
		const marker = '\n(continued in REQ-1)';
		expect(body).toBe(`${'y'.repeat(10_000 - marker.length)}${marker}`);
		expect([...body]).toHaveLength(10_000);
	});

	it('refuses merging into itself, into a closed request, into a task or nothing, or with a reused comment id', () => {
		let s = created(created(created(emptyWork(), 1), 2), 3);
		s = run(s, { method: 'request.decline', item_id: id(3), expected_version: 1, reason: 'no' }).state;
		s = run(s, { method: 'task.create', item_id: id(4), project_id: 'p1', title: 'T' }).state;
		const merge = (into: string, comment_id = id(31)) => () => run(s, { method: 'request.merge', item_id: id(1), expected_version: v(s, 1), into, comment_id });
		expect(refused(merge(id(1))).message).toContain('itself');
		expect(refused(merge(id(3))).code).toBe('invalid_request');
		expect(refused(merge(id(4))).code).toBe('invalid_request');
		expect(refused(merge('nope')).code).toBe('not_found');
		const merged = merge(id(2))().state;
		expect(refused(() => run(merged, { method: 'request.merge', item_id: id(2), expected_version: v(merged, 2), into: id(1), comment_id: id(31) })).code).toBe('invalid_request');
		const other = created(merged, 5);
		expect(refused(() => run(other, { method: 'request.merge', item_id: id(5), expected_version: 1, into: id(2), comment_id: id(31) })).code).toBe('already_exists');
	});
});

describe('REQ updating', () => {
	it('changes the title, kind and project of an open request, and clears the project', () => {
		let s = created(emptyWork(), 1);
		s = run(s, { method: 'request.update', item_id: id(1), expected_version: 1, title: 'New title', request_kind: 'technical', project_id: 'p1' }).state;
		expect([at(s, 1).summary.title, at(s, 1).summary.request_kind, at(s, 1).summary.project_id]).toEqual(['New title', 'technical', 'p1']);
		s = run(s, { method: 'request.update', item_id: id(1), expected_version: 2, clear_project: true }).state;
		expect(at(s, 1).summary.project_id).toBeUndefined();
	});

	it('refuses nothing to change, a project and its clearing together, an archived project, and a bad kind', () => {
		const s = created(emptyWork(), 1);
		const up = (extra: object) => () => run(s, { method: 'request.update', item_id: id(1), expected_version: 1, ...extra });
		expect(refused(up({})).message).toBe('Nothing to change.');
		expect(refused(up({ project_id: 'p1', clear_project: true })).code).toBe('invalid_request');
		expect(refused(up({ project_id: 'old' })).code).toBe('invalid_request');
		expect(refused(up({ request_kind: 'bug' })).code).toBe('invalid_request');
		expect(refused(up({ title: 'a\nb' })).code).toBe('invalid_request');
	});

	it('never edits the original words', () => {
		const s = run(created(emptyWork(), 1, { text: 'As typed' }), { method: 'request.update', item_id: id(1), expected_version: 1, title: 'Other' }).state;
		expect(itemOf(s, id(1), ctx.now).text).toBe('As typed');
	});
});

describe('REQ activity', () => {
	it('records declining with its reason, merging on both requests, and approving with the tasks it made', () => {
		let s = created(created(created(emptyWork(), 1, { project_id: 'p1' }), 2), 3);
		s = run(s, { method: 'request.decline', item_id: id(2), expected_version: 1, reason: 'Not now' }).state;
		s = run(s, { method: 'request.merge', item_id: id(3), expected_version: 1, into: id(1), comment_id: id(30) }).state;
		expect(story(s, 2).at(-1)).toEqual([2, 'owner', 'status_changed', 'new>declined:Not now']);
		expect(story(s, 3).at(-1)).toEqual([2, 'owner', 'status_changed', 'new>merged:Merged into REQ-1']);
		expect(story(s, 1).at(-1)).toEqual([2, 'owner', 'comment', 'Request 3']);
		const ready = scoped(s, 1);
		const approved = run(ready, { method: 'request.approve', item_id: id(1), expected_version: v(ready, 1), tasks: approvedTasks(1) }).state;
		expect(story(approved, 1).at(-1)?.slice(2)).toEqual(['status_changed', 'scoped>approved']);
		expect(story(approved, 90)).toEqual([[1, 'owner', 'created', '']]);
	});

	it('a request with a repeated comment id is refused in ask changes and in merge, and the feed reads as a copy', () => {
		const s = scoped(created(created(emptyWork(), 1), 2), 1);
		const noted = run(s, { method: 'request.ask_changes', item_id: id(1), expected_version: v(s, 1), comment_id: id(30), note: 'n' }).state;
		expect(refused(() => run(noted, { method: 'request.merge', item_id: id(2), expected_version: 1, into: id(1), comment_id: id(30) })).code).toBe('already_exists');
		const feed = activityOf(noted, id(1));
		feed.length = 0;
		expect(activityOf(noted, id(1)).length).toBeGreaterThan(0);
	});

	const story = (state: WorkState, n: number) =>
		activityOf(state, id(n)).map((e) => [
			e.version,
			e.actor_id,
			e.change.type,
			e.change.type === 'status_changed'
				? `${e.change.from}>${e.change.to}${e.change.reason ? `:${e.change.reason}` : ''}`
				: e.change.type === 'comment'
					? e.change.body
					: e.change.type === 'routed'
						? `${e.change.from}>${e.change.to} ${e.change.scoper_from ?? '-'}>${e.change.scoper_to ?? '-'}${e.change.reason ? ` (${e.change.reason})` : ''}${e.change.routed_by ? ` by ${e.change.routed_by}` : ''}`
						: e.change.type === 'fields_changed'
							? e.change.fields.map((f) => `${f.field}:${f.from ?? '-'}>${f.to ?? '-'}`).join('+')
							: ''
		]);

	it('tells what happened to a request, in order: one event per call, at the version that call made', () => {
		let s = created(emptyWork(), 1, { scoper_id: 'eden', route_reason: 'designer', routed_by: 'rule' });
		s = run(s, { method: 'request.update', item_id: id(1), expected_version: 1, title: 'Renamed', project_id: 'p1' }).state;
		s = run(s, { method: 'request.route', item_id: id(1), expected_version: 2, scoper_id: 'ada', route_reason: 'protocol', routed_by: 'person' }).state;
		s = run(s, { method: 'request.accept', item_id: id(1), expected_version: 3 }, { ...ctx, actor: 'ada' }).state;
		s = run(s, { method: 'request.scope', item_id: id(1), expected_version: 4, outcome: outcome() }, { ...ctx, actor: 'ada' }).state;
		s = run(s, { method: 'request.ask_changes', item_id: id(1), expected_version: 5, comment_id: id(30), note: 'Daily or weekly?' }).state;
		expect(story(s, 1)).toEqual([
			[1, 'owner', 'created', ''],
			[2, 'owner', 'fields_changed', 'title:Request 1>Renamed+project_id:->p1'],
			[3, 'owner', 'routed', 'routed>routed eden>ada (protocol) by person'],
			[4, 'ada', 'routed', 'routed>scoping ada>ada'],
			[5, 'ada', 'status_changed', 'scoping>scoped'],
			[6, 'owner', 'status_changed', 'scoped>scoping:Daily or weekly?']
		]);
	});

	it('a route from Not routed and taking it back are each one routed event, and routing again to the same scoper is one too', () => {
		let s = created(emptyWork(), 1);
		s = run(s, { method: 'request.route', item_id: id(1), expected_version: 1, scoper_id: 'eden' }).state;
		s = run(s, { method: 'request.route', item_id: id(1), expected_version: 2, scoper_id: 'eden', route_reason: 'again' }).state;
		s = run(s, { method: 'request.route', item_id: id(1), expected_version: 3 }).state;
		expect(story(s, 1).slice(1)).toEqual([
			[2, 'owner', 'routed', 'new>routed ->eden'],
			[3, 'owner', 'routed', 'routed>routed eden>eden (again)'],
			[4, 'owner', 'routed', 'routed>new eden>-']
		]);
	});

	it('updating to what it already is changes nothing: no version, no event', () => {
		let s = created(emptyWork(), 1, { project_id: 'p1' });
		s = run(s, { method: 'request.update', item_id: id(1), expected_version: 1, title: 'Request 1', project_id: 'p1' }).state;
		expect(v(s, 1)).toBe(1);
		expect(story(s, 1)).toEqual([[1, 'owner', 'created', '']]);
	});

	it('every version of a request has exactly one event, across a whole story, on every item it touches', () => {
		let s = created(created(created(emptyWork(), 1, { scoper_id: 'eden', project_id: 'p1' }), 2), 3);
		s = scoped(s, 1);
		s = run(s, { method: 'request.ask_changes', item_id: id(1), expected_version: v(s, 1), comment_id: id(30), note: 'n' }).state;
		s = run(s, { method: 'request.merge', item_id: id(3), expected_version: 1, into: id(1), comment_id: id(31) }).state;
		s = run(s, { method: 'request.decline', item_id: id(2), expected_version: 1, reason: 'no' }).state;
		s = run(s, { method: 'request.route', item_id: id(1), expected_version: v(s, 1), scoper_id: 'ada' }).state;
		for (const key of Object.keys(s.items)) {
			const versions = activityOf(s, key).map((e) => e.version);
			expect(versions, key).toEqual(Array.from({ length: s.items[key].summary.version }, (_, i) => i + 1));
		}
	});

	it('a note cited by ask changes can not be cited again, here or by a merge', () => {
		const s = scoped(created(created(emptyWork(), 1), 2), 1);
		const noted = run(s, { method: 'request.ask_changes', item_id: id(1), expected_version: v(s, 1), comment_id: id(30), note: 'n' }).state;
		const again = run(noted, { method: 'request.scope', item_id: id(1), expected_version: v(noted, 1), outcome: outcome() }).state;
		expect(refused(() => run(again, { method: 'request.ask_changes', item_id: id(1), expected_version: v(again, 1), comment_id: id(30), note: 'n' })).code).toBe('already_exists');
		expect(refused(() => run(noted, { method: 'request.merge', item_id: id(2), expected_version: 1, into: id(1), comment_id: id(30) })).code).toBe('already_exists');
	});
});
