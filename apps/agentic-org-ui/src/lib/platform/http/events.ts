import { NotDispatchedError } from './errors';
import {
	SESSION_PROOF_HEADER,
	WireDecodeError,
	readEventFrame,
	readJsonObject,
	readResponse,
	readScope,
	readStreamRequest,
	type EventFrame,
	type GatewayError,
	type Scope
} from './wire';

export const EVENTS_STREAM_PATH = '/api/v1/events/stream';
export const MAX_SUBSCRIPTIONS = 32;
export const DEFAULT_MAX_REQUEST_BYTES = 64 * 1024;
export const DEFAULT_MAX_EVENT_BYTES = 1024 * 1024;

export type StreamRequest = ReturnType<typeof readStreamRequest>;
export type Subscription = StreamRequest['subscriptions'][number];
export type Feed = Subscription['feed'];
export type { EventFrame };

export interface EventStreamHandlers {
	onFrame?: (frame: EventFrame) => void | Promise<void>;
	onReady?: (frame: Extract<EventFrame, { type: 'ready' }>) => void | Promise<void>;
	onChange?: (frame: Extract<EventFrame, { type: 'change' }>) => void | Promise<void>;
	onHeartbeat?: (frame: Extract<EventFrame, { type: 'heartbeat' }>) => void | Promise<void>;
	onResync?: (frame: Extract<EventFrame, { type: 'resync' }>) => void | Promise<void>;
	onUnavailable?: (frame: Extract<EventFrame, { type: 'unavailable' }>) => void | Promise<void>;
	onSessionEnded?: (frame: Extract<EventFrame, { type: 'session_ended' }>) => void | Promise<void>;
	onUnsupported?: (frame: Extract<EventFrame, { type: 'unsupported' }>) => void | Promise<void>;
}

export class EventStreamError extends Error {
	readonly status: number;
	readonly gatewayError?: GatewayError;

	constructor(status: number, gatewayError?: GatewayError, message?: string) {
		super(message ?? gatewayError?.message ?? `Event stream request failed (${status})`);
		this.name = 'EventStreamError';
		this.status = status;
		if (gatewayError) this.gatewayError = gatewayError;
	}
}

export interface EventStreamClientOptions {
	origin: string;
	fetch?: typeof fetch;
	maxRequestBytes?: number;
	maxEventBytes?: number;
}

export interface EventStreamOpenRequest {
	subscriptions: readonly Subscription[];
	proof: string;
	handlers?: EventStreamHandlers;
	signal?: AbortSignal;
}

export interface EventStreamResult {
	reason: 'ended' | 'session_ended' | 'aborted';
	cursors: ReadonlyMap<string, string>;
}

export interface EventStreamHandle {
	readonly closed: Promise<EventStreamResult>;
	close(): void;
	cursor(scope: Scope, feed: Feed): string | undefined;
}

interface SseParserState {
	lineBuffer: string;
	sawTrailingCr: boolean;
	dataLines: string[];
	eventBytes: number;
	readonly maxEventBytes: number;
}

function assertHeaderProof(proof: unknown): asserts proof is string {
	if (typeof proof !== 'string' || !/^[\x21-\x7e]{1,1024}$/.test(proof))
		throw new NotDispatchedError('Invalid session proof');
}

function subscriptionKey(scope: Scope, feed: Feed): string {
	return `${JSON.stringify(readScope(scope))}:${feed}`;
}

/** Dispatches a decoded EventFrame to `onFrame` and its typed variant handler. */
export async function dispatchEventFrame(
	frame: EventFrame,
	handlers: EventStreamHandlers
): Promise<void> {
	await handlers.onFrame?.(frame);
	switch (frame.type) {
		case 'ready':
			await handlers.onReady?.(frame);
			return;
		case 'change':
			await handlers.onChange?.(frame);
			return;
		case 'heartbeat':
			await handlers.onHeartbeat?.(frame);
			return;
		case 'resync':
			await handlers.onResync?.(frame);
			return;
		case 'unavailable':
			await handlers.onUnavailable?.(frame);
			return;
		case 'session_ended':
			await handlers.onSessionEnded?.(frame);
			return;
		case 'unsupported':
			await handlers.onUnsupported?.(frame);
			return;
		default: {
			const unreachable: never = frame;
			return unreachable;
		}
	}
}

function decodeSseData(lines: readonly string[]): EventFrame {
	let parsed: unknown;
	try {
		parsed = JSON.parse(lines.join('\n'));
	} catch {
		throw new WireDecodeError();
	}
	return readEventFrame(parsed);
}

function parseSseLine(line: string, state: SseParserState): EventFrame | undefined {
	if (line === '') {
		if (state.dataLines.length === 0) return undefined;
		const lines = state.dataLines;
		state.dataLines = [];
		state.eventBytes = 0;
		return decodeSseData(lines);
	}
	if (line.startsWith(':')) return undefined;
	const colon = line.indexOf(':');
	const field = colon === -1 ? line : line.slice(0, colon);
	if (field !== 'data') return undefined;
	const raw = colon === -1 ? '' : line.slice(colon + 1);
	const value = raw.startsWith(' ') ? raw.slice(1) : raw;
	state.eventBytes += value.length + 1;
	if (state.eventBytes > state.maxEventBytes) throw new Error('SSE event exceeds its byte limit');
	state.dataLines.push(value);
	return undefined;
}

