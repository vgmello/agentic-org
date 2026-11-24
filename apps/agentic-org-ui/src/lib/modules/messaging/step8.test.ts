import { describe, expect, it } from 'vitest';
import type { OrgApi } from '$lib/modules/organization/api';
import { sampleOrg } from '$lib/modules/organization/sample';
import { createMockChat } from '../../../../e2e/fakes/modules/chat';
import { SAMPLE_DIRECTORY, sampleChatFull } from '../../../../e2e/fakes/modules/chat-sample';
import { OFFICE, SYSTEM_NO_REPLY, tick } from './engine';
import {
	browseChannels,
	cannotRunFor,
	cannotRunWords,
	chatContext,
	deadlineWords,
	handoffOutcome,
	nameOf,
	outcomeWords,
	pinsIn,
	requiredWords
} from './model';
import { byId, ctxAs, fresh, minutesAfter, NOW, org, refusal, run } from './testkit';
import type { ChatCall } from './types';

const orgOf = (): Pick<OrgApi, 'view'> => ({
	view: async () => ({ snapshot: sampleOrg('office-1'), sample: true })
});

describe('Step 8 chat domain rules', () => {
	it('withdraw closes an ask only for its asker and records optional reason', () => {
		const base = fresh();
		const ask = run(
			base,
			{
				method: 'msg.send',
				conversation_id: 'c-general',
				kind: 'request',
				body: 'Should we cut a patch?',
				addressed_to: 'sam',
				options: [{ id: 'y', label: 'Yes' }]
			},
			ctxAs('you')
		);
		expect(
			refusal(
				ask.state,
				{ method: 'chat.withdraw', ask_id: ask.accepted.id! },
				ctxAs('sam', 'agent')
			)?.code
		).toBe('not_authorized');

		const withdrawn = run(
			ask.state,
			{ method: 'chat.withdraw', ask_id: ask.accepted.id!, reason: 'No longer needed' },
			ctxAs('you')
		);
		const msg = byId(withdrawn.state, ask.accepted.id!);
		expect(msg.withdrawn?.reason).toBe('No longer needed');
		expect(outcomeWords(msg, ctxAs('you'), () => '12:00')).toBe(
			'Withdrawn by you · 12:00: No longer needed'
		);
		expect(
			refusal(
				withdrawn.state,
				{ method: 'chat.answer', ask_id: ask.accepted.id!, option_id: 'y' },
				ctxAs('sam', 'agent')
			)?.code
		).toBe('already_exists');
	});

	it('hand_back requires a reason, publishes a reply, and prevents later acceptance', () => {
		const base = fresh();
		const handoff = run(
			base,
			{
				method: 'msg.send',
				conversation_id: 'c-dm-sam',
				kind: 'handoff',
				body: 'Take the migration review',
				addressed_to: 'you',
				linked_ref: 'T-42'
			},
			ctxAs('sam', 'agent')
		);
		expect(
			refusal(handoff.state, {
				method: 'chat.hand_back',
				message_id: handoff.accepted.id!,
				reason: '   '
			})?.code
		).toBe('invalid_request');

		const returned = run(handoff.state, {
			method: 'chat.hand_back',
			message_id: handoff.accepted.id!,
			reason: 'Missing schema notes'
		});
		const msg = byId(returned.state, handoff.accepted.id!);
		expect(handoffOutcome(msg, ctxAs('you'), () => '12:05')).toBe(
			'Handed back by you · 12:05: Missing schema notes'
		);
		expect(returned.state.messages.at(-1)?.body).toBe('Handed back: Missing schema notes');
		expect(
			refusal(returned.state, { method: 'chat.accept_handoff', message_id: handoff.accepted.id! })
				?.code
		).toBe('already_exists');
	});

	it('multi-question asks validate answers and required asks never apply defaults', () => {
		const base = fresh();
		expect(
			refusal(
				base,
				{
					method: 'msg.send',
					conversation_id: 'c-general',
					kind: 'request',
					body: 'Required ask',
					addressed_to: 'you',
					required: true,
					default_option: 'a',
					default_at: minutesAfter(NOW, 10),
					options: [{ id: 'a', label: 'A' }]
				},
				ctxAs('sam', 'agent')
			)?.code
		).toBe('invalid_request');

		const reqAsk = run(
			base,
			{
				method: 'msg.send',
				conversation_id: 'c-general',
				kind: 'request',
				body: 'Confirm rollout?',
				addressed_to: 'you',
				required: true,
				wait_until: minutesAfter(NOW, 5),
				questions: [
					{
						id: 'env',
						question_type: 'choice',
						prompt: 'Environments',
						allow_multiple: true,
						options: [
							{ id: 'staging', label: 'Staging' },
							{ id: 'prod', label: 'Production' }
						]
					},
					{ id: 'notes', question_type: 'free_text', prompt: 'Notes', optional: true }
				]
			},
			ctxAs('sam', 'agent')
		);
		const askMsg = byId(reqAsk.state, reqAsk.accepted.id!);
		expect(requiredWords(askMsg, ctxAs('you'), NOW)).toContain(
			"can't continue without this answer"
		);
		expect(requiredWords(askMsg, ctxAs('you'), minutesAfter(NOW, 10))).toContain(
			'is stopped, waiting for this answer'
		);
		const ticked = tick(reqAsk.state, minutesAfter(NOW, 120));
		expect(byId(ticked, reqAsk.accepted.id!).answer).toBeUndefined();

		const answered = run(reqAsk.state, {
			method: 'chat.answer',
			ask_id: reqAsk.accepted.id!,
			answers: [{ question_id: 'env', option_ids: ['staging', 'prod'] }]
		});
		expect(
			outcomeWords(byId(answered.state, reqAsk.accepted.id!), ctxAs('you'), () => '12:10')
		).toBe('Answered by you · 12:10: 1 of 2 answered');
	});

	it('browseChannels hides restricted channels and supports join and invitation requests', () => {
		const ctx = chatContext(org, { now: NOW, directory: SAMPLE_DIRECTORY });
		const state = sampleChatFull(ctx);
		const rows = browseChannels(state, ctx, org.projects ?? [], org.teams);
		expect(rows.some((r) => r.conversation.id === 'c-security-review')).toBe(false);
		expect(rows.find((r) => r.conversation.id === 'c-design-crit')?.action).toBe('join');
		expect(rows.find((r) => r.conversation.id === 'c-incident-drills')?.action).toBe('request');

		const joined = run(state, { method: 'channel.join', id: 'c-design-crit' }, ctx).state;
		expect(joined.conversations.find((c) => c.id === 'c-design-crit')?.members).toContain('you');

		const requested = run(
			joined,
			{ method: 'channel.request_invitation', id: 'c-incident-drills' },
			ctx
		).state;
		expect(
			browseChannels(requested, ctx).find((r) => r.conversation.id === 'c-incident-drills')?.action
		).toBe('requested');

		expect(refusal(state, { method: 'channel.join', id: 'c-security-review' }, ctx)?.code).toBe(
			'not_found'
		);
	});

	it('system notices display as System, refuse replies, and pinsIn orders newest first', () => {
		const ctx = chatContext(org, { now: NOW, directory: SAMPLE_DIRECTORY });
		const state = sampleChatFull(ctx);
		expect(nameOf(ctx, OFFICE)).toBe('System');
		const notice = state.messages.find((m) => m.from === OFFICE)!;
		expect(
			refusal(
				state,
				{
					method: 'msg.send',
					conversation_id: notice.conversation_id,
					thread_root_id: notice.id,
					kind: 'msg',
					body: 'reply'
				},
				ctx
			)?.message
		).toBe(SYSTEM_NO_REPLY);

		const pins = pinsIn(state, 'c-general');
		expect(pins.length).toBeGreaterThan(0);
		for (let i = 1; i < pins.length; i++) {
			expect(Date.parse(pins[i - 1].created_at)).toBeGreaterThanOrEqual(
				Date.parse(pins[i].created_at)
			);
		}
	});

	it('cannotRunFor detects HR lead mentions and produces hire guidance copy', () => {
		const ctx = chatContext(org, { now: NOW, directory: SAMPLE_DIRECTORY });
		const state = sampleChatFull(ctx);
		const general = state.conversations.find((c) => c.id === 'c-general')!;
		expect(cannotRunFor(ctx, general, 'Hey @hr can we hire?', ['hr'])).toEqual(['hr']);
		expect(cannotRunWords(ctx, 'hr')).toContain('use Hire and fill in the details yourself');
	});

	it('mock chat scenarios support status reconciliation, lost delivery, nocopy and hr-down', async () => {
		const lostApi = createMockChat(orgOf(), {
			now: () => NOW,
			scenario: () => 'lost',
			schedule: () => undefined
		});
		const call: ChatCall = {
			method: 'msg.send',
			change_id: 'lost-1',
			conversation_id: 'c-general',
			kind: 'msg',
			body: 'Did it land?'
		};
		await expect(lostApi.command('o', call)).rejects.toMatchObject({ code: 'unavailable' });
		await expect(lostApi.status('o', 'lost-1')).resolves.toMatchObject({ state: 'applied' });
		await expect(lostApi.status('o', 'missing-1')).resolves.toEqual({ state: 'not_found' });

		const noCopyApi = createMockChat(orgOf(), { now: () => NOW, scenario: () => 'nocopy' });
		await expect(noCopyApi.snapshot('o')).rejects.toMatchObject({ code: 'unavailable' });

		const hrDownApi = createMockChat(orgOf(), { now: () => NOW, scenario: () => 'hr-down' });
		const snap = await hrDownApi.snapshot('o');
		expect(snap.cannot_run).toEqual(['hr']);
	});

	it('single yes_no ask populates options and default_option, shows deadlineWords, and accepts option_id answers', () => {
		const ctx = chatContext(org, { now: NOW, actor: 'mira' });
		let state = sampleChatFull(chatContext(org, { now: NOW, directory: SAMPLE_DIRECTORY }));
		const created = run(
			state,
			{
				method: 'msg.send',
				conversation_id: 'c-general',
				kind: 'request',
				addressed_to: 'you',
				body: 'Ship the hotfix tonight?',
				questions: [
					{ id: 'q1', prompt: 'Ship tonight?', question_type: 'yes_no', default_option: 'no' }
				],
				default_at: '2026-11-18T18:00:00Z'
			},
			ctx
		);
		state = created.state;
		const ask = state.messages.find((m) => m.id === created.accepted.id)!;
		expect(ask.options?.map((o) => o.id)).toEqual(['yes', 'no']);
		expect(ask.default_option).toBe('no');
		expect(deadlineWords(ask, NOW)?.text).toContain('No');

		const answered = run(
			state,
			{ method: 'chat.answer', ask_id: ask.id, option_id: 'yes' },
			chatContext(org, { now: '2026-11-18T10:05:00Z', actor: 'you' })
		);
		const updated = answered.state.messages.find((m) => m.id === ask.id)!;
		expect(updated.answer?.option_id).toBe('yes');
		expect(outcomeWords(updated, chatContext(org, { now: NOW }))).toContain('Yes');
	});

	it('multi-question ask with question-level defaults shows deadlineWords before and after tick', () => {
		const ctx = chatContext(org, { now: NOW, actor: 'mira' });
		const base = sampleChatFull(chatContext(org, { now: NOW, directory: SAMPLE_DIRECTORY }));
		const created = run(
			base,
			{
				method: 'msg.send',
				conversation_id: 'c-general',
				kind: 'request',
				addressed_to: 'you',
				body: 'Two rollout questions',
				questions: [
					{ id: 'q1', prompt: 'Enable flag?', question_type: 'yes_no', default_option: 'yes' },
					{
						id: 'q2',
						prompt: 'Region?',
						question_type: 'choice',
						options: [
							{ id: 'eu', label: 'EU first' },
							{ id: 'us', label: 'US first' }
						],
						default_option: 'eu'
					}
				],
				default_at: '2026-11-18T18:00:00Z'
			},
			ctx
		);
		const ask = created.state.messages.find((m) => m.id === created.accepted.id)!;
		expect(deadlineWords(ask, NOW)?.text).toContain('Yes, EU first');

		const ticked = tick(created.state, '2026-11-18T18:05:00Z');
		const after = ticked.messages.find((m) => m.id === ask.id)!;
		expect(deadlineWords(after, '2026-11-18T18:05:00Z')?.applied).toBe(true);
		expect(deadlineWords(after, '2026-11-18T18:05:00Z')?.text).toContain('Yes, EU first');
	});
});
