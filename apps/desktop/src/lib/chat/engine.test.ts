import { describe, expect, it } from 'vitest';
import { applyChat, canRead, emptyChat, feedFor, messagesIn, syncAuto, tick } from './engine';
import { byId, call, channelNamed, conv, ctxAs, fresh, kaiAsk, miraApproval, minutesAfter, NOW, refusal, run } from './testkit';

describe('CHAT-2 · automatic channels and channels people add', () => {
	it('CHAT-2 · #general, one channel per project and one per team exist, with the owner in each', () => {
		const s = syncAuto(emptyChat(), ctxAs());
		const names = s.conversations.map((c) => c.name).sort();
		expect(names).toEqual(['atlas', 'beacon', 'design', 'engineering', 'general', 'onboarding-kit', 'people', 'platform', 'scratchpad', 'web']);
		expect(s.conversations.every((c) => c.kind === 'channel' && c.auto)).toBe(true);
		expect(conv(s, 'c-general').members).toContain('you');
		expect(channelNamed(s, 'atlas').members).toContain('you');
	});
	it('CHAT-2 · members follow the team or project, and a team that is gone archives its channel', () => {
		const s = fresh();
		expect([...channelNamed(s, 'web').members].sort()).toEqual(['mira', 'noor', 'you']);
		expect([...channelNamed(s, 'atlas').members].sort()).toEqual(['kai', 'mira', 'you']);
		const ctx = ctxAs();
		const moved = syncAuto(s, { ...ctx, teams: ctx.teams.map((t) => (t.id === 'web' ? { ...t, member_ids: ['noor', 'kai'] } : t)).filter((t) => t.id !== 'people') });
		expect([...channelNamed(moved, 'web').members].sort()).toEqual(['kai', 'noor', 'you']);
		expect(channelNamed(moved, 'people').archived).toBe(true);
	});
	it('CHAT-2 · an offboarded member leaves the automatic channels', () => {
		const ctx = ctxAs();
		const s = syncAuto(fresh(), { ...ctx, members: ctx.members.map((m) => (m.id === 'noor' ? { ...m, former: true } : m)) });
		expect(conv(s, 'c-general').members).not.toContain('noor');
		expect(channelNamed(s, 'web').members).not.toContain('noor');
	});
	it('CHAT-2 · people add channels and invite members; an automatic channel cannot be edited by hand', () => {
		let s = fresh();
		const created = run(s, { method: 'channel.create', name: 'Release V2', members: ['sam'] });
		s = created.state;
		const c = conv(s, created.accepted.conversation_id!);
		expect(c).toMatchObject({ kind: 'channel', name: 'release-v2' });
		expect(c.members).toEqual(['you', 'sam']);
		s = run(s, { method: 'channel.add_member', id: c.id, member_id: 'kai' }).state;
		expect(conv(s, c.id).members).toContain('kai');
		s = run(s, { method: 'channel.rename', id: c.id, name: 'release-two' }).state;
		expect(conv(s, c.id).name).toBe('release-two');
		expect(refusal(s, { method: 'channel.create', name: 'general', members: [] })?.code).toBe('already_exists');
		expect(refusal(s, { method: 'channel.rename', id: 'c-general', name: 'everyone' })?.code).toBe('invalid_request');
		expect(refusal(s, { method: 'channel.add_member', id: 'c-general', member_id: 'kai' })?.code).toBe('invalid_request');
		s = run(s, { method: 'channel.archive', id: c.id }).state;
		expect(conv(s, c.id).archived).toBe(true);
		expect(refusal(s, { method: 'msg.send', conversation_id: c.id, kind: 'msg', body: 'hi' })?.code).toBe('conversation_archived');
	});
	it('CHAT-2 · agents and bridges do not run channel commands', () => {
		const s = fresh();
		expect(refusal(s, { method: 'channel.create', name: 'x', members: [] }, ctxAs('sam', 'agent'))?.code).toBe('not_authorized');
		expect(refusal(s, { method: 'channel.create', name: 'x', members: [] }, ctxAs('avery', 'bridge'))?.code).toBe('not_authorized');
	});
});

