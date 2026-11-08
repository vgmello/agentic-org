import { describe, expect, it, vi } from 'vitest';
import type { OrgApi } from '$lib/org/api';
import { emptyOrg, sampleOrg } from '$lib/org/sample';
import { createMockChat } from './api';
import { minutesAfter, NOW } from './testkit';
import type { ChatCall } from './types';

const orgOf = (empty = false): Pick<OrgApi, 'view'> => ({ view: async () => ({ snapshot: empty ? emptyOrg() : sampleOrg('office-1'), sample: true }) });
let n = 0;
const send = (conversation_id: string, body: string): ChatCall => ({ method: 'msg.send', change_id: `c${++n}`, conversation_id, kind: 'msg', body });

describe('the mock chat office', () => {
	it('serves the sample conversations, marked as sample data', async () => {
		const api = createMockChat(orgOf(), { now: () => NOW });
		const s = await api.snapshot('o');
		expect(s.sample).toBe(true);
		expect(s.office_running).toBe(true);
		expect(s.state.conversations.length).toBeGreaterThan(10);
		expect(s.state.messages.length).toBeGreaterThan(20);
	});
	it('an organization with only its owner has #general and nothing else (the welcome card)', async () => {
		const s = await createMockChat(orgOf(true), { now: () => NOW }).snapshot('o');
		expect(s.state.conversations.map((c) => c.name)).toEqual(['general']);
		expect(s.state.messages).toEqual([]);
	});
	it('applies defaults whose deadline has passed when a snapshot is read', async () => {
		let now = NOW;
		const api = createMockChat(orgOf(), { now: () => now });
		const before = (await api.snapshot('o')).state.messages.find((m) => m.default_option)!;
		expect(before.answer).toBeUndefined();
		now = minutesAfter(NOW, 130);
		expect((await api.snapshot('o')).state.messages.find((m) => m.id === before.id)!.answer?.by).toBe('default');
	});
	it('a message to an asleep agent wakes it, shows it typing, and it answers a moment later', async () => {
		const later: (() => void)[] = [];
		const api = createMockChat(orgOf(), { now: () => NOW, replyMs: 10, schedule: (run) => void later.push(run) });
		const changed = vi.fn();
		api.subscribe('o', changed);
		const accepted = await api.command('o', send('c-dm-ines', 'hello'));
		expect(accepted.woke).toEqual(['ines']);
		expect(changed).toHaveBeenCalledTimes(1);
		let s = (await api.snapshot('o')).state;
		expect(s.typing.map((t) => t.member_id)).toEqual(['ines']);
		later.forEach((run) => run());
		expect(changed).toHaveBeenCalledTimes(2);
		s = (await api.snapshot('o')).state;
		expect(s.typing).toEqual([]);
		expect(s.messages.at(-1)).toMatchObject({ from: 'ines', kind: 'reply', conversation_id: 'c-dm-ines' });
		expect(s.messages.at(-1)!.body).toContain('Sample reply');
	});
	it('subscribers stop being told after they unsubscribe', async () => {
		const api = createMockChat(orgOf(), { now: () => NOW, schedule: () => undefined });
		const changed = vi.fn();
		const stop = api.subscribe('o', changed);
		stop();
		await api.command('o', send('c-general', 'x'));
		expect(changed).not.toHaveBeenCalled();
	});
	it('a refused call rejects with the office\'s error and changes nothing', async () => {
		const api = createMockChat(orgOf(), { now: () => NOW });
		await expect(api.command('o', { method: 'chat.answer', change_id: 'z1', ask_id: 'nope', text: 'x' })).rejects.toMatchObject({ source: 'api', code: 'not_found' });
	});
	it('the offline scenario is read-only: the snapshot says so and calls fail', async () => {
		const api = createMockChat(orgOf(), { now: () => NOW, scenario: () => 'offline' });
		expect((await api.snapshot('o')).office_running).toBe(false);
		await expect(api.command('o', send('c-general', 'x'))).rejects.toMatchObject({ code: 'unavailable' });
	});
	it('the fail scenario fails a message once, and sends it on the retry', async () => {
		const api = createMockChat(orgOf(), { now: () => NOW, scenario: () => 'fail', schedule: () => undefined });
		await expect(api.command('o', send('c-general', 'retry me'))).rejects.toMatchObject({ code: 'unavailable', message: "The office didn't answer." });
		await expect(api.command('o', send('c-general', 'retry me'))).resolves.toMatchObject({ conversation_id: 'c-general' });
		expect((await api.snapshot('o')).state.messages.filter((m) => m.body === 'retry me')).toHaveLength(1);
	});
	it('two calls at once are both applied, in order, with their own ids', async () => {
		const api = createMockChat(orgOf(), { now: () => NOW, schedule: () => undefined });
		const [a, b] = await Promise.all([api.command('o', send('c-general', 'first')), api.command('o', send('c-general', 'second'))]);
		expect(a.id).not.toBe(b.id);
		const bodies = (await api.snapshot('o')).state.messages.map((m) => m.body);
		expect(bodies.indexOf('first')).toBeLessThan(bodies.indexOf('second'));
		expect(bodies).toContain('second');
	});
	it('a call that fails does not stop the ones queued behind it', async () => {
		const api = createMockChat(orgOf(), { now: () => NOW, schedule: () => undefined });
		const [bad, good] = await Promise.allSettled([api.command('o', { method: 'chat.answer', change_id: 'q1', ask_id: 'nope', text: 'x' }), api.command('o', send('c-general', 'still goes'))]);
		expect(bad.status).toBe('rejected');
		expect(good.status).toBe('fulfilled');
	});
	it('a retry with the same change id posts the message once', async () => {
		const api = createMockChat(orgOf(), { now: () => NOW, schedule: () => undefined });
		const call = send('c-general', 'only once');
		await api.command('o', call);
		await api.command('o', call);
		expect((await api.snapshot('o')).state.messages.filter((m) => m.body === 'only once')).toHaveLength(1);
	});
	it('keeps each organization\'s conversations apart', async () => {
		const api = createMockChat(orgOf(), { now: () => NOW, schedule: () => undefined });
		await api.command('a', send('c-general', 'only in a'));
		expect((await api.snapshot('a')).state.messages.some((m) => m.body === 'only in a')).toBe(true);
		expect((await api.snapshot('b')).state.messages.some((m) => m.body === 'only in a')).toBe(false);
	});
});
