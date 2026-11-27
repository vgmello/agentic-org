import { asNum, asRec, asStr, type Rec } from '$lib/platform/http/decode';
import type { GatewayHttpTransport, GatewayReply } from '$lib/platform/http/gateway-errors';
import type { InboxItem } from './types';

export interface SnoozedAppendDeps {
	http: Pick<GatewayHttpTransport, 'submit'>;
	unwrapInboxReply: (reply: GatewayReply, fallbackMessage: string) => Rec;
	extractMessageId: (reference: string) => string | undefined;
	mapSnoozedChatInboxItem: (
		raw: unknown,
		asOf: string,
		prev?: InboxItem
	) => { item: InboxItem; rawSource?: string } | undefined;
}

function snoozedCatchMessage(err: unknown, signal?: AbortSignal): string | undefined {
	if (signal?.aborted) throw err;
	const appErr = asRec(err);
	const code = asStr(appErr?.code);
	return code !== 'unsupported' && code !== 'not_found'
		? asStr(appErr?.message) || 'Could not load snoozed items. Try again.'
		: undefined;
}

async function fetchSnoozedPage(
	deps: SnoozedAppendDeps,
	path: string,
	cursor: number | undefined,
	signal?: AbortSignal
): Promise<{ items: unknown[]; nextCursor?: number } | undefined> {
	const body = { state: 'snoozed', limit: 100, ...(cursor !== undefined ? { cursor } : {}) };
	const reply = await (signal
		? deps.http.submit(path, body, signal)
		: deps.http.submit(path, body));
	if (!reply || reply.status === 404 || reply.status === 501) return undefined;
	const res = deps.unwrapInboxReply(reply, 'Snoozed items could not be loaded.');
	return { items: Array.isArray(res.items) ? res.items : [], nextCursor: asNum(res.next_cursor) };
}

function mergeSnoozedEntries(
	deps: SnoozedAppendDeps,
	rawEntries: unknown[],
	asOf: string,
	prevByMsgId: Map<string, InboxItem>,
	seenMsgIds: Set<string>,
	mapped: { item: InboxItem; rawSource?: string }[]
): void {
	for (const entry of rawEntries) {
		const rawId = asStr(asRec(entry)?.id);
		const item = deps.mapSnoozedChatInboxItem(
			entry,
			asOf,
			rawId ? prevByMsgId.get(rawId) : undefined
		);
		if (!item) continue;
		const msgId = deps.extractMessageId(item.item.id);
		if (msgId && seenMsgIds.has(msgId)) continue;
		if (msgId) seenMsgIds.add(msgId);
		mapped.push(item);
	}
}

export async function appendSnoozedChatItems(
	deps: SnoozedAppendDeps,
	organizationId: string,
	asOf: string,
	prevEntries: Iterable<{ item: InboxItem }>,
	mapped: { item: InboxItem; rawSource?: string }[],
	signal?: AbortSignal
): Promise<string | undefined> {
	const prevByMsgId = new Map<string, InboxItem>();
	for (const entry of prevEntries) {
		const mid = deps.extractMessageId(entry.item.id);
		if (mid) prevByMsgId.set(mid, entry.item);
	}
	const seenMsgIds = new Set(
		mapped.map((m) => deps.extractMessageId(m.item.id)).filter((id): id is string => Boolean(id))
	);
	const path = `/api/v1/organizations/${encodeURIComponent(organizationId)}/chat/inbox`;
	let cursor: number | undefined;
	try {
		for (let page = 0; page < 20; page += 1) {
			const res = await fetchSnoozedPage(deps, path, cursor, signal);
			if (!res) break;
			mergeSnoozedEntries(deps, res.items, asOf, prevByMsgId, seenMsgIds, mapped);
			if (res.nextCursor === undefined || res.nextCursor === cursor) break;
			if (page === 19) return 'Showing the first 2,000 snoozed items; more exist on the server.';
			cursor = res.nextCursor;
		}
		return undefined;
	} catch (err) {
		return snoozedCatchMessage(err, signal);
	}
}