describe('CHAT-3 · direct messages', () => {
	it('CHAT-3 · a DM opens with an agent or a person and is reused', () => {
		const s = fresh();
		const a = run(s, { method: 'dm.open', members: ['kai'] });
		expect(conv(a.state, a.accepted.conversation_id!)).toMatchObject({ kind: 'dm', members: ['you', 'kai'] });
		expect(run(a.state, { method: 'dm.open', members: ['kai'] }).accepted.conversation_id).toBe(a.accepted.conversation_id);
		expect(run(s, { method: 'dm.open', members: ['avery'] }).accepted.conversation_id).toBe('c-dm-avery');
	});
	it('CHAT-3 · a group of up to 8 works and a ninth member is refused', () => {
		const s = fresh();
		const seven = ['avery', 'hana', 'leo', 'sam', 'noor', 'kai', 'mira'];
		const ok = run(s, { method: 'dm.open', members: seven });
		expect(conv(ok.state, ok.accepted.conversation_id!)).toMatchObject({ kind: 'group_dm' });
		expect(conv(ok.state, ok.accepted.conversation_id!).members).toHaveLength(8);
		expect(refusal(s, { method: 'dm.open', members: [...seven, 'juno'] })?.code).toBe('too_many_members');
		expect(refusal(s, { method: 'dm.open', members: [] })?.code).toBe('invalid_request');
		expect(refusal(s, { method: 'dm.open', members: ['nobody'] })?.code).toBe('not_found');
	});
});

describe('CHAT-4 · agents post only questions, hand-offs, replies and announcements', () => {
	const sam = ctxAs('sam', 'agent');
	it('CHAT-4 · an agent cannot post a plain message or tool output', () => {
		const s = fresh();
		expect(refusal(s, { method: 'msg.send', conversation_id: 'c-general', kind: 'msg', body: 'ran cargo test: ok' }, sam)?.code).toBe('kind_not_allowed');
		expect(refusal(s, { method: 'msg.send', conversation_id: 'c-general', kind: 'unknown', body: 'x' }, sam)?.code).toBe('invalid_request');
	});
	it('CHAT-4 · an announcement needs a reason from the short list', () => {
		const s = fresh();
		expect(refusal(s, { method: 'msg.send', conversation_id: 'c-release-v1', kind: 'announcement', body: 'done' }, sam)?.code).toBe('reason_required');
		expect(refusal(s, { method: 'msg.send', conversation_id: 'c-release-v1', kind: 'announcement', body: 'done', reason: 'progress' as never }, sam)?.code).toBe('reason_required');
		expect(refusal(s, { method: 'msg.send', conversation_id: 'c-dm-sam', kind: 'announcement', body: 'done', reason: 'delivered' }, sam)).toBeUndefined();
	});
	it('CHAT-4 · there is no chat kind called request besides the ask: the ask is the request kind', () => {
		const s = fresh();
		const r = run(s, { method: 'msg.send', conversation_id: 'c-dm-sam', kind: 'request', body: 'Ship it?', addressed_to: 'you' }, sam);
		expect(byId(r.state, r.accepted.id!).kind).toBe('request');
		expect(refusal(s, { method: 'msg.send', conversation_id: 'c-dm-sam', kind: 'feature_request' as never, body: 'x' }, sam)?.code).toBe('invalid_request');
	});
	it('CHAT-4 · an agent post longer than 15 lines is cut, pointing at the session log', () => {
		const s = fresh();
		const body = Array.from({ length: 40 }, (_, i) => `line ${i}`).join('\n');
		const r = run(s, { method: 'msg.send', conversation_id: 'c-dm-sam', kind: 'reply', body }, sam);
		const m = byId(r.state, r.accepted.id!);
		expect(m.body.split('\n')).toHaveLength(16);
		expect(m.body).toContain('session log');
		// A person's long message is kept whole.
		const p = run(s, { method: 'msg.send', conversation_id: 'c-dm-sam', kind: 'msg', body });
		expect(byId(p.state, p.accepted.id!).body).toBe(body);
	});
	it('CHAT-4 · an ask or hand-off says who it is for', () => {
		const s = fresh();
		expect(refusal(s, { method: 'msg.send', conversation_id: 'c-general', kind: 'request', body: 'ok?' }, sam)?.code).toBe('invalid_request');
		expect(refusal(s, { method: 'msg.send', conversation_id: 'c-general', kind: 'handoff', body: 'yours', addressed_to: 'sam' }, sam)?.code).toBe('invalid_request');
		expect(refusal(s, { method: 'msg.send', conversation_id: 'c-general', kind: 'handoff', body: 'yours', addressed_to: 'kai' }, sam)).toBeUndefined();
	});
});

