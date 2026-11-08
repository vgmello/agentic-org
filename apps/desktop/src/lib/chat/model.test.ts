import { describe, expect, it } from 'vitest';
import { run, byId, channelNamed, conv, ctxAs, fresh, kaiAsk, miraApproval, minutesAfter, NOW, org } from './testkit';
import { tick } from './engine';
import {
	activityRows,
	answerWords,
	applyMention,
	appOnly,
	chatHref,
	collapse,
	composerState,
	conversationView,
	deadlineWords,
	foldedWords,
	locate,
	mentionAt,
	mentionOptions,
	mentionsOf,
	needsYou,
	parseChatLocation,
	parseSearch,
	parseSlash,
	remindDelay,
	searchMessages,
	sidebar,
	slashSuggestions,
	threadsOf,
	titleOf,
	turnInto,
	typingWords,
	unreadIn,
	wakesFor,
	waitingFor
} from './model';

const ctx = ctxAs();

describe('CHAT-1 · the sidebar', () => {
	const bar = sidebar(fresh(), ctx);
	it('CHAT-1 · Needs you holds the asks and hand-offs for the owner, newest first', () => {
		expect(bar.needsYou.map((m) => m.kind)).toEqual(['request', 'request']);
		expect(bar.needsYou.map((m) => m.from)).toEqual(['mira', 'kai']);
		expect(bar.needsYou.every((m) => m.addressed_to === 'you')).toBe(true);
	});
	it('CHAT-1 · Mentions holds the messages that mention the owner', () => {
		expect(bar.mentions.map((m) => m.from)).toEqual(['juno']);
	});
	it('CHAT-1 · Threads holds the threads the owner is in, with the newest activity first', () => {
		const s = fresh();
		// The owner started none and replied in none: only a thread that mentions them counts.
		expect(threadsOf(s, ctx)).toEqual([]);
		const r = run(s, { method: 'msg.send', conversation_id: 'c-release-v1', thread_root_id: s.messages.find((m) => m.body.startsWith('@sam your part'))!.id, kind: 'msg', body: 'Looks good.' });
		const threads = threadsOf(r.state, ctx);
		expect(threads).toHaveLength(1);
		expect(threads[0]).toMatchObject({ replies: 4, new_replies: 3 });
		expect(threadsOf(run(r.state, { method: 'chat.mark_read', conversation_id: 'c-release-v1' }).state, ctx)[0].new_replies).toBe(0);
	});
	it('CHAT-1 · channels are A–Z and DMs show the status of the other member', () => {
		expect(bar.channels.map((c) => c.title)).toEqual(['#atlas', '#beacon', '#design', '#engineering', '#general', '#onboarding-kit', '#people', '#platform', '#release-v1', '#scratchpad', '#web']);
		const status = Object.fromEntries(bar.dms.filter((d) => d.other).map((d) => [d.other!.name, d.presence]));
		expect(status).toMatchObject({ 'Sam Okafor': 'working', 'Inês Duarte': 'asleep', 'Pax Moreau': 'paused', 'Avery Stone': 'online' });
		expect(bar.dms.find((d) => d.conversation.kind === 'group_dm')?.title).toBe('Sam Okafor, Noor Haddad');
	});
	it('CHAT-1 · unread counts leave out your own messages, and a mark-as-read clears them', () => {
		let s = fresh();
		const before = unreadIn(s, ctx, conv(s, 'c-general'));
		expect(before.unread).toBeGreaterThan(0);
		expect(before.mentions).toBe(1);
		s = run(s, { method: 'chat.mark_read', conversation_id: 'c-general' }).state;
		expect(unreadIn(s, ctx, conv(s, 'c-general'))).toEqual({ unread: 0, mentions: 0 });
		expect(unreadIn(s, ctx, conv(s, 'c-dm-sam'))).toEqual({ unread: 0, mentions: 0 });
	});
	it('CHAT-1 · titles: a channel, the other member of a DM, the members of a group', () => {
		const s = fresh();
		expect(titleOf(conv(s, 'c-general'), ctx, 'you')).toBe('#general');
		expect(titleOf(conv(s, 'c-dm-ines'), ctx, 'you')).toBe('Inês Duarte');
		expect(titleOf(conv(s, 'c-group'), ctx, 'you')).toBe('Sam Okafor, Noor Haddad');
	});
});

