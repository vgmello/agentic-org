import { describe, expect, it, vi } from 'vitest';
import type { GatewayHttpTransport, GatewayReply } from '$lib/platform/http/gateway-errors';
import { chatApi as reexportedChatApi } from '$lib/modules/chat/api';
import { chatApi as messagingChatApi } from './api';
import { createHttpChatApi } from './http-api';

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

describe('createHttpChatApi (real Rust BFF wire shapes)', () => {
	it('shares a single chatApi singleton between chat and messaging modules', () => {
		expect(reexportedChatApi).toBe(messagingChatApi);
	});

	it('maps real ConversationSummary (member objects, auto_scope, pins, last_read_message_id) and employee senders', async () => {
		const http = makeHttp(
			{
				[`${BASE}/conversations`]: ok({
					conversations: [
						{
							id: 'c-general',
							kind: 'channel',
							name: 'general',
							auto_scope: 'organization',
							members: [
								{ employee_id: 'owner-1', joined_at: '2026-11-20T09:00:00Z' },
								{ employee_id: 'agent-1', joined_at: '2026-11-20T09:01:00Z' },
								{
									employee_id: 'former-1',
									joined_at: '2026-11-20T09:02:00Z',
									left_at: '2026-11-21T08:00:00Z'
								}
							],
							last_read_message_id: 'm-ask-1',
							pins: [{ message_id: 'm-ask-1' }],
							created_at: '2026-11-20T09:00:00Z'
						},
						{
							id: 'c-team-plat',
							kind: 'channel',
							name: 'platform',
							auto_scope: 'team',
							team_id: 't-plat',
							members: [{ employee_id: 'owner-1', joined_at: '2026-11-20T09:00:00Z' }],
							created_at: '2026-11-20T09:00:00Z'
						}
					]
				}),
				[`${BASE}/channels`]: ok({
					channels: [
						{
							id: 'c-proj-atlas',
							kind: 'channel',
							name: 'atlas',
							auto_scope: 'project',
							project_id: 'p-atlas',
							access: 'open',
							members: [{ employee_id: 'agent-1', joined_at: '2026-11-20T09:01:00Z' }],
							created_at: '2026-11-20T09:05:00Z'
						}
					]
				})
			},
			{
				[`${BASE}/messages/list`]: (body) => {
					const conv = (body as { conversation: string }).conversation;
					if (conv !== 'c-general') return ok({ conversation_id: conv, messages: [] });
					return ok({
						conversation_id: 'c-general',
						messages: [
							{
								id: 'm-ask-1',
								conversation_id: 'c-general',
								thread_id: 'm-ask-1',
								sender_kind: 'employee',
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
												default_option: '',
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
								sender_kind: 'employee',
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
							}
						]
					});
				}
			}
		);

		const api = createHttpChatApi({ http, viewerId: () => 'owner-1' });
		const snap = await api.snapshot(ORG);

		expect(snap.sample).toBe(false);
		expect(snap.office_running).toBe(true);
		expect(snap.state.seq).toBe(13);

		const general = snap.state.conversations.find((c) => c.id === 'c-general')!;
		expect(general.members).toEqual(['owner-1', 'agent-1']);
		expect(general.auto).toEqual({ type: 'general' });
		expect(snap.state.conversations.find((c) => c.id === 'c-team-plat')?.auto).toEqual({
			type: 'team',
			team_id: 't-plat'
		});
		expect(snap.state.conversations.find((c) => c.id === 'c-proj-atlas')?.auto).toEqual({
			type: 'project',
			project_id: 'p-atlas'
		});
		expect(snap.state.read_at['c-general']).toBe('2026-11-21T10:10:00Z');
		expect(snap.directory).toContainEqual(
			expect.objectContaining({ id: 'agent-1', name: 'Kai Lindqvist' })
		);

		const ask = snap.state.messages.find((m) => m.id === 'm-ask-1')!;
		expect(ask.pinned).toBe(true);
		expect(ask.questions?.[0]).toMatchObject({
			id: 'q-single',
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
	});

	it('routes every request kind to /chat/ask with top-level addressed_to, synthesized approval options, and default_value-only questions', async () => {
		const http = makeHttp(
			{
				[`${BASE}/conversations`]: ok({ conversations: [] }),
				[`${BASE}/channels`]: ok({ channels: [] })
			},
			{
				[`${BASE}/ask`]: (body) =>
					ok({
						status: 'pending',
						ask_id: (body as { id: string }).id,
						conversation_id: 'c-general'
					}),
				[`${BASE}/messages`]: (body) =>
					ok({ id: (body as { id: string }).id, conversation_id: 'c-general' }),
				[`${BASE}/answer`]: (body) =>
					ok({ ask_id: (body as { ask_id: string }).ask_id, conversation_id: 'c-general' }),
				[`${BASE}/withdraw`]: (body) =>
					ok({ ask_id: (body as { ask_id: string }).ask_id, conversation_id: 'c-general' })
			}
		);

		const api = createHttpChatApi({ http, viewerId: () => 'owner-1' });

		// 1. Free-text request without explicit questions/options still goes to /chat/ask.
		await api.command(ORG, {
			method: 'msg.send',
			change_id: 'ask-free',
			conversation_id: 'c-general',
			kind: 'request',
			body: 'What is the ETA?',
			addressed_to: 'agent-1'
		});
		expect(http.submitCalls[0]).toEqual({
			path: `${BASE}/ask`,
			body: {
				id: 'ask-free',
				to: 'c-general',
				body: 'What is the ETA?',
				addressed_to: 'agent-1',
				questions: [{ id: 'q1', question_type: 'free_text', prompt: 'What is the ETA?' }]
			}
		});

		// 2. Approval request without explicit options synthesizes Approve/Reject choice question.
		await api.command(ORG, {
			method: 'msg.send',
			change_id: 'ask-approval',
			conversation_id: 'c-general',
			kind: 'request',
			body: 'Deploy v1?',
			approval: true,
			addressed_to: 'owner-1'
		});
		expect(http.submitCalls[1]).toEqual({
			path: `${BASE}/ask`,
			body: {
				id: 'ask-approval',
				to: 'c-general',
				body: 'Deploy v1?',
				addressed_to: 'owner-1',
				approval: true,
				questions: [
					{
						id: 'q1',
						question_type: 'choice',
						prompt: 'Deploy v1?',
						options: [
							{ id: 'approve', label: 'Approve' },
							{ id: 'reject', label: 'Reject' }
						]
					}
				]
			}
		});

		// 3. Structured multi-question ask emits ONLY `default_value` (never duplicate `default_option`).
		await api.command(ORG, {
			method: 'msg.send',
			change_id: 'ask-multi',
			conversation_id: 'c-general',
			kind: 'request',
			body: 'Release config',
			required: true,
			questions: [
				{
					id: 'q1',
					question_type: 'yes_no',
					prompt: 'Ship?',
					default_option: 'yes',
					default_value: 'yes'
				}
			]
		});
		const sentQ = (http.submitCalls[2].body as { questions: Record<string, unknown>[] })
			.questions[0];
		expect(sentQ).toEqual({
			id: 'q1',
			question_type: 'yes_no',
			prompt: 'Ship?',
			default_value: 'yes'
		});
		expect('default_option' in sentQ).toBe(false);

		// 4. Multi-question answer + withdraw.
		await api.command(ORG, {
			method: 'chat.answer',
			change_id: 'ans-1',
			ask_id: 'ask-multi',
			answers: [{ question_id: 'q1', option_ids: ['yes'] }]
		});
		await api.command(ORG, {
			method: 'chat.withdraw',
			change_id: 'wd-1',
			ask_id: 'ask-free',
			reason: 'Done'
		});
		expect(http.submitCalls[3].path).toBe(`${BASE}/answer`);
		expect(http.submitCalls[4].path).toBe(`${BASE}/withdraw`);
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

		await expect(messagingChatApi.snapshot(ORG)).rejects.toMatchObject({
			source: 'api',
			code: 'unavailable'
		});
		const stop = vi.fn();
		expect(() => messagingChatApi.subscribe(ORG, stop)()).not.toThrow();
	});
});