describe('CHAT-5 · an ask is a card that answers itself', () => {
	it('CHAT-5 · answering with an option answers the ask, replies under it, and clears Needs you', () => {
		const s = fresh();
		const ask = kaiAsk(s);
		expect(ask.options?.map((o) => o.id)).toEqual(['yes', 'no']);
		const r = run(s, { method: 'chat.answer', ask_id: ask.id, option_id: 'yes' });
		const answered = byId(r.state, ask.id);
		expect(answered.answer).toMatchObject({ by: 'you', option_id: 'yes' });
		const reply = r.state.messages.find((m) => m.thread_root_id === ask.id && m.kind === 'reply');
		expect(reply).toMatchObject({ from: 'you', body: 'Yes, block' });
		expect(refusal(r.state, { method: 'chat.answer', ask_id: ask.id, option_id: 'no' })?.code).toBe('already_exists');
	});
	it('CHAT-5 · Reply… answers with text', () => {
		const s = fresh();
		const r = run(s, { method: 'chat.answer', ask_id: kaiAsk(s).id, text: 'Block, but warn first.' });
		expect(byId(r.state, kaiAsk(s).id).answer).toMatchObject({ by: 'you', text: 'Block, but warn first.' });
		expect(refusal(s, { method: 'chat.answer', ask_id: kaiAsk(s).id })?.code).toBe('invalid_request');
		expect(refusal(s, { method: 'chat.answer', ask_id: kaiAsk(s).id, option_id: 'maybe' })?.code).toBe('invalid_request');
	});
	it('CHAT-5 · an approval is approved, or rejected with a reason', () => {
		const s = fresh();
		const a = miraApproval(s);
		expect(a.options?.map((o) => o.id)).toEqual(['approve', 'reject']);
		expect(refusal(s, { method: 'chat.answer', ask_id: a.id, option_id: 'reject' })?.code).toBe('invalid_request');
		const rejected = run(s, { method: 'chat.answer', ask_id: a.id, option_id: 'reject', text: 'Not before the audit.' });
		expect(rejected.state.messages.at(-1)?.body).toBe('Rejected: Not before the audit.');
		expect(run(s, { method: 'chat.answer', ask_id: a.id, option_id: 'approve' }).state.messages.at(-1)?.body).toBe('Approved');
	});
	it('CHAT-5 · an agent asks with up to 6 options, a default must be one of them, an approval is asked of the owner', () => {
		const s = fresh();
		const sam = ctxAs('sam', 'agent');
		const opts = Array.from({ length: 7 }, (_, i) => ({ id: `o${i}`, label: `Option ${i}` }));
		const ask = (extra: object) => ({ method: 'msg.send' as const, conversation_id: 'c-dm-sam', kind: 'request' as const, body: 'Which?', addressed_to: 'you', ...extra });
		expect(refusal(s, ask({ options: opts }), sam)?.code).toBe('invalid_request');
		expect(refusal(s, ask({ options: opts.slice(0, 2), default_option: 'zzz', default_at: NOW }), sam)?.code).toBe('invalid_request');
		expect(refusal(s, ask({ options: [{ id: 'a', label: 'A' }, { id: 'a', label: 'B' }] }), sam)?.code).toBe('invalid_request');
		expect(refusal(s, ask({ approval: true, addressed_to: 'kai' }), sam)?.code).toBe('invalid_request');
		expect(refusal(s, ask({ options: opts.slice(0, 6) }), sam)).toBeUndefined();
	});
	it('CHAT-5 · a hand-off is accepted by the person it is handed to, and the owner', () => {
		const s = fresh();
		const h = s.messages.find((m) => m.kind === 'handoff' && m.from === 'kai')!;
		expect(refusal(s, { method: 'chat.accept_handoff', message_id: h.id }, ctxAs('noor', 'agent'))?.code).toBe('not_authorized');
		const r = run(s, { method: 'chat.accept_handoff', message_id: h.id }, ctxAs('sam', 'agent'));
		expect(byId(r.state, h.id).accepted_by).toBe('sam');
		expect(refusal(r.state, { method: 'chat.accept_handoff', message_id: h.id }, ctxAs('sam', 'agent'))?.code).toBe('already_exists');
	});
});