describe('CHAT-3 · DMs show each member\'s status', () => {
	it('CHAT-3 · an unplaced presence reads Offline for an agent and Online for a person', () => {
		const s = fresh();
		const row = (id: string) => sidebar({ ...s, presence: {} }, ctx).dms.find((d) => d.conversation.id === id)!;
		expect(row('c-dm-sam').presence).toBe('offline');
		expect(row('c-dm-avery').presence).toBe('online');
	});
});

describe('CHAT-5 · answering clears Needs you', () => {
	it('CHAT-5 · the card says who answered and the ask leaves Needs you', () => {
		const s = fresh();
		const r = run(s, { method: 'chat.answer', ask_id: kaiAsk(s).id, option_id: 'no' }).state;
		expect(needsYou(r, ctx).map((m) => m.id)).toEqual(needsYou(s, ctx).filter((m) => m.id !== kaiAsk(s).id).map((m) => m.id));
		expect(answerWords(byId(r, kaiAsk(s).id), ctx)).toBe("Answered by Alex Kim: No, allow (Eden's AREA-7)");
		expect(answerWords(kaiAsk(s), ctx)).toBeUndefined();
	});
	it('CHAT-5 · a hand-off for the owner is in Needs you until accepted', () => {
		let s = fresh();
		const h = run(s, { method: 'msg.send', conversation_id: 'c-dm-sam', kind: 'handoff', body: 'Yours.', addressed_to: 'you' }, ctxAs('sam', 'agent'));
		s = h.state;
		expect(needsYou(s, ctx).some((m) => m.id === h.accepted.id)).toBe(true);
		s = run(s, { method: 'chat.accept_handoff', message_id: h.accepted.id! }).state;
		expect(needsYou(s, ctx).some((m) => m.id === h.accepted.id)).toBe(false);
	});
	it('CHAT-5 · the card says how long it has waited', () => {
		expect(waitingFor(kaiAsk(fresh()), NOW)).toBe('Waiting 12 min');
	});
});

describe('CHAT-6 · an ask with a default shows its deadline', () => {
	const clock = (iso: string) => iso.slice(11, 16);
	it('CHAT-6 · before the deadline the card shows when the default applies', () => {
		const w = deadlineWords(kaiAsk(fresh()), NOW, clock)!;
		expect(w.applied).toBe(false);
		expect(w.text).toBe("If no answer by 14:00, the default applies: No, allow (Eden's AREA-7). (2 h left)");
	});
	it('CHAT-6 · when the default is applied the card says so', () => {
		const later = tick(fresh(), minutesAfter(NOW, 130));
		const w = deadlineWords(byId(later, kaiAsk(fresh()).id), minutesAfter(NOW, 130), clock)!;
		expect(w).toEqual({ applied: true, text: "No answer by 14:00: the default was applied (No, allow (Eden's AREA-7))." });
		expect(answerWords(byId(later, kaiAsk(fresh()).id), ctx)).toContain('Answered by the default');
		expect(needsYou(later, ctx).some((m) => m.id === kaiAsk(fresh()).id)).toBe(false);
	});
	it('CHAT-6 · an ask without a default, or one that was answered, shows no deadline', () => {
		expect(deadlineWords(miraApproval(fresh()), NOW, clock)).toBeUndefined();
		const answered = run(fresh(), { method: 'chat.answer', ask_id: kaiAsk(fresh()).id, option_id: 'yes' }).state;
		expect(deadlineWords(byId(answered, kaiAsk(fresh()).id), NOW, clock)).toBeUndefined();
	});
});

