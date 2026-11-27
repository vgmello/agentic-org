import type {
	AppError,
	ItemKind,
	ItemStatus,
	Priority,
	Resolution,
	Severity,
	WorkItemSummary,
	WorkPosition
} from '$lib/proto';
import {
	unwrapGatewayResult,
	type GatewayHttpTransport,
	type GatewayReply
} from '$lib/platform/http/gateway-errors';
import { unavailableCall } from '$lib/platform/unavailable-api';
import type { WorkApi } from './api';

import { asNum, asRec, asStr, type Rec } from '$lib/platform/http/decode';

const VALID_PRIORITIES = new Set<Priority>(['urgent', 'high', 'normal', 'low']);
const VALID_SEVERITIES = new Set<Severity>(['critical', 'major', 'minor', 'trivial']);
const VALID_STATUSES = new Set<ItemStatus>([
	'todo',
	'in_progress',
	'blocked',
	'done',
	'new',
	'triaged',
	'fixed',
	'closed',
	'routed',
	'scoping',
	'scoped',
	'approved',
	'declined',
	'merged'
]);

function mapItemKind(raw: unknown): ItemKind {
	if (raw === 'task' || raw === 'bug' || raw === 'request') return raw;
	return 'task';
}

function mapStatusAndResolution(
	kind: ItemKind,
	rawStatus: string | undefined
): { status: ItemStatus; resolution?: Resolution } {
	if (rawStatus === 'cancelled') {
		if (kind === 'bug') return { status: 'closed', resolution: 'cancelled' };
		if (kind === 'request') return { status: 'declined', resolution: 'cancelled' };
		return { status: 'closed', resolution: 'cancelled' };
	}
	if (rawStatus && VALID_STATUSES.has(rawStatus as ItemStatus)) {
		return { status: rawStatus as ItemStatus };
	}
	return { status: 'unknown' };
}

function asEnum<T extends string>(v: unknown, valid: ReadonlySet<T>): T | undefined {
	const str = asStr(v);
	return str && valid.has(str as T) ? (str as T) : undefined;
}

function decodeOptionalTaskFields(r: Rec, resolution?: Resolution): Partial<WorkItemSummary> {
	const extra: Partial<WorkItemSummary> = {};
	const number = asNum(r.number);
	const priority = asEnum(r.priority, VALID_PRIORITIES);
	const severity = asEnum(r.severity, VALID_SEVERITIES);
	const projectId = asStr(r.project_id);
	const assigneeId = asStr(r.assignee_id);
	const teamId = asStr(r.team_id);
	const blockedReason = asStr(r.blocked_reason);
	if (number !== undefined) extra.number = number;
	if (resolution) extra.resolution = resolution;
	if (priority) extra.priority = priority;
	if (severity) extra.severity = severity;
	if (projectId) extra.project_id = projectId;
	if (assigneeId) extra.assignee_id = assigneeId;
	if (teamId) extra.team_id = teamId;
	if (blockedReason) extra.blocked_reason = blockedReason;
	return extra;
}

function decodeTaskSummary(raw: unknown): WorkItemSummary | undefined {
	const r = asRec(raw);
	if (!r) return undefined;
	const id = asStr(r.id);
	const key = asStr(r.key);
	const title = asStr(r.title);
	const createdAt = asStr(r.created_at);
	const updatedAt = asStr(r.updated_at);
	if (!id || !key || !title || !createdAt || !updatedAt) return undefined;

	const kind = mapItemKind(r.type);
	const { status, resolution } = mapStatusAndResolution(kind, asStr(r.status));
	return {
		id,
		kind,
		short_id: key,
		title,
		status,
		...decodeOptionalTaskFields(r, resolution),
		created_by: asStr(r.created_by_id) ?? '',
		created_at: createdAt,
		updated_at: updatedAt,
		severity_confirmed: kind === 'bug',
		reopen_count: asNum(r.reopen_count) ?? 0,
		fixes: [],
		fixed_by: [],
		checklist: {
			total: asNum(r.checklist_total) ?? 0,
			done: asNum(r.checklist_done) ?? 0,
			required_left: asNum(r.checklist_required_left) ?? 0
		},
		done_anyway: false,
		owner_office_id: '',
		generation: 1,
		version: asNum(r.revision) ?? 1,
		deleted: false
	};
}

/**
 * Production read-only HTTP adapter for `GET /api/v1/organizations/{org}/tasks`.
 * Maps unified gateway task summaries into `WorkItemSummary` for Home and shell summaries.
 */
export function createHttpWorkApi({ http }: { http: Pick<GatewayHttpTransport, 'read'> }): WorkApi {
	return Object.freeze({
		async snapshot(
			organizationId: string,
			projectId?: string,
			_closedSince?: string,
			signal?: AbortSignal
		): Promise<{ position: WorkPosition; items: WorkItemSummary[] }> {
			const params = new URLSearchParams();
			if (projectId) params.set('project_id', projectId);
			const qs = params.toString();
			const path = `/api/v1/organizations/${encodeURIComponent(organizationId)}/tasks${qs ? `?${qs}` : ''}`;

			let reply: GatewayReply;
			try {
				reply = await (signal ? http.read(path, signal) : http.read(path));
			} catch (err) {
				if (signal?.aborted) throw err;
				throw {
					source: 'api',
					code: 'unavailable',
					message: 'Work items could not be reached.'
				} satisfies AppError;
			}

			const result = unwrapGatewayResult(reply, 'Work items could not be loaded.');
			if (!Array.isArray(result.tasks)) {
				throw {
					source: 'api',
					code: 'unavailable',
					message: 'The tasks response could not be read.'
				} satisfies AppError;
			}

			const items = result.tasks
				.map(decodeTaskSummary)
				.filter((item): item is WorkItemSummary => item !== undefined);
			const seq = items.reduce((max, item) => Math.max(max, item.version), 0);
			return { position: { incarnation: 1, seq }, items };
		},
		get: unavailableCall,
		command: unavailableCall,
		activity: unavailableCall,
		similar: unavailableCall,
		requests: false
	});
}
