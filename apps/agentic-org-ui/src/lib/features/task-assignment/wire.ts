/** Selected v1 data boundary. Revisions and public source never cross Number/JSON coercion. */
import { WireDecodeError, readOperationId, readSafeInteger } from '$lib/platform/http/wire';
export type Scope = {
	instanceId: string;
	accountId: string;
	organizationId: string;
	officeId: string;
};
export type Pair = { assigned_team_id: string | null; assignee_id: string | null };
export type ContentRef = {
	task_id: string;
	revision: string;
	kind: 'snapshot' | 'event';
	sha256: string;
	byte_length: number;
};
export type Card = Pair & {
	task_id: string;
	revision: string;
	accountable_team_id: string;
	project_id: string | null;
	number: string | null;
	title_preview: string;
	status: string;
	snapshot: ContentRef;
};
export type Target = { id: string; name_preview: string; active: boolean };
export type Current = Pair & {
	organization_id: string;
	office_id: string;
	task_id: string;
	revision: string;
};
export type Completion = Current & { status: 'completed'; event_sha256: string };
export const invalid = (): never => {
	throw new WireDecodeError();
};
export function record(value: unknown): Record<string, unknown> {
	if (
		!value ||
		typeof value !== 'object' ||
		Array.isArray(value) ||
		![Object.prototype, null].includes(Object.getPrototypeOf(value))
	)
		return invalid();
	return value as Record<string, unknown>;
}
export function text(value: unknown): string {
	return typeof value === 'string' ? value : invalid();
}
export function id(value: unknown): string {
	const v = text(value);
	return v && new TextEncoder().encode(v).length <= 255 ? v : invalid();
}
export function revision(value: unknown): string {
	const v = text(value);
	return /^[1-9][0-9]{0,18}$/.test(v) && BigInt(v) <= 9223372036854775807n ? v : invalid();
}
export const nullableId = (v: unknown) => (v === null ? null : id(v));
export function pair(value: unknown): Pair {
	const v = record(value);
	return {
		assigned_team_id: nullableId(v.assigned_team_id),
		assignee_id: nullableId(v.assignee_id)
	};
}
export function hash(value: unknown): string {
	const v = text(value);
	return /^[0-9a-f]{64}$/.test(v) ? v : invalid();
}
export function reference(value: unknown): ContentRef {
	const v = record(value);
	if (v.kind !== 'event' && v.kind !== 'snapshot') return invalid();
	const byte_length = readSafeInteger(v.byte_length);
	if (byte_length > 16 * 1024 * 1024) return invalid();
	return {
		task_id: id(v.task_id),
		revision: revision(v.revision),
		kind: v.kind,
		sha256: hash(v.sha256),
		byte_length
	};
}
export function card(value: unknown): Card {
	const v = record(value),
		snapshot = reference(v.snapshot),
		task_id = id(v.task_id),
		rev = revision(v.revision);
	if (snapshot.kind !== 'snapshot' || snapshot.task_id !== task_id || snapshot.revision !== rev)
		return invalid();
	return {
		...pair(v),
		task_id,
		revision: rev,
		accountable_team_id: id(v.accountable_team_id),
		project_id: nullableId(v.project_id),
		number: v.number === null ? null : revision(v.number),
		title_preview: text(v.title_preview),
		status: text(v.status),
		snapshot
	};
}
export function current(value: unknown): Current {
	const v = record(value);
	return {
		...pair(v),
		organization_id: id(v.organization_id),
		office_id: id(v.office_id),
		task_id: id(v.task_id),
		revision: revision(v.revision)
	};
}
export function completion(value: unknown, scope: Scope, task?: string): Completion {
	const v = record(value),
		c = current(v);
	if (
		v.status !== 'completed' ||
		c.organization_id !== scope.organizationId ||
		c.office_id !== scope.officeId ||
		(task !== undefined && c.task_id !== task)
	)
		return invalid();
	return { ...c, status: 'completed', event_sha256: hash(v.event_sha256) };
}
export function publicRef(value: unknown): {
	kind: 'work';
	id: string;
	scope: Scope;
	taskId: string;
} {
	const v = record(value),
		s = record(v.scope);
	if (v.kind !== 'work') return invalid();
	return {
		kind: 'work',
		id: readOperationId(v.id),
		taskId: id(v.taskId),
		scope: {
			instanceId: id(s.instanceId),
			accountId: id(s.accountId),
			organizationId: id(s.organizationId),
			officeId: id(s.officeId)
		}
	};
}
