import type { AppError, OrgSnapshot } from '$lib/proto';
import {
	unwrapGatewayResult,
	type GatewayHttpTransport,
	type GatewayReply
} from '$lib/platform/http/gateway-errors';
import type { InboxApi } from '$lib/modules/inbox/api';
import type { InboxItem } from '$lib/modules/inbox/types';
import type { OrgApi } from '$lib/modules/organization/api';
import type { WorkItemsApi } from '$lib/modules/workitems/api';
import { STATUS_WORD, TYPE_WORD, taskHref } from '$lib/modules/workitems/present';
import type { TaskRow } from '$lib/modules/workitems/types';
import type { ActivityApi } from './api';
import { matchesQuery } from './filter';
import type {
	ActivityAction,
	ActivityActor,
	ActivityOptions,
	ActivityPage,
	ActivityQuery,
	ActivityRow,
	ActivityThread
} from './types';

import { asRec, asStr } from '$lib/platform/http/decode';

interface WireConversation {
	id: string;
	kind: string;
	name?: string;
	members: string[];
	projectId?: string;
	teamId?: string;
}

interface WireMessage {
	id: string;
	conversationId: string;
	from: string;
	fromName?: string;
	fromKind?: 'agent' | 'person';
	kind: string;
	body: string;
	at: string;
	threadRootId?: string;
	folded?: boolean;
}

export interface HttpActivityApiDeps {
	http: GatewayHttpTransport;
	orgApi: Pick<OrgApi, 'view'>;
	workItemsApi: Pick<WorkItemsApi, 'queue'>;
	inboxApi: Pick<InboxApi, 'list'>;
	viewerId?: (organizationId: string) => string | undefined;
	subscribe?: (organizationId: string, onEvent: () => void) => () => void;
}

function asIso(v: unknown): string {
	if (typeof v === 'string' && v) return v;
	if (typeof v === 'number' && Number.isFinite(v)) return new Date(v).toISOString();
	return new Date(0).toISOString();
}

function actorMap(snapshot: OrgSnapshot): Map<string, ActivityActor> {
	const map = new Map<string, ActivityActor>();
	for (const m of snapshot.members) {
		map.set(m.id, {
			id: m.id,
			name: m.name,
			kind: m.kind === 'person' ? 'person' : 'agent'
		});
	}
	return map;
}

function resolveActor(
	map: Map<string, ActivityActor>,
	id: string,
	fallback?: ActivityActor
): ActivityActor {
	const known = map.get(id);
	if (known) return known;
	if (fallback && fallback.name && fallback.name !== fallback.id) return fallback;
	if (id === 'local-owner') return { id, name: 'You', kind: 'person' };
	return fallback ?? { id, name: id, kind: 'agent' };
}

function extractMemberIds(raw: unknown): string[] {
	if (!Array.isArray(raw)) return [];
	const out: string[] = [];
	for (const entry of raw) {
		if (typeof entry === 'string') {
			if (entry) out.push(entry);
			continue;
		}
		const rec = asRec(entry);
		const id = asStr(rec?.employee_id) ?? asStr(rec?.id);
		if (id && !asStr(rec?.left_at)) out.push(id);
	}
	return out;
}

function decodeConversation(raw: unknown): WireConversation | undefined {
	const r = asRec(raw);
	const id = asStr(r?.id);
	if (!r || !id) return undefined;
	const auto = asRec(r.auto);
	const projectId =
		asStr(r.project_id) ?? (auto?.type === 'project' ? asStr(auto.project_id) : undefined);
	const teamId = asStr(r.team_id) ?? (auto?.type === 'team' ? asStr(auto.team_id) : undefined);
	const members = Array.isArray(r.members)
		? extractMemberIds(r.members)
		: extractMemberIds(r.participants);
	return {
		id,
		kind: asStr(r.kind) ?? 'dm',
		name: asStr(r.name),
		members,
		...(projectId ? { projectId } : {}),
		...(teamId ? { teamId } : {})
	};
}

