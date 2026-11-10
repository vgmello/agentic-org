import { describe, expect, it, vi } from 'vitest';
import { createOperationClock } from './clock';
import { createOperationClient, type OperationRef, type OperationRequest, type Session } from './client';

const id = '01ARZ3NDEKTSV4RRFFQ69G5FAV';
const scope = { instanceId: 'instance', accountId: 'account', organizationId: 'org' };
const decode = (value: unknown): { saved: boolean } => {
	if (!value || typeof value !== 'object' || !('saved' in value) || typeof value.saved !== 'boolean') throw new Error('Invalid result');
	return { saved: value.saved };
};
function setup() {
	let auth: Session | undefined = { ...scope, proof: 'secret-proof' };
	const transport = { submit: vi.fn(async (_request: OperationRequest, _session: Session): Promise<unknown> => ({ operationId: id, status: 'unknown' })), lookup: vi.fn(async (_operation: OperationRef, _session: Session): Promise<unknown> => ({ operationId: id, status: 'missing' })) };
	const newId = vi.fn(() => id);
	const client = createOperationClient({ transport, clock: { newId }, session: () => auth });
	const prepare = () => client.prepare({ kind: 'org', method: 'team.rename', scope, params: { expected_version: 4, name: 'QA' }, decode });
	return { client, transport, newId, prepare, authenticate: (next?: Session) => { auth = next; } };
}

