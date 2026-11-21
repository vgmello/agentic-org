import { describe, expect, it, vi } from 'vitest';
import { NotDispatchedError } from './errors';
import {
	EVENTS_STREAM_PATH,
	EventStreamError,
	createEventStreamClient,
	readSseFrames,
	type EventFrame,
	type Subscription
} from './events';
import { SESSION_PROOF_HEADER, WireDecodeError } from './wire';

const ORIGIN = 'https://org.example.test';
const PROOF = 'a'.repeat(64);
const ORG_SCOPE = {
	kind: 'organization' as const,
	instance_id: '01M72N8MG00000000000000001',
	org_id: '01M72N8MG00000000000000003'
};
const OFFICE_SCOPE = {
	kind: 'office' as const,
	instance_id: '01M72N8MG00000000000000001',
	org_id: '01M72N8MG00000000000000003',
	office_id: '01M72N8MG00000000000000005'
};
const SUBS: Subscription[] = [{ scope: ORG_SCOPE, feed: 'organization' }];

const enc = new TextEncoder();
const sseLine = (frame: unknown) => `event: frame\ndata: ${JSON.stringify(frame)}\n\n`;

function streamFromChunks(chunks: (string | Uint8Array)[]): ReadableStream<Uint8Array> {
	let i = 0;
	return new ReadableStream<Uint8Array>({
		pull(controller) {
			if (i >= chunks.length) {
				controller.close();
				return;
			}
			const next = chunks[i++];
			controller.enqueue(typeof next === 'string' ? enc.encode(next) : next);
		}
	});
}

describe('readSseFrames', () => {
	it('parses frames split across arbitrary chunk boundaries, CRLF endings, comments, and multi-byte UTF-8', async () => {
		const ready = sseLine({
			type: 'ready',
			subscriptions: [{ scope: ORG_SCOPE, feed: 'organization', cursor: 'org:1:live' }]
		});
		const change = sseLine({
			type: 'change',
			scope: ORG_SCOPE,
			feed: 'organization',
			cursor: 'org:1:2',
			event: 'team.created',
			data: { name: 'Inês — 東京' }
		}).replaceAll('\n', '\r\n');
		const bytes = enc.encode(`: keepalive comment\n\n${ready}${change}`);
		// Slice into tiny 7-byte chunks to split both CRLF pairs and multi-byte UTF-8 characters.
		const chunks: Uint8Array[] = [];
		for (let i = 0; i < bytes.length; i += 7) chunks.push(bytes.slice(i, i + 7));

		const frames: EventFrame[] = [];
		for await (const frame of readSseFrames(streamFromChunks(chunks))) frames.push(frame);

		expect(frames).toHaveLength(2);
		expect(frames[0]).toEqual({
			type: 'ready',
			subscriptions: [{ scope: ORG_SCOPE, feed: 'organization', cursor: 'org:1:live' }]
		});
		expect(frames[1]).toEqual({
			type: 'change',
			scope: ORG_SCOPE,
			feed: 'organization',
			cursor: 'org:1:2',
			event: 'team.created',
			data: { name: 'Inês — 東京' }
		});
	});

	it('rejects malformed JSON and invalid EventFrame payloads with WireDecodeError', async () => {
		const badJson = streamFromChunks(['event: frame\ndata: {not json}\n\n']);
		const drain = async (stream: ReadableStream<Uint8Array>, opts?: { maxEventBytes?: number }) => {
			const frames: EventFrame[] = [];
			for await (const frame of readSseFrames(stream, opts)) frames.push(frame);
			return frames;
		};
		await expect(drain(badJson)).rejects.toBeInstanceOf(WireDecodeError);

		const badFrame = streamFromChunks([
			sseLine({ type: 'heartbeat', server_time: '2026-11-21T00:00:00Z' })
		]);
		await expect(drain(badFrame)).rejects.toBeInstanceOf(WireDecodeError);
	});

	it('handles CR + empty chunk + LF without flushing early, joins multi-line data, and ignores non-frame events', async () => {
		const splitCrlf = [
			'event: ignored_custom\n',
			'data: {"not":"a frame"}\n\n',
			'event: frame\r',
			new Uint8Array(0),
			'\ndata: {"type":"heartbeat",\r',
			new Uint8Array(0),
			'\ndata: "server_time_ms":42}\r',
			new Uint8Array(0),
			'\n\r',
			new Uint8Array(0),
			'\n'
		];
		const frames: EventFrame[] = [];
		for await (const frame of readSseFrames(streamFromChunks(splitCrlf))) frames.push(frame);
		expect(frames).toEqual([{ type: 'heartbeat', server_time_ms: 42 }]);
	});

	it('enforces maxEventBytes using UTF-8 byte length against oversized SSE frames', async () => {
		const multiByte = streamFromChunks([
			sseLine({
				type: 'change',
				scope: ORG_SCOPE,
				feed: 'organization',
				cursor: 'c1',
				event: 'x',
				// 15 CJK chars = 45 UTF-8 bytes, total payload ~120 chars / ~150 bytes
				data: { blob: '東'.repeat(15) }
			})
		]);
		const drain = async (stream: ReadableStream<Uint8Array>, maxEventBytes: number) => {
			const frames: EventFrame[] = [];
			for await (const frame of readSseFrames(stream, { maxEventBytes })) frames.push(frame);
			return frames;
		};
		// Limit 160 exceeds UTF-16 char length of data line (~155) only if char count were used;
		// UTF-8 byte length of the data line is ~185 bytes so it must reject.
		await expect(drain(multiByte, 160)).rejects.toThrow('SSE event exceeds its byte limit');
	});
});

