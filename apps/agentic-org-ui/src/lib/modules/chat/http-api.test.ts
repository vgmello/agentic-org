import { describe, expect, it, vi } from 'vitest';
import type { GatewayHttpTransport, GatewayReply } from '$lib/platform/http/gateway-errors';
import { createHttpChatApi } from './http-api';
import { chatApi as messagingChatApi } from '$lib/modules/messaging/api';

const ORG = '0193a5c0-0000-7000-8000-000000000001';
const BASE = `/api/v1/organizations/${ORG}/chat`;

function ok(result: unknown): GatewayReply {
	return { status: 200, body: { ok: true, result } };
}

function makeHttp(
	reads: Record<string, GatewayReply>,
	submits: Record<string, (body: unknown) => GatewayReply>
): GatewayHttpTransport & {
	readCalls: string[];
	submitCalls: { path: string; body: unknown }[];
} {
	const readCalls: string[] = [];
	const submitCalls: { path: string; body: unknown }[] = [];
	return {
		readCalls,
		submitCalls,
		async read(path) {
			readCalls.push(path);
			return (
				reads[path] ?? {
					status: 404,
					body: { ok: false, error: { code: 'not_found', message: 'Not found' } }
				}
			);
		},
		async submit(path, body) {
			submitCalls.push({ path, body });
			const handler = submits[path];
			if (!handler) {
				return {
					status: 404,
					body: { ok: false, error: { code: 'not_found', message: 'Not found' } }
				};
			}
			return handler(body);
		}
	};
}

