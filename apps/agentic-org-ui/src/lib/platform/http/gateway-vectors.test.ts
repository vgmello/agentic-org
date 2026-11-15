import { readFileSync, readdirSync } from 'node:fs';
import { isDeepStrictEqual } from 'node:util';
import { describe, expect, it } from 'vitest';
import * as wire from './wire';
import { createOperationClock } from './clock';
import { NotDispatchedError } from './errors';

const root = new URL('../../../../../../crates/org-proto/test-vectors/v2/', import.meta.url);
const counts = {
	'gateway-envelopes': 17, 'gateway-idempotency': 4, 'gateway-lookup-query': 18, 'gateway-lookups': 26,
	'gateway-mutation-results': 44, 'gateway-outcomes': 35, 'gateway-paths': 17, 'gateway-resource-errors': 3,
	'gateway-session': 8, 'gateway-stream': 15, 'gateway-views': 17
} as const;
type Name = keyof typeof counts;
type Case = Record<string, unknown> & { name: string; expect: Record<string, unknown> };
type Fixture = Record<string, unknown> & { schema_version: number; cases: Case[] };
// These casts describe trusted fixture metadata. Wire values always enter runtime readers.
const record = (value: unknown) => value as Record<string, unknown>;
const list = (value: unknown) => value as unknown[];
const fixtures = Object.fromEntries(Object.keys(counts).map((name) => [name,
	JSON.parse(readFileSync(new URL(`${name}.json`, root), 'utf8'))
])) as Record<Name, Fixture>;
function readCase<T>(c: Case, reader: wire.Reader<T>): T | undefined {
	if (c.expect.valid === false) { expect(() => reader(c.wire)).toThrow(wire.WireDecodeError); return; }
	return reader(c.wire);
}
function normalized<T>(c: Case, reader: wire.Reader<T>): T | undefined {
	const result = readCase(c, reader);
	if (c.expect.valid !== false) expect(result).toEqual(c.expect.normalized);
	return result;
}
const projection = (operation: unknown): wire.Reader<unknown> => {
	if (operation === 'organization.create') return wire.readOrganization;
	if (operation === 'office.restart') return wire.readRestartResult;
	throw new Error('Unknown fixture projection');
};
const views: Record<string, wire.Reader<unknown>> = {
	version: wire.readVersion, instance: wire.readInstance, office: wire.readOffice,
	organization: wire.readOrganization, capabilities: wire.readCapabilities, snapshot: wire.readSnapshot
};

function assertCoreCounts(data: Record<Name, Fixture>): void {
	const revision = data['gateway-lookup-query'].contract_revision;
	if (revision !== undefined && revision !== 1 && revision !== 2) {
		throw new Error('Unsupported gateway fixture contract_revision');
	}
	const pinned = revision === 2
		? { counts: { ...counts, 'gateway-lookup-query': 19 }, total: 205 }
		: { counts, total: 204 };
	for (const name of Object.keys(counts) as Name[]) {
		expect(data[name].schema_version, `${name} schema_version`).toBe(1);
		expect(data[name].cases, `${name} main cases`).toHaveLength(pinned.counts[name]);
	}
	expect(Object.values(data).reduce((sum, fixture) => sum + fixture.cases.length, 0), 'core total').toBe(pinned.total);
}

it('pins core main cases and every landed gateway file', () => {
	expect(readdirSync(root).filter((name) => /^gateway-.*\.json$/.test(name)).sort()).toEqual(Object.keys(counts).map((name) => `${name}.json`).sort());
	assertCoreCounts(fixtures);
});

