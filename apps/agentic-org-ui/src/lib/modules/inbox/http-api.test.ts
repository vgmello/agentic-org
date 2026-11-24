import { describe, expect, it, vi } from 'vitest';
import type { GatewayHttpTransport, GatewayReply } from '$lib/platform/http/gateway-errors';
import { inboxApi } from './api';
import { createHttpInboxApi, extractMessageId } from './http-api';

const ORG = '0193a5c0-0000-7000-8000-000000000001';
const INBOX_PATH = `/api/v1/organizations/${ORG}/inbox`;
const ANSWER_PATH = `/api/v1/organizations/${ORG}/chat/answer`;
const REPLY_PATH = `/api/v1/organizations/${ORG}/chat/reply`;
const DONE_PATH = `/api/v1/organizations/${ORG}/chat/done`;
const DEFER_PATH = `/api/v1/organizations/${ORG}/chat/defer`;
const UNSNOOZE_PATH = `/api/v1/organizations/${ORG}/chat/unsnooze`;
const CHAT_INBOX_PATH = `/api/v1/organizations/${ORG}/chat/inbox`;

function ok(result: unknown): GatewayReply {
	return { status: 200, body: { result } };
}

describe('createHttpInboxApi (real InboxListResult wire schema)', () => {
	it('extracts message IDs from inbox refs', () => {
		expect(extractMessageId('office-1:msg:0193-abc')).toBe('0193-abc');
		expect(extractMessageId('office-1:run:r-1')).toBeUndefined();
		expect(extractMessageId('plain-id')).toBeUndefined();
	});

	it('decodes real wire ref, as_of object, degraded[], tagged actions, and bare string options safely', async () => {
		const http: GatewayHttpTransport = {
			read: vi.fn().mockResolvedValue(
				ok({
					as_of: { epoch: 1, seq: 42 },
					counts: { approvals: 1, questions: 2, decisions: 1, problems: 0, total: 4 },
					degraded: [],
					items: [
						{
							ref: 'off-1:msg:ask-1',
							source: 'approval',
							kind: 'approval',
							member: { id: 'm-kai', name: 'Kai Lindqvist', kind: 'agent' },
							team: { id: 't-plat', name: 'Platform' },
							project: { id: 'p-atlas', name: 'Atlas' },
							summary: 'Publish 0.2 release notes',
							context: 'Needs owner sign-off before tag.',
							recommendation: 'Approve once tests pass',
							created_at_ms: Date.parse('2026-11-21T11:30:00Z'),
							expires_at_ms: Date.parse('2026-11-21T14:00:00Z'),
							state: 'open',
							clears: 'answered',
							dismissable: false,
							actions: [
								{
									type: 'call',
									label: 'Approve',
									op: 'chat.answer',
									params: { option_id: 'approve' },
									fresh: true
								},
								{
									type: 'call',
									label: 'Reject',
									op: 'chat.answer',
									params: { option_id: 'reject' },
									fresh: true
								},
								{
									type: 'open',
									label: 'View approval in Chat',
									open: { target: 'message', id: 'ask-1' }
								}
							]
						},
						{
							ref: 'off-1:msg:ask-2',
							source: 'ask',
							kind: 'decision',
							member: 'm-sam',
							summary: 'Pick rollout target',
							options: ['Staging', 'Production'],
							created_at_ms: Date.parse('2026-11-21T11:45:00Z'),
							state: 'open',
							clears: 'answered',
							dismissable: true,
							actions: [
								{ type: 'call', label: 'Answer', op: 'chat.answer', params: {}, fresh: true },
								{ type: 'open', label: 'Answer in Chat', open: { target: 'message', id: 'ask-2' } }
							]
						},
						{
							ref: 'off-1:msg:mail-1',
							source: 'mailbox',
							kind: 'question',
							member: { id: 'm-noor', name: 'Noor Haddad', kind: 'agent' },
							summary: 'Can you check the design draft?',
							created_at_ms: Date.parse('2026-11-21T11:50:00Z'),
							state: 'open',
							clears: 'replied',
							dismissable: true,
							actions: [
								{ type: 'call', label: 'Reply', op: 'chat.reply', params: {}, fresh: true },
								{ type: 'call', label: 'Done', op: 'chat.done', params: {}, fresh: true }
							]
						},
						{
							ref: 'off-1:msg:ask-3',
							source: 'ask',
							kind: 'question',
							member: { id: 'm-sam', name: 'Sam Okafor', kind: 'agent' },
							summary: 'Already answered question',
							created_at_ms: Date.parse('2026-11-21T10:00:00Z'),
							handled_at_ms: Date.parse('2026-11-21T10:15:00Z'),
							outcome: 'Answered',
							state: 'handled',
							clears: 'answered',
							dismissable: false,
							actions: []
						}
					]
				})
			),
			submit: vi.fn()
		};

		const api = createHttpInboxApi({ http, now: () => '2026-11-21T12:00:00Z' });
		const items = await api.list(ORG);

		expect(http.read).toHaveBeenCalledWith(INBOX_PATH);
		expect(items).toHaveLength(4);
		expect(items[0]).toMatchObject({
			id: 'off-1:msg:ask-1',
			kind: 'approval',
			source: 'Chat approval',
			actions: ['approve', 'deny'],
			open: { path: '/chat?m=ask-1', label: 'View approval in Chat' },
			state: 'open'
		});
		// Bare string `options` must NOT emit `choose` or send labels as option IDs; user opens in Chat or dismisses.
		expect(items[1]).toMatchObject({
			id: 'off-1:msg:ask-2',
			kind: 'decision',
			actions: ['dismiss'],
			open: { path: '/chat?m=ask-2', label: 'Answer in Chat' }
		});
		expect(items[1].choices).toBeUndefined();
		expect(items[2]).toMatchObject({
			id: 'off-1:msg:mail-1',
			source: 'Chat message',
			actions: ['reply', 'dismiss'],
			open: { path: '/chat?m=mail-1', label: 'Open in Chat' }
		});
		expect(items[3]).toMatchObject({
			id: 'off-1:msg:ask-3',
			state: 'handled',
			actions: [],
			handled: { outcome: 'Answered', at: '2026-11-21T10:15:00.000Z' }
		});
	});

	it('routes mailbox replies to /chat/reply, ask/approval actions to /chat/answer, and dismiss to /chat/done', async () => {
		const submit = vi.fn().mockResolvedValue(ok({ ok: true }));
		const http: GatewayHttpTransport = {
			read: vi.fn().mockResolvedValue(
				ok({
					as_of: { epoch: 1, seq: 9 },
					degraded: [],
					items: [
						{
							ref: 'off-1:msg:m-ask',
							source: 'approval',
							kind: 'approval',
							summary: 'Approve deploy',
							state: 'open',
							clears: 'answered',
							dismissable: true,
							actions: []
						},
						{
							ref: 'off-1:msg:m-mail',
							source: 'mailbox',
							kind: 'question',
							summary: 'Quick ping',
							state: 'open',
							clears: 'replied',
							dismissable: true,
							actions: []
						}
					]
				})
			),
			submit
		};
		const api = createHttpInboxApi({
			http,
			now: () => '2026-11-21T12:05:00Z',
			newId: () => '0193a5c0-0000-7000-8000-000000000099'
		});
		const onChange = vi.fn();
		const stop = await api.subscribe(ORG, onChange);

		await api.list(ORG);

		await api.act(ORG, 'off-1:msg:m-ask', { type: 'approve' });
		expect(submit).toHaveBeenLastCalledWith(ANSWER_PATH, { ask_id: 'm-ask', option_id: 'approve' });

		await api.act(ORG, 'off-1:msg:m-ask', { type: 'deny', reason: 'Wait for QA' });
		expect(submit).toHaveBeenLastCalledWith(ANSWER_PATH, {
			ask_id: 'm-ask',
			option_id: 'reject',
			text: 'Wait for QA'
		});

		await api.act(ORG, 'off-1:msg:m-mail', { type: 'reply', text: 'Looks good' });
		expect(submit).toHaveBeenLastCalledWith(REPLY_PATH, {
			id: '0193a5c0-0000-7000-8000-000000000099',
			to_id: 'm-mail',
			body: 'Looks good'
		});

		await api.act(ORG, 'off-1:msg:m-mail', { type: 'dismiss' });
		expect(submit).toHaveBeenLastCalledWith(DONE_PATH, { id: 'm-mail' });
		expect(onChange).toHaveBeenCalledTimes(4);
		stop();
	});

	it('fails closed on non-message references, gateway errors, and signed-out inboxApi', async () => {
		const http: GatewayHttpTransport = {
			read: vi.fn().mockResolvedValue({
				status: 503,
				body: { ok: false, error: { code: 'unavailable', message: 'Office offline' } }
			}),
			submit: vi.fn()
		};
		const api = createHttpInboxApi({ http });
		await expect(api.list(ORG)).rejects.toMatchObject({
			source: 'api',
			code: 'unavailable',
			message: 'Office offline'
		});
		await expect(api.act(ORG, 'off-1:run:r9', { type: 'approve' })).rejects.toMatchObject({
			source: 'api',
			code: 'unsupported'
		});

		await expect(inboxApi.list(ORG)).rejects.toMatchObject({ source: 'api', code: 'unavailable' });
		await expect(inboxApi.act(ORG, 'off-1:msg:m1', { type: 'approve' })).rejects.toMatchObject({
			source: 'api',
			code: 'unavailable'
		});
		await expect(inboxApi.defer(ORG, 'off-1:msg:m1', '2026-11-24T09:00:00Z')).rejects.toMatchObject(
			{
				source: 'api',
				code: 'unavailable'
			}
		);
		await expect(inboxApi.unsnooze(ORG, 'off-1:msg:m1')).rejects.toMatchObject({
			source: 'api',
			code: 'unavailable'
		});
		await expect(inboxApi.subscribe(ORG, () => undefined)).rejects.toMatchObject({
			source: 'api',
			code: 'unavailable'
		});
	});

	it('merges paginated snoozed chat items on real { result } envelope, marks non-msg unsnoozeable, surfaces snoozed errors, and routes defer/unsnooze (TASK-81)', async () => {
		const submit = vi.fn().mockImplementation(async (path: string, body?: { cursor?: number }) => {
			if (path === CHAT_INBOX_PATH) {
				if (body?.cursor === 11) {
					return ok({
						items: [
							{
								id: 'snz-2',
								conversation_id: 'conv-2',
								thread_id: 'snz-2',
								from_member_id: 'm-kai',
								from_display_name: 'Kai',
								kind: 'msg',
								priority: 'normal',
								urgent: false,
								subject: 'Second page snoozed item',
								created_at: '2026-11-21T08:05:00.000Z',
								state: 'snoozed',
								triage_gen: 1,
								snoozed_until: '2026-11-26T09:00:00.000Z',
								delivered_because: 'direct',
								local_seq: 12
							}
						]
					});
				}
				return ok({
					items: [
						{
							id: 'snz-1',
							conversation_id: 'conv-1',
							thread_id: 'snz-1',
							from_member_id: 'm-ada',
							from_display_name: 'Ada',
							kind: 'msg',
							priority: 'normal',
							urgent: false,
							subject: 'Follow up on gateway TLS rotation',
							created_at: '2026-11-21T08:00:00.000Z',
							state: 'snoozed',
							triage_gen: 2,
							snoozed_until: '2026-11-24T09:00:00.000Z',
							snooze_note: 'After Monday standup',
							delivered_because: 'direct',
							local_seq: 11
						}
					],
					next_cursor: 11
				});
			}
			if (path === DEFER_PATH) {
				return ok({
					id: 'm-open',
					triage_gen: 1,
					snoozed_until: '2026-11-25T14:00:00.000Z',
					snooze_note: 'Check after release'
				});
			}
			if (path === UNSNOOZE_PATH) {
				return ok({
					id: 'snz-1',
					state: 'unread',
					triage_gen: 3
				});
			}
			return ok({ ok: true });
		});
		const http: GatewayHttpTransport = {
			read: vi.fn().mockResolvedValue(
				ok({
					as_of: { epoch: 1, seq: 12 },
					degraded: [],
					items: [
						{
							ref: 'off-1:msg:m-open',
							source: 'mailbox',
							kind: 'question',
							member: { id: 'm-noor', name: 'Noor Haddad', kind: 'agent' },
							summary: 'Confirm migration checklist',
							created_at_ms: Date.parse('2026-11-21T10:00:00Z'),
							state: 'open',
							clears: 'replied',
							dismissable: true,
							actions: []
						},
						{
							ref: 'off-1:budget:b-1',
							source: 'budget',
							kind: 'problem',
							member: { id: 'm-kai', name: 'Kai', kind: 'agent' },
							summary: 'Daily budget reached',
							created_at_ms: Date.parse('2026-11-21T10:05:00Z'),
							state: 'open',
							clears: 'resolved',
							dismissable: false,
							actions: []
						}
					]
				})
			),
			submit
		};

		const api = createHttpInboxApi({ http, now: () => '2026-11-21T12:00:00Z' });
		const items = await api.list(ORG);
		expect(submit).toHaveBeenCalledWith(CHAT_INBOX_PATH, { state: 'snoozed', limit: 100 });
		expect(submit).toHaveBeenCalledWith(CHAT_INBOX_PATH, {
			state: 'snoozed',
			limit: 100,
			cursor: 11
		});
		expect(items).toHaveLength(4);
		expect(items[0].snoozeable).toBe(true);
		expect(items[1].snoozeable).toBe(false);
		expect(items[2]).toMatchObject({
			id: 'chat:msg:snz-1',
			title: 'Follow up on gateway TLS rotation',
			state: 'snoozed',
			snoozeable: true,
			snoozed_until: '2026-11-24T09:00:00.000Z',
			snooze_note: 'After Monday standup',
			open: { path: '/chat?c=conv-1&m=snz-1', label: 'Open in Chat' }
		});
		expect(items[3]).toMatchObject({
			id: 'chat:msg:snz-2',
			state: 'snoozed'
		});
		expect(api.snoozedError?.(ORG)).toBeUndefined();

		// Transient snoozed-list error keeps active items usable and reports retryable snoozedError.
		submit.mockResolvedValueOnce({
			status: 503,
			body: { error: { code: 'unavailable', message: 'Office temporarily busy' } }
		});
		const activeOnly = await api.list(ORG);
		expect(activeOnly).toHaveLength(2);
		expect(api.snoozedError?.(ORG)).toBe('Office temporarily busy');

		const deferred = await api.defer(
			ORG,
			'off-1:msg:m-open',
			'2026-11-25T14:00:00.000Z',
			'Check after release'
		);
		expect(submit).toHaveBeenCalledWith(DEFER_PATH, {
			id: 'm-open',
			until: { type: 'until', at: '2026-11-25T14:00:00.000Z' },
			note: 'Check after release'
		});
		expect(deferred).toMatchObject({
			id: 'off-1:msg:m-open',
			state: 'snoozed',
			snoozed_until: '2026-11-25T14:00:00.000Z',
			snooze_note: 'Check after release'
		});

		const restored = await api.unsnooze(ORG, 'chat:msg:snz-1');
		expect(submit).toHaveBeenCalledWith(UNSNOOZE_PATH, { id: 'snz-1' });
		expect(restored.state).toBe('open');
		expect(restored.snoozed_until).toBeUndefined();
		expect(restored.snooze_note).toBeUndefined();

		await expect(api.defer(ORG, 'off-1:run:r9', '2026-11-25T14:00:00.000Z')).rejects.toMatchObject({
			source: 'api',
			code: 'unsupported'
		});
		await expect(api.unsnooze(ORG, 'off-1:run:r9')).rejects.toMatchObject({
			source: 'api',
			code: 'unsupported'
		});
	});

	it('prevents stale overlapping refreshes from overwriting newer snoozed state and flags max-page truncation', async () => {
		let releaseSlowSnooze!: (reply: GatewayReply) => void;
		const slowSnooze = new Promise<GatewayReply>((resolve) => {
			releaseSlowSnooze = resolve;
		});
		let callCount = 0;
		const submit = vi.fn().mockImplementation(async () => {
			callCount += 1;
			if (callCount === 1) return slowSnooze;
			return ok({ items: [] });
		});
		const read = vi.fn().mockResolvedValue(ok({ as_of: { epoch: 1, seq: 1 }, items: [] }));
		const api = createHttpInboxApi({ http: { read, submit } });

		const older = api.list(ORG);
		const newer = api.list(ORG);
		await newer;
		expect(api.snoozedError?.(ORG)).toBeUndefined();

		// Older request now fails with 503 — must NOT clobber newer clean state.
		releaseSlowSnooze({
			status: 503,
			body: { error: { code: 'unavailable', message: 'Stale older failure' } }
		});
		await older;
		expect(api.snoozedError?.(ORG)).toBeUndefined();

		// 20 full pages with remaining next_cursor surfaces explicit truncation notice.
		let page = 0;
		const truncApi = createHttpInboxApi({
			http: {
				read: vi.fn().mockResolvedValue(ok({ as_of: { epoch: 1, seq: 2 }, items: [] })),
				submit: vi.fn().mockImplementation(async () => {
					page += 1;
					return ok({ items: [], next_cursor: page * 100 });
				})
			}
		});
		await truncApi.list(ORG);
		expect(truncApi.snoozedError?.(ORG)).toContain('first 2,000 snoozed items');
	});
});
