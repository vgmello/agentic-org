import { describe, expect, it, vi } from 'vitest';
import { render } from 'svelte/server';
import Rows from './row-data.test.svelte';
import { messageRows } from './row-data';
import { nameOf } from './model';
import { ctxAs, fresh, NOW } from './testkit';
import type { ChatMessage } from './types';

vi.mock('./row-data', async importActual => {
	const actual = await importActual<typeof import('./row-data')>();
	return { ...actual, messageRows: vi.fn(actual.messageRows) };
});
vi.mock('./model', async importActual => {
	const actual = await importActual<typeof import('./model')>();
	return { ...actual, nameOf: vi.fn(actual.nameOf) };
});
const ctx = ctxAs();
const message = (id: string, extra: Partial<ChatMessage> = {}): ChatMessage => ({ id, conversation_id: 'c-general', from: 'sam', kind: 'reply', body: 'Short.', created_at: NOW, ...extra });

describe('MEM-2 row data', () => {
	it('groups once for 1,000 actual row renders, and once again for a changed snapshot', () => {
		const state = { ...fresh(), messages: Array.from({ length: 1000 }, (_, i) => message(`m-${i}`)) };
		vi.mocked(messageRows).mockClear();
		const first = render(Rows, { props: { state, ctx } });
		expect(first.body.match(/data-testid="message"/g)).toHaveLength(1000);
		expect(messageRows).toHaveBeenCalledTimes(1);
		const next = { ...state, seq: state.seq + 1, messages: [...state.messages, message('new')] };
		expect(render(Rows, { props: { state: next, ctx } }).body.match(/data-testid="message"/g)).toHaveLength(1001);
		expect(messageRows).toHaveBeenCalledTimes(2);
	});
	it('preserves sorted replies without mutating snapshot order, with deterministic timestamp ties', () => {
		const root = message('root');
		const replies = [message('z', { thread_root_id: root.id, created_at: '2026-11-08T13:00:00Z' }), message('b', { thread_root_id: root.id }), message('a', { thread_root_id: root.id })];
		const state = { ...fresh(), messages: [root, ...replies, message('other', { thread_root_id: 'elsewhere' })] };
		const rows = messageRows(state, ctx);
		expect(rows.forMessage(root).replies.map(m => m.id)).toEqual(['a', 'b', 'z']);
		expect(state.messages.slice(1, 4)).toEqual(replies);
		expect(rows.forMessage(message('missing')).replies).toEqual([]);
	});
	it('uses chronological instants rather than timestamp spelling', () => {
		const root = message('root');
		const state = { ...fresh(), messages: [message('later', { thread_root_id: root.id, created_at: '2026-11-08T12:10:00Z' }), message('earlier', { thread_root_id: root.id, created_at: '2026-11-08T13:00:00+01:00' })] };
		expect(messageRows(state, ctx).forMessage(root).replies.map(m => m.id)).toEqual(['earlier', 'later']);
	});
	it('orders an entirely tied reply group by id in either input order', () => {
		for (const ids of [['a', 'b'], ['b', 'a']]) {
			const state = { ...fresh(), messages: ids.map(id => message(id, { thread_root_id: 'root' })) };
			expect(messageRows(state, ctx).forMessage(message('root')).replies.map(m => m.id)).toEqual(['a', 'b']);
		}
	});
	it('does not sort ordered reply groups or collect unrelated top-level messages', () => {
		const state = { ...fresh(), messages: [message('newer', { created_at: '2026-11-09T12:00:00Z' }), message('older'), message('a', { thread_root_id: 'root' }), message('b', { thread_root_id: 'root' })] };
		const sort = vi.spyOn(Array.prototype, 'sort');
		let calls: number;
		try { messageRows(state, ctx); calls = sort.mock.calls.length; }
		finally { sort.mockRestore(); }
		expect(calls).toBe(0);
	});
	it('resolves a folded sender once across multiple roots', () => {
		vi.mocked(nameOf).mockClear();
		const state = { ...fresh(), messages: [message('a', { thread_root_id: 'root-a', folded: true }), message('b', { thread_root_id: 'root-b', folded: true })] };
		const rows = messageRows(state, ctx);
		expect(rows.forMessage(message('root-a')).folded).toBe('Sam Okafor posted 1 more update');
		expect(rows.forMessage(message('root-b')).folded).toBe('Sam Okafor posted 1 more update');
		expect(nameOf).toHaveBeenCalledTimes(1);
	});
	it('keeps already ordered groups and reports only folded replies using the first folded sender', () => {
		const root = message('root');
		const state = { ...fresh(), messages: [message('a', { thread_root_id: root.id, from: 'kai' }), message('b', { thread_root_id: root.id, folded: true }), message('c', { thread_root_id: root.id, folded: true, from: 'kai' })] };
		const data = messageRows(state, ctx).forMessage(root);
		expect(data.replies.map(m => m.id)).toEqual(['a', 'b', 'c']);
		expect(data.folded).toBe('Sam Okafor posted 2 more updates');
		state.messages.pop();
		expect(messageRows(state, ctx).forMessage(root).folded).toBe('Sam Okafor posted 1 more update');
		state.messages.pop();
		expect(messageRows(state, ctx).forMessage(root).folded).toBeUndefined();
	});
	it('scopes mutes to both conversation and member, preserving first-match behavior', () => {
		const state = { ...fresh(), mutes: [{ conversation_id: 'a:b', member_id: 'c', mode: 'muted' as const }, { conversation_id: 'a', member_id: 'b:c', mode: 'digest' as const }, { conversation_id: 'a:b', member_id: 'c', mode: 'none' as const }] };
		const rows = messageRows(state, ctx);
		expect(rows.forMessage(message('one', { conversation_id: 'a:b', from: 'c' })).mode).toBe('muted');
		expect(rows.forMessage(message('two', { conversation_id: 'a', from: 'b:c' })).mode).toBe('digest');
		expect(rows.forMessage(message('three')).mode).toBe('none');
	});
});
