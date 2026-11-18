import { describe, expect, it, vi } from 'vitest';
import { NotDispatchedError } from '../http/errors';
import { createOrganizationsApi, nextPollDelay, UNREACHABLE_LIMIT } from './api';
import {
	accepted,
	completed,
	INSTANCE,
	looked,
	lookedAccepted,
	lookedCompleted,
	notFound,
	OP,
	OP2,
	organization,
	params,
	rejected,
	scripted,
	unknown
} from './testing';

const LOOKUP = `/api/v1/operations/host/${OP}?instance_id=${INSTANCE}`;

describe('create', () => {
	it('posts the instance-scoped envelope with the four fields and nothing else', async () => {
		const s = scripted();
		s.submit.mockResolvedValueOnce(accepted());
		expect(await s.api.create(OP, params)).toEqual({ kind: 'accepted' });
		expect(s.submit.mock.calls).toEqual([
			[
				'/api/v1/organizations',
				{
					operation_id: OP,
					scope: { kind: 'instance', instance_id: INSTANCE },
					params: {
						name: 'Side Project',
						slug: 'side-project',
						organization_repository: {
							kind: 'remote',
							remote_url: 'https://example.test/org/state.git'
						},
						state_branch: 'main'
					}
				},
				undefined
			]
		]);
	});
	it('mints a canonical ULID and refuses anything else before sending', async () => {
		const s = scripted([OP, 'not-a-ulid']);
		expect(s.api.newOperationId()).toBe(OP);
		expect(() => s.api.newOperationId()).toThrow();
		await expect(s.api.create('0193a5c0-0000-7000-8000-000000000009', params)).rejects.toThrow();
		expect(s.submit).not.toHaveBeenCalled();
	});
	it.each([
		['200 completed', completed(), { kind: 'completed', organization }],
		['504 unknown', unknown(), { kind: 'unknown' }],
		[
			'409 already_exists from the host',
			rejected(409, { code: 'already_exists', field: 'name' }),
			{ kind: 'rejected', error: { code: 'already_exists', field: 'name' } }
		],
		[
			'403 not_authorized from the host',
			rejected(403, { code: 'not_authorized' }),
			{ kind: 'rejected', error: { code: 'not_authorized' } }
		],
		[
			'400 from the web process',
			{
				status: 400,
				body: { error: { code: 'invalid_request', message: 'm', field: 'slug', rule: 'format' } }
			},
			{ kind: 'rejected', error: { code: 'invalid_request', field: 'slug', rule: 'format' } }
		],
		[
			'503',
			{ status: 503, body: { error: { code: 'not_ready', message: 'm' } } },
			{ kind: 'unavailable', code: 'not_ready' }
		],
		['503 unreadable', { status: 503, body: 'x' }, { kind: 'unavailable', code: 'unknown' }],
		[
			'410',
			{ status: 410, body: { error: { code: 'expired', message: 'm' } } },
			{ kind: 'expired' }
		],
		[
			'401',
			{ status: 401, body: { error: { code: 'not_authorized', message: 'm' } } },
			{ kind: 'session' }
		]
	])('%s', async (_, reply, expected) => {
		const s = scripted();
		s.submit.mockResolvedValueOnce(reply);
		expect(await s.api.create(OP, params)).toEqual(expected);
	});
	it.each([
		['a completed reply about another operation', completed(OP2)],
		['a 202 that claims completion', { status: 202, body: completed().body }],
		['a 200 without a view', { status: 200, body: { outcome: 'completed', operation_id: OP } }],
		['a 200 that is only accepted', { status: 200, body: accepted().body }],
		['an unreadable refusal', { status: 409, body: { outcome: 'rejected', operation_id: OP } }],
		['a status this route never sends', { status: 500, body: {} }],
		['a 404', notFound]
	])('%s leaves the outcome unknown, never created and never refused', async (_, reply) => {
		const s = scripted();
		s.submit.mockResolvedValueOnce(reply);
		expect(await s.api.create(OP, params)).toEqual({ kind: 'unknown' });
	});
	it('a lost reply is unknown; only a proven non-dispatch is not sent', async () => {
		const s = scripted();
		s.submit
			.mockRejectedValueOnce(new Error('network'))
			.mockRejectedValueOnce(new NotDispatchedError());
		expect(await s.api.create(OP, params)).toEqual({ kind: 'unknown' });
		expect(await s.api.create(OP, params)).toEqual({ kind: 'not_sent' });
	});
	it('a request that cannot be built because the session is gone is not sent', async () => {
		const submit = vi.fn();
		const api = createOrganizationsApi({
			http: { read: vi.fn(), submit },
			instanceId: () => {
				throw new Error('Sign in required');
			},
			newId: () => OP
		});
		expect(await api.create(OP, params)).toEqual({ kind: 'not_sent' });
		expect(submit).not.toHaveBeenCalled();
	});
});

describe('lookup', () => {
	it('asks the host lookup of this instance, with a UUID instance ID as written', async () => {
		const s = scripted();
		s.read.mockResolvedValueOnce(lookedAccepted());
		expect(await s.api.lookup(OP)).toEqual({ kind: 'accepted' });
		expect(s.read.mock.calls[0][0]).toBe(LOOKUP);
	});
	it.each([
		['completed', lookedCompleted(), { kind: 'completed', organization }],
		[
			'rejected',
			looked({
				outcome: 'rejected',
				error: { code: 'already_exists', message: 'm', field: 'slug' }
			}),
			{ kind: 'rejected', error: { code: 'already_exists', field: 'slug' } }
		],
		['404', notFound, { kind: 'missing' }],
		[
			'410',
			{ status: 410, body: { error: { code: 'expired', message: 'm' } } },
			{ kind: 'expired' }
		],
		['401', { status: 401, body: {} }, { kind: 'session' }],
		[
			'503',
			{ status: 503, body: { error: { code: 'unavailable', message: 'm' } } },
			{ kind: 'unreachable' }
		],
		['another operation', lookedCompleted(OP2), { kind: 'unreachable' }],
		['an unreadable body', { status: 200, body: { result: {} } }, { kind: 'unreachable' }]
	])('%s', async (_, reply, expected) => {
		const s = scripted();
		s.read.mockResolvedValueOnce(reply);
		expect(await s.api.lookup(OP)).toEqual(expected);
	});
	it('a failed request is unreachable', async () => {
		const s = scripted();
		s.read.mockRejectedValueOnce(new Error('offline'));
		expect(await s.api.lookup(OP)).toEqual({ kind: 'unreachable' });
	});
});

