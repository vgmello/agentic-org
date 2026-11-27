import { tick } from 'svelte';
import { toast } from 'svelte-sonner';
import { toAppError } from '$lib/proto';
import {
	locate,
	repliesTo,
	type ChatCall,
	type ChatMessage,
	type ChatState,
	type ChatView
} from '$lib/modules/messaging';
import type { ChatApi } from '../../api';
import type { ChatLocation, PendingMessage } from '../../types';
import { scrollMessageBelowStickyHeader } from './chat-scroll';

export function buildChatPlaceUrl(
	baseUrl: URL,
	view: ChatView,
	threadRoot: string | undefined,
	query: string,
	messageId?: string
): URL {
	const next = new URL(baseUrl);
	for (const key of ['c', 'm', 'thread', 'v', 'q']) next.searchParams.delete(key);
	if (view.type === 'conversation') next.searchParams.set('c', view.id);
	else next.searchParams.set('v', view.type);
	if (threadRoot) next.searchParams.set('thread', threadRoot);
	if (messageId) next.searchParams.set('m', messageId);
	if (query.trim()) next.searchParams.set('q', query);
	return next;
}

export interface ResolvedLocationTarget {
	redirectInbox?: true;
	view: ChatView;
	query: string;
	threadRoot?: string;
	highlight?: string;
}

export function resolveChatLocation(
	chatState: ChatState,
	loc: ChatLocation
): ResolvedLocationTarget {
	const found = locate(chatState, loc);
	if (!found.conversation) {
		if (loc.view === 'needs') return { redirectInbox: true, view: { type: 'mentions' }, query: '' };
		const general = chatState.conversations.find((c) => c.auto?.type === 'general');
		const view: ChatView =
			loc.view === 'mentions' || loc.view === 'threads' || !general
				? { type: loc.view === 'threads' ? 'threads' : 'mentions' }
				: { type: 'conversation', id: general.id };
		return { view, query: loc.query ?? '' };
	}
	let threadRoot: string | undefined;
	if (found.thread) threadRoot = found.thread.id;
	else if (found.message && repliesTo(chatState, found.message.id).length)
		threadRoot = found.message.id;
	if (
		loc.thread &&
		chatState.messages.some(
			(m) =>
				m.id === loc.thread && m.conversation_id === found.conversation!.id && !m.thread_root_id
		)
	) {
		threadRoot = loc.thread;
	}
	return {
		view: { type: 'conversation', id: found.conversation.id },
		query: loc.query ?? '',
		threadRoot,
		highlight: found.message?.id
	};
}

export function scrollMessageIntoView(
	messageId: string | undefined,
	listEl?: HTMLElement | (() => HTMLElement | undefined)
): void {
	scrollMessageBelowStickyHeader(listEl, messageId);
}

export function computeQueryNavigation(
	value: string,
	currentUrl: URL,
	pendingPlace: URL | undefined,
	state: Record<string, unknown> & { chatSearchEntry?: true }
):
	| { kind: 'back' }
	| { kind: 'navigate'; next: URL; replaceState: boolean; state?: Record<string, unknown> } {
	const next = new URL(pendingPlace ?? currentUrl);
	const existingSearch = currentUrl.searchParams.has('q');
	if (value.trim()) {
		next.searchParams.set('q', value);
		return {
			kind: 'navigate',
			next,
			replaceState: existingSearch,
			state: existingSearch ? state : { ...state, chatSearchEntry: true }
		};
	}
	if (existingSearch && state.chatSearchEntry) return { kind: 'back' };
	next.searchParams.delete('q');
	return { kind: 'navigate', next, replaceState: existingSearch };
}

export function findNewMarkerMessageId(
	readMarker: string | undefined,
	shown: readonly ChatMessage[] | undefined,
	ownerId: string
): string | undefined {
	if (!readMarker || !shown) return undefined;
	return shown.find((m) => m.from !== ownerId && m.created_at > readMarker)?.id;
}

export async function restoreTemporaryFocus(
	origin: Element | null,
	place: string,
	currentPlace: () => string,
	target: () => HTMLElement | null,
	fallback: () => HTMLElement | undefined
): Promise<void> {
	await tick();
	if (
		!origin ||
		place !== currentPlace() ||
		(document.activeElement !== document.body && document.activeElement !== origin)
	) {
		return;
	}
	const next = target() ?? fallback();
	if (!next) return;
	const temporary = next.tabIndex < 0 && !next.hasAttribute('tabindex');
	if (temporary) next.tabIndex = -1;
	next.focus({ preventScroll: true });
	if (temporary) {
		const clear = () => {
			if (next.getAttribute('tabindex') === '-1') next.removeAttribute('tabindex');
		};
		if (document.activeElement === next) next.addEventListener('blur', clear, { once: true });
		else clear();
	}
}

export async function deliverPendingMessage(input: {
	chatApi: ChatApi;
	organizationId: string;
	p: PendingMessage;
	retry: boolean;
	retryButton?: HTMLElement;
	focusPlace: () => string;
	removePending: (key: number) => void;
	reload: () => Promise<void>;
	listEl: () => HTMLElement | undefined;
}): Promise<void> {
	const { p } = input;
	const origin = input.retry ? document.activeElement : null;
	const area = (input.retryButton ?? origin)?.closest('[data-focus-scope]');
	const place = input.focusPlace();
	let deliveredId: string | undefined;
	p.status = 'sending';
	try {
		const accepted = await input.chatApi.command(input.organizationId, {
			method: 'msg.send',
			change_id: p.change_id,
			conversation_id: p.conversation_id,
			thread_root_id: p.thread_root_id,
			also_in_channel: p.also_in_channel,
			kind: 'msg',
			body: p.body,
			attachments: p.attachments.length ? p.attachments : undefined
		} satisfies ChatCall);
		deliveredId = accepted.id;
		input.removePending(p.key);
		await input.reload();
	} catch (e) {
		const error = toAppError(e);
		if (error.code === 'unavailable') p.status = 'unknown';
		else {
			p.status = 'failed';
			p.error = error.message;
		}
	}
	if (origin) {
		await restoreTemporaryFocus(
			origin,
			place,
			input.focusPlace,
			() =>
				area?.querySelector<HTMLElement>(
					`[data-id="${CSS.escape(deliveredId ?? `pending-${p.key}`)}"]`
				) ?? null,
			input.listEl
		);
	}
}

export async function checkPendingMessageStatus(input: {
	chatApi: ChatApi;
	organizationId: string;
	p: PendingMessage;
	removePending: (key: number) => void;
	reload: () => Promise<void>;
}): Promise<void> {
	const { p } = input;
	p.status = 'checking';
	try {
		const known = await input.chatApi.status(input.organizationId, p.change_id);
		if (known.state === 'applied') {
			input.removePending(p.key);
			await input.reload();
			toast.success('Delivered.');
		} else {
			p.status = 'failed';
			p.error = 'the office has no record of it. Retry sends it once.';
		}
	} catch {
		p.status = 'unknown';
	}
}
