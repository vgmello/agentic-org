import { describe, expect, it } from 'vitest';
import { applyWork, emptyWork, similarRequests, type WorkCall, type WorkContext, type WorkState } from './engine';

const ctx: WorkContext = { now: '2026-11-08T10:00:00.000Z', actor: 'owner', projects: [{ id: 'p1', teamIds: [] }], members: ['owner', 'eden'] };
const id = (n: number) => `01JC00000000000000000000${String(n).padStart(2, '0')}`;
let change = 0;
const run = (state: WorkState, call: object) => applyWork(state, { change_id: `01JC0000000000000000009${String(++change).padStart(3, '0')}`, ...call } as WorkCall, ctx).state;
const request = (s: WorkState, n: number, text: string, extra: object = {}) => run(s, { method: 'request.create', item_id: id(n), text, ...extra });
const refused = (fn: () => unknown) => {
	try {
		fn();
	} catch (e) {
		return e as { code: string };
	}
	throw new Error('expected a refusal');
};
const titles = (s: WorkState, text: string, extra: object = {}) => similarRequests(s, { text, ...extra }).map((i) => i.title);

describe('request.similar', () => {
	const base = request(request(request(emptyWork(), 1, 'Dark mode for the docs reader'), 2, 'Checklists on tasks', { title: 'Checklists' }), 3, 'A weekly digest of the work, sent by email', { title: 'Digest' });

	it('ranks open requests by how many of the words start a word of the title or the request\'s own words', () => {
		expect(titles(base, 'I would like a dark mode for reading the docs')).toEqual(['Dark mode for the docs reader']);
		// "Digest" matches by its own words (weekly, email), not its title.
		expect(titles(base, 'send the weekly work by email')).toEqual(['Digest']);
	});

	it('needs two of the words, or the only one there is, and ignores short and common words and the @handle', () => {
		expect(titles(base, 'something about the docs')).toEqual([]);
		expect(titles(base, 'checklists')).toEqual(['Checklists']);
		expect(titles(base, '@ac-product-designer-eden please')).toEqual([]);
		expect(titles(base, 'an of to it')).toEqual([]);
	});

	it('does not count the @handle as words, reads words of 3 characters, and returns five by default', () => {
		const s = request(emptyWork(), 1, 'Dark mode', { title: 'Dark mode' });
		// Were the handle's own words counted, these would match "Dark mode".
		expect(titles(s, '@dark-mode please')).toEqual([]);
		expect(titles(s, 'dark mode')).toEqual(['Dark mode']);
		// Words shorter than 3 characters are not words: "ui ux" says nothing.
		expect(titles(request(emptyWork(), 1, 'UI UX polish'), 'ui ux')).toEqual([]);
		const many = Array.from({ length: 7 }, (_, n) => n + 1).reduce((acc, n) => request(acc, n, 'Weekly digest by email'), emptyWork());
		expect(similarRequests(many, { text: 'weekly digest' })).toHaveLength(5);
		expect(similarRequests(many, { text: 'weekly digest', limit: 10 })).toHaveLength(7);
	});

	it('puts the one with most hits first, then the newest, up to the limit', () => {
		const s = request(request(request(base, 4, 'Dark mode please', { title: 'Dark mode' }), 5, 'Dark mode for docs, the reader', { title: 'Dark mode docs' }), 6, 'Dark mode', { title: 'Dark mode again' });
		// Two have all four words (the newer first), two have only dark and mode (the newer first).
		expect(titles(s, 'dark mode docs reader')).toEqual(['Dark mode docs', 'Dark mode for the docs reader', 'Dark mode again', 'Dark mode']);
		expect(titles(s, 'dark mode docs reader', { limit: 2 })).toHaveLength(2);
		expect(titles(s, 'dark mode')).toEqual(['Dark mode again', 'Dark mode docs', 'Dark mode', 'Dark mode for the docs reader']);
	});

	it('is only for requests not yet scoped: not scoped, approved, declined, deleted or tasks', () => {
		let s = request(request(request(emptyWork(), 1, 'Dark mode one'), 2, 'Dark mode two'), 3, 'Dark mode three');
		s = run(s, { method: 'request.accept', item_id: id(1), expected_version: 1 });
		s = run(s, { method: 'request.decline', item_id: id(2), expected_version: 1, reason: 'no' });
		s = run(s, { method: 'work.delete', item_id: id(3), expected_version: 1 });
		s = run(s, { method: 'task.create', item_id: id(9), project_id: 'p1', title: 'Dark mode task' });
		expect(titles(s, 'dark mode')).toEqual(['Dark mode one']);
		s = run(s, { method: 'request.scope', item_id: id(1), expected_version: 2, outcome: { docs: [], draft_tasks: [] } });
		expect(titles(s, 'dark mode')).toEqual([]);
	});

	it('can be narrowed to a project', () => {
		const s = request(request(emptyWork(), 1, 'Dark mode in Atlas', { project_id: 'p1' }), 2, 'Dark mode elsewhere');
		expect(titles(s, 'dark mode', { project_id: 'p1' })).toEqual(['Dark mode in Atlas']);
	});

	it('refuses text over 200 characters and a limit out of range, and returns copies of nothing it should not', () => {
		expect(refused(() => similarRequests(base, { text: 'x'.repeat(201) })).code).toBe('invalid_request');
		expect(() => similarRequests(base, { text: 'x'.repeat(200) })).not.toThrow();
		for (const limit of [0, 11, 2.5]) expect(refused(() => similarRequests(base, { text: 'dark mode', limit })).code).toBe('invalid_request');
		expect(similarRequests(base, { text: '' })).toEqual([]);
	});
});