describe('createHttpChatApi', () => {
	it('maps conversations, channels, all 4 question types, defaults, answers, withdrawals, and reactions', async () => {
		const http = makeHttp(
			{
				[`${BASE}/conversations`]: ok({
					conversations: [
						{
							id: 'c-general',
							kind: 'channel',
							name: 'general',
							members: ['owner-1', 'agent-1'],
							created_at: '2026-11-21T10:00:00Z',
							read_at: '2026-11-21T10:05:00Z'
						}
					]
				}),
				[`${BASE}/channels`]: ok({
					channels: [
						{
							id: 'c-general',
							kind: 'channel',
							name: 'general',
							members: ['owner-1', 'agent-1'],
							created_at: '2026-11-21T10:00:00Z'
						},
						{
							id: 'c-design',
							kind: 'channel',
							name: 'design',
							access: 'open',
							members: ['agent-1'],
							created_at: '2026-11-21T10:01:00Z'
						}
					]
				})
			},
			{
				[`${BASE}/messages/list`]: () =>
					ok({
						conversation_id: 'c-general',
						messages: [
							{
								id: 'm-ask-1',
								conversation_id: 'c-general',
								thread_id: 'm-ask-1',
								sender_kind: 'agent',
								sender_employee_id: 'agent-1',
								sender_display_name: 'Kai Lindqvist',
								kind: 'request',
								priority: 'normal',
								body: 'Confirm release plan',
								created_at: '2026-11-21T10:10:00Z',
								origin_sequence: 12,
								mentions: ['owner-1'],
								reactions: { seen: ['owner-1'] },
								meta: {
									card: {
										addressed_to: 'owner-1',
										required: true,
										blocks: 'TASK-46',
										wait_until: '2026-11-21T11:00:00Z',
										questions: [
											{
												id: 'q-single',
												question_type: 'choice',
												prompt: 'Target environment',
												options: [
													{ id: 'staging', label: 'Staging', description: 'Safe first' },
													{ id: 'prod', label: 'Production' }
												],
												default_value: 'staging'
											},
											{
												id: 'q-multi',
												question_type: 'choice',
												prompt: 'Areas to enable',
												allow_multiple: true,
												options: [
													{ id: 'home', label: 'Home' },
													{ id: 'chat', label: 'Chat' }
												],
												default_option: 'chat'
											},
											{
												id: 'q-yn',
												question_type: 'yes_no',
												prompt: 'Run smoke tests?',
												default_value: 'yes'
											},
											{
												id: 'q-text',
												question_type: 'free_text',
												prompt: 'Notes',
												optional: true,
												default_value: 'Ship when green'
											}
										]
									}
								}
							},
							{
								id: 'm-ask-withdrawn',
								conversation_id: 'c-general',
								thread_id: 'm-ask-withdrawn',
								sender_kind: 'person',
								sender_employee_id: 'owner-1',
								sender_display_name: 'You',
								kind: 'request',
								body: 'Old ask',
								created_at: '2026-11-21T10:12:00Z',
								origin_sequence: 13,
								mentions: [],
								reactions: [],
								meta: { card: { addressed_to: 'agent-1' } },
								withdrawal: {
									by: 'owner-1',
									at: '2026-11-21T10:15:00Z',
									reason: 'Superseded'
								}
							},
							{
								id: 'm-answered',
								conversation_id: 'c-general',
								thread_id: 'm-answered',
								sender_kind: 'agent',
								sender_employee_id: 'agent-1',
								sender_display_name: 'Kai Lindqvist',
								kind: 'request',
								body: 'Publish notes?',
								created_at: '2026-11-21T10:16:00Z',
								origin_sequence: 14,
								mentions: [],
								reactions: [{ reaction: 'looking', employee_id: 'owner-1' }],
								meta: { card: { approval: true, addressed_to: 'owner-1' } },
								answer: {
									by: 'owner-1',
									at: '2026-11-21T10:17:00Z',
									option_id: 'approve',
									answers: [{ question_id: 'q1', option_ids: ['approve'] }]
								}
							}
						]
					})
			}
		);

		const api = createHttpChatApi({ http, viewerId: () => 'owner-1' });
		const snap = await api.snapshot(ORG);

		expect(snap.sample).toBe(false);
		expect(snap.office_running).toBe(true);
		expect(snap.state.seq).toBe(14);
		expect(snap.state.conversations.map((c) => c.id)).toEqual(['c-general', 'c-design']);
		expect(snap.state.read_at['c-general']).toBe('2026-11-21T10:05:00Z');
		expect(snap.directory).toContainEqual(
			expect.objectContaining({ id: 'agent-1', kind: 'agent', name: 'Kai Lindqvist' })
		);

		const ask = snap.state.messages.find((m) => m.id === 'm-ask-1')!;
		expect(ask.required).toBe(true);
		expect(ask.blocks).toBe('TASK-46');
		expect(ask.addressed_to).toBe('owner-1');
		expect(ask.reactions).toEqual({ seen: ['owner-1'] });
		expect(ask.questions).toHaveLength(4);
		expect(ask.questions?.[0]).toMatchObject({
			id: 'q-single',
			question_type: 'choice',
			default_option: 'staging',
			default_value: 'staging'
		});
		expect(ask.questions?.[1]).toMatchObject({
			id: 'q-multi',
			allow_multiple: true,
			default_option: 'chat'
		});
		expect(ask.questions?.[2]).toMatchObject({
			id: 'q-yn',
			question_type: 'yes_no',
			default_option: 'yes'
		});
		expect(ask.questions?.[3]).toMatchObject({
			id: 'q-text',
			question_type: 'free_text',
			optional: true,
			default_value: 'Ship when green'
		});

		const withdrawn = snap.state.messages.find((m) => m.id === 'm-ask-withdrawn')!;
		expect(withdrawn.withdrawn).toEqual({
			by: 'owner-1',
			at: '2026-11-21T10:15:00Z',
			reason: 'Superseded'
		});

		const answered = snap.state.messages.find((m) => m.id === 'm-answered')!;
		expect(answered.approval).toBe(true);
		expect(answered.reactions).toEqual({ looking: ['owner-1'] });
		expect(answered.answer?.answers).toEqual([{ question_id: 'q1', option_ids: ['approve'] }]);
	});

	it('dispatches messages, structured asks, multi-question answers, withdrawals, reactions, read, mute, channels and DMs', async () => {
		const http = makeHttp(
			{
				[`${BASE}/conversations`]: ok({
					conversations: [
						{ id: 'c-general', kind: 'channel', name: 'general', members: ['owner-1'] }
					]
				}),
				[`${BASE}/channels`]: ok({ channels: [] })
			},
			{
				[`${BASE}/messages/list`]: () =>
					ok({
						conversation_id: 'c-general',
						messages: [
							{
								id: 'm-1',
								conversation_id: 'c-general',
								sender_kind: 'agent',
								sender_employee_id: 'a-1',
								sender_display_name: 'Agent',
								kind: 'msg',
								body: 'hi',
								created_at: '2026-11-21T10:00:00Z',
								origin_sequence: 1,
								mentions: [],
								reactions: { seen: ['owner-1'] }
							}
						]
					}),
				[`${BASE}/messages`]: (body) =>
					ok({ id: (body as { id: string }).id, conversation_id: 'c-general' }),
				[`${BASE}/ask`]: (body) =>
					ok({
						status: 'pending',
						ask_id: (body as { id: string }).id,
						conversation_id: 'c-general'
					}),
				[`${BASE}/answer`]: (body) =>
					ok({
						ask_id: (body as { ask_id: string }).ask_id,
						conversation_id: 'c-general',
						answer: { by: 'owner-1', at: '2026-11-21T10:05:00Z' }
					}),
				[`${BASE}/withdraw`]: (body) =>
					ok({ ask_id: (body as { ask_id: string }).ask_id, conversation_id: 'c-general' }),
				[`${BASE}/accept`]: (body) => ok({ id: (body as { id: string }).id }),
				[`${BASE}/react`]: () => ok({ message_id: 'm-1', reaction: 'seen', present: false }),
				[`${BASE}/read`]: () =>
					ok({ conversation_id: 'c-general', read_at: '2026-11-21T10:06:00Z' }),
				[`${BASE}/mute`]: () =>
					ok({ conversation_id: 'c-general', muted_employee_id: 'a-1', mute_mode: 'muted' }),
				[`${BASE}/channels`]: () =>
					ok({ channel: { id: 'c-ops', kind: 'channel', name: 'ops', members: ['owner-1'] } }),
				[`${BASE}/channels/join`]: () => ok({ channel: 'c-ops', joined: true }),
				[`${BASE}/dm`]: () =>
					ok({
						conversation: { id: 'c-dm-1', kind: 'dm', members: ['owner-1', 'a-1'] },
						created: true
					})
			}
		);

		let sseTrigger: (() => void) | undefined;
		const api = createHttpChatApi({
			http,
			viewerId: () => 'owner-1',
			subscribe: (_org, listener) => {
				sseTrigger = listener;
				return () => {
					sseTrigger = undefined;
				};
			}
		});

		const changed = vi.fn();
		const stop = api.subscribe(ORG, changed);
		sseTrigger?.();
		expect(changed).toHaveBeenCalledTimes(1);

		await api.snapshot(ORG);

		const sent = await api.command(ORG, {
			method: 'msg.send',
			change_id: 'chg-msg',
			conversation_id: 'c-general',
			kind: 'msg',
			body: 'Hello team',
			thread_root_id: 'm-1',
			also_in_channel: true
		});
		expect(sent).toEqual({ id: 'chg-msg', conversation_id: 'c-general' });
		expect(await api.status(ORG, 'chg-msg')).toEqual({ state: 'applied', accepted: sent });

		await api.command(ORG, {
			method: 'msg.send',
			change_id: 'chg-ask',
			conversation_id: 'c-general',
			kind: 'request',
			body: 'Release?',
			required: true,
			questions: [
				{ id: 'q1', question_type: 'yes_no', prompt: 'Ship?' },
				{ id: 'q2', question_type: 'free_text', prompt: 'Note', optional: true }
			]
		});
		expect(http.submitCalls.find((c) => c.path === `${BASE}/ask`)?.body).toMatchObject({
			id: 'chg-ask',
			to: 'c-general',
			required: true
		});

		await api.command(ORG, {
			method: 'chat.answer',
			change_id: 'chg-ans',
			ask_id: 'chg-ask',
			answers: [
				{ question_id: 'q1', option_ids: ['yes'] },
				{ question_id: 'q2', text: 'Go ahead' }
			]
		});
		expect(http.submitCalls.find((c) => c.path === `${BASE}/answer`)?.body).toEqual({
			ask_id: 'chg-ask',
			answers: [
				{ question_id: 'q1', option_ids: ['yes'] },
				{ question_id: 'q2', text: 'Go ahead' }
			]
		});

		await api.command(ORG, {
			method: 'chat.withdraw',
			change_id: 'chg-wd',
			ask_id: 'chg-ask',
			reason: 'No longer needed'
		});
		await api.command(ORG, {
			method: 'chat.accept_handoff',
			change_id: 'chg-acc',
			message_id: 'm-1'
		});
		await api.command(ORG, {
			method: 'chat.react',
			change_id: 'chg-rx',
			message_id: 'm-1',
			reaction: 'seen'
		});
		expect(http.submitCalls.find((c) => c.path === `${BASE}/react`)?.body).toEqual({
			message_id: 'm-1',
			reaction: 'seen',
			remove: true
		});
		await api.command(ORG, {
			method: 'chat.mark_read',
			change_id: 'chg-rd',
			conversation_id: 'c-general'
		});
		expect(http.submitCalls.find((c) => c.path === `${BASE}/read`)?.body).toEqual({
			conversation_id: 'c-general',
			last_read_message_id: 'm-1'
		});
		await api.command(ORG, {
			method: 'chat.mute',
			change_id: 'chg-mute',
			conversation_id: 'c-general',
			member_id: 'a-1',
			mode: 'muted'
		});
		expect(
			await api.command(ORG, {
				method: 'channel.create',
				change_id: 'chg-ch',
				name: 'ops',
				members: ['owner-1']
			})
		).toEqual({ id: 'c-ops', conversation_id: 'c-ops' });
		await api.command(ORG, { method: 'channel.join', change_id: 'chg-join', id: 'c-ops' });
		expect(
			await api.command(ORG, { method: 'dm.open', change_id: 'chg-dm', members: ['a-1'] })
		).toEqual({ id: 'c-dm-1', conversation_id: 'c-dm-1' });

		stop();
		expect(sseTrigger).toBeUndefined();
	});

	it('fails closed on gateway refusals, unsupported methods, and signed-out messagingApi', async () => {
		const http = makeHttp(
			{},
			{
				[`${BASE}/answer`]: () => ({
					status: 400,
					body: { ok: false, error: { code: 'invalid_request', message: 'Already answered.' } }
				})
			}
		);
		const api = createHttpChatApi({ http });
		await expect(
			api.command(ORG, { method: 'chat.answer', change_id: 'c1', ask_id: 'a1', text: 'yes' })
		).rejects.toMatchObject({
			source: 'api',
			code: 'invalid_request',
			message: 'Already answered.'
		});
		await expect(
			api.command(ORG, { method: 'chat.pin', change_id: 'c2', message_id: 'm1' })
		).rejects.toMatchObject({ source: 'api', code: 'unavailable' });

		// Signed-out messagingApi also fails closed.
		await expect(messagingChatApi.snapshot(ORG)).rejects.toMatchObject({
			source: 'api',
			code: 'unavailable'
		});
	});
});