function decodeMessage(raw: unknown, fallbackConvId: string): WireMessage | undefined {
	const r = asRec(raw);
	const id = asStr(r?.id);
	if (!r || !id) return undefined;
	const senderRec = asRec(r.from) ?? asRec(r.sender);
	const senderKind = asStr(r.sender_kind);
	const from =
		senderKind === 'office' || senderKind === 'system'
			? 'office'
			: (asStr(r.sender_employee_id) ??
				asStr(r.from) ??
				asStr(r.sender_id) ??
				asStr(senderRec?.id) ??
				asStr(r.sender_display_name) ??
				'unknown');
	const fromName = asStr(r.sender_display_name) ?? asStr(senderRec?.name) ?? asStr(r.sender_name);
	const fromKind =
		senderRec?.kind === 'person' || senderKind === 'person'
			? 'person'
			: senderRec?.kind === 'agent' || senderKind === 'agent' || senderKind === 'employee'
				? 'agent'
				: undefined;
	return {
		id,
		conversationId: asStr(r.conversation_id) ?? fallbackConvId,
		from,
		...(fromName ? { fromName } : {}),
		...(fromKind ? { fromKind } : {}),
		kind: asStr(r.kind) ?? 'msg',
		body: asStr(r.body) ?? '',
		at: asIso(r.created_at ?? r.at ?? r.timestamp_ms),
		threadRootId: asStr(r.thread_root_id) ?? asStr(r.reply_to),
		folded: Boolean(r.folded)
	};
}

async function readChatFeed(
	http: GatewayHttpTransport,
	organizationId: string,
	viewerId?: string
): Promise<{ conversations: WireConversation[]; messages: WireMessage[] }> {
	const base = `/api/v1/organizations/${encodeURIComponent(organizationId)}/chat`;
	const convReply: GatewayReply = await http.read(`${base}/conversations`);
	const convResult = unwrapGatewayResult(convReply, 'Conversations could not be loaded.');
	const rawConvs = Array.isArray(convResult.conversations) ? convResult.conversations : [];
	const conversations = rawConvs
		.map(decodeConversation)
		.filter((c): c is WireConversation => c !== undefined);

	// Only fetch messages for conversations the viewer belongs to (or when viewerId is unknown),
	// avoiding unnecessary 403 responses from restricted/unjoined conversations.
	const readable = viewerId
		? conversations.filter((c) => c.members.length === 0 || c.members.includes(viewerId))
		: conversations;

	const lists = await Promise.all(
		readable.slice(0, 20).map(async (conv) => {
			try {
				const reply = await http.submit(`${base}/messages/list`, {
					conversation: conv.id,
					limit: 30
				});
				const res = unwrapGatewayResult(reply, 'Messages could not be loaded.');
				const items = Array.isArray(res.messages) ? res.messages : [];
				return items
					.map((item) => decodeMessage(item, conv.id))
					.filter((m): m is WireMessage => m !== undefined);
			} catch {
				return [];
			}
		})
	);

	return { conversations, messages: lists.flat() };
}

function taskVerb(row: TaskRow): { action: ActivityAction; verb: string } {
	if (row.status === 'done') return { action: 'task-done', verb: 'completed' };
	if (row.status === 'blocked') return { action: 'task', verb: 'marked blocked' };
	if (row.status === 'in_progress') return { action: 'task', verb: 'is working on' };
	if (row.type === 'bug') return { action: 'bug', verb: 'reported' };
	return { action: 'task', verb: 'updated' };
}

function buildWorkRows(rows: readonly TaskRow[]): ActivityRow[] {
	return rows.map((row) => {
		const actor = row.assignee ?? row.reporter;
		const { action, verb } = taskVerb(row);
		const teamIds = [row.team.id, ...(row.assignedTeam ? [row.assignedTeam.id] : [])];
		const memberIds = [row.reporter.id, ...(row.assignee ? [row.assignee.id] : [])];
		return {
			id: `work:${row.id}:${row.revision}`,
			at: row.updatedAt || row.statusAt || row.createdAt,
			actor: { id: actor.id, name: actor.name, kind: actor.kind },
			kind: 'work',
			action,
			text: `${verb} ${TYPE_WORD[row.type]} ${row.ref} · ${row.title}`,
			preview:
				row.status === 'blocked' && row.blockedReason
					? `Blocked: ${row.blockedReason}`
					: STATUS_WORD[row.status],
			href: { path: taskHref(row.ref), label: row.ref },
			teamIds,
			projectIds: row.project ? [row.project.id] : [],
			memberIds
		};
	});
}

