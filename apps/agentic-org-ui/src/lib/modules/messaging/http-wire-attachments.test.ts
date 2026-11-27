import { describe, expect, it, vi } from 'vitest';
import type { GatewayHttpTransport } from '$lib/platform/http/gateway-errors';
import { createDraftStore } from './drafts/store';
import { createHttpChatApi } from './http-api';
import { buildChatInboxBody, buildMessageListBody } from './http-dispatch';
import { mapMessage } from './http-wire';
import { hasMessageContent } from './http-wire-attachments';
import { parseSearch, searchMessages } from './model-search';
import { applyChat, emptyChat } from './testing/chat-engine';
import { ctxAs, NOW } from './testkit';
import type { AttachmentMeta, ChatState } from './types';

const IMG_META: AttachmentMeta = {
	id: '0190a5d2-7c3e-7a01-8b2c-000000000001',
	filename: 'architecture.png',
	title: 'Architecture Diagram',
	artifact_status: 'final',
	mime_type: 'image/png',
	byte_count: 24_576,
	width: 1280,
	height: 720,
	uploaded_by_employee_id: 'owner',
	created_at: NOW
};

const PDF_META: AttachmentMeta = {
	id: '0190a5d2-7c3e-7a01-8b2c-000000000002',
	filename: 'roadmap.pdf',
	title: 'Q4 Roadmap',
	artifact_status: 'review',
	mime_type: 'application/pdf',
	byte_count: 148_000,
	page_count: 6,
	uploaded_by_employee_id: 'sam',
	created_at: NOW
};