describe('CHAT-6 · an ask with a default', () => {
	it('CHAT-6 · past its deadline the default is applied by the office', () => {
		const s = fresh();
		const ask = kaiAsk(s);
		expect(tick(s, NOW)).toBe(s);
		const later = tick(s, minutesAfter(NOW, 121));
		expect(byId(later, ask.id).answer).toMatchObject({ by: 'default', option_id: 'no' });
		expect(byId(later, ask.id).answer?.at).toBe(ask.default_at);
	});
	it('CHAT-6 · an ask answered before its deadline keeps its answer', () => {
		const s = run(fresh(), { method: 'chat.answer', ask_id: kaiAsk(fresh()).id, option_id: 'yes' }).state;
		expect(byId(tick(s, minutesAfter(NOW, 500)), kaiAsk(s).id).answer?.by).toBe('you');
	});
});

describe('CHAT-7 · talking to an asleep agent wakes it', () => {
	it('CHAT-7 · a message to an asleep agent wakes it and it shows as typing', () => {
		const s = fresh();
		expect(s.presence.ines).toBe('asleep');
		const r = run(s, { method: 'msg.send', conversation_id: 'c-dm-ines', kind: 'msg', body: 'Can you pull the headcount?' });
		expect(r.accepted.woke).toEqual(['ines']);
		expect(r.state.presence.ines).toBe('working');
		expect(r.state.typing).toEqual([{ conversation_id: 'c-dm-ines', member_id: 'ines', until: '2026-11-08T12:00:04.000Z' }]);
	});
	it('CHAT-7 · a message to a working agent shows typing without waking, and a paused agent is not woken', () => {
		const s = fresh();
		const sam = run(s, { method: 'msg.send', conversation_id: 'c-dm-sam', kind: 'msg', body: 'ping' });
		expect(sam.accepted.woke).toBeUndefined();
		expect(sam.state.typing.map((t) => t.member_id)).toEqual(['sam']);
		const pax = run(s, { method: 'msg.send', conversation_id: 'c-dm-pax', kind: 'msg', body: 'ping' });
		expect(pax.accepted.woke).toBeUndefined();
		expect(pax.state.presence.pax).toBe('paused');
		expect(pax.state.typing).toEqual([]);
	});
	it('CHAT-7 · a mention in a channel wakes an asleep agent that is a member', () => {
		const s = fresh();
		const r = run(s, { method: 'msg.send', conversation_id: 'c-general', kind: 'msg', body: '@ines are you there?' });
		expect(r.accepted.woke).toEqual(['ines']);
	});
	it('CHAT-7 · typing ends with its time', () => {
		const r = run(fresh(), { method: 'msg.send', conversation_id: 'c-dm-ines', kind: 'msg', body: 'hello' });
		expect(tick(r.state, minutesAfter(NOW, 1)).typing).toEqual([]);
	});
});