describe('CHAT-7 · the composer says when a message wakes an agent', () => {
	it('CHAT-7 · an asleep agent: "Inês Duarte is asleep. Your message will wake it."', () => {
		const s = fresh();
		const c = composerState(s, ctx, conv(s, 'c-dm-ines'), true);
		expect(c).toEqual({ canWrite: true, note: 'Inês Duarte is asleep. Your message will wake it.', wakes: ['ines'] });
		expect(composerState(s, ctx, conv(s, 'c-dm-sam'), true)).toEqual({ canWrite: true, wakes: [] });
	});
	it('CHAT-7 · a paused agent says it will not read the message', () => {
		const s = fresh();
		expect(composerState(s, ctx, conv(s, 'c-dm-pax'), true).note).toBe('Pax Moreau is paused and will not read this until resumed.');
	});
	it('CHAT-7 · a mention of an asleep agent in a channel says who it would wake', () => {
		const s = fresh();
		expect(wakesFor(s, ctx, conv(s, 'c-general'), 'hey @ines and @sam')).toEqual(['ines']);
		expect(wakesFor(s, ctx, conv(s, 'c-general'), 'hello')).toEqual([]);
	});
	it('CHAT-7 · a typing indicator shows while the agent works', () => {
		const r = run(fresh(), { method: 'msg.send', conversation_id: 'c-dm-ines', kind: 'msg', body: 'hello' }).state;
		expect(typingWords(r.typing, ctx, 'c-dm-ines', NOW)).toBe('Inês Duarte is typing…');
		expect(typingWords(r.typing, ctx, 'c-dm-sam', NOW)).toBeUndefined();
		expect(typingWords(r.typing, ctx, 'c-dm-ines', minutesAfter(NOW, 1))).toBeUndefined();
		const two = [...r.typing, { conversation_id: 'c-dm-ines', member_id: 'sam', until: minutesAfter(NOW, 1) }];
		expect(typingWords(two, ctx, 'c-dm-ines', NOW)).toBe('Inês Duarte and Sam Okafor are typing…');
	});
	it('states: the office is not running, an archived channel, an offboarded recipient', () => {
		const s = fresh();
		expect(composerState(s, ctx, conv(s, 'c-general'), false)).toMatchObject({ canWrite: false, note: "The office isn't running. Restart it from Offices." });
		expect(composerState({ ...s, conversations: s.conversations.map((c) => (c.id === 'c-release-v1' ? { ...c, archived: true } : c)) }, ctx, { ...conv(s, 'c-release-v1'), archived: true }, true).note).toBe('#release-v1 is archived.');
		const gone = { ...ctx, members: ctx.members.map((m) => (m.id === 'sam' ? { ...m, former: true } : m)) };
		expect(composerState(s, gone, conv(s, 'c-dm-sam'), true)).toMatchObject({ canWrite: false, note: 'Sam Okafor was offboarded.' });
	});
});

describe('CHAT-9 · slash commands and Turn into…', () => {
	it('CHAT-9 · /bug /request /task /status /wake /remind parse', () => {
		expect(parseSlash('/bug the list is empty')).toMatchObject({ ok: true, name: 'bug', rest: 'the list is empty' });
		expect(parseSlash('/request dark mode')).toMatchObject({ ok: true, name: 'request', rest: 'dark mode' });
		expect(parseSlash('/task @sam write the docs')).toMatchObject({ ok: true, name: 'task', handle: 'sam', rest: 'write the docs' });
		expect(parseSlash('/status atlas')).toMatchObject({ ok: true, name: 'status', rest: 'atlas' });
		expect(parseSlash('/status')).toMatchObject({ ok: true, name: 'status', rest: '' });
		expect(parseSlash('/wake @ines')).toMatchObject({ ok: true, name: 'wake', handle: 'ines' });
		expect(parseSlash('/remind in 2h check the build')).toMatchObject({ ok: true, name: 'remind', rest: 'in 2h check the build' });
	});
	it('CHAT-9 · a command that is missing something says what', () => {
		expect(parseSlash('/task')).toMatchObject({ ok: false, message: 'Say which agent: /task @agent [title]' });
		expect(parseSlash('/task @sam')).toMatchObject({ ok: false, message: 'Give the task a title: /task @agent [title]' });
		expect(parseSlash('/wake')).toMatchObject({ ok: false });
		expect(parseSlash('/bgu the list')).toMatchObject({ ok: false, message: 'There is no /bgu command. Did you mean /bug?' });
		expect(parseSlash('not a command')).toBeUndefined();
		// Ordinary text that starts with a slash is a message, not a mistake.
		expect(parseSlash('/tmp is full again')).toBeUndefined();
		expect(parseSlash('/etc/hosts has the wrong entry')).toBeUndefined();
		expect(parseSlash('/nope')).toBeUndefined();
		expect(parseSlash('http://example.com')).toBeUndefined();
	});
	it('CHAT-9 · commands complete while typing, and a reminder reads its delay', () => {
		expect(slashSuggestions('/').map((c) => c.name)).toEqual(['bug', 'request', 'task', 'status', 'wake', 'remind']);
		expect(slashSuggestions('/re').map((c) => c.name)).toEqual(['request', 'remind']);
		expect(slashSuggestions('/bug x')).toEqual([]);
		expect(remindDelay('in 30m look at this')).toEqual({ ms: 1_800_000, what: 'look at this' });
		expect(remindDelay('in 2 h')).toEqual({ ms: 7_200_000, what: '' });
		expect(remindDelay('tomorrow')).toBeUndefined();
	});
	it('CHAT-9 · Turn into… prefills the form from the message', () => {
		const m = byId(fresh(), kaiAsk(fresh()).id);
		const bug = turnInto({ ...m, body: '@sam the export button does nothing on **Safari**. Any idea?' }, 'bug');
		expect(bug.title).toBe('the export button does nothing on Safari.');
		expect(bug.body).toContain('Safari');
		expect(turnInto({ ...m, body: 'x'.repeat(200) }, 'task').title).toHaveLength(80);
	});
});