// Vary only manifest cardinality; canonical wire cases remain unchanged on disk.
function revisionFixtures(revision: unknown, queryCount: number): Record<Name, Fixture> {
	return {
		...fixtures,
		'gateway-lookup-query': {
			...fixtures['gateway-lookup-query'], contract_revision: revision,
			cases: Array.from({ length: queryCount }, () => fixtures['gateway-lookup-query'].cases[0])
		}
	};
}
describe('core fixture revision count gate', () => {
	it.each([
		['legacy', undefined, 18], ['revision 1', 1, 18], ['revision 2', 2, 19]
	] as const)('accepts the fixed %s case set', (_, revision, queryCount) => {
		expect(() => assertCoreCounts(revisionFixtures(revision, queryCount))).not.toThrow();
	});
	it.each([0, 3, '2', null])('refuses unsupported test revision %j', (revision) => {
		expect(() => assertCoreCounts(revisionFixtures(revision, 18))).toThrow('Unsupported gateway fixture contract_revision');
	});
	it.each([[undefined, 19], [1, 19], [2, 18], [2, 20]] as const)(
		'refuses revision %j with %i query cases', (revision, queryCount) => {
			expect(() => assertCoreCounts(revisionFixtures(revision, queryCount))).toThrow('gateway-lookup-query main cases');
		}
	);
	it('does not let another fixture compensate for a missing revision 2 query case', () => {
		const data = revisionFixtures(2, 18);
		data['gateway-views'] = { ...fixtures['gateway-views'], cases: [...fixtures['gateway-views'].cases, fixtures['gateway-views'].cases[0]] };
		expect(() => assertCoreCounts(data)).toThrow('gateway-lookup-query main cases');
	});
	it('keeps schema_version pinned to 1 independently of contract_revision', () => {
		const data = revisionFixtures(2, 19);
		data['gateway-lookup-query'].schema_version = 2;
		expect(() => assertCoreCounts(data)).toThrow('gateway-lookup-query schema_version');
	});
});
for (const name of Object.keys(counts) as Name[]) describe(name, () => {
	for (const c of fixtures[name].cases) it(c.name, () => {
		switch (name) {
			case 'gateway-envelopes': {
				const envelope = readCase(c, wire.readCommandEnvelope);
				if (envelope) {
					expect(wire.scopeMatches(envelope.scope, wire.readScope(c.route_scope))).toBe(c.expect.target_matches);
					if (c.expect.native_params !== undefined) expect(wire.nativeParams(envelope, c.kind as wire.OperationKind)).toEqual(c.expect.native_params);
				}
				break;
			}
			case 'gateway-outcomes': {
				const result = normalized(c, wire.readMutationOutcome);
				if (result) {
					expect(result.operation_id).toBe(record(c.wire).operation_id);
					expect(result.outcome).toBe(c.expect.variant);
					expect(wire.outcomeStatus(result, c.authenticated === true) ?? null).toBe(c.expect.http_status);
					expect(wire.outcomeFinal(result)).toBe(c.expect.final);
				}
				break;
			}
			case 'gateway-lookups': {
				if (c.category === 'cancel') {
					const response = readCase(c, (value) => wire.readResponse(value, wire.readCancelResult));
					if (response) {
						expect('result' in response ? response.result : response.error).toEqual(c.expect.normalized);
						expect(('result' in response ? wire.cancelStatus(response.result) : wire.gatewayErrorStatus(response.error, true)) ?? null).toBe(c.expect.http_status);
					}
					break;
				}
				const response = readCase(c, (value) => wire.readResponse(value, wire.readOperationLookup));
				if (!response) break;
				if ('result' in response) { expect(response.result).toEqual(c.expect.normalized); expect(wire.lookupFinal(response.result)).toBe(c.expect.final); }
				else {
					expect(wire.gatewayErrorStatus(response.error, true) ?? null).toBe(c.expect.http_status);
					expect(response.error.code === 'not_found' ? 'unresolved' : response.error.code).toBe(c.expect.resolution);
				}
				break;
			}
			case 'gateway-idempotency': {
				const a = record(c.first), b = record(c.second);
				const first = wire.readCommandEnvelope(a.wire), second = wire.readCommandEnvelope(b.wire);
				expect(first.operation_id === second.operation_id).toBe(c.expect.same_native_id);
				expect(wire.nativeParams(first, 'org').command_id).toBe(wire.nativeParams(second, 'org').command_id);
				expect(isDeepStrictEqual(first.params, second.params)).toBe(c.expect.same_params);
				expect(a.trusted_account === b.trusted_account).toBe(c.expect.same_account);
				// The worker owns replay. This pins the shared context without a browser registry.
				expect(c.expect.replay).toBe(c.expect.same_params === true && c.expect.same_account === true);
				const response = wire.readMutationOutcome(c.response);
				expect(response).toEqual(c.response);
				expect(wire.outcomeStatus(response, true) ?? null).toBe(c.expect.http_status);
				break;
			}
			case 'gateway-lookup-query': {
				const parse = () => wire.readLookupQuery(c.fields as [string, string][]);
				// Test grammar separately: instance mismatch must not mask a malformed query.
				if (c.expect.parse_valid === false) { expect(parse).toThrow(wire.WireDecodeError); expect(c.expect.valid).toBe(false); break; }
				const query = parse(), scope = () => wire.lookupScope(query, c.kind as wire.OperationKind, c.session_instance as string);
				if (c.expect.valid === false) expect(scope).toThrow(wire.WireDecodeError);
				else { expect(scope()).toEqual(c.expect.scope); expect(query).toEqual(c.expect.normalized); }
				break;
			}
			case 'gateway-paths': expect(wire.validRawPathSegment(c.raw_segment as string)).toBe(c.expect.valid); break;
			case 'gateway-resource-errors': {
				const error = wire.resourceError(c.access as 'missing' | 'hidden' | 'read_only');
				expect({ error }).toEqual(c.wire); expect(wire.gatewayErrorStatus(error, true)).toBe(c.expect.http_status); break;
			}
			case 'gateway-views': {
				const response = normalized(c, (value) => wire.readResponse(value, views[c.category as string]));
				if (response && c.category === 'capabilities' && c.expect.available !== undefined && 'result' in response) {
					expect(wire.readCapabilities(response.result).operations.map((cap) => wire.capabilityAvailable(cap, wire.GATEWAY_OPERATION_NAMES))).toEqual(c.expect.available);
				}
				break;
			}
			case 'gateway-session': {
				if (c.category === 'sign_in') readCase(c, wire.readSignIn);
				else readCase(c, (value) => wire.readResponse(value, c.category === 'established' ? wire.readEstablishedSession : wire.readSession));
				break;
			}
			case 'gateway-stream': normalized(c, c.category === 'poll' ? wire.readPollResponse : wire.readEventFrame as wire.Reader<unknown>); break;
			case 'gateway-mutation-results': {
				const result = projection(c.operation);
				normalized(c, c.response_kind === 'mutation' ? (value) => wire.readMutationOutcome(value, result)
					: (value) => wire.readResponse(value, (value) => wire.readOperationLookup(value, result)) as unknown);
				break;
			}
		}
	});
});