describe('CHAT-8 · threads', () => {
	it('CHAT-8 · an agent replying to a channel message lands in that message\'s thread', () => {
		const s = fresh();
		const root = s.messages.find((m) => m.body.startsWith('@sam your part'))!;
		const r = run(s, { method: 'msg.send', conversation_id: root.conversation_id, kind: 'reply', body: 'Done.', reply_to: root.id }, ctxAs('sam', 'agent'));
		expect(byId(r.state, r.accepted.id!).thread_root_id).toBe(root.id);
		expect(messagesIn(r.state, ctxAs('sam', 'agent'), 'sam', root.conversation_id).some((m) => m.id === r.accepted.id)).toBe(false);
	});
	it('CHAT-8 · a reply to a reply goes to the same root; a reply with nothing to answer is refused in a channel', () => {
		const s = fresh();
		const root = s.messages.find((m) => m.body.startsWith('@sam your part'))!;
		const inThread = s.messages.find((m) => m.thread_root_id === root.id)!;
		const r = run(s, { method: 'msg.send', conversation_id: root.conversation_id, kind: 'reply', body: 'ok', reply_to: inThread.id }, ctxAs('sam', 'agent'));
		expect(byId(r.state, r.accepted.id!).thread_root_id).toBe(root.id);
		expect(refusal(s, { method: 'msg.send', conversation_id: root.conversation_id, kind: 'reply', body: 'ok' }, ctxAs('sam', 'agent'))?.code).toBe('invalid_request');
	});
	it('CHAT-8 · a thread reply can also be sent to the channel', () => {
		const s = fresh();
		const root = s.messages.find((m) => m.body.startsWith('@sam your part'))!;
		const r = run(s, { method: 'msg.send', conversation_id: root.conversation_id, thread_root_id: root.id, kind: 'msg', body: 'FYI all', also_in_channel: true });
		expect(messagesIn(r.state, ctxAs(), 'you', root.conversation_id).some((m) => m.id === r.accepted.id)).toBe(true);
		expect(messagesIn(r.state, ctxAs(), 'you', root.conversation_id, root.id).some((m) => m.id === r.accepted.id)).toBe(true);
		expect(refusal(s, { method: 'msg.send', conversation_id: root.conversation_id, kind: 'msg', body: 'x', also_in_channel: true })?.code).toBe('invalid_request');
	});
	it('CHAT-8 · a thread that is not in the conversation is not found', () => {
		const s = fresh();
		expect(refusal(s, { method: 'msg.send', conversation_id: 'c-general', thread_root_id: 'nope', kind: 'msg', body: 'x' })?.code).toBe('not_found');
	});
});

describe('CHAT-10 · announcements are limited per agent per channel', () => {
	const sam = ctxAs('sam', 'agent');
	const announce = (state = fresh(), ctx = sam, conversation_id = 'c-release-v1') => run(state, { method: 'msg.send', conversation_id, kind: 'announcement', body: 'Something', reason: 'report' }, ctx);
	it('CHAT-10 · the first announcement posts, and extras within 15 minutes fold into a thread under the last one', () => {
		const first = announce();
		const root = byId(first.state, first.accepted.id!);
		expect(root.thread_root_id).toBeUndefined();
		const second = announce(first.state, ctxAs('sam', 'agent', minutesAfter(NOW, 5)));
		expect(second.accepted.folded_into).toBe(root.id);
		expect(byId(second.state, second.accepted.id!)).toMatchObject({ thread_root_id: root.id, folded: true });
		const third = announce(second.state, ctxAs('sam', 'agent', minutesAfter(NOW, 14)));
		expect(third.accepted.folded_into).toBe(root.id);
	});
	it('CHAT-10 · after 15 minutes a new announcement posts again; DMs and other agents and channels are not limited', () => {
		const first = announce();
		expect(announce(first.state, ctxAs('sam', 'agent', minutesAfter(NOW, 15))).accepted.folded_into).toBeUndefined();
		expect(announce(first.state, ctxAs('kai', 'agent', minutesAfter(NOW, 1))).accepted.folded_into).toBeUndefined();
		expect(announce(first.state, sam, 'c-general').accepted.folded_into).toBeUndefined();
		expect(announce(first.state, sam, 'c-dm-sam').accepted.folded_into).toBeUndefined();
	});
	it('CHAT-10 · the sample already folds Sam\'s extra updates under the last announcement', () => {
		const s = fresh();
		const root = s.messages.find((m) => m.kind === 'announcement' && m.body.startsWith('Ready for review'))!;
		expect(s.messages.filter((m) => m.thread_root_id === root.id && m.folded)).toHaveLength(3);
	});
});

describe('CHAT-11 · mute and digest', () => {
	it('CHAT-11 · only the owner, in the app, mutes an agent that is a member', () => {
		const s = fresh();
		expect(refusal(s, { method: 'chat.mute', conversation_id: 'c-general', member_id: 'sam', mode: 'muted' }, ctxAs('avery', 'bridge'))?.code).toBe('not_authorized');
		expect(refusal(s, { method: 'chat.mute', conversation_id: 'c-general', member_id: 'avery', mode: 'muted' })?.code).toBe('not_a_member');
		expect(refusal(s, { method: 'chat.mute', conversation_id: 'c-design-crit', member_id: 'sam', mode: 'muted' })?.code).toBe('not_a_member');
		const m = run(s, { method: 'chat.mute', conversation_id: 'c-general', member_id: 'sam', mode: 'digest' }).state;
		expect(m.mutes).toEqual([{ conversation_id: 'c-general', member_id: 'sam', mode: 'digest' }]);
		expect(run(m, { method: 'chat.mute', conversation_id: 'c-general', member_id: 'sam', mode: 'none' }).state.mutes).toEqual([]);
	});
});