function buildInboxRows(
	items: readonly InboxItem[],
	people: Map<string, ActivityActor>
): ActivityRow[] {
	return items.map((item) => {
		const isDecision = item.kind === 'decision';
		const actor = resolveActor(people, item.who.id, {
			id: item.who.id,
			name: item.who.name,
			kind: item.who.kind
		});
		return {
			id: `inbox:${item.id}`,
			at: item.handled?.at ?? item.created_at,
			actor,
			kind: isDecision ? 'decision' : 'conversation',
			action: isDecision ? 'decision' : 'message',
			text: item.handled?.outcome ? `${item.title} · ${item.handled.outcome}` : item.title,
			preview: item.why || undefined,
			href: item.open ?? { path: `/inbox?item=${encodeURIComponent(item.id)}`, label: 'Inbox' },
			teamIds: item.team ? [item.team.id] : [],
			projectIds: item.project ? [item.project.id] : [],
			memberIds: [actor.id]
		};
	});
}

function conversationTitle(conv: WireConversation, people: Map<string, ActivityActor>): string {
	if (conv.kind === 'channel') return `#${conv.name ?? conv.id}`;
	const names = conv.members.map((id) => resolveActor(people, id).name);
	return names.join(', ') || 'Direct message';
}

function buildChatRows(
	conversations: readonly WireConversation[],
	messages: readonly WireMessage[],
	people: Map<string, ActivityActor>
): ActivityRow[] {
	const convById = new Map(conversations.map((c) => [c.id, c]));
	const recent = [...messages]
		.filter((m) => !m.threadRootId && !m.folded)
		.sort((a, b) => b.at.localeCompare(a.at))
		.slice(0, 80);

	return recent.map((msg) => {
		const conv = convById.get(msg.conversationId);
		const title = conv ? conversationTitle(conv, people) : 'Conversation';
		const actor = resolveActor(
			people,
			msg.from,
			msg.fromName ? { id: msg.from, name: msg.fromName, kind: msg.fromKind ?? 'agent' } : undefined
		);
		const action: ActivityAction = msg.kind === 'announcement' ? 'broadcast' : 'message';
		return {
			id: `chat:${msg.id}`,
			at: msg.at,
			actor,
			kind: 'conversation',
			action,
			text:
				msg.kind === 'announcement' ? `posted an update in ${title}` : `sent a message in ${title}`,
			preview: msg.body.trim().slice(0, 180) || undefined,
			href: {
				path: `/chat?c=${encodeURIComponent(msg.conversationId)}`,
				label: title
			},
			teamIds: conv?.teamId ? [conv.teamId] : [],
			projectIds: conv?.projectId ? [conv.projectId] : [],
			memberIds: conv?.members ?? [msg.from],
			thread: { id: msg.conversationId, with: title }
		};
	});
}