it('uses identical serialized public result bytes in both mutation/lookup pairs', () => {
	const pairs = list(fixtures['gateway-mutation-results'].pairs); expect(pairs).toHaveLength(2);
	for (const item of pairs) {
		const pair = record(item), reader = projection(pair.operation), mutation = wire.readMutationOutcome(pair.mutation, reader);
		const response = wire.readResponse(pair.lookup, (value) => wire.readOperationLookup(value, reader));
		if (mutation.outcome !== 'completed' || !('result' in response) || !('outcome' in response.result) || response.result.outcome !== 'completed') throw new Error('Expected paired completion');
		const bytes = JSON.stringify(mutation.result);
		expect(bytes).toBe(JSON.stringify(response.result.result));
		expect(JSON.stringify(mutation)).toContain(`"result":${bytes}`);
		expect(JSON.stringify(response)).toContain(`"result":${bytes}`);
	}
});
it('continues snapshot/changes/reconnect cursors without gaps or duplicate positions', () => {
	const vectors = fixtures['gateway-stream'], trace = record(vectors.trace), snapshot = wire.readSnapshot(trace.snapshot);
	const request = wire.readStreamRequest(trace.subscribe); expect(request.subscriptions[0].cursor).toBe(snapshot.resume_cursor);
	let previous = snapshot.snapshot.position.seq, cursor = snapshot.resume_cursor; const positions: number[] = [];
	for (const value of list(trace.updates)) {
		const frame = wire.readEventFrame(value); if (frame.type !== 'change') throw new Error('Expected change');
		expect(frame.scope).toEqual(request.subscriptions[0].scope); expect(frame.feed).toBe('organization');
		const position = wire.readPosition(record(frame.data).position);
		expect(position.epoch).toBe(snapshot.snapshot.position.epoch); expect(position.seq).toBe(previous + 1);
		previous = position.seq; cursor = frame.cursor; positions.push(previous);
	}
	expect(positions).toEqual(record(trace.expect).positions); expect(cursor).toBe(record(trace.expect).last_cursor);
	expect(wire.readStreamRequest(trace.reconnect).subscriptions[0].cursor).toBe(cursor);
	expect(wire.readEventFrame(trace.resync)).toMatchObject({ type: 'resync', reason: 'snapshot_required' });
	expect(wire.readPollResponse(vectors.poll)).toEqual(vectors.poll);
});
it('distinguishes an unstored scope refusal from a stored rule-level refusal', () => {
	const vectors = fixtures['gateway-resource-errors'], attempts = list(vectors.attempts); expect(attempts).toHaveLength(2);
	expect(vectors.cases[0].wire).toEqual(vectors.cases[1].wire);
	for (const item of attempts) {
		const trace = record(item), expected = record(trace.expect);
		expect(wire.readMutationOutcome(trace.submit)).toMatchObject({ outcome: 'rejected', error: { code: 'not_found' } });
		const lookedUp = wire.readResponse(trace.lookup, wire.readOperationLookup);
		const resolution = 'error' in lookedUp ? (lookedUp.error.code === 'not_found' ? 'unresolved' : lookedUp.error.code)
			: 'outcome' in lookedUp.result ? lookedUp.result.outcome : 'abandoned';
		expect(resolution).toBe(expected.lookup_if_submit_lost);
		expect(expected.keep_draft).toBe(true); expect(expected.auto_resubmit).toBe(false); expect(expected.new_user_attempt).toBe('new_id');
	}
	const visibility = record(fixtures['gateway-lookup-query'].visibility);
	expect(wire.lookupScope(wire.readLookupQuery(visibility.fields as [string, string][]), 'org', '01M72N8MG00000000000000001').kind).toBe('organization');
	expect({ error: wire.resourceError('hidden') }).toEqual(visibility.hidden); expect({ error: wire.resourceError('missing') }).toEqual(visibility.missing);
	expect(visibility.hidden).toEqual(visibility.missing);
});
it('preserves proof separation and both shared server-offset clock cases', () => {
	const vectors = fixtures['gateway-session']; expect(vectors.proof_header).toBe(wire.SESSION_PROOF_HEADER);
	const response = wire.readResponse(vectors.cases[1].wire, wire.readEstablishedSession); if (!('result' in response)) throw new Error('Expected session');
	const view = wire.readSession(response.result); expect(view).toEqual(record(vectors.cases[2].wire).result);
	for (const key of ['proof', 'session', 'grant', 'channel_id']) expect(view).not.toHaveProperty(key);
	const clock = record(vectors.clock);
	expect(response.result.server_time_ms).toBe(clock.server_time_ms);
	expect((clock.server_time_ms as number) - (clock.browser_time_ms as number)).toBe(clock.expected_offset_ms);
	expect(wire.operationTimeMs(clock.operation_id)).toBe(clock.server_time_ms);
	const clockCases = list(vectors.clock_cases); expect(clockCases).toHaveLength(2);
	for (const item of clockCases) {
		const c = record(item), local = c.browser_time_ms as number, server = c.server_time_ms as number;
		expect(local + (c.expected_offset_ms as number)).toBe(server);
		const corrected = createOperationClock({ now: () => local }); corrected.synchronize(server);
		expect(wire.operationTimeMs(corrected.newId())).toBe(server); expect(wire.operationTimeMs(c.corrected_operation_id)).toBe(server);
		expect(wire.operationTimeMs(c.raw_local_operation_id)).toBe(local);
		expect(wire.gatewayErrorStatus(wire.readGatewayError(c.raw_local_refusal), true)).toBe(record(c.expect).raw_http_status);
	}
});
it('negotiates additive revisions and validates exact nonnegative integer times', () => {
	for (const contract_revision of [2, 99]) expect(wire.versionCompatible(wire.readVersion({ api_version: 1, contract_revision }))).toBe(true);
	for (const value of [{ api_version: 1, contract_revision: 0 }, { api_version: 1, contract_revision: 1 }, { api_version: 2, contract_revision: 2 }]) expect(wire.versionCompatible(wire.readVersion(value))).toBe(false);
	for (const value of [1.5, -1, Number.MAX_SAFE_INTEGER + 1, '1', NaN, Infinity]) expect(() => wire.readSafeInteger(value)).toThrow(wire.WireDecodeError);
	expect(wire.readSafeInteger(1.0)).toBe(1); expect(wire.readSafeInteger(Number.MAX_SAFE_INTEGER)).toBe(Number.MAX_SAFE_INTEGER);
});
it('validates typed create/restart and dedicated empty cancellation requests', () => {
	const first = record(fixtures['gateway-envelopes'].cases[0].wire);
	expect(wire.readCreateRequest(first).params).toEqual({ name: 'Acme' }); expect(() => wire.readCreateRequest({ ...first, params: {} })).toThrow(wire.WireDecodeError);
	expect(wire.readRestartRequest({ ...first, params: {} }).params).toEqual({}); expect(wire.readCancelRequest({})).toEqual({});
	expect(() => wire.readCancelRequest({ operation_id: first.operation_id })).toThrow(wire.WireDecodeError);
});
it('never echoes mistyped credentials or turns decoder failure into dispatch certainty', () => {
	const secret = 'PUBLIC FIXTURE SENSITIVE VALUE';
	for (const attempt of [() => wire.readSignIn({ login: 'alex', password: 3141592653589793 }), () => wire.readScope({ get kind() { throw new NotDispatchedError(secret); } })]) {
		try { attempt(); throw new Error('Expected decoding to fail'); }
		catch (error) {
			expect(error).toBeInstanceOf(wire.WireDecodeError); expect(error).not.toBeInstanceOf(NotDispatchedError);
			expect(String(error)).not.toContain(secret); expect(String(error)).not.toContain('314159'); expect(error).not.toHaveProperty('cause');
		}
	}
});