describe('CHAT-12 · the owner reads any conversation', () => {
	it('CHAT-12 · the owner reads and replies in a conversation they are not in', () => {
		const s = fresh();
		expect(conv(s, 'c-dm-sam-noor').members).not.toContain('you');
		expect(canRead(ctxAs(), 'you', conv(s, 'c-dm-sam-noor'))).toBe(true);
		expect(messagesIn(s, ctxAs(), 'you', 'c-dm-sam-noor').length).toBeGreaterThan(0);
		expect(refusal(s, { method: 'msg.send', conversation_id: 'c-dm-sam-noor', kind: 'msg', body: 'Carry on.' })).toBeUndefined();
	});
	it('CHAT-12 · no one else reads or posts in a conversation they are not in', () => {
		const s = fresh();
		expect(refusal(s, { method: 'msg.send', conversation_id: 'c-dm-sam-noor', kind: 'msg', body: 'hi' }, ctxAs('kai', 'agent'))?.code).toBe('not_a_member');
		expect(() => messagesIn(s, ctxAs('kai', 'agent'), 'kai', 'c-dm-sam-noor')).toThrow();
	});
});

describe('CHAT-16 · what an agent can read', () => {
	const kai = ctxAs('kai', 'agent');
	it('CHAT-16 · an agent reads a channel only while it is a member', () => {
		const s = fresh();
		expect(messagesIn(s, kai, 'kai', 'c-release-v1').length).toBeGreaterThan(0);
		expect(() => messagesIn(s, kai, 'noor', 'c-release-v1')).toThrow(expect.objectContaining({ code: 'not_a_member' }));
	});
	it('CHAT-16 · a thread is read only through its parent conversation', () => {
		const s = fresh();
		const root = s.messages.find((m) => m.body.startsWith('@sam your part'))!;
		expect(messagesIn(s, kai, 'kai', root.conversation_id, root.id).map((m) => m.id)).toContain(root.id);
		// Noor is not in #atlas: the thread is closed to them too.
		expect(() => messagesIn(s, ctxAs('noor', 'agent'), 'noor', root.conversation_id, root.id)).toThrow(expect.objectContaining({ code: 'not_a_member' }));
		expect(() => messagesIn(s, kai, 'kai', root.conversation_id, 'missing')).toThrow(expect.objectContaining({ code: 'not_found' }));
	});
	it('CHAT-16 · after it leaves, an agent stops seeing the channel, including new messages', () => {
		let s = fresh();
		s = run(s, { method: 'channel.remove_member', id: 'c-release-v1', member_id: 'kai' }).state;
		s = run(s, { method: 'msg.send', conversation_id: 'c-release-v1', kind: 'msg', body: 'After kai left.' }).state;
		expect(() => messagesIn(s, kai, 'kai', 'c-release-v1')).toThrow(expect.objectContaining({ code: 'not_a_member' }));
		expect(feedFor(s, kai, 'kai').some((m) => m.conversation_id === 'c-release-v1')).toBe(false);
		expect(refusal(s, { method: 'msg.send', conversation_id: 'c-release-v1', kind: 'reply', body: 'x', reply_to: s.messages.at(-1)!.id }, kai)?.code).toBe('not_a_member');
		expect(refusal(s, { method: 'channel.remove_member', id: 'c-release-v1', member_id: 'kai' })?.code).toBe('not_a_member');
	});
	it('CHAT-16 · a DM is read only by its two members', () => {
		const s = fresh();
		expect(feedFor(s, ctxAs('sam', 'agent'), 'sam').some((m) => m.conversation_id === 'c-dm-ines')).toBe(false);
		expect(feedFor(s, ctxAs('sam', 'agent'), 'sam').some((m) => m.conversation_id === 'c-dm-sam')).toBe(true);
	});
});

