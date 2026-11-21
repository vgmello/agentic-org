import { describe, expect, it, vi } from 'vitest';
import type { GatewayHttpTransport, GatewayReply } from '$lib/platform/http/gateway-errors';
import { inboxApi } from './api';
import { createHttpInboxApi, extractMessageId } from './http-api';

const ORG = '0193a5c0-0000-7000-8000-000000000001';
const INBOX_PATH = `/api/v1/organizations/${ORG}/inbox`;
const ANSWER_PATH = `/api/v1/organizations/${ORG}/chat/answer`;
const DONE_PATH = `/api/v1/organizations/${ORG}/chat/done`;

function ok(result: unknown): GatewayReply {
	return { status: 200, body: { ok: true, result } };
}

describe('createHttpInboxApi', () => {
	it('extracts message IDs from inbox references', () => {
		expect(extractMessageId('office-1:msg:0193-abc')).toBe('0193-abc');
		expect(extractMessageId('office-1:run:r-1')).toBeUndefined();
		expect(extractMessageId('plain-id')).toBeUndefined();
	});

	it('maps asks, approvals, decisions, and handled items from GET /inbox', async () => {
		const http: GatewayHttpTransport = {
			read: vi.fn().mockResolvedValue(
				ok({
					as_of: '2026-11-21T12:00:00Z',
					counts: { approvals: 1, questions: 1, decisions: 1, problems: 0, total: 3 },
					degraded: false,
					items: [
						{
							reference: 'off-1:msg:ask-1',
							source: 'chat',
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
							actions: ['approve', 'deny']
						},
						{
							reference: 'off-1:msg:ask-2',
							source: 'chat',
							kind: 'decision',
							member: 'm-sam',
							summary: 'Pick rollout target',
							options: [
								{ id: 'staging', label: 'Staging' },
								{ id: 'prod', label: 'Production' }
							],
							created_at_ms: Date.parse('2026-11-21T11:45:00Z'),
							state: 'open',
							clears: 'answered',
							dismissable: true,
							actions: ['choose']
						},
						{
							reference: 'off-1:msg:ask-3',
							source: 'chat',
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

		const api = createHttpInboxApi({ http });
		const items = await api.list(ORG);

		expect(http.read).toHaveBeenCalledWith(INBOX_PATH);
		expect(items).toHaveLength(3);
		expect(items[0]).toMatchObject({
			id: 'off-1:msg:ask-1',
			kind: 'approval',
			source: 'Chat approval',
			title: 'Publish 0.2 release notes',
			who: { id: 'm-kai', name: 'Kai Lindqvist', kind: 'agent' },
			team: { id: 't-plat', name: 'Platform' },
			project: { id: 'p-atlas', name: 'Atlas' },
			why: 'Needs owner sign-off before tag.',
			recommendation: 'Approve once tests pass',
			actions: ['approve', 'deny'],
			open: { path: '/chat?m=ask-1', label: 'Open in Chat' },
			state: 'open'
		});
		expect(items[1]).toMatchObject({
			id: 'off-1:msg:ask-2',
			kind: 'decision',
			actions: ['choose', 'dismiss'],
			choices: [
				{ id: 'staging', label: 'Staging' },
				{ id: 'prod', label: 'Production' }
			]
		});
		expect(items[2]).toMatchObject({
			id: 'off-1:msg:ask-3',
			state: 'handled',
			actions: [],
			handled: { outcome: 'Answered', at: '2026-11-21T10:15:00.000Z' }
		});
	});

	it('dispatches approve, deny, choose, reply to /chat/answer and dismiss to /chat/done', async () => {
		const submit = vi.fn().mockResolvedValue(ok({ ok: true }));
		const http: GatewayHttpTransport = {
			read: vi.fn().mockResolvedValue(
				ok({
					as_of: '2026-11-21T12:00:00Z',
					items: [
						{
							reference: 'off-1:msg:m1',
							source: 'chat',
							kind: 'decision',
							summary: 'Choose env',
							options: [{ id: 'staging', label: 'Staging' }],
							state: 'open',
							clears: 'answered',
							dismissable: true,
							actions: ['approve', 'deny', 'choose', 'reply', 'dismiss']
						}
					]
				})
			),
			submit
		};
		const api = createHttpInboxApi({ http, now: () => '2026-11-21T12:05:00Z' });
		const onChange = vi.fn();
		const stop = await api.subscribe(ORG, onChange);

		await api.list(ORG);
		await api.act(ORG, 'off-1:msg:m1', { type: 'approve' });
		expect(submit).toHaveBeenLastCalledWith(ANSWER_PATH, { ask_id: 'm1', option_id: 'approve' });

		await api.act(ORG, 'off-1:msg:m1', { type: 'deny', reason: 'Wait for QA' });
		expect(submit).toHaveBeenLastCalledWith(ANSWER_PATH, {
			ask_id: 'm1',
			option_id: 'reject',
			text: 'Wait for QA'
		});

		const chosen = await api.act(ORG, 'off-1:msg:m1', { type: 'choose', choice: 'staging' });
		expect(submit).toHaveBeenLastCalledWith(ANSWER_PATH, { ask_id: 'm1', option_id: 'staging' });
		expect(chosen.handled?.outcome).toBe('Decided: Staging');

		await api.act(ORG, 'off-1:msg:m1', { type: 'reply', text: 'Ship Friday' });
		expect(submit).toHaveBeenLastCalledWith(ANSWER_PATH, { ask_id: 'm1', text: 'Ship Friday' });

		await api.act(ORG, 'off-1:msg:m1', { type: 'dismiss' });
		expect(submit).toHaveBeenLastCalledWith(DONE_PATH, { id: 'm1' });
		expect(onChange).toHaveBeenCalledTimes(5);
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
		await expect(inboxApi.subscribe(ORG, () => undefined)).rejects.toMatchObject({
			source: 'api',
			code: 'unavailable'
		});
	});
});