const populatedSnapshot = () => ({
	position: { epoch: 1, seq: 4 }, teams: [{ id: 'team', name: 'Platform', version: 1 }],
	members: [{ id: 'member', kind: 'agent', name: 'Sam', version: 1,
		agent: { role: 'engineer', title: 'Engineer', messaging_name: 'sam', runtime: { future: true }, status: { future: true } } }],
	memberships: [{ id: 'membership', team_id: 'team', member_id: 'member', version: 1 }],
	projects: [{ id: 'project', name: 'App', version: 1, repo: { default_branch: 'v1', path: '/public/repo', github: { owner: 'org', name: 'app', repo_id: 3 } } }],
	project_teams: [{ id: 'link', project_id: 'project', team_id: 'team', version: 1 }],
	areas: [{ id: 'area', project_id: 'project', name: 'General', version: 1 }]
});
it('validates nonempty domain records, read defaults and unknown runtimes without applying host-result privacy to domain paths', () => {
	const snapshot = wire.readOrgSnapshot(populatedSnapshot());
	expect(snapshot.members[0].agent?.runtime).toBe('unknown');
	expect(snapshot.members[0].agent?.status).toBe('unknown');
	expect(snapshot.projects[0].key).toBe('');
	expect(snapshot.projects[0].repo?.host).toBe('unknown');
	expect(snapshot.projects[0].repo?.github?.private).toBe(false);
	expect(snapshot.projects[0].repo?.path).toBe('/public/repo');
	expect(snapshot.members[0]).not.toHaveProperty('owner');
	expect(snapshot.areas[0]).not.toHaveProperty('general');
	const minimal = wire.readOrgSnapshot({ position: { epoch: 1, seq: 0 }, teams: [], members: [], memberships: [] });
	expect(minimal.projects).toEqual([]); expect(minimal.project_teams).toEqual([]); expect(minimal.areas).toEqual([]);
});
it('rejects unsafe versions and positions at every known snapshot record boundary', () => {
	const unsafe = Number.MAX_SAFE_INTEGER + 1;
	const corruptions = [
		(s: ReturnType<typeof populatedSnapshot>) => { s.position.epoch = unsafe; },
		(s: ReturnType<typeof populatedSnapshot>) => { s.position.seq = unsafe; },
		(s: ReturnType<typeof populatedSnapshot>) => { s.teams[0].version = unsafe; },
		(s: ReturnType<typeof populatedSnapshot>) => { s.members[0].version = unsafe; },
		(s: ReturnType<typeof populatedSnapshot>) => { s.memberships[0].version = unsafe; },
		(s: ReturnType<typeof populatedSnapshot>) => { s.projects[0].version = unsafe; },
		(s: ReturnType<typeof populatedSnapshot>) => { s.projects[0].repo.github.repo_id = unsafe; },
		(s: ReturnType<typeof populatedSnapshot>) => { s.project_teams[0].version = unsafe; },
		(s: ReturnType<typeof populatedSnapshot>) => { s.areas[0].version = unsafe; }
	];
	for (const corrupt of corruptions) {
		const snapshot = populatedSnapshot(); corrupt(snapshot);
		expect(() => wire.readOrgSnapshot(snapshot)).toThrow(wire.WireDecodeError);
	}
});
it('does not hide malformed known frames behind future-scope compatibility', () => {
	const scope = { kind: 'project', project_id: 'future' };
	expect(() => wire.readEventFrame({ type: 'ready', subscriptions: [{ scope, feed: 'organization', cursor: '' }] })).toThrow(wire.WireDecodeError);
	expect(() => wire.readEventFrame({ type: 'change', scope, feed: 'organization', cursor: 'cursor', event: 'change' })).toThrow(wire.WireDecodeError);
});
it('copies prepared params and future scopes instead of retaining mutable network objects', () => {
	const input = { operation_id: '01M72N8MG00000000000000006', scope: { kind: 'instance', instance_id: 'instance' }, params: { nested: { title: 'before' } } };
	const envelope = wire.readCommandEnvelope(input); input.params.nested.title = 'after';
	expect(envelope.params).toEqual({ nested: { title: 'before' } });
	const future = { kind: 'project', metadata: { name: 'before' } }, decoded = wire.readResponseScope(future);
	future.metadata.name = 'after'; expect(decoded).toEqual({ kind: 'project', metadata: { name: 'before' } });
	expect(wire.knownScope(decoded)).toBeUndefined();
});
it('preserves exact integer normalization for negative zero and the pre-move reuse vocabulary', () => {
	expect(wire.readSafeInteger(-0)).toBe(0);
	expect(fixtures['gateway-idempotency'].native_before_move2a_reuse_code).toBe('invalid_request');
});

const terminatorReaders: Record<string, (suffix: string) => unknown> = {
	'operation ID': (suffix) => wire.readOperationId('01M72N8MG00000000000000006' + suffix),
	'lookup ID': (suffix) => wire.readLookupQuery([['instance_id', 'instance' + suffix]]),
	'proof': (suffix) => wire.readEstablishedSession({ ...record(record(fixtures['gateway-session'].cases[1].wire).result), proof: 'ab'.repeat(32) + suffix })
};
for (const [name, read] of Object.entries(terminatorReaders)) {
	it.each(['\n', '\r', '\r\n', '\u2028', '\u2029'])(`${name} refuses trailing terminator %j`, (suffix) => {
		expect(() => read(suffix)).toThrow(wire.WireDecodeError);
	});
}