describe('CHAT-10 · the announcement summary', () => {
	it('CHAT-10 · the thread under the last announcement says how many more updates came', () => {
		const s = fresh();
		const root = s.messages.find((m) => m.kind === 'announcement' && m.body.startsWith('Ready for review'))!;
		expect(foldedWords(s, ctx, root.id)).toBe('Sam Okafor posted 3 more updates');
		expect(foldedWords(s, ctx, kaiAsk(s).id)).toBeUndefined();
	});
});

describe('CHAT-11 · mute and digest', () => {
	const muted = (mode: 'muted' | 'digest') => run(fresh(), { method: 'chat.mute', conversation_id: 'c-general', member_id: 'sam', mode }).state;
	it('CHAT-11 · a muted agent\'s messages leave the conversation and the unread count', () => {
		const before = fresh();
		const s = muted('muted');
		const view = conversationView(s, ctx, 'c-general');
		expect(view.shown.some((m) => m.from === 'sam')).toBe(false);
		expect(view.muted.every((m) => m.from === 'sam')).toBe(true);
		expect(view.muted.length).toBeGreaterThan(0);
		expect(unreadIn(s, ctx, conv(s, 'c-general')).unread).toBeLessThan(unreadIn(before, ctx, conv(before, 'c-general')).unread);
	});
	it('CHAT-11 · a digest agent\'s messages collapse into one digest row', () => {
		const view = conversationView(muted('digest'), ctx, 'c-general');
		expect(view.shown.some((m) => m.from === 'sam')).toBe(false);
		expect(view.digests).toHaveLength(1);
		expect(view.digests[0].member_id).toBe('sam');
	});
	it('CHAT-11 · muted messages still appear in Activity', () => {
		const s = muted('muted');
		const rows = activityRows(s, ctx);
		expect(rows).toHaveLength(s.messages.length);
		expect(rows.filter((r) => r.message.from === 'sam' && r.message.conversation_id === 'c-general').every((r) => r.muted)).toBe(true);
	});
	it('CHAT-11 · an ask that is waiting for the owner is never muted away', () => {
		let s = fresh();
		s = run(s, { method: 'chat.mute', conversation_id: channelNamed(s, 'atlas').id, member_id: 'kai', mode: 'muted' }).state;
		expect(conversationView(s, ctx, channelNamed(s, 'atlas').id).shown.some((m) => m.id === kaiAsk(s).id)).toBe(true);
		expect(needsYou(s, ctx).some((m) => m.id === kaiAsk(s).id)).toBe(true);
	});
	it('CHAT-11 · muting is per agent per conversation', () => {
		const view = conversationView(muted('muted'), ctx, 'c-release-v1');
		expect(view.muted).toEqual([]);
		expect(view.shown.some((m) => m.from === 'sam')).toBe(true);
	});
});

describe('CHAT-12 · reading as the owner', () => {
	it('CHAT-12 · a conversation the owner is not in is listed apart and marked', () => {
		const bar = sidebar(fresh(), ctx);
		expect(bar.asOwner.map((r) => r.title).sort()).toEqual(['#design-crit', 'Sam Okafor, Noor Haddad'].sort());
		expect(bar.asOwner.every((r) => r.readingAsOwner)).toBe(true);
		expect(bar.channels.concat(bar.dms).every((r) => !r.readingAsOwner)).toBe(true);
	});
});