describe('CHAT-17 · bridges never act as the owner', () => {
	const bridge = (actor = 'avery') => ctxAs(actor, 'bridge', NOW, { bridge: 'slack' });
	it('CHAT-17 · a bridged message is posted as the bridged person, marked with its platform', () => {
		const r = run(fresh(), { method: 'msg.send', conversation_id: 'c-general', kind: 'msg', body: 'From Slack' }, bridge());
		expect(byId(r.state, r.accepted.id!)).toMatchObject({ from: 'avery', bridge: 'slack' });
	});
	it('CHAT-17 · an ask to the owner cannot be answered from a bridge, even by the owner\'s own identity', () => {
		const s = fresh();
		expect(refusal(s, { method: 'chat.answer', ask_id: kaiAsk(s).id, option_id: 'yes' }, bridge())?.code).toBe('not_authorized');
		expect(refusal(s, { method: 'chat.answer', ask_id: kaiAsk(s).id, option_id: 'yes' }, bridge('you'))?.code).toBe('not_authorized');
		expect(byId(s, kaiAsk(s).id).answer).toBeUndefined();
	});
	it('CHAT-17 · an approval cannot be answered from a bridge or by another person', () => {
		const s = fresh();
		for (const ctx of [bridge(), bridge('you'), ctxAs('avery', 'app')]) expect(refusal(s, { method: 'chat.answer', ask_id: miraApproval(s).id, option_id: 'approve' }, ctx)?.code).toBe('not_authorized');
	});
	it('CHAT-17 · an ask addressed to a bridged person can be answered from the bridge', () => {
		let s = fresh();
		const dm = run(s, { method: 'dm.open', members: ['avery'] }, ctxAs('sam', 'agent'));
		s = dm.state;
		const asked = run(s, { method: 'msg.send', conversation_id: dm.accepted.conversation_id!, kind: 'request', body: 'Thursday ok?', addressed_to: 'avery', options: [{ id: 'y', label: 'Yes' }] }, ctxAs('sam', 'agent'));
		s = asked.state;
		const r = run(s, { method: 'chat.answer', ask_id: asked.accepted.id!, option_id: 'y' }, bridge());
		expect(byId(r.state, asked.accepted.id!).answer?.by).toBe('avery');
		expect(refusal(s, { method: 'chat.answer', ask_id: asked.accepted.id!, option_id: 'y' }, bridge('hana'))?.code).toBe('not_authorized');
	});
	it('CHAT-17 · a bridge cannot accept a hand-off to the owner either', () => {
		let s = fresh();
		const h = run(s, { method: 'msg.send', conversation_id: 'c-dm-sam', kind: 'handoff', body: 'Yours now.', addressed_to: 'you' }, ctxAs('sam', 'agent'));
		s = h.state;
		expect(refusal(s, { method: 'chat.accept_handoff', message_id: h.accepted.id! }, bridge('you'))?.code).toBe('not_authorized');
		expect(refusal(s, { method: 'chat.accept_handoff', message_id: h.accepted.id! })).toBeUndefined();
	});
});