describe('createEventStreamClient', () => {
	it('POSTs /api/v1/events/stream with proof header, dispatches every frame type, tracks cursors, and ends on session_ended', async () => {
		const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
			expect(String(input)).toBe(`${ORIGIN}${EVENTS_STREAM_PATH}`);
			expect(init?.method).toBe('POST');
			expect(init?.mode).toBe('same-origin');
			expect(init?.credentials).toBe('same-origin');
			expect(init?.redirect).toBe('error');
			expect(init?.cache).toBe('no-store');
			const headers = new Headers(init?.headers);
			expect(headers.get('accept')).toBe('text/event-stream');
			expect(headers.get('content-type')).toBe('application/json');
			expect(headers.get(SESSION_PROOF_HEADER)).toBe(PROOF);
			expect(JSON.parse(String(init?.body))).toEqual({ subscriptions: SUBS });

			const payload = [
				sseLine({
					type: 'ready',
					subscriptions: [{ scope: ORG_SCOPE, feed: 'organization', cursor: 'org:3:0' }]
				}),
				sseLine({
					type: 'change',
					scope: ORG_SCOPE,
					feed: 'organization',
					cursor: 'org:3:1',
					event: 'team.created',
					data: { team_id: 't1' }
				}),
				sseLine({ type: 'heartbeat', server_time_ms: 1794312000000 }),
				sseLine({
					type: 'resync',
					scope: ORG_SCOPE,
					feed: 'organization',
					reason: 'slow_consumer'
				}),
				sseLine({
					type: 'unavailable',
					scope: OFFICE_SCOPE,
					feed: 'work',
					error: { code: 'unavailable', message: 'Office offline.' }
				}),
				sseLine({ type: 'future_event_kind', extra: 1 }),
				sseLine({ type: 'session_ended', reason: 'session_ended' })
			];

			return new Response(streamFromChunks(payload), {
				status: 200,
				headers: {
					'content-type': 'text/event-stream; charset=utf-8',
					'cache-control': 'no-store'
				}
			});
		});

		const client = createEventStreamClient({
			origin: ORIGIN,
			fetch: fetchMock as unknown as typeof fetch
		});
		const seen: string[] = [];
		const handle = client.open({
			subscriptions: SUBS,
			proof: PROOF,
			handlers: {
				onFrame: (f) => {
					seen.push(`frame:${f.type}`);
				},
				onReady: (f) => {
					seen.push(`ready:${f.subscriptions[0].cursor}`);
				},
				onChange: (f) => {
					seen.push(`change:${f.event}:${f.cursor}`);
				},
				onHeartbeat: (f) => {
					seen.push(`heartbeat:${f.server_time_ms}`);
				},
				onResync: (f) => {
					seen.push(`resync:${f.reason}`);
				},
				onUnavailable: (f) => {
					seen.push(`unavailable:${f.error.code}`);
				},
				onUnsupported: () => {
					seen.push('unsupported');
				},
				onSessionEnded: (f) => {
					seen.push(`session_ended:${f.reason}`);
				}
			}
		});

		const result = await handle.closed;
		expect(result.reason).toBe('session_ended');
		expect(handle.cursor(ORG_SCOPE, 'organization')).toBe('org:3:1');
		expect(seen).toEqual([
			'frame:ready',
			'ready:org:3:0',
			'frame:change',
			'change:team.created:org:3:1',
			'frame:heartbeat',
			'heartbeat:1794312000000',
			'frame:resync',
			'resync:slow_consumer',
			'frame:unavailable',
			'unavailable:unavailable',
			'frame:unsupported',
			'unsupported',
			'frame:session_ended',
			'session_ended:session_ended'
		]);
	});

	it('stops cleanly when aborted via AbortSignal or handle.close()', async () => {
		let cancelled = false;
		const stream = new ReadableStream<Uint8Array>({
			start(controller) {
				controller.enqueue(
					enc.encode(
						sseLine({
							type: 'ready',
							subscriptions: [{ scope: ORG_SCOPE, feed: 'organization', cursor: 'org:3:live' }]
						})
					)
				);
			},
			cancel() {
				cancelled = true;
			}
		});
		const fetchMock = vi.fn(
			async () =>
				new Response(stream, {
					status: 200,
					headers: { 'content-type': 'text/event-stream; charset=utf-8' }
				})
		);

		const abort = new AbortController();
		const client = createEventStreamClient({
			origin: ORIGIN,
			fetch: fetchMock as unknown as typeof fetch
		});
		const handle = client.open({
			subscriptions: SUBS,
			proof: PROOF,
			signal: abort.signal,
			handlers: {
				onReady: () => abort.abort()
			}
		});

		const result = await handle.closed;
		expect(result.reason).toBe('aborted');
		expect(handle.cursor(ORG_SCOPE, 'organization')).toBe('org:3:live');
		expect(cancelled).toBe(true);

		let closeCancelled = false;
		const closeStream = new ReadableStream<Uint8Array>({
			start(controller) {
				controller.enqueue(
					enc.encode(
						sseLine({
							type: 'ready',
							subscriptions: [{ scope: ORG_SCOPE, feed: 'organization', cursor: 'org:3:close' }]
						})
					)
				);
			},
			cancel() {
				closeCancelled = true;
			}
		});
		const closeClient = createEventStreamClient({
			origin: ORIGIN,
			fetch: vi.fn(
				async () =>
					new Response(closeStream, {
						status: 200,
						headers: { 'content-type': 'text/event-stream; charset=utf-8' }
					})
			) as unknown as typeof fetch
		});
		const handleRef = closeClient.open({
			subscriptions: SUBS,
			proof: PROOF,
			handlers: {
				onReady: () => handleRef.close()
			}
		});
		const closeResult = await handleRef.closed;
		expect(closeResult.reason).toBe('aborted');
		expect(handleRef.cursor(ORG_SCOPE, 'organization')).toBe('org:3:close');
		expect(closeCancelled).toBe(true);
	});

	it('does not swallow handler errors or WireDecodeError even when aborted', async () => {
		const handlerAbort = new AbortController();
		const client = createEventStreamClient({
			origin: ORIGIN,
			fetch: vi.fn(
				async () =>
					new Response(
						streamFromChunks([
							sseLine({
								type: 'ready',
								subscriptions: [{ scope: ORG_SCOPE, feed: 'organization', cursor: 'c0' }]
							})
						]),
						{ status: 200, headers: { 'content-type': 'text/event-stream' } }
					)
			) as unknown as typeof fetch
		});
		const handlerBoom = new Error('handler exploded');
		const h = client.open({
			subscriptions: SUBS,
			proof: PROOF,
			signal: handlerAbort.signal,
			handlers: {
				onReady: () => {
					handlerAbort.abort();
					throw handlerBoom;
				}
			}
		});
		await expect(h.closed).rejects.toBe(handlerBoom);

		const badClient = createEventStreamClient({
			origin: ORIGIN,
			fetch: vi.fn(
				async () =>
					new Response(streamFromChunks(['event: frame\ndata: {bad\n\n']), {
						status: 200,
						headers: { 'content-type': 'text/event-stream' }
					})
			) as unknown as typeof fetch
		});
		await expect(
			badClient.open({ subscriptions: SUBS, proof: PROOF }).closed
		).rejects.toBeInstanceOf(WireDecodeError);
	});

	it('surfaces non-200 HTTP errors (JSON, non-JSON, oversized JSON) and rejects non-SSE 200 responses', async () => {
		const authFail = vi.fn(
			async () =>
				new Response(
					JSON.stringify({
						error: {
							code: 'not_authorized',
							message: 'Session expired.',
							session_reason: 'session_ended'
						}
					}),
					{ status: 401, headers: { 'content-type': 'application/json' } }
				)
		);
		const client = createEventStreamClient({
			origin: ORIGIN,
			fetch: authFail as unknown as typeof fetch
		});
		await expect(client.open({ subscriptions: SUBS, proof: PROOF }).closed).rejects.toMatchObject({
			name: 'EventStreamError',
			status: 401,
			gatewayError: {
				code: 'not_authorized',
				message: 'Session expired.',
				session_reason: 'session_ended'
			}
		});

		const plainFail = vi.fn(
			async () =>
				new Response('Bad Gateway', { status: 502, headers: { 'content-type': 'text/plain' } })
		);
		const plainClient = createEventStreamClient({
			origin: ORIGIN,
			fetch: plainFail as unknown as typeof fetch
		});
		await expect(
			plainClient.open({ subscriptions: SUBS, proof: PROOF }).closed
		).rejects.toMatchObject({
			name: 'EventStreamError',
			status: 502,
			gatewayError: undefined
		});

		const hugeJsonFail = vi.fn(
			async () =>
				new Response(JSON.stringify({ error: { code: 'unavailable', message: 'x'.repeat(256) } }), {
					status: 503,
					headers: { 'content-type': 'application/json' }
				})
		);
		const boundedClient = createEventStreamClient({
			origin: ORIGIN,
			maxErrorBytes: 64,
			fetch: hugeJsonFail as unknown as typeof fetch
		});
		await expect(
			boundedClient.open({ subscriptions: SUBS, proof: PROOF }).closed
		).rejects.toMatchObject({
			name: 'EventStreamError',
			status: 503,
			gatewayError: undefined
		});

		const wrongType = vi.fn(
			async () =>
				new Response('{}', { status: 200, headers: { 'content-type': 'application/json' } })
		);
		const client2 = createEventStreamClient({
			origin: ORIGIN,
			fetch: wrongType as unknown as typeof fetch
		});
		await expect(client2.open({ subscriptions: SUBS, proof: PROOF }).closed).rejects.toBeInstanceOf(
			EventStreamError
		);
	});

	it('refuses invalid proofs, empty/oversized/unknown-feed subscriptions, proof leakage, and pre-aborted signals before dispatch', () => {
		const fetchMock = vi.fn();
		const client = createEventStreamClient({
			origin: ORIGIN,
			fetch: fetchMock as unknown as typeof fetch
		});

		expect(() => client.open({ subscriptions: [], proof: PROOF })).toThrow(NotDispatchedError);
		expect(() =>
			client.open({
				subscriptions: Array.from({ length: 33 }, () => SUBS[0]),
				proof: PROOF
			})
		).toThrow(NotDispatchedError);
		expect(() =>
			client.open({
				subscriptions: [{ scope: ORG_SCOPE, feed: 'unknown' as unknown as Subscription['feed'] }],
				proof: PROOF
			})
		).toThrow(NotDispatchedError);
		expect(() => client.open({ subscriptions: SUBS, proof: 'bad proof with spaces' })).toThrow(
			NotDispatchedError
		);
		expect(() =>
			client.open({
				subscriptions: [{ scope: ORG_SCOPE, feed: 'organization', cursor: PROOF }],
				proof: PROOF
			})
		).toThrow(NotDispatchedError);

		const preAborted = new AbortController();
		preAborted.abort();
		expect(() =>
			client.open({ subscriptions: SUBS, proof: PROOF, signal: preAborted.signal })
		).toThrow(NotDispatchedError);
		expect(fetchMock).not.toHaveBeenCalled();
	});
});
