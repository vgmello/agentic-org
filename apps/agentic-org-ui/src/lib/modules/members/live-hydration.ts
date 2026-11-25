import {
	unwrapGatewayResult,
	type GatewayHttpTransport,
	type GatewayReply
} from '$lib/platform/http/gateway-errors';
import { STATUS_WORD, TYPE_WORD, isOpen } from '$lib/modules/workitems/present';
import type { TaskRow } from '$lib/modules/workitems/types';
import type {
	ActivityEntry,
	AssignableTask,
	ConversationRow,
	WorkRef,
	WorkRow,
	WorkState
} from './types';

type Rec = Record<string, unknown>;

const asRec = (v: unknown): Rec | undefined =>
	v && typeof v === 'object' && !Array.isArray(v) ? (v as Rec) : undefined;
const asStr = (v: unknown): string | undefined => (typeof v === 'string' ? v : undefined);

function asIso(v: unknown): string {
	if (typeof v === 'string' && v) return v;
	if (typeof v === 'number' && Number.isFinite(v)) return new Date(v).toISOString();
	return new Date(0).toISOString();
}

function toWorkState(row: TaskRow): WorkState | undefined {
	if (row.status === 'cancelled') return undefined;
	if (row.status === 'in_progress' && row.awaitingVerification) return 'in_review';
	return row.status;
}

function toWorkPriority(priority: TaskRow['priority']): WorkRow['priority'] {
	if (priority === 'urgent') return 'urgent';
	if (priority === 'high') return 'high';
	if (priority === 'low') return 'low';
	return 'medium';
}

export function buildMemberWorkRows(rows: readonly TaskRow[], memberId: string): WorkRow[] {
	const out: WorkRow[] = [];
	for (const row of rows) {
		if (row.assignee?.id !== memberId) continue;
		const state = toWorkState(row);
		if (!state) continue;
		out.push({
			ref: row.ref,
			kind: row.type,
			title: row.title,
			...(row.project?.name ? { project: row.project.name } : {}),
			assigneeId: memberId,
			assignee: row.assignee.name,
			priority: toWorkPriority(row.priority),
			state
		});
	}
	return out;
}

const ACTIVE_RANK: Record<string, number> = {
	in_progress: 0,
	blocked: 1,
	todo: 2
};

export function pickCurrentTask(rows: readonly TaskRow[], memberId: string): WorkRef | undefined {
	const mine = rows
		.filter((r) => r.assignee?.id === memberId && isOpen(r.status))
		.sort((a, b) => {
			const ra = ACTIVE_RANK[a.status] ?? 9;
			const rb = ACTIVE_RANK[b.status] ?? 9;
			if (ra !== rb) return ra - rb;
			return (b.updatedAt || b.createdAt).localeCompare(a.updatedAt || a.createdAt);
		});
	const top = mine[0];
	if (!top) return undefined;
	return {
		ref: top.ref,
		title: top.title,
		...(top.project?.name ? { project: top.project.name } : {})
	};
}

export function buildMemberActivity(rows: readonly TaskRow[], memberId: string): ActivityEntry[] {
	return rows
		.filter((row) => row.assignee?.id === memberId || row.reporter.id === memberId)
		.map((row) => ({
			id: `member-work:${row.id}:${row.revision}`,
			at: row.updatedAt || row.statusAt || row.createdAt,
			actor: row.assignee?.name ?? row.reporter.name,
			kind: TYPE_WORD[row.type],
			text: `${row.ref} · ${row.title} (${STATUS_WORD[row.status]})`
		}))
		.sort((a, b) => b.at.localeCompare(a.at));
}

export function buildAssignableTasks(rows: readonly TaskRow[]): AssignableTask[] {
	return rows
		.filter((row) => isOpen(row.status))
		.map((row) => ({
			ref: row.ref,
			title: row.title,
			...(row.project?.name ? { project: row.project.name } : {})
		}));
}

function hasMemberParticipant(rec: Rec, memberId: string): boolean {
	if (asStr(rec.peer_member_id) === memberId || asStr(rec.member_id) === memberId) return true;
	const raw = Array.isArray(rec.participant_ids)
		? rec.participant_ids
		: Array.isArray(rec.members)
			? rec.members
			: [];
	return raw.some((item) => {
		if (typeof item === 'string') return item === memberId;
		const id = asStr(asRec(item)?.id ?? asRec(item)?.member_id);
		return id === memberId;
	});
}

function convKind(raw: string | undefined): ConversationRow['kind'] {
	if (raw === 'channel') return 'channel';
	if (raw === 'direct' || raw === 'dm') return 'direct';
	return 'thread';
}

async function hydrateConversation(
	http: GatewayHttpTransport,
	base: string,
	conv: Rec
): Promise<ConversationRow> {
	const id = asStr(conv.id)!;
	const kind = convKind(asStr(conv.kind));
	const rawName = asStr(conv.name) ?? id;
	const title = kind === 'channel' && !rawName.startsWith('#') ? `#${rawName}` : rawName;
	let lastAt = asIso(conv.updated_at ?? conv.created_at);
	let preview = asStr(conv.purpose) ?? (kind === 'direct' ? 'Direct conversation' : 'Channel');
	try {
		const msgReply = await http.submit(`${base}/messages/list`, { conversation: id, limit: 5 });
		const msgRes = unwrapGatewayResult(msgReply, 'Messages could not be loaded.');
		const list = Array.isArray(msgRes.messages) ? msgRes.messages : [];
		const latest = list
			.map(asRec)
			.filter((m): m is Rec => Boolean(m))
			.sort((a, b) =>
				asIso(b.created_at ?? b.at ?? b.timestamp_ms).localeCompare(
					asIso(a.created_at ?? a.at ?? a.timestamp_ms)
				)
			)[0];
		if (latest) {
			lastAt = asIso(latest.created_at ?? latest.at ?? latest.timestamp_ms);
			preview = (asStr(latest.body) ?? preview).trim().slice(0, 160);
		}
	} catch {
		// Keep metadata fallback when message list fails.
	}
	return { id, title, kind, lastAt, preview };
}

export async function readMemberConversations(
	http: GatewayHttpTransport,
	organizationId: string,
	memberId: string
): Promise<ConversationRow[]> {
	const base = `/api/v1/organizations/${encodeURIComponent(organizationId)}/chat`;
	let reply: GatewayReply;
	try {
		reply = await http.read(`${base}/conversations`);
	} catch {
		return [];
	}
	let rawConvs: unknown[];
	try {
		const res = unwrapGatewayResult(reply, 'Conversations could not be loaded.');
		rawConvs = Array.isArray(res.conversations) ? res.conversations : [];
	} catch {
		return [];
	}

	const matched = rawConvs
		.map(asRec)
		.filter((r): r is Rec => Boolean(r && asStr(r.id) && hasMemberParticipant(r, memberId)));

	const rows = await Promise.all(matched.map((conv) => hydrateConversation(http, base, conv)));
	return rows.sort((a, b) => b.lastAt.localeCompare(a.lastAt));
}