describe('CHAT-13 · search', () => {
	const s = fresh();
	const find = (q: string) => searchMessages(s, ctx, parseSearch(q));
	it('CHAT-13 · filters parse like Slack', () => {
		expect(parseSearch('from:@ada in:#agentic-org has:ask budget plan')).toEqual({ text: 'budget plan', from: 'ada', in: 'agentic-org', has: 'ask' });
		expect(parseSearch('kind:ask before:2026-11-08 after:2026-11-01 member:@sam')).toMatchObject({ kind: 'request', before: '2026-11-08', after: '2026-11-01', member: 'sam' });
		expect(parseSearch('plain words from:')).toEqual({ text: 'plain words from:' });
	});
	it('CHAT-13 · by text', () => {
		expect(find('release notes').map((m) => m.from)).toEqual(['mira']);
		expect(find('CREATE-ORG').length).toBeGreaterThan(2);
		const hits = find('create-org');
		expect(hits.map((m) => m.created_at)).toEqual(hits.map((m) => m.created_at).sort().reverse());
		expect(find('Inês')).toEqual([]);
		expect(find('ines duarte')).toEqual([]);
	});
	it('CHAT-13 · by member and by sender', () => {
		expect(find('from:@kai').every((m) => m.from === 'kai')).toBe(true);
		expect(find('from:@kai').length).toBeGreaterThan(1);
		expect(find('member:@sam').some((m) => m.addressed_to === 'sam')).toBe(true);
		expect(find('from:@nobody')).toEqual([]);
	});
	it('CHAT-13 · by channel', () => {
		expect(find('in:#atlas').every((m) => m.conversation_id === channelNamed(s, 'atlas').id)).toBe(true);
		expect(find('in:#atlas')).toHaveLength(2);
		expect(find('in:#release-v1 from:@kai')).toHaveLength(2);
	});
	it('CHAT-13 · by kind', () => {
		expect(find('has:ask').every((m) => m.kind === 'request')).toBe(true);
		expect(find('kind:announcement').every((m) => m.kind === 'announcement')).toBe(true);
		expect(find('has:ask in:#atlas')).toHaveLength(2);
		expect(find('kind:handoff')).toHaveLength(2);
		expect(find('has:thread').length).toBe(2);
	});
	it('CHAT-13 · by date', () => {
		expect(find('after:2026-11-08T11:30:00Z').every((m) => m.created_at >= '2026-11-08T11:30:00.000Z')).toBe(true);
		expect(find('before:2026-11-08T11:30:00Z').every((m) => m.created_at < '2026-11-08T11:30:00.000Z')).toBe(true);
		expect(find('before:2026-11-08T11:30:00Z').length + find('after:2026-11-08T11:30:00Z').length).toBe(s.messages.length);
	});
	it('CHAT-13 · search finds only what the owner may read', () => {
		expect(searchMessages(s, ctx, parseSearch('settings page'))).toHaveLength(1);
		expect(searchMessages(s, ctxAs('kai', 'agent'), parseSearch('settings page'))).toEqual([]);
	});
});

describe('CHAT-14 · composing and long messages', () => {
	it('CHAT-14 · @ completes by display name to the member\'s handle', () => {
		const s = fresh();
		const c = conv(s, 'c-general');
		expect(mentionAt('hi @ha', 6)).toEqual({ start: 3, query: 'ha' });
		expect(mentionAt('hi @', 4)).toEqual({ start: 3, query: '' });
		expect(mentionAt('mail a@b', 8)).toBeUndefined();
		expect(mentionOptions(ctx, c, 'weiss').map((o) => o.handle)).toEqual(['hana-weiss']);
		expect(mentionOptions(ctx, c, 'Ok').map((o) => o.handle)).toEqual(['sam']);
		expect(mentionOptions(ctx, c, 'ine').map((o) => o.handle)).toEqual(['ines']);
		expect(mentionOptions(ctx, c, 'ine')[0].name).toBe('Inês Duarte');
		expect(mentionOptions(ctx, c, 'he')[0]).toMatchObject({ handle: 'here', kind: 'here' });
		expect(mentionOptions(ctx, conv(s, 'c-dm-sam'), 'he').some((o) => o.handle === 'here')).toBe(false);
		const at = mentionAt('hi @sa', 6)!;
		expect(applyMention('hi @sa', at, 6, 'sam')).toEqual({ text: 'hi @sam ', caret: 8 });
	});
	it('CHAT-14 · @ completes only to the members of a channel', () => {
		const s = fresh();
		expect(mentionOptions(ctx, conv(s, 'c-release-v1'), '').map((o) => o.handle).sort()).toEqual(['avery-stone', 'here', 'kai', 'sam']);
	});
	it('CHAT-14 · more than 15 lines collapses behind Show more', () => {
		const long = Array.from({ length: 20 }, (_, i) => `l${i}`).join('\n');
		expect(collapse(long)).toEqual({ shown: long.split('\n').slice(0, 15).join('\n'), hidden: 5 });
		expect(collapse('short\ntext')).toEqual({ shown: 'short\ntext', hidden: 0 });
		expect(collapse(Array.from({ length: 15 }, () => 'x').join('\n')).hidden).toBe(0);
		const sample = fresh().messages.find((m) => m.from === 'avery' && m.body.startsWith('Plan for'))!;
		expect(collapse(sample.body).hidden).toBe(6);
	});
});