describe('review fixes', () => {
	const bridgeOwner = ctxAs('you', 'bridge', NOW, { bridge: 'slack' });
	it('CHAT-17 · a bridge cannot act as the owner at all: not post, react, pin or read', () => {
		const s = fresh();
		for (const draft of [
			{ method: 'msg.send', conversation_id: 'c-general', kind: 'msg', body: 'as the owner' },
			{ method: 'chat.react', message_id: s.messages[0].id, reaction: 'seen' },
			{ method: 'chat.pin', message_id: s.messages[0].id },
			{ method: 'chat.mark_read', conversation_id: 'c-general' }
		] as const) expect(refusal(s, draft, bridgeOwner)?.code).toBe('not_authorized');
		// The owner's read-everything right is the app's: a DM the owner is in is not readable through a bridge.
		expect(canRead(bridgeOwner, 'you', conv(s, 'c-dm-sam-noor'))).toBe(false);
		expect(canRead(ctxAs(), 'you', conv(s, 'c-dm-sam-noor'))).toBe(true);
	});
	it('CHAT-10 · posting inside a thread does not get around the announcement limit', () => {
		const sam = ctxAs('sam', 'agent');
		const root = s0().messages.find((m) => m.body.startsWith('@sam your part'))!;
		let s = s0();
		const post = (state: typeof s, minutes: number) => run(state, { method: 'msg.send', conversation_id: root.conversation_id, thread_root_id: root.id, kind: 'announcement', reason: 'report', body: 'update' }, ctxAs('sam', 'agent', minutesAfter(NOW, minutes)));
		const first = post(s, 0);
		s = first.state;
		const second = post(s, 3);
		expect(second.accepted.folded_into).toBe(root.id);
		expect(byId(second.state, second.accepted.id!).folded).toBe(true);
		expect(post(second.state, 16).accepted.folded_into).toBeUndefined();
		void sam;
	});
	it('CHAT-16 · mentioning an agent that is not in a group DM does not wake it', () => {
		const s = fresh();
		expect(conv(s, 'c-group').members).not.toContain('ines');
		const r = run(s, { method: 'msg.send', conversation_id: 'c-group', kind: 'msg', body: '@ines are you there?' });
		expect(r.accepted.woke).toBeUndefined();
		expect(r.state.presence.ines).toBe('asleep');
	});
	it('CHAT-6 · a default applied at its deadline goes back to the agent as a reply from the office, under the ask', () => {
		const s = fresh();
		const later = tick(s, minutesAfter(NOW, 121));
		const reply = later.messages.find((m) => m.thread_root_id === kaiAsk(s).id)!;
		expect(reply).toMatchObject({ from: 'office', kind: 'reply', body: "No answer by the deadline: the default applies (No, allow (Eden's AREA-7))." });
		expect(tick(later, minutesAfter(NOW, 200))).toBe(later);
	});
	it('a reply or a message over the size limit is refused, counted in bytes', () => {
		const s = fresh();
		const emoji = '😀'.repeat(70_000);
		expect(emoji.length).toBeLessThan(256 * 1024);
		expect(refusal(s, { method: 'msg.send', conversation_id: 'c-general', kind: 'msg', body: emoji })?.code).toBe('invalid_request');
		expect(refusal(s, { method: 'chat.answer', ask_id: kaiAsk(s).id, text: emoji })?.code).toBe('invalid_request');
	});
	it('CHAT-10 · every folded message in the sample is within the window after its root', () => {
		const s = fresh();
		const folded = s.messages.filter((m) => m.folded);
		expect(folded.length).toBeGreaterThan(0);
		for (const m of folded) expect(Date.parse(m.created_at) - Date.parse(byId(s, m.thread_root_id!).created_at)).toBeLessThan(15 * 60_000);
	});
});
const s0 = fresh;

describe('calls are idempotent and reactions are acknowledgements', () => {
	it('a repeated change id changes nothing', () => {
		const s = fresh();
		const c = call({ method: 'msg.send', conversation_id: 'c-general', kind: 'msg', body: 'once' });
		const a = applyChat(s, c, ctxAs());
		const b = applyChat(a.state, c, ctxAs());
		expect(b.state).toBe(a.state);
		expect(b.accepted).toEqual(a.accepted);
		expect(a.state.messages.filter((m) => m.body === 'once')).toHaveLength(1);
	});
	it('a refused call changes nothing', () => {
		const s = fresh();
		expect(() => applyChat(s, call({ method: 'chat.answer', ask_id: 'nope', text: 'x' }), ctxAs())).toThrow();
		expect(s.messages.length).toBe(fresh().messages.length);
	});
	it('CHAT-4 · a reaction is an acknowledgement and posts no message', () => {
		const s = fresh();
		const before = s.messages.length;
		const ann = s.messages.find((m) => m.kind === 'announcement')!;
		const r = run(s, { method: 'chat.react', message_id: ann.id, reaction: 'looking' });
		expect(r.state.messages).toHaveLength(before);
		expect(byId(r.state, ann.id).reactions?.looking).toEqual(['you']);
		expect(byId(run(r.state, { method: 'chat.react', message_id: ann.id, reaction: 'looking' }).state, ann.id).reactions?.looking).toEqual([]);
	});
	it('only a current member uses chat', () => {
		expect(refusal(fresh(), { method: 'msg.send', conversation_id: 'c-general', kind: 'msg', body: 'x' }, ctxAs('ghost', 'agent'))?.code).toBe('not_authorized');
	});
});