describe('messaging wire attachments, filters, and attachment-only sends', () => {
	it('decodes AttachmentMeta[] on messages and ask answers', () => {
		const mapped = mapMessage(
			{
				id: 'm-1',
				conversation_id: 'c-1',
				sender_kind: 'person',
				sender_employee_id: 'owner',
				sender_display_name: 'Owner',
				kind: 'request',
				body: 'Please check both',
				created_at: NOW,
				attachments: [IMG_META],
				answer: {
					by: 'sam',
					at: NOW,
					text: 'Looks good',
					attachments: [PDF_META]
				}
			},
			'c-1',
			new Set()
		);
		expect(mapped?.message.attachment_meta).toEqual([IMG_META]);
		expect(mapped?.message.answer?.attachment_meta).toEqual([PDF_META]);
	});

	it('sends attachment_ids on /chat/messages, /chat/ask, /chat/answer, and /chat/reply', async () => {
		const calls: Array<{ path: string; body: unknown }> = [];
		const http: GatewayHttpTransport = {
			read: vi.fn(async () => ({ status: 200, body: { result: {} } })),
			submit: vi.fn(async (path, body) => {
				calls.push({ path, body });
				return {
					status: 200,
					body: { result: { id: 'ok-1', ask_id: 'ok-1', conversation_id: 'c-1' } }
				};
			})
		};
		const api = createHttpChatApi({ http });

		await api.command('org-1', {
			method: 'msg.send',
			change_id: 'chg-1',
			conversation_id: 'c-1',
			kind: 'msg',
			body: '',
			attachment_ids: [IMG_META.id]
		});
		await api.command('org-1', {
			method: 'msg.send',
			change_id: 'chg-2',
			conversation_id: 'c-1',
			kind: 'request',
			body: 'Approve mock?',
			addressed_to: 'sam',
			attachment_ids: [IMG_META.id, PDF_META.id]
		});
		await api.command('org-1', {
			method: 'chat.answer',
			change_id: 'chg-3',
			ask_id: 'ask-1',
			text: 'Approved with notes',
			attachment_ids: [PDF_META.id]
		});
		await api.command('org-1', {
			method: 'msg.reply',
			change_id: 'chg-4',
			to_id: 'm-1',
			body: '',
			attachment_ids: [PDF_META.id]
		});

		expect(calls).toEqual([
			{
				path: '/api/v1/organizations/org-1/chat/messages',
				body: expect.objectContaining({ body: '', attachment_ids: [IMG_META.id] })
			},
			{
				path: '/api/v1/organizations/org-1/chat/ask',
				body: expect.objectContaining({ attachment_ids: [IMG_META.id, PDF_META.id] })
			},
			{
				path: '/api/v1/organizations/org-1/chat/answer',
				body: expect.objectContaining({ ask_id: 'ask-1', attachment_ids: [PDF_META.id] })
			},
			{
				path: '/api/v1/organizations/org-1/chat/reply',
				body: expect.objectContaining({ to_id: 'm-1', body: '', attachment_ids: [PDF_META.id] })
			}
		]);
	});

	it('builds has_image and has_file wire filters and filters messages in client search', () => {
		expect(buildMessageListBody('c-1', 50, { has_image: true })).toEqual({
			conversation: 'c-1',
			limit: 50,
			has_image: true
		});
		expect(buildChatInboxBody({ include_all: true, has_file: true })).toEqual({
			include_all: true,
			has_file: true
		});

		const ctx = ctxAs();
		const state: ChatState = {
			...emptyChat(),
			conversations: [
				{ id: 'c-1', kind: 'channel', name: 'general', members: ['owner', 'sam'], created_at: NOW }
			],
			messages: [
				{
					id: 'm-img',
					conversation_id: 'c-1',
					from: 'owner',
					kind: 'msg',
					body: '',
					created_at: NOW,
					attachment_meta: [IMG_META]
				},
				{
					id: 'm-pdf',
					conversation_id: 'c-1',
					from: 'sam',
					kind: 'msg',
					body: 'See doc',
					created_at: NOW,
					attachment_meta: [PDF_META]
				},
				{
					id: 'm-plain',
					conversation_id: 'c-1',
					from: 'sam',
					kind: 'msg',
					body: 'Just text',
					created_at: NOW
				}
			]
		};

		expect(searchMessages(state, ctx, parseSearch('has:image')).map((m) => m.id)).toEqual([
			'm-img'
		]);
		expect(searchMessages(state, ctx, parseSearch('has:file')).map((m) => m.id)).toEqual(['m-pdf']);
		expect(searchMessages(state, ctx, parseSearch('has:attachment')).map((m) => m.id)).toEqual([
			'm-img',
			'm-pdf'
		]);
		expect(searchMessages(state, ctx, parseSearch('roadmap.pdf')).map((m) => m.id)).toEqual([
			'm-pdf'
		]);
	});

	it('allows attachment-only messages in hasMessageContent and chat-engine', () => {
		expect(hasMessageContent('', [IMG_META.id])).toBe(true);
		expect(hasMessageContent('   ', [])).toBe(false);

		const ctx = ctxAs();
		const base: ChatState = {
			...emptyChat(),
			conversations: [
				{
					id: 'c-1',
					kind: 'channel',
					name: 'general',
					members: [ctx.owner, 'sam'],
					created_at: NOW
				}
			]
		};
		const { state } = applyChat(
			base,
			{
				method: 'msg.send',
				change_id: 'chg-empty-body',
				conversation_id: 'c-1',
				kind: 'msg',
				body: '',
				attachment_ids: [IMG_META.id]
			},
			ctx
		);
		expect(state.messages).toHaveLength(1);
		expect(state.messages[0].body).toBe('');
	});

	it('preserves staged attachments in DraftStore per conversation and revokes previews on clear/dispose', () => {
		const revokeSpy = vi.spyOn(URL, 'revokeObjectURL').mockImplementation(() => {});
		let orgId = 'org-1';
		const store = createDraftStore(() => orgId);
		const convA = store.forThread('conversation:c-a').reply;
		const convB = store.forThread('conversation:c-b').reply;

		convA.writeAttachments?.([
			{
				localId: 'l1',
				filename: 'shot.png',
				mimeType: 'image/png',
				byteCount: 120,
				previewUrl: 'blob:draft/1',
				status: 'ready',
				uploadId: IMG_META.id
			}
		]);
		expect(convA.readAttachments?.()).toHaveLength(1);
		expect(convB.readAttachments?.()).toHaveLength(0);

		const clearA = convA.clearOnSuccess();
		clearA();
		expect(convA.readAttachments?.()).toHaveLength(0);
		expect(revokeSpy).toHaveBeenCalledWith('blob:draft/1');

		convB.writeAttachments?.([
			{
				localId: 'l2',
				filename: 'shot2.png',
				mimeType: 'image/png',
				byteCount: 120,
				previewUrl: 'blob:draft/2',
				status: 'ready',
				uploadId: IMG_META.id
			}
		]);
		orgId = 'org-2';
		store.syncScope();
		expect(revokeSpy).toHaveBeenCalledWith('blob:draft/2');

		store.dispose();
		revokeSpy.mockRestore();
	});
});