function splitSseLines(text: string, state: SseParserState): string[] {
	const lines: string[] = [];
	let start = 0;
	if (state.sawTrailingCr) {
		state.sawTrailingCr = false;
		if (text.startsWith('\n')) start = 1;
	}
	for (let i = start; i < text.length; i++) {
		const code = text.charCodeAt(i);
		if (code !== 0x0a && code !== 0x0d) continue;
		lines.push(state.lineBuffer + text.slice(start, i));
		state.lineBuffer = '';
		if (code === 0x0d) {
			if (i + 1 < text.length && text.charCodeAt(i + 1) === 0x0a) i++;
			else if (i + 1 === text.length) state.sawTrailingCr = true;
		}
		start = i + 1;
	}
	if (start < text.length) {
		state.lineBuffer += text.slice(start);
		if (state.eventBytes + state.lineBuffer.length > state.maxEventBytes)
			throw new Error('SSE event exceeds its byte limit');
	}
	return lines;
}

function* decodeSseChunk(text: string, state: SseParserState): Generator<EventFrame> {
	for (const line of splitSseLines(text, state)) {
		const frame = parseSseLine(line, state);
		if (frame) yield frame;
	}
}

/**
 * Parses a WHATWG Server-Sent Events byte stream into validated gateway `EventFrame` records.
 * Handles arbitrary chunk boundaries, `\r\n` / `\n` / `\r` line endings, comments, and `AbortSignal`.
 */
export async function* readSseFrames(
	stream: ReadableStream<Uint8Array>,
	options: { signal?: AbortSignal; maxEventBytes?: number } = {}
): AsyncGenerator<EventFrame> {
	const maxEventBytes = options.maxEventBytes ?? DEFAULT_MAX_EVENT_BYTES;
	if (!Number.isSafeInteger(maxEventBytes) || maxEventBytes <= 0)
		throw new Error('maxEventBytes must be a positive integer');
	const signal = options.signal;
	const reader = stream.getReader();
	const cancel = () => {
		void reader.cancel().catch(() => {});
	};
	signal?.addEventListener('abort', cancel, { once: true });

	const decoder = new TextDecoder('utf-8');
	const state: SseParserState = {
		lineBuffer: '',
		sawTrailingCr: false,
		dataLines: [],
		eventBytes: 0,
		maxEventBytes
	};

	try {
		while (!signal?.aborted) {
			const { done, value } = await reader.read();
			if (done || signal?.aborted) break;
			for (const frame of decodeSseChunk(decoder.decode(value, { stream: true }), state)) {
				yield frame;
				if (signal?.aborted) return;
			}
		}
		const tail = signal?.aborted ? '' : decoder.decode();
		if (tail) yield* decodeSseChunk(tail, state);
	} finally {
		signal?.removeEventListener('abort', cancel);
		try {
			await reader.cancel();
		} catch {
			// Stream already closed or cancelled.
		}
		reader.releaseLock();
	}
}

async function readErrorPayload(response: Response): Promise<GatewayError | undefined> {
	const contentType = response.headers.get('content-type') ?? '';
	if (!/^application\/(?:[\w.+-]+\+)?json(?:;|$)/i.test(contentType)) {
		void response.body?.cancel().catch(() => {});
		return undefined;
	}
	try {
		const raw: unknown = await response.json();
		const decoded = readResponse(raw, readJsonObject);
		return 'error' in decoded ? decoded.error : undefined;
	} catch {
		return undefined;
	}
}

function parseClientUrl(origin: string): URL {
	let base: URL;
	try {
		base = new URL(origin);
	} catch {
		throw new NotDispatchedError('Invalid browser origin');
	}
	if (base.origin !== origin || !['http:', 'https:'].includes(base.protocol))
		throw new NotDispatchedError('Invalid browser origin');
	if (typeof globalThis.location !== 'undefined' && globalThis.location.origin !== origin)
		throw new NotDispatchedError('The API must use this browser origin');
	return new URL(EVENTS_STREAM_PATH, base);
}

function assertPositiveLimit(value: number): void {
	if (!Number.isSafeInteger(value) || value <= 0)
		throw new NotDispatchedError('Event stream requires finite body limits');
}

function encodeStreamPayload(
	subscriptions: readonly Subscription[],
	proof: string,
	maxRequestBytes: number
): string {
	const validated = readStreamRequest({ subscriptions });
	if (validated.subscriptions.length === 0 || validated.subscriptions.length > MAX_SUBSCRIPTIONS)
		throw new NotDispatchedError('Event stream requires 1 to 32 subscriptions');
	const encoded = JSON.stringify(validated, (key, value: unknown) => {
		if (key.includes(proof) || (typeof value === 'string' && value.includes(proof)))
			throw new NotDispatchedError('Session proof must be supplied only in the header');
		return value;
	});
	if (new TextEncoder().encode(encoded).byteLength > maxRequestBytes)
		throw new NotDispatchedError('Stream request exceeds its limit');
	return encoded;
}

