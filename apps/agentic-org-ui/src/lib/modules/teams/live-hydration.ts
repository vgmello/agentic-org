import {
	unwrapGatewayResult,
	type GatewayHttpTransport,
	type GatewayReply
} from '$lib/platform/http/gateway-errors';
import { STATUS_WORD, TYPE_WORD } from '$lib/modules/workitems/present';
import type { WorkState } from '$lib/modules/members';
import type { TaskRow } from '$lib/modules/workitems/types';
import type { ActivityEntry, ConversationRow, WorkRow } from './types';

import { asRec, asStr, type Rec } from '$lib/platform/http/decode';

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

export function taskBelongsToTeam(
	row: TaskRow,
	teamId: string,
	memberIds: ReadonlySet<string>
): boolean {
	if (row.team?.id === teamId || row.assignedTeam?.id === teamId) return true;
	return Boolean(row.assignee?.id && memberIds.has(row.assignee.id));
}

export function buildTeamWorkRows(
	rows: readonly TaskRow[],
	teamId: string,
	memberIds: ReadonlySet<string>
): WorkRow[] {
	const out: WorkRow[] = [];
	for (const row of rows) {
		if (!taskBelongsToTeam(row, teamId, memberIds)) continue;
		const state = toWorkState(row);
		if (!state) continue;
		out.push({
			ref: row.ref,
			kind: row.type,
			title: row.title,
			...(row.project?.name ? { project: row.project.name } : {}),
			...(row.assignee?.id ? { assigneeId: row.assignee.id } : {}),
			...(row.assignee?.name ? { assignee: row.assignee.name } : {}),
			priority: toWorkPriority(row.priority),
			state
		});
	}
	return out;
}

export function buildTeamActivity(
	rows: readonly TaskRow[],
	teamId: string,
	memberIds: ReadonlySet<string>
): ActivityEntry[] {
	return rows
		.filter((row) => taskBelongsToTeam(row, teamId, memberIds))
		.map((row) => ({
			id: `team-work:${row.id}:${row.revision}`,
			at: row.updatedAt || row.statusAt || row.createdAt,
			actor: row.assignee?.name ?? row.reporter.name,
			kind: TYPE_WORD[row.type],
			text: `${row.ref} · ${row.title} (${STATUS_WORD[row.status]})`
		}))
		.sort((a, b) => b.at.localeCompare(a.at));
}

function slugify(name: string): string {
	return name
		.toLowerCase()
		.replace(/[^a-z0-9]+/g, '-')
		.replace(/^-+|-+$/g, '');
}

function hasMatchingParticipant(rec: Rec, memberIds: ReadonlySet<string>): boolean {
	const raw = Array.isArray(rec.participant_ids)
		? rec.participant_ids
		: Array.isArray(rec.members)
			? rec.members
			: [];
	return raw.some((item) => {
		if (typeof item === 'string') return memberIds.has(item);
		const id = asStr(asRec(item)?.id ?? asRec(item)?.member_id);
		return Boolean(id && memberIds.has(id));
	});
}

function isTeamConversation(
	rec: Rec,
	teamId: string,
	teamSlug: string,
	memberIds: ReadonlySet<string>
): boolean {
	const auto = asRec(rec.auto);
	if (auto?.type === 'team' && asStr(auto.team_id) === teamId) return true;
	if (asStr(rec.team_id) === teamId) return true;
	const name = asStr(rec.name)?.toLowerCase();
	if (name && teamSlug && (name === teamSlug || name === `team-${teamSlug}`)) return true;
	return hasMatchingParticipant(rec, memberIds);
}

function convKind(raw: string | undefined): ConversationRow['kind'] {
	if (raw === 'channel') return 'channel';
	if (raw === 'direct' || raw === 'dm') return 'direct';
	return 'thread';
}

async function hydrateConversationRow(
	http: GatewayHttpTransport,
	base: string,
	conv: Rec
): Promise<ConversationRow> {
	const id = asStr(conv.id)!;
	const kind = convKind(asStr(conv.kind));
	const rawName = asStr(conv.name) ?? id;
	const title = kind === 'channel' && !rawName.startsWith('#') ? `#${rawName}` : rawName;
	let lastAt = asIso(conv.updated_at ?? conv.created_at);
	let preview = asStr(conv.purpose) ?? (kind === 'channel' ? 'Team channel' : 'Conversation');
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
		// Keep conversation metadata fallback when message list fails.
	}
	return { id, title, kind, lastAt, preview };
}

export async function readTeamConversations(
	http: GatewayHttpTransport,
	organizationId: string,
	teamId: string,
	teamName: string,
	memberIds: ReadonlySet<string>
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

	const teamSlug = slugify(teamName);
	const matched = rawConvs
		.map(asRec)
		.filter((r): r is Rec =>
			Boolean(r && asStr(r.id) && isTeamConversation(r, teamId, teamSlug, memberIds))
		);

	const rows = await Promise.all(matched.map((conv) => hydrateConversationRow(http, base, conv)));
	return rows.sort((a, b) => b.lastAt.localeCompare(a.lastAt));
}
