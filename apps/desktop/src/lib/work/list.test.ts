import { describe, expect, it } from 'vitest';
import { applyWork, emptyWork, listItems, type WorkCall, type WorkContext, type WorkState } from './engine';

const ctx: WorkContext = { now: '2026-11-08T10:00:00.000Z', actor: 'owner', projects: [{ id: 'p1', teamIds: [] }, { id: 'p2', teamIds: [] }], members: ['owner', 'eden'] };
const id = (n: number) => `01JC00000000000000000000${String(n).padStart(2, '0')}`;
let change = 0;
const run = (state: WorkState, call: object) => applyWork(state, { change_id: `01JC0000000000000000009${String(++change).padStart(3, '0')}`, ...call } as WorkCall, ctx).state;
const request = (s: WorkState, n: number, text: string, extra: object = {}) => run(s, { method: 'request.create', item_id: id(n), text, ...extra });
const titles = (s: WorkState, q: Parameters<typeof listItems>[1]) => listItems(s, q).map((i) => i.title);
const refused = (fn: () => unknown) => {
	try {
		fn();
	} catch (e) {
		return e as { code: string };
	}
	throw new Error('expected a refusal');
};

describe('work.list text', () => {
	const s = request(request(request(emptyWork(), 1, 'Dark mode for the docs reader'), 2, 'Weekly digest by email', { title: 'Digest' }), 3, 'A cheat sheet of keyboard shortcuts');

	it('matches word prefixes, without case, over the title', () => {
		expect(titles(s, { text: 'DOC' })).toEqual(['Dark mode for the docs reader']);
		expect(titles(s, { text: 'dar mo' })).toEqual(['Dark mode for the docs reader']);
		expect(titles(s, { text: 'ocs' })).toEqual([]); // the start of a word, not the middle
	});

	it('also reads the request\'s own words, not only the title', () => {
		expect(titles(s, { text: 'weekly email' })).toEqual(['Digest']);
		expect(titles(s, { text: 'shortc' })).toEqual(['A cheat sheet of keyboard shortcuts']);
	});

	it('wants every word of the text to start some word, and a blank text asks for everything', () => {
		expect(titles(s, { text: 'dark digest' })).toEqual([]);
		expect(listItems(s, { text: '   ' })).toHaveLength(3);
		expect(listItems(s, {})).toHaveLength(3);
	});

	it('lists newest number first, up to the limit', () => {
		expect(listItems(s, {}).map((i) => i.number)).toEqual([3, 2, 1]);
		expect(listItems(s, { limit: 2 }).map((i) => i.number)).toEqual([3, 2]);
	});

	it('refuses a text over 200 characters or a limit out of range', () => {
		expect(refused(() => listItems(s, { text: 'x'.repeat(201) })).code).toBe('invalid_request');
		expect(() => listItems(s, { text: 'x'.repeat(200) })).not.toThrow();
		for (const limit of [0, 501, 1.5]) expect(refused(() => listItems(s, { limit })).code).toBe('invalid_request');
	});

	it('leaves out deleted and ended items unless asked, and filters by kind, status and project', () => {
		let t = run(s, { method: 'task.create', item_id: id(10), project_id: 'p1', title: 'Dark task' });
		t = run(t, { method: 'request.decline', item_id: id(2), expected_version: 1, reason: 'no' });
		t = run(t, { method: 'work.delete', item_id: id(3), expected_version: 1 });
		// (A task and a request have numbers of their own, so the order among them is not the point here.)
		expect(titles(t, {}).sort()).toEqual(['Dark mode for the docs reader', 'Dark task']);
		expect(titles(t, { include_closed: true }).sort()).toEqual(['Dark mode for the docs reader', 'Dark task', 'Digest']);
		expect(titles(t, { statuses: ['declined'] })).toEqual(['Digest']);
		expect(titles(t, { kinds: ['request'] })).toEqual(['Dark mode for the docs reader']);
		expect(titles(t, { kinds: ['task'], project_id: 'p1' })).toEqual(['Dark task']);
		expect(titles(t, { project_id: 'p2' })).toEqual([]);
	});
});