export function createHttpActivityApi(deps: HttpActivityApiDeps): ActivityApi {
	async function collectRows(organizationId: string): Promise<{
		rows: ActivityRow[];
		messaging: 'connected' | 'not-connected';
	}> {
		const { snapshot } = await deps.orgApi.view(organizationId);
		const viewerId = deps.viewerId?.(organizationId) ?? snapshot.members.find((m) => m.owner)?.id;
		const [workRes, inboxRes, chatRes] = await Promise.allSettled([
			deps.workItemsApi.queue(organizationId, { kind: 'organization' }),
			deps.inboxApi.list(organizationId),
			readChatFeed(deps.http, organizationId, viewerId)
		]);

		const people = actorMap(snapshot);
		const rows: ActivityRow[] = [];
		if (workRes.status === 'fulfilled') rows.push(...buildWorkRows(workRes.value.rows));
		if (inboxRes.status === 'fulfilled') rows.push(...buildInboxRows(inboxRes.value, people));
		const messaging = chatRes.status === 'fulfilled' ? 'connected' : 'not-connected';
		if (chatRes.status === 'fulfilled') {
			rows.push(...buildChatRows(chatRes.value.conversations, chatRes.value.messages, people));
		}

		rows.sort((a, b) => b.at.localeCompare(a.at));
		return { rows, messaging };
	}

	return Object.freeze<ActivityApi>({
		async feed(organizationId: string, query: ActivityQuery, limit: number): Promise<ActivityPage> {
			const { rows, messaging } = await collectRows(organizationId);
			const matching = rows.filter((row) => matchesQuery(row, query));
			return {
				rows: matching.slice(0, limit),
				more: Math.max(0, matching.length - limit),
				organizationEmpty: rows.length === 0,
				sources: { messaging, git: 'ok' }
			};
		},

		async options(organizationId: string): Promise<ActivityOptions> {
			const { snapshot } = await deps.orgApi.view(organizationId);
			return {
				teams: [...snapshot.teams]
					.map((t) => ({ id: t.id, name: t.name }))
					.sort((a, b) => a.name.localeCompare(b.name)),
				members: [...snapshot.members]
					.map((m) => ({ id: m.id, name: m.name }))
					.sort((a, b) => a.name.localeCompare(b.name)),
				projects: [...(snapshot.projects ?? [])]
					.filter((p) => !p.archived)
					.map((p) => ({ id: p.id, name: p.name }))
					.sort((a, b) => a.name.localeCompare(b.name))
			};
		},

		async thread(organizationId: string, threadId: string): Promise<ActivityThread> {
			const { snapshot } = await deps.orgApi.view(organizationId);
			const viewerId = deps.viewerId?.(organizationId) ?? snapshot.members.find((m) => m.owner)?.id;
			const chat = await readChatFeed(deps.http, organizationId, viewerId);
			const conv = chat.conversations.find((c) => c.id === threadId);
			if (!conv) {
				throw {
					source: 'api',
					code: 'not_found',
					message: 'That conversation is no longer available.'
				} satisfies AppError;
			}
			const people = actorMap(snapshot);
			const viewer = deps.viewerId?.(organizationId);
			const otherId = conv.members.find((id) => id !== viewer) ?? conv.members[0] ?? 'unknown';
			const messages = chat.messages
				.filter((m) => m.conversationId === threadId)
				.sort((a, b) => a.at.localeCompare(b.at))
				.map((m) => ({
					id: m.id,
					at: m.at,
					from: resolveActor(
						people,
						m.from,
						m.fromName ? { id: m.from, name: m.fromName, kind: m.fromKind ?? 'agent' } : undefined
					),
					body: m.body
				}));
			return {
				id: conv.id,
				counterpart: resolveActor(people, otherId),
				ownerMayReply: Boolean(viewer && conv.members.includes(viewer)),
				messages
			};
		},

		async subscribe(
			organizationId: string,
			onRow: (row: ActivityRow) => void
		): Promise<() => void> {
			if (!deps.subscribe) return () => {};
			const seen = new Set<string>();
			let closed = false;
			let inFlight = false;

			try {
				const initial = await collectRows(organizationId);
				for (const r of initial.rows) seen.add(r.id);
			} catch {
				// Start with empty seen set; first live event will hydrate.
			}

			const stop = deps.subscribe(organizationId, () => {
				if (closed || inFlight) return;
				inFlight = true;
				void collectRows(organizationId)
					.then(({ rows }) => {
						if (closed) return;
						for (const row of rows) {
							if (!seen.has(row.id)) {
								seen.add(row.id);
								onRow(row);
							}
						}
					})
					.catch(() => {})
					.finally(() => {
						inFlight = false;
					});
			});

			return () => {
				closed = true;
				stop();
			};
		}
	});
}