describe('follow', () => {
	it('waits 1 s, then 1.5 times longer, to at most 10 s', () => {
		const delays: number[] = [];
		for (let i = 0, d: number | undefined; i < 9; i++) delays.push((d = nextPollDelay(d)));
		expect(delays).toEqual([1000, 1500, 2250, 3375, 5063, 7595, 10000, 10000, 10000]);
	});
	it('polls until completed, only ever reading', async () => {
		const s = scripted();
		s.read
			.mockResolvedValueOnce(lookedAccepted())
			.mockResolvedValueOnce(lookedAccepted())
			.mockResolvedValueOnce(lookedCompleted());
		const sleep = vi.fn<(ms: number, signal?: AbortSignal) => Promise<void>>(async () => {});
		const waiting = vi.fn();
		expect(await s.api.follow(OP, { sleep, waiting })).toEqual({ kind: 'completed', organization });
		expect(sleep.mock.calls.map(([ms]) => ms)).toEqual([1000, 1500]);
		expect(waiting.mock.calls).toEqual([['accepted'], ['accepted']]);
		expect(s.read.mock.calls.map(([path]) => path)).toEqual([LOOKUP, LOOKUP, LOOKUP]);
		expect(s.submit).not.toHaveBeenCalled();
	});
	it('stops on a rejected outcome and on no record', async () => {
		const s = scripted();
		s.read
			.mockResolvedValueOnce(
				looked({ outcome: 'rejected', error: { code: 'storage_error', message: 'm' } })
			)
			.mockResolvedValueOnce(notFound);
		const sleep = vi.fn<(ms: number, signal?: AbortSignal) => Promise<void>>(async () => {});
		expect(await s.api.follow(OP, { sleep })).toEqual({
			kind: 'rejected',
			error: { code: 'storage_error' }
		});
		expect(await s.api.follow(OP, { sleep })).toEqual({ kind: 'missing' });
		expect(sleep).not.toHaveBeenCalled();
	});
	it('reports unreachable only after consecutive failures; an answer resets the count', async () => {
		const s = scripted();
		const down = { status: 503, body: {} };
		s.read
			.mockResolvedValueOnce(down)
			.mockResolvedValueOnce(down)
			.mockResolvedValueOnce(lookedAccepted())
			.mockResolvedValue(down);
		const sleep = vi.fn<(ms: number, signal?: AbortSignal) => Promise<void>>(async () => {});
		expect(await s.api.follow(OP, { sleep })).toEqual({ kind: 'unreachable' });
		expect(s.read).toHaveBeenCalledTimes(3 + UNREACHABLE_LIMIT);
	});
	it('stops when aborted, during a wait or a lookup', async () => {
		const s = scripted();
		s.read.mockResolvedValue(lookedAccepted());
		const stop = new AbortController();
		const sleep = vi.fn(async () => {
			stop.abort();
			throw new Error('aborted');
		});
		expect(await s.api.follow(OP, { sleep, signal: stop.signal })).toEqual({ kind: 'stopped' });
		expect(await s.api.follow(OP, { sleep, signal: stop.signal })).toEqual({ kind: 'stopped' });
		expect(s.read).toHaveBeenCalledTimes(1);
	});
});

describe('list and instance', () => {
	it('lists organizations and creates in flight', async () => {
		const s = scripted();
		const creating = [{ operation_id: OP, name: 'Other', slug: 'other' }];
		s.read.mockResolvedValueOnce({
			status: 200,
			body: { result: { organizations: [organization], creating } }
		});
		expect(await s.api.list()).toEqual({
			kind: 'listed',
			list: { organizations: [organization], creating }
		});
		expect(s.read.mock.calls[0][0]).toBe('/api/v1/organizations');
	});
	it.each([
		[{ status: 401, body: {} }, { kind: 'session' }],
		[
			{ status: 503, body: { error: { code: 'unavailable', message: 'm' } } },
			{ kind: 'unreachable' }
		],
		[{ status: 200, body: { result: { organizations: [{ id: 'x' }] } } }, { kind: 'unreachable' }]
	])('never shows a list it could not read: %j', async (reply, expected) => {
		const s = scripted();
		s.read.mockResolvedValueOnce(reply);
		expect(await s.api.list()).toEqual(expected);
	});
	it('reads the instance name, or nothing when it cannot', async () => {
		const s = scripted();
		const view = {
			instance_id: INSTANCE,
			name: 'default',
			server_version: '1.2.3',
			web_status: 'ok',
			listen_scheme: 'http_loopback',
			server_time_ms: 5
		};
		s.read
			.mockResolvedValueOnce({ status: 200, body: { result: view } })
			.mockRejectedValueOnce(new Error('offline'));
		expect(await s.api.instance()).toEqual({
			instance_id: INSTANCE,
			name: 'default',
			server_version: '1.2.3',
			server_time_ms: 5
		});
		expect(await s.api.instance()).toBeUndefined();
	});
});