describe('one operation intent', () => {
	it.each(['org', 'work', 'host', 'msg'] as const)('uses the native %s ID, preserves versions and never creates a second ID', async (kind) => {
		const s = setup();
		const field = { org: 'command_id', work: 'change_id', host: 'operation_id', msg: 'id' }[kind];
		const params = { [field]: id, expected_version: 7, nested: { expected_version: 3 } };
		const operation = s.client.prepare({ kind, method: 'edit', scope, params, decode });
		params.expected_version = 99;
		params.nested.expected_version = 99;
		await Promise.all([operation.submit(), operation.submit()]);
		await operation.submit();
		expect(s.transport.submit).toHaveBeenCalledTimes(1);
		expect(s.newId).not.toHaveBeenCalled();
		const [sent, auth] = s.transport.submit.mock.calls[0];
		expect(sent.params).toEqual({ [field]: id, expected_version: 7, nested: { expected_version: 3 } });
		expect(Object.isFrozen(sent.params)).toBe(true);
		expect(Object.isFrozen(sent.params.nested)).toBe(true);
		expect(JSON.stringify(operation)).not.toContain(auth.proof);
	});

	it('resolves a lost response by the same ID, keeps missing unresolved and stops on expiry', async () => {
		const s = setup();
		s.transport.submit.mockRejectedValueOnce(new Error('lost after commit'));
		const operation = s.prepare();
		expect(await operation.submit()).toEqual({ status: 'unresolved', reason: 'transport' });
		expect(await operation.lookup()).toEqual({ status: 'unresolved', reason: 'missing' });
		s.transport.lookup.mockResolvedValueOnce({ operationId: id, status: 'expired' });
		expect(await operation.lookup()).toEqual({ status: 'expired' });
		await operation.submit();
		await operation.lookup();
		expect(s.transport.submit).toHaveBeenCalledTimes(1);
		expect(s.transport.lookup).toHaveBeenCalledTimes(2);
		expect(s.transport.lookup.mock.calls[0]).toEqual([{ kind: 'org', id, scope }, { ...scope, proof: 'secret-proof' }]);
		expect(s.newId).toHaveBeenCalledTimes(1);
	});

	it('distinguishes acceptance from completion and caches only a terminal result', async () => {
		const s = setup();
		s.transport.submit.mockResolvedValueOnce({ operationId: id, status: 'accepted' });
		const operation = s.prepare();
		expect(await operation.submit()).toEqual({ status: 'accepted' });
		s.transport.lookup.mockResolvedValueOnce({ operationId: id, status: 'completed', result: { saved: true } });
		expect(await operation.lookup()).toEqual({ status: 'completed', result: { saved: true } });
		await operation.lookup();
		expect(s.transport.lookup).toHaveBeenCalledTimes(1);
	});

	it('makes operation_reused a terminal conflict and never retries it with a fresh ID', async () => {
		const s = setup();
		s.transport.submit.mockResolvedValueOnce({ operationId: id, status: 'rejected', error: { code: 'operation_reused', message: 'Earlier attempt differs' } });
		const operation = s.prepare();
		expect(await operation.submit()).toMatchObject({ status: 'rejected', error: { code: 'operation_reused', category: 'conflict' } });
		await operation.submit();
		expect(s.transport.submit).toHaveBeenCalledTimes(1);
		expect(s.newId).toHaveBeenCalledTimes(1);
	});

	it.each([
		{ operationId: 'another-id', status: 'completed', result: { saved: true } },
		{ operationId: id, status: 'accepted', result: { saved: true } },
		{ operationId: id, status: 'completed', result: { saved: 'bad' } },
		{ operationId: id, status: 'rejected', result: {}, error: { code: 'conflict', message: 'No' } }
	])('never converts malformed/foreign replies into confirmed results: %j', async (reply) => {
		const s = setup();
		s.transport.submit.mockResolvedValueOnce(reply);
		expect(await s.prepare().submit()).toEqual({ status: 'unresolved', reason: 'invalid_response' });
	});

	it('shows an unsupported state for a future outcome instead of success', async () => {
		const s = setup();
		s.transport.submit.mockResolvedValueOnce({ operationId: id, status: 'future', result: {} });
		expect(await s.prepare().submit()).toEqual({ status: 'unsupported' });
	});

	it.each([
		['not_authorized', { status: 'unresolved', reason: 'authorization' }],
		['clock_skew', { status: 'unresolved', reason: 'clock' }],
		['unknown', { status: 'unresolved', reason: 'unknown' }],
		['unsupported_version', { status: 'unsupported' }],
		['expired', { status: 'expired' }]
	])('does not turn %s admission failure into a stored business rejection', async (code, expected) => {
		const s = setup();
		s.transport.submit.mockResolvedValueOnce({ operationId: id, status: 'rejected', error: { code, message: 'Refused' } });
		expect(await s.prepare().submit()).toEqual(expected);
	});

	it('requires the original account, permits same-account lookup after reauth and never resends', async () => {
		const s = setup();
		const operation = s.prepare();
		await operation.submit();
		s.authenticate({ ...scope, accountId: 'different', proof: 'different-proof' });
		await expect(operation.lookup()).rejects.toThrow('original account');
		s.authenticate({ ...scope, proof: 'new-proof' });
		await operation.submit();
		await operation.lookup();
		expect(s.transport.submit).toHaveBeenCalledTimes(1);
		expect(s.transport.lookup.mock.calls[0][1]).toMatchObject({ proof: 'new-proof' });
	});

	it('does not dispatch before authentication or automatically submit when authentication returns', async () => {
		const s = setup();
		const operation = s.prepare();
		s.authenticate();
		await expect(operation.submit()).rejects.toThrow('Reauthenticate');
		expect(operation.state).toEqual({ status: 'not_submitted' });
		s.authenticate({ ...scope, proof: 'renewed-proof' });
		expect(s.transport.submit).not.toHaveBeenCalled();
		await operation.submit();
		expect(s.transport.submit).toHaveBeenCalledTimes(1);
	});

	it('discards a late result when the active session changes while the call is in flight', async () => {
		const s = setup();
		let finish!: (reply: unknown) => void;
		s.transport.submit.mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
		const operation = s.prepare();
		const pending = operation.submit();
		s.authenticate();
		finish({ operationId: id, status: 'completed', result: { saved: true } });
		expect(await pending).toEqual({ status: 'unresolved', reason: 'session_changed' });
	});

	it('does not return a cached completed result to another active account', async () => {
		const s = setup();
		s.transport.submit.mockResolvedValueOnce({ operationId: id, status: 'completed', result: { saved: true } });
		const operation = s.prepare();
		await operation.submit();
		s.authenticate({ ...scope, accountId: 'different', proof: 'different-proof' });
		expect(operation.state).toEqual({ status: 'unresolved', reason: 'session_changed' });
		await expect(operation.lookup()).rejects.toThrow('original account');
		await expect(operation.submit()).rejects.toThrow('original account');
		expect(s.transport.submit).toHaveBeenCalledTimes(1);
	});

	it('resumes lookup with an immutable reference, without recreating a request body', async () => {
		const s = setup();
		const ref = { kind: 'org' as const, id, scope: { ...scope } };
		const operation = s.client.resume(ref, decode);
		ref.id = '01ARZ3NDEKTSV4RRFFQ69G5FAW';
		ref.scope.accountId = 'changed';
		s.transport.lookup.mockResolvedValueOnce({ operationId: id, status: 'completed', result: { saved: true } });
		expect(await operation.lookup()).toEqual({ status: 'completed', result: { saved: true } });
		await operation.submit();
		expect(s.transport.submit).not.toHaveBeenCalled();
		expect(s.newId).not.toHaveBeenCalled();
	});

	it('rejects non-JSON/invalid IDs before dispatch and does not globally serialize independent intents', async () => {
		const s = setup();
		expect(() => s.client.prepare({ kind: 'org', method: 'edit', scope, params: { expected_version: NaN }, decode })).toThrow('JSON');
		expect(() => s.client.resume({ kind: 'org', id: 'bad', scope }, decode)).toThrow('ID');
		let finish!: (reply: unknown) => void;
		s.transport.submit.mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
		const first = s.prepare().submit();
		await s.client.prepare({ kind: 'work', method: 'edit', scope, params: { change_id: id }, decode }).submit();
		expect(s.transport.submit).toHaveBeenCalledTimes(2);
		finish({ operationId: id, status: 'unknown' });
		await first;
	});
});

describe('server-clock ULIDs', () => {
	it('requires synchronization, uses server offset and keeps 80 random bits', () => {
		let local = 9_000_000;
		const bytes = new Uint8Array(10).fill(255);
		const clock = createOperationClock({ now: () => local, random: () => bytes });
		expect(() => clock.newId()).toThrow('Synchronize');
		clock.synchronize(1000);
		local += 123;
		const value = clock.newId();
		const alphabet = '0123456789ABCDEFGHJKMNPQRSTVWXYZ';
		const time = [...value.slice(0, 10)].reduce((n, char) => n * 32 + alphabet.indexOf(char), 0);
		expect(time).toBe(1123);
		expect(value.slice(10)).toBe('Z'.repeat(16));
		expect(() => clock.synchronize(NaN)).toThrow('server_time_ms');
		expect(() => clock.synchronize(2 ** 48)).toThrow('server_time_ms');
	});
});
