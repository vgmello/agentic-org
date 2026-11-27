import type { AppError } from '$lib/proto';
import { formatUuidV7 } from '$lib/platform/http/clock';
import { readOperationId } from '$lib/platform/http/wire';
import { normalizeRef } from './present';
import type { WirePrivateNote, WireProject, WireTaskView } from './wire-map';
import type { CloseAs, NewTask, TaskField, TaskStatus } from './types';

export interface PendingIds {
	commandId: string;
	entityId: string;
}

export function defaultUuidV7(): string {
	return formatUuidV7(Date.now(), globalThis.crypto.getRandomValues(new Uint8Array(10)));
}

export function normalizeUuidV7(candidate: string, fallback: () => string): string {
	try {
		return readOperationId(candidate);
	} catch {
		return readOperationId(fallback());
	}
}

export function inferTaskField(error: AppError): TaskField | undefined {
	const msg = error.message.toLowerCase();
	if (msg.includes('title')) return 'title';
	if (msg.includes('team')) return 'team';
	if (msg.includes('project')) return 'project';
	if (msg.includes('blocked')) return 'reason';
	if (msg.includes('step')) return 'step';
	return undefined;
}

export function isWireProject(v: unknown): v is WireProject {
	if (!v || typeof v !== 'object') return false;
	const p = v as Partial<WireProject>;
	return (
		typeof p.id === 'string' &&
		typeof p.number === 'number' &&
		typeof p.name === 'string' &&
		typeof p.owner_team_id === 'string' &&
		typeof p.archived === 'boolean'
	);
}

export function isWireTaskView(v: unknown): v is WireTaskView {
	if (!v || typeof v !== 'object') return false;
	const t = v as Partial<WireTaskView>;
	return (
		typeof t.id === 'string' &&
		typeof t.key === 'string' &&
		typeof t.revision === 'number' &&
		typeof t.team_id === 'string' &&
		typeof t.title === 'string' &&
		typeof t.status === 'string'
	);
}

export function isWirePrivateNote(v: unknown): v is WirePrivateNote {
	if (!v || typeof v !== 'object') return false;
	const n = v as Partial<WirePrivateNote>;
	return (
		typeof n.id === 'string' &&
		typeof n.task_id === 'string' &&
		typeof n.author_id === 'string' &&
		typeof n.revision === 'number' &&
		typeof n.body_markdown === 'string' &&
		typeof n.created_at === 'string' &&
		typeof n.updated_at === 'string' &&
		typeof n.editable === 'boolean'
	);
}

function buildCreateDetails(draft: NewTask): Record<string, unknown> | undefined {
	if (draft.type === 'bug' && draft.bug) {
		return {
			what_happened: draft.bug.whatHappened || undefined,
			expected: draft.bug.expected || undefined,
			steps: draft.bug.steps || undefined,
			environment: draft.bug.environment || undefined,
			severity_confirmed: false,
			reopen_count: 0
		};
	}
	if (draft.type === 'request') {
		return {
			request_text: draft.description || undefined,
			severity_confirmed: false,
			reopen_count: 0
		};
	}
	return undefined;
}

export function buildCreateBody(draft: NewTask, ids: PendingIds): Record<string, unknown> {
	const dependsOn = [draft.fixesTaskId, draft.relatesToTaskId].filter(
		(v): v is string => typeof v === 'string' && v.length > 0
	);
	const details = buildCreateDetails(draft);
	const body: Record<string, unknown> = {
		command_id: ids.commandId,
		task_id: ids.entityId,
		type: draft.type,
		title: draft.title,
		body_markdown: draft.description,
		priority: draft.priority
	};
	if (draft.projectId) body.project_id = draft.projectId;
	else body.team_id = draft.teamId;
	if (draft.assignedTeamId) body.assigned_team_id = draft.assignedTeamId;
	if (draft.assigneeId) body.assignee_id = draft.assigneeId;
	if (draft.severity) body.severity = draft.severity;
	if (dependsOn.length > 0) body.depends_on = dependsOn;
	if (details) body.details = details;
	return body;
}

export function buildClosePayload(change: CloseAs): {
	status: TaskStatus;
	resolution: string;
	note?: string;
} {
	if (change.as === 'done') return { status: 'done', resolution: 'completed' };
	if (change.as === 'duplicate') {
		return {
			status: 'cancelled',
			resolution: 'duplicate',
			note: `Duplicate of ${normalizeRef(change.originalRef)}`
		};
	}
	if (change.as === 'wont_fix' || change.as === 'cant_reproduce') {
		return { status: 'cancelled', resolution: change.as, note: change.reason };
	}
	return {
		status: 'cancelled',
		resolution: 'cancelled',
		...(change.reason ? { note: change.reason } : {})
	};
}
