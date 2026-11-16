import type { createJsonHttp } from '$lib/platform/http/json-http';
import type { OperationTransport } from '$lib/platform/http/client';
import { NotDispatchedError } from '$lib/platform/http/errors';
import { readOperationId, readSafeInteger } from '$lib/platform/http/wire';
import {
	card,
	completion,
	current,
	id,
	invalid,
	pair,
	record,
	reference,
	revision,
	text,
	type Card,
	type ContentRef,
	type Scope,
	type Target
} from './wire';
export type Queue =
	{ kind: 'all' } | { kind: 'owned' | 'assigned_team' | 'assigned_employee'; targetId: string };
type Http = ReturnType<typeof createJsonHttp>;
type Read = (path: string, signal?: AbortSignal) => Promise<{ status: number; body: unknown }>;
const base = (scope: Scope) =>
	`/api/v1/organizations/${encodeURIComponent(id(scope.organizationId))}/tasks`;
const scopeQuery = (scope: Scope) =>
	new URLSearchParams({ instance_id: id(scope.instanceId), office_id: id(scope.officeId) });
const taskBase = (scope: Scope, task: string) => `${base(scope)}/${encodeURIComponent(id(task))}`;
function result(response: { status: number; body: unknown }, kind: string) {
	const v = record(response.body);
	if (response.status !== 200 || Object.hasOwn(v, 'error'))
		throw new Error(
			response.status === 401
				? 'Sign in again to continue.'
				: 'The selected task service could not complete this read.'
		);
	const r = record(v.result);
	return r.kind === kind ? r : invalid();
}
const list = <T>(v: unknown, decode: (v: unknown) => T) =>
	Array.isArray(v) && v.length <= 50 ? v.map(decode) : invalid();