describe('CHAT-15 · Activity message rows open the message in Chat', () => {
	it('CHAT-15 · a row links to its conversation and message, and the link finds them', () => {
		const s = fresh();
		const row = activityRows(s, ctx).find((r) => r.message.id === kaiAsk(s).id)!;
		expect(row.href).toBe(chatHref(kaiAsk(s)));
		expect(row.href).toMatch(/^\/chat\?c=[^&]+&m=[^&]+$/);
		const loc = parseChatLocation(row.href.split('?')[1]);
		expect(locate(s, loc)).toMatchObject({ conversation: { id: channelNamed(s, 'atlas').id }, message: { id: kaiAsk(s).id } });
	});
	it('CHAT-15 · a thread reply opens its thread; a missing message falls back to the conversation', () => {
		const s = fresh();
		const reply = s.messages.find((m) => m.thread_root_id)!;
		expect(locate(s, { message: reply.id }).thread?.id).toBe(reply.thread_root_id);
		expect(locate(s, { conversation: 'c-general', message: 'gone' }).conversation?.id).toBe('c-general');
		expect(locate(s, { conversation: 'nope' }).conversation).toBeUndefined();
		expect(parseChatLocation('')).toEqual({});
	});
	it('CHAT-15 · ids with odd characters are encoded in the link', () => {
		expect(chatHref({ id: 'a&b', conversation_id: 'c=d' })).toBe('/chat?c=c%3Dd&m=a%26b');
	});
});

describe('CHAT-17 · asks to the owner and approvals are app-only', () => {
	it('CHAT-17 · the card shows read-only, with Open in Agentic Org, on a bridge', () => {
		const s = fresh();
		expect(appOnly(kaiAsk(s), ctx)).toBe(true);
		expect(appOnly(miraApproval(s), ctx)).toBe(true);
		const handoff = s.messages.find((m) => m.kind === 'handoff')!;
		expect(appOnly({ ...handoff, addressed_to: 'you' }, ctx)).toBe(true);
		expect(appOnly(handoff, ctx)).toBe(false);
		const toAvery = { ...kaiAsk(s), addressed_to: 'avery' };
		expect(appOnly(toAvery, ctx)).toBe(false);
		expect(appOnly({ ...toAvery, approval: true }, ctx)).toBe(true);
	});
	it('CHAT-17 · a bridged message in the sample is the bridged person\'s, never the owner\'s', () => {
		const m = fresh().messages.find((x) => x.bridge)!;
		expect(m).toMatchObject({ from: 'leo', bridge: 'slack' });
		expect(m.from).not.toBe('you');
	});
});

describe('mentions of the owner', () => {
	it('@here in a channel the owner is in is a mention; a message from the owner is not', () => {
		const sam = ctxAs('sam', 'agent');
		const post = (s: ReturnType<typeof fresh>, body: string, c = sam) => run(s, { method: 'msg.send', conversation_id: 'c-release-v1', kind: c.actor === 'you' ? 'msg' : 'announcement', reason: 'report', body }, c).state;
		let s = post(fresh(), 'standup');
		expect(mentionsOf(s, ctx)).toHaveLength(1);
		s = post(s, '@here release at 5', ctxAs('kai', 'agent'));
		expect(mentionsOf(s, ctx)).toHaveLength(2);
		s = post(s, '@owner me', ctx);
		expect(mentionsOf(s, ctx)).toHaveLength(2);
	});
	it('the sample org has the owner under the handle owner', () => {
		expect(org.members.find((m) => m.owner)?.id).toBe('you');
		expect(ctx.members.find((m) => m.owner)?.handle).toBe('owner');
	});
});