function prepareOpenRequest(
	{ subscriptions, proof, signal }: EventStreamOpenRequest,
	maxRequestBytes: number
): { encoded: string; headers: Headers } {
	try {
		assertHeaderProof(proof);
		const encoded = encodeStreamPayload(subscriptions, proof, maxRequestBytes);
		const headers = new Headers({
			Accept: 'text/event-stream',
			'Content-Type': 'application/json',
			[SESSION_PROOF_HEADER]: proof
		});
		if (signal?.aborted) throw new NotDispatchedError('Request cancelled');
		return { encoded, headers };
	} catch (error) {
		if (error instanceof NotDispatchedError) throw error;
		throw new NotDispatchedError('Invalid event stream request');
	}
}

function recordCursorEntry(
	cursors: Map<string, string>,
	scope: unknown,
	feed: Feed,
	cursor: string
) {
	try {
		cursors.set(subscriptionKey(readScope(scope), feed), cursor);
	} catch {
		// Unknown scopes are already normalized to unsupported in readEventFrame.
	}
}

function recordFrameCursor(cursors: Map<string, string>, frame: EventFrame): void {
	if (frame.type === 'ready') {
		for (const sub of frame.subscriptions)
			recordCursorEntry(cursors, sub.scope, sub.feed, sub.cursor);
	} else if (frame.type === 'change') {
		recordCursorEntry(cursors, frame.scope, frame.feed, frame.cursor);
	}
}

async function validateStreamResponse(response: Response): Promise<ReadableStream<Uint8Array>> {
	if (response.status !== 200)
		throw new EventStreamError(response.status, await readErrorPayload(response));
	const contentType = response.headers.get('content-type') ?? '';
	if (!/^text\/event-stream(?:\s*;|$)/i.test(contentType)) {
		void response.body?.cancel().catch(() => {});
		throw new EventStreamError(response.status, undefined, 'Expected text/event-stream');
	}
	if (!response.body)
		throw new EventStreamError(response.status, undefined, 'Missing event stream body');
	return response.body;
}

async function consumeStream(params: {
	url: URL;
	send: typeof fetch;
	encoded: string;
	headers: Headers;
	controller: AbortController;
	maxEventBytes: number;
	cursors: Map<string, string>;
	handlers: EventStreamHandlers;
}): Promise<EventStreamResult> {
	const { url, send, encoded, headers, controller, maxEventBytes, cursors, handlers } = params;
	const response = await send(url, {
		method: 'POST',
		headers,
		body: encoded,
		mode: 'same-origin',
		credentials: 'same-origin',
		redirect: 'error',
		cache: 'no-store',
		signal: controller.signal
	});
	const body = await validateStreamResponse(response);
	for await (const frame of readSseFrames(body, { signal: controller.signal, maxEventBytes })) {
		recordFrameCursor(cursors, frame);
		await dispatchEventFrame(frame, handlers);
		if (frame.type === 'session_ended')
			return { reason: 'session_ended', cursors: new Map(cursors) };
	}
	return {
		reason: controller.signal.aborted ? 'aborted' : 'ended',
		cursors: new Map(cursors)
	};
}

/**
 * Typed SSE live-event client speaking `POST /api/v1/events/stream` against `agentic-org-web`.
 * Holds no business policy, tracks scoped resume cursors, and enforces same-origin + proof hygiene.
 */
export function createEventStreamClient({
	origin,
	fetch: send = globalThis.fetch,
	maxRequestBytes = DEFAULT_MAX_REQUEST_BYTES,
	maxEventBytes = DEFAULT_MAX_EVENT_BYTES
}: EventStreamClientOptions) {
	const url = parseClientUrl(origin);
	assertPositiveLimit(maxRequestBytes);
	assertPositiveLimit(maxEventBytes);

	return Object.freeze({
		open(request: EventStreamOpenRequest): EventStreamHandle {
			const { encoded, headers } = prepareOpenRequest(request, maxRequestBytes);
			const handlers = request.handlers ?? {};
			const controller = new AbortController();
			const onAbort = () => controller.abort(request.signal?.reason ?? new Error('Stream aborted'));
			request.signal?.addEventListener('abort', onAbort, { once: true });

			const cursors = new Map<string, string>();
			const closed = consumeStream({
				url,
				send,
				encoded,
				headers,
				controller,
				maxEventBytes,
				cursors,
				handlers
			})
				.catch((error: unknown): EventStreamResult => {
					if (controller.signal.aborted) return { reason: 'aborted', cursors: new Map(cursors) };
					throw error;
				})
				.finally(() => {
					request.signal?.removeEventListener('abort', onAbort);
				});

			return Object.freeze({
				closed,
				close() {
					controller.abort(new Error('Stream closed'));
				},
				cursor(scope: Scope, feed: Feed) {
					return cursors.get(subscriptionKey(scope, feed));
				}
			});
		}
	});
}