const next = (v: unknown) => (v === null ? null : id(v));
export function createTaskReads(read: Read, scope: Scope) {
	async function request(path: string, query: URLSearchParams, kind: string, signal?: AbortSignal) {
		return result(await read(`${path}?${query}`, signal), kind);
	}
	return Object.freeze({
		async ready(signal?: AbortSignal) {
			const q = scopeQuery(scope);
			q.set('kind', 'status');
			return revision((await request(base(scope), q, 'status', signal)).directory_generation);
		},
		async tasks(
			queue: Queue,
			after?: string,
			signal?: AbortSignal
		): Promise<{ items: Card[]; next: string | null }> {
			const q = scopeQuery(scope);
			q.set('kind', 'list');
			q.set('queue', queue.kind);
			q.set('limit', '50');
			if (queue.kind !== 'all') q.set('target_id', id(queue.targetId));
			if (after !== undefined) q.set('after', id(after));
			const v = await request(base(scope), q, 'list', signal);
			return { items: list(v.items, card), next: next(v.next) };
		},
		async detail(task: string, signal?: AbortSignal) {
			const v = await request(taskBase(scope, task), scopeQuery(scope), 'detail', signal),
				c = card(v.task);
			if (c.task_id !== task) return invalid();
			return { task: c, historyCoverage: text(v.history_coverage) };
		},
		async targets(
			task: string,
			kind: 'team' | 'employee',
			after?: string,
			signal?: AbortSignal
		): Promise<{ generation: string; items: Target[]; next: string | null }> {
			const q = scopeQuery(scope);
			q.set('target_kind', kind);
			q.set('limit', '50');
			if (after !== undefined) q.set('after', id(after));
			const v = await request(`${taskBase(scope, task)}/assignment-targets`, q, 'targets', signal);
			return {
				generation: revision(v.directory_generation),
				next: next(v.next),
				items: list(v.items, (raw) => {
					const t = record(raw);
					if (typeof t.active !== 'boolean') return invalid();
					return { id: id(t.id), name_preview: text(t.name_preview), active: t.active };
				})
			};
		},
		async history(task: string, after?: string, signal?: AbortSignal) {
			const q = scopeQuery(scope);
			q.set('limit', '50');
			if (after !== undefined) q.set('after', revision(after));
			const v = await request(`${taskBase(scope, task)}/history`, q, 'history', signal);
			const events = list(v.events, reference);
			if (events.some((r) => r.task_id !== task || r.kind !== 'event')) return invalid();
			return {
				events,
				coverage: text(v.coverage),
				next: v.next === null ? null : revision(v.next)
			};
		},
		async content(ref: ContentRef, signal?: AbortSignal) {
			const r = reference(ref),
				bytes = new Uint8Array(r.byte_length);
			let offset = 0;
			do {
				const q = scopeQuery(scope);
				Object.entries({
					revision: r.revision,
					kind: r.kind,
					sha256: r.sha256,
					byte_length: String(r.byte_length),
					offset: String(offset),
					limit: String(256 * 1024)
				}).forEach(([key, value]) => q.set(key, value));
				const v = await request(`${taskBase(scope, r.task_id)}/content`, q, 'chunk', signal);
				if (
					JSON.stringify(reference(v.reference)) !== JSON.stringify(r) ||
					readSafeInteger(v.offset) !== offset
				)
					return invalid();
				const hex = text(v.hex);
				if (
					!/^(?:[0-9a-f]{2})*$/.test(hex) ||
					hex.length > 512 * 1024 ||
					offset + hex.length / 2 > bytes.length
				)
					return invalid();
				for (let i = 0; i < hex.length; i += 2)
					bytes[offset + i / 2] = parseInt(hex.slice(i, i + 2), 16);
				const end = offset + hex.length / 2;
				if (end === bytes.length) {
					if (v.next !== null) return invalid();
				} else if (end <= offset || readSafeInteger(v.next) !== end) return invalid();
				offset = end;
			} while (offset < bytes.length);
			const digest = [...new Uint8Array(await crypto.subtle.digest('SHA-256', bytes))]
				.map((b) => b.toString(16).padStart(2, '0'))
				.join('');
			if (digest !== r.sha256) return invalid();
			// Decoding follows concatenation: a chunk can split a multi-byte scalar.
			return new TextDecoder('utf-8', { fatal: true, ignoreBOM: true }).decode(bytes);
		}
	});
}
export function createAssignmentTransport(
	http: Http,
	scope: Scope,
	task: string
): OperationTransport {
	function bound(
		ref: {
			kind: string;
			scope: { instanceId: string; accountId: string; organizationId?: string; officeId?: string };
		},
		session: { instanceId: string; accountId: string }
	) {
		if (
			ref.kind !== 'work' ||
			ref.scope.instanceId !== scope.instanceId ||
			ref.scope.accountId !== scope.accountId ||
			ref.scope.organizationId !== scope.organizationId ||
			ref.scope.officeId !== scope.officeId ||
			session.instanceId !== scope.instanceId ||
			session.accountId !== scope.accountId
		)
			throw new NotDispatchedError('Reauthenticate the original account.');
	}
	function outcome(
		response: { status: number; body: unknown },
		operationId: string,
		lookup = false
	) {
		const v = record(response.body);
		if (readOperationId(v.operation_id) !== operationId) return invalid();
		switch (v.outcome) {
			case 'completed':
				if (response.status !== 200) return invalid();
				return { operationId, status: 'completed', result: completion(v.result, scope, task) };
			case 'accepted':
				if (response.status !== 202) return invalid();
				return { operationId, status: 'accepted' };
			case 'unknown':
				return { operationId, status: 'unknown' };
			case 'rejected': {
				if (response.status < 400 && !(lookup && response.status === 200)) return invalid();
				const e = record(v.error),
					c = e.current === undefined ? undefined : current(e.current);
				if (
					c &&
					(c.organization_id !== scope.organizationId ||
						c.office_id !== scope.officeId ||
						c.task_id !== task)
				)
					return invalid();
				// Storage/timeout/unavailable after publication cannot prove rejection.
				const code = text(e.code);
				if (['storage_error', 'unavailable', 'not_ready', 'timeout', 'unknown'].includes(code))
					return { operationId, status: 'unknown' };
				return {
					operationId,
					status: 'rejected',
					error: { code, message: text(e.message), ...(c ? { current: c } : {}) }
				};
			}
			default:
				return { operationId, status: 'unsupported' };
		}
	}
	return {
		async submit(request, session) {
			bound(request, session);
			if (
				request.method !== 'task.set_assignment' ||
				request.params.task_id !== task ||
				request.params.change_id !== request.id
			)
				throw new NotDispatchedError();
			const command = record(request.params.command);
			if (
				Object.keys(command).sort().join(',') !== 'assigned_team_id,assignee_id,expected_revision'
			)
				throw new NotDispatchedError();
			const params = { expected_revision: revision(command.expected_revision), ...pair(command) };
			return outcome(
				await http({
					method: 'POST',
					path: `${taskBase(scope, task)}/assignment`,
					proof: session.proof,
					body: {
						operation_id: request.id,
						scope: {
							kind: 'office',
							instance_id: scope.instanceId,
							org_id: scope.organizationId,
							office_id: scope.officeId
						},
						params
					}
				}),
				request.id
			);
		},
		async lookup(ref, session) {
			bound(ref, session);
			const q = new URLSearchParams({
				instance_id: scope.instanceId,
				org_id: scope.organizationId,
				office_id: scope.officeId
			});
			const response = await http({
				method: 'GET',
				path: `/api/v1/operations/work/${encodeURIComponent(readOperationId(ref.id))}?${q}`,
				proof: session.proof
			});
			const v = record(response.body);
			if ('error' in v && response.status === 404 && record(v.error).code === 'not_found')
				return { operationId: ref.id, status: 'missing' };
			if ('error' in v && response.status === 410 && record(v.error).code === 'expired')
				return { operationId: ref.id, status: 'expired' };
			const stored = record(v.result);
			if (stored.kind !== 'work') return invalid();
			return outcome({ status: response.status, body: stored }, ref.id, true);
		}
	};
}
