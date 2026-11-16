import { describe, expect, it } from 'vitest';
import { createOperationClient } from '$lib/platform/http/client';
import { createJsonHttp } from '$lib/platform/http/json-http';
import { createTaskFeature } from './feature';
import { createTaskReads } from './gateway';
import { revision, type Scope } from './wire';
const scope: Scope = {
	instanceId: 'instance',
	accountId: 'account',
	organizationId: 'org',
	officeId: 'office'
};
const operationId = '01ARZ3NDEKTSV4RRFFQ69G5FAV';
const session = () => ({
	instanceId: scope.instanceId,
	accountId: scope.accountId,
	proof: 'private-proof'
});
const final = (pair: { assigned_team_id: string | null; assignee_id: string | null }) => ({
	status: 'completed',
	organization_id: 'org',
	office_id: 'office',
	task_id: 'opaque task',
	revision: '9223372036854775807',
	...pair,
	event_sha256: 'a'.repeat(64)
});
const response = (value: unknown, status = 200) =>
	new Response(JSON.stringify(value), { status, headers: { 'Content-Type': 'application/json' } });
function fixture(send: typeof fetch, overrides = {}) {
	const stored = new Map<string, string>();
	const storage = {
		getItem: (key: string) => stored.get(key) ?? null,
		setItem: (key: string, v: string) => {
			stored.set(key, v);
		},
		removeItem: (key: string) => {
			stored.delete(key);
		}
	};
	const http = createJsonHttp({
		origin: 'http://localhost',
		fetch: send,
		timeoutMs: 1000,
		maxRequestBytes: 1048576,
		maxResponseBytes: 1048576
	});
	const feature = () =>
		createTaskFeature({
			scope,
			storage,
			http,
			read: (path) => http({ path, method: 'GET', proof: 'private-proof' }),
			operations: (transport) =>
				createOperationClient({ transport, session, clock: { newId: () => operationId } }),
			...overrides
		});
	return { feature, stored, storage };
}
describe('production selected HTTP adapter', () => {
	it('carries every complete pair without Number coercion and persists only its reference before send', async () => {
		for (const assigned_team_id of [null, 'cross-home-team'])
			for (const assignee_id of [null, 'employee']) {
				const desired = { assigned_team_id, assignee_id };
				const { feature, stored } = fixture((async (url, init) => {
					expect(String(url)).toBe(
						'http://localhost/api/v1/organizations/org/tasks/opaque%20task/assignment'
					);
					expect(init?.credentials).toBe('same-origin');
					expect(new Headers(init?.headers).get('X-Org-Session-Proof')).toBe('private-proof');
					expect([...stored.values()][0]).not.toMatch(/proof|expected_revision|assigned_team_id/);
					expect(JSON.parse(String(init?.body))).toEqual({
						operation_id: operationId,
						scope: { kind: 'office', instance_id: 'instance', org_id: 'org', office_id: 'office' },
						params: { expected_revision: '9223372036854775806', ...desired }
					});
					return response({
						outcome: 'completed',
						operation_id: operationId,
						result: final(desired)
					});
				}) as typeof fetch);
				const op = feature().forTask('opaque task').prepare('9223372036854775806', desired);
				expect(await op.submit()).toMatchObject({
					status: 'completed',
					result: { revision: '9223372036854775807', ...desired }
				});
			}
	});
	it('uses lookup alone after lost reply and reload, retaining the exact original conflict state', async () => {
		let sends = 0;
		const { feature, stored } = fixture((async (_url, init) => {
			if (init?.method === 'POST') {
				sends++;
				throw new Error('Lost reply');
			}
			return response({
				result: {
					kind: 'work',
					outcome: 'rejected',
					operation_id: operationId,
					error: {
						code: 'conflict',
						message: 'Revision conflict.',
						current: {
							organization_id: 'org',
							office_id: 'office',
							task_id: 'opaque task',
							revision: '9223372036854775806',
							assigned_team_id: 'old-observed',
							assignee_id: null
						}
					}
				}
			});
		}) as typeof fetch);
		const first = feature()
			.forTask('opaque task')
			.prepare('1', { assigned_team_id: null, assignee_id: null });
		expect(await first.submit()).toEqual({ status: 'unresolved', reason: 'transport' });
		expect(await first.submit()).toEqual(first.state);
		expect(sends).toBe(1);
		const resumed = feature().forTask('opaque task').resume()!;
		expect(resumed.id).toBe(operationId);
		expect(stored.size).toBe(1);
		expect(await resumed.submit()).toMatchObject({ status: 'unresolved' });
		expect(await resumed.lookup()).toMatchObject({
			status: 'rejected',
			error: {
				code: 'conflict',
				current: { revision: '9223372036854775806', assigned_team_id: 'old-observed' }
			}
		});
		expect(sends).toBe(1);
	});
	it('does not call possible published storage errors a definite rejection or repeat a command', async () => {
		let sends = 0;
		const { feature } = fixture((async () => {
			sends++;
			return response(
				{
					outcome: 'rejected',
					operation_id: operationId,
					error: { code: 'storage_error', message: 'Unavailable.' }
				},
				503
			);
		}) as typeof fetch);
		const op = feature()
			.forTask('opaque task')
			.prepare('1', { assigned_team_id: null, assignee_id: null });
		expect(await op.submit()).toEqual({ status: 'unresolved', reason: 'unknown' });
		await op.submit();
		expect(sends).toBe(1);
	});
	it('does not dispatch if durable reference storage fails or recovered scope is changed', () => {
		const { feature, stored } = fixture((async () => {
			throw new Error('Must not send');
		}) as typeof fetch);
		feature().forTask('opaque task').prepare('1', { assigned_team_id: null, assignee_id: null });
		const [key, raw] = [...stored.entries()][0];
		stored.set(key, raw.replace('"account"', '"different-account"'));
		expect(() => feature().forTask('opaque task').resume()).toThrow();
		const failed = fixture(fetch, {
			storage: {
				getItem: () => null,
				setItem: () => {
					throw new Error('Storage full');
				},
				removeItem: () => {}
			}
		});
		expect(() =>
			failed
				.feature()
				.forTask('opaque task')
				.prepare('1', { assigned_team_id: null, assignee_id: null })
		).toThrow('Storage full');
	});
	it('rejects unsafe browser revisions including numeric input', () => {
		for (const value of [
			Number('9223372036854775807'),
			'9223372036854775808',
			'1.0',
			'1e0',
			'+1',
			'01',
			'0',
			'-1'
		])
			expect(() => revision(value)).toThrow();
		expect(revision('9223372036854775807')).toBe('9223372036854775807');
	});
	it('reassembles and verifies raw UTF-8 chunks before decoding public extensions', async () => {
		const original =
			'\ufeff---\nexact: 123456789012345678901234567890.000E+003\n---\ncafé \u{1F980}';
		const bytes = new TextEncoder().encode(original);
		const sha = [...new Uint8Array(await crypto.subtle.digest('SHA-256', bytes))]
			.map((b) => b.toString(16).padStart(2, '0'))
			.join('');
		const reference = {
			task_id: 'opaque task',
			revision: '9223372036854775807',
			kind: 'snapshot' as const,
			sha256: sha,
			byte_length: bytes.length
		};
		const reader = createTaskReads(async (path) => {
			const url = new URL(path, 'http://localhost'),
				offset = Number(url.searchParams.get('offset')),
				end = Math.min(bytes.length, offset + 1);
			return {
				status: 200,
				body: {
					result: {
						kind: 'chunk',
						reference,
						offset,
						hex: [...bytes.subarray(offset, end)]
							.map((b) => b.toString(16).padStart(2, '0'))
							.join(''),
						next: end < bytes.length ? end : null
					}
				}
			};
		}, scope);
		expect(await reader.content(reference)).toBe(original);
		await expect(reader.content({ ...reference, sha256: '0'.repeat(64) })).rejects.toThrow();
	});
});

it('retains an outstanding reference until the exact terminal operation is acknowledged', async () => {
	const desired = { assigned_team_id: null, assignee_id: null };
	const { feature, stored } = fixture((async () =>
		response({
			outcome: 'completed',
			operation_id: operationId,
			result: final(desired)
		})) as typeof fetch);
	const task = feature().forTask('opaque task');
	const operation = task.prepare('1', desired);
	expect(() => task.prepare('1', desired)).toThrow('Reconcile');
	expect(() => task.acknowledge(operation)).not.toThrow(); // Proven not submitted, no request sent.
	const next = task.prepare('1', desired);
	await next.submit();
	expect(stored.size).toBe(1);
	task.acknowledge(next);
	expect(stored.size).toBe(0);
});
