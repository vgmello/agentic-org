import {
	unwrapGatewayResult,
	type GatewayHttpTransport,
	type GatewayReply
} from '$lib/platform/http/gateway-errors';
import { STATUS_WORD, TYPE_WORD, isOpen } from '$lib/modules/workitems/present';
import type { TaskRow } from '$lib/modules/workitems/types';
import type { ActivityEntry, BugCounts, ConversationRow, TaskCounts } from './types';

import { asRec, asStr, type Rec } from '$lib/platform/http/decode';

export interface ProjectWorkTotals {
	tasks: TaskCounts;
	bugs: BugCounts;
}

function asIso(v: unknown): string {
	if (typeof v === 'string' && v) return v;
	if (typeof v === 'number' && Number.isFinite(v)) return new Date(v).toISOString();
	return new Date(0).toISOString();
}

export function emptyTaskCounts(): TaskCounts {
	return { todo: 0, inProgress: 0, blocked: 0, done: 0, cancelled: 0 };
}

export function emptyBugCounts(): BugCounts {
	return { critical: 0, major: 0, minor: 0, trivial: 0, unset: 0 };
}

function bumpStatus(counts: TaskCounts, status: TaskRow['status']): void {
	if (status === 'todo') counts.todo += 1;
	else if (status === 'in_progress') counts.inProgress += 1;
	else if (status === 'blocked') counts.blocked += 1;
	else if (status === 'done') counts.done += 1;
	else if (status === 'cancelled') counts.cancelled += 1;
}

function bumpBugSeverity(bugs: BugCounts, severity: TaskRow['severity']): void {
	if (severity === 'critical') bugs.critical += 1;
	else if (severity === 'major') bugs.major += 1;
	else if (severity === 'minor') bugs.minor += 1;
	else if (severity === 'trivial') bugs.trivial += 1;
	else bugs.unset += 1;
}

export function aggregateProjectWork(rows: readonly TaskRow[]): Map<string, ProjectWorkTotals> {
	const totals = new Map<string, ProjectWorkTotals>();
	for (const row of rows) {
		const projId = row.project?.id;
		if (!projId) continue;
		let entry = totals.get(projId);
		if (!entry) {
			entry = { tasks: emptyTaskCounts(), bugs: emptyBugCounts() };
			totals.set(projId, entry);
		}
		bumpStatus(entry.tasks, row.status);
		if (row.type === 'bug' && isOpen(row.status)) {
			bumpBugSeverity(entry.bugs, row.severity);
		}
	}
	return totals;
}

export function buildProjectActivity(rows: readonly TaskRow[], projectId: string): ActivityEntry[] {
	return rows
		.filter((row) => row.project?.id === projectId)
		.map((row) => {
			const actor = row.assignee?.name ?? row.reporter.name;
			const statusText = STATUS_WORD[row.status];
			return {
				id: `work:${row.id}:${row.revision}`,
				at: row.updatedAt || row.statusAt || row.createdAt,
				actor,
				kind: TYPE_WORD[row.type],
				text: `${row.ref} · ${row.title} (${statusText})`
			};
		})
		.sort((a, b) => b.at.localeCompare(a.at));
}

function isProjectConversation(rec: Rec, projectId: string, projectSlug: string): boolean {
	const auto = asRec(rec.auto);
	if (auto?.type === 'project' && asStr(auto.project_id) === projectId) return true;
	if (asStr(rec.project_id) === projectId) return true;
	const name = asStr(rec.name)?.toLowerCase();
	return Boolean(name && projectSlug && name === projectSlug.toLowerCase());
}

export async function readProjectConversations(
	http: GatewayHttpTransport,
	organizationId: string,
	projectId: string,
	projectSlug: string
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
		.filter((r): r is Rec =>
			Boolean(r && asStr(r.id) && isProjectConversation(r, projectId, projectSlug))
		);

	const rows = await Promise.all(
		matched.map(async (conv): Promise<ConversationRow> => {
			const id = asStr(conv.id)!;
			const kind = asStr(conv.kind) === 'channel' ? 'channel' : 'thread';
			const title = kind === 'channel' ? `#${asStr(conv.name) ?? id}` : (asStr(conv.name) ?? id);
			let lastAt = asIso(conv.updated_at ?? conv.created_at);
			let preview = asStr(conv.purpose) ?? 'Project channel';
			try {
				const msgReply = await http.submit(`${base}/messages/list`, {
					conversation: id,
					limit: 5
				});
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
				// Keep channel metadata fallback when message list fails.
			}
			return { id, title, kind, lastAt, preview };
		})
	);

	return rows.sort((a, b) => b.lastAt.localeCompare(a.lastAt));
}
