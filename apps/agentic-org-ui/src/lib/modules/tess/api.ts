import { appSession } from '$lib/platform/auth/app-session.svelte';
import { asNum, asRec, asStr as rawStr, type Rec } from '$lib/platform/http/decode';
import type {
	TessActionProposal,
	TessApi,
	TessAskRequest,
	TessAskResult,
	TessEntityRow,
	TessEntityTone,
	TessMetricCell,
	TessProposalApplyResult,
	TessProposalParam,
	TessProposalRisk,
	TessStreamEvent,
	TessStreamHandlers,
	TessToolCall,
	TessUiBlock
} from './types';

const asStr = (v: unknown): string | undefined => {
	const s = rawStr(v);
	return s && s.length > 0 ? s : undefined;
};

const VALID_TONES = new Set<TessEntityTone>(['ok', 'warn', 'bad', 'accent', 'muted']);

function readTone(v: unknown): TessEntityTone | undefined {
	const s = asStr(v);
	return s && VALID_TONES.has(s as TessEntityTone) ? (s as TessEntityTone) : undefined;
}

function readRisk(v: unknown): TessProposalRisk {
	const s = asStr(v)?.toLowerCase();
	if (s === 'high' || s === 'irreversible' || s === 'destructive') return 'high';
	if (s === 'review' || s === 'medium' || s === 'confirm') return 'review';
	return 'reversible';
}

function safeAppHref(raw: unknown): string | undefined {
	const s = asStr(raw)?.trim();
	if (!s || !s.startsWith('/') || s.startsWith('//') || s.startsWith('/\\')) return undefined;
	for (let i = 0; i < s.length; i++) {
		const code = s.charCodeAt(i);
		if (code <= 0x1f || code === 0x7f) return undefined;
	}
	return s;
}

function decodeParams(raw: unknown): TessProposalParam[] {
	if (!Array.isArray(raw)) return [];
	const out: TessProposalParam[] = [];
	for (const entry of raw) {
		const rec = asRec(entry);
		const label = asStr(rec?.label) ?? asStr(rec?.key);
		const value = asStr(rec?.value);
		if (label && value !== undefined) out.push({ label, value });
	}
	return out;
}

function decodeProposal(raw: unknown): TessActionProposal | undefined {
	const r = asRec(raw);
	const id = asStr(r?.id) ?? asStr(r?.proposal_id);
	const title = asStr(r?.title);
	if (!id || !title) return undefined;
	return {
		id,
		kind: asStr(r?.kind) ?? 'action',
		title,
		summary: asStr(r?.summary),
		consequence: asStr(r?.consequence),
		risk: readRisk(r?.risk),
		riskLabel: asStr(r?.risk_label) ?? asStr(r?.riskLabel),
		ctaLabel: asStr(r?.cta_label) ?? asStr(r?.ctaLabel) ?? 'Apply change',
		parameters: decodeParams(r?.parameters ?? r?.params),
		editHref: safeAppHref(r?.edit_href ?? r?.editHref),
		expectedRevision: asStr(r?.expected_revision ?? r?.if_match),
		status: 'pending'
	};
}

function decodeEntityRows(raw: unknown): TessEntityRow[] {
	if (!Array.isArray(raw)) return [];
	const rows: TessEntityRow[] = [];
	for (const entry of raw) {
		const r = asRec(entry);
		const title = asStr(r?.title) ?? asStr(r?.name);
		if (!title) continue;
		rows.push({
			id: asStr(r?.id),
			ref: asStr(r?.ref),
			title,
			subtitle: asStr(r?.subtitle) ?? asStr(r?.detail),
			badge: asStr(r?.badge) ?? asStr(r?.status),
			tone: readTone(r?.tone),
			href: safeAppHref(r?.href ?? r?.path)
		});
	}
	return rows;
}

function decodeMetrics(raw: unknown): TessMetricCell[] {
	if (!Array.isArray(raw)) return [];
	const out: TessMetricCell[] = [];
	for (const entry of raw) {
		const r = asRec(entry);
		const label = asStr(r?.label);
		const value = asStr(r?.value);
		if (!label || !value) continue;
		out.push({ label, value, note: asStr(r?.note), tone: readTone(r?.tone) });
	}
	return out;
}

export function decodeUiBlock(raw: unknown): TessUiBlock | undefined {
	const r = asRec(raw);
	const kind = asStr(r?.kind);
	if (kind === 'action_proposal') {
		const proposal = decodeProposal(r?.proposal ?? r);
		return proposal ? { kind: 'action_proposal', proposal } : undefined;
	}
	if (kind === 'entity_list' || kind === 'navigation') {
		const items = decodeEntityRows(r?.items);
		return items.length > 0 ? { kind, title: asStr(r?.title), items } : undefined;
	}
	if (kind === 'metrics') {
		const metrics = decodeMetrics(r?.metrics);
		return metrics.length > 0 ? { kind: 'metrics', title: asStr(r?.title), metrics } : undefined;
	}
	if (kind === 'follow_ups' && Array.isArray(r?.prompts)) {
		const prompts = r.prompts.filter(
			(p): p is string => typeof p === 'string' && p.trim().length > 0
		);
		return prompts.length > 0 ? { kind: 'follow_ups', prompts: prompts.slice(0, 3) } : undefined;
	}
	return undefined;
}

let fallbackToolSeq = 0;

function decodeToolCall(raw: unknown): TessToolCall | undefined {
	const r = asRec(raw);
	const id = asStr(r?.id) ?? asStr(r?.call_id) ?? `tool-${++fallbackToolSeq}`;
	const label = asStr(r?.label) ?? asStr(r?.summary);
	if (!label) return undefined;
	const stateRaw = asStr(r?.state) ?? asStr(r?.status);
	const state: TessToolCall['state'] =
		stateRaw === 'done' || stateRaw === 'completed'
			? 'done'
			: stateRaw === 'error' || stateRaw === 'failed'
				? 'error'
				: 'running';
	return {
		id,
		label,
		detail: asStr(r?.detail),
		state,
		startedAt: asNum(r?.started_at) ?? Date.now(),
		durationMs: asNum(r?.duration_ms)
	};
}

export function parseTessSseEvent(
	eventType: string,
	dataText: string
): TessStreamEvent | undefined {
	if (!dataText.trim()) return undefined;
	let parsed: unknown;
	try {
		parsed = JSON.parse(dataText);
	} catch {
		return eventType === 'text.delta' ? { type: 'text.delta', delta: dataText } : undefined;
	}
	const r = asRec(parsed);
	const type = eventType || asStr(r?.type) || '';
	if (type === 'turn.started') {
		return { type: 'turn.started', turnId: asStr(r?.turn_id) ?? '' };
	}
	if (type === 'tool.progress') {
		const call = decodeToolCall(r?.call ?? r);
		return call ? { type: 'tool.progress', call } : undefined;
	}
	if (type === 'text.delta') {
		const delta =
			typeof r?.delta === 'string' ? r.delta : typeof r?.text === 'string' ? r.text : '';
		return delta ? { type: 'text.delta', delta } : undefined;
	}
	if (type === 'ui.block') {
		const block = decodeUiBlock(r?.block ?? r);
		return block ? { type: 'ui.block', block } : undefined;
	}
	if (type === 'turn.completed') {
		const followUps = Array.isArray(r?.follow_ups)
			? r.follow_ups
					.filter((p): p is string => typeof p === 'string' && p.trim().length > 0)
					.slice(0, 3)
			: undefined;
		return { type: 'turn.completed', turnId: asStr(r?.turn_id), followUps };
	}
	if (type === 'turn.error' || type === 'error') {
		return { type: 'turn.error', message: asStr(r?.message) ?? 'Tess encountered an error.' };
	}
	return undefined;
}

export interface TextRefSegment {
	text: string;
	href?: string;
}

const CANONICAL_REF_RE = /\b(T-(?:[A-Z]+\d+|\d+)|BUG-\d+|REQ-\d+|P-\d+|TEAM-\d+)\b/g;

function hrefForCanonicalRef(ref: string): string {
	if (ref.startsWith('P-')) return `/projects`;
	if (ref.startsWith('TEAM-')) return `/teams`;
	return `/work/${encodeURIComponent(ref)}`;
}

/** Splits plain reply text into safe segments with auto-linked canonical entity refs (no HTML injection). */
export function splitCanonicalRefSegments(text: string): TextRefSegment[] {
	if (!text) return [];
	const segments: TextRefSegment[] = [];
	let lastIndex = 0;
	for (const match of text.matchAll(CANONICAL_REF_RE)) {
		const idx = match.index ?? 0;
		if (idx > lastIndex) segments.push({ text: text.slice(lastIndex, idx) });
		const ref = match[0];
		segments.push({ text: ref, href: hrefForCanonicalRef(ref) });
		lastIndex = idx + ref.length;
	}
	if (lastIndex < text.length) segments.push({ text: text.slice(lastIndex) });
	return segments;
}

export interface TessSessionTransport {
	submit: (
		path: string,
		body: unknown,
		signal?: AbortSignal
	) => Promise<{ status: number; body: unknown }>;
	openSseStream: (path: string, signal?: AbortSignal) => Promise<Response>;
}

export interface HttpTessApiOptions {
	session?: TessSessionTransport;
	maxEventBytes?: number;
}

const DEFAULT_MAX_SSE_BYTES = 1024 * 1024;

function defaultSessionTransport(): TessSessionTransport {
	return {
		submit: (path, body, signal) => appSession.controller().submit(path, body, signal),
		openSseStream: (path, signal) => appSession.controller().openSseStream(path, signal)
	};
}

function canonicalStreamPath(orgId: string, turnId: string): string {
	return `/api/v1/organizations/${encodeURIComponent(orgId)}/tess/turns/${encodeURIComponent(turnId)}/stream`;
}

function unwrapBodyResult(raw: unknown): Rec | undefined {
	const root = asRec(raw);
	return asRec(root?.result) ?? root;
}

function mapApplyErrorCode(
	status: number,
	codeRaw?: string
): Extract<TessProposalApplyResult, { ok: false }>['code'] {
	if (status === 409 || codeRaw === 'conflict' || codeRaw === 'stale_revision') return 'conflict';
	if (status === 403 || codeRaw === 'forbidden') return 'forbidden';
	if (status === 410 || codeRaw === 'expired') return 'expired';
	if (status === 404 || status === 501 || status === 503) return 'unavailable';
	return 'error';
}

function dispatchSseBlock(rawBlock: string, onEvent: (ev: TessStreamEvent) => void): void {
	let eventType = '';
	const dataLines: string[] = [];
	for (const line of rawBlock.split('\n')) {
		if (!line || line.startsWith(':')) continue;
		if (line.startsWith('event:')) eventType = line.slice(6).trim();
		else if (line.startsWith('data:')) {
			dataLines.push(line.startsWith('data: ') ? line.slice(6) : line.slice(5));
		}
	}
	const parsed = parseTessSseEvent(eventType, dataLines.join('\n'));
	if (parsed) onEvent(parsed);
}

interface JsonReply {
	status: number;
	ok: boolean;
	body: unknown;
}

function createPostJson(session: TessSessionTransport) {
	return async (path: string, body: unknown, signal?: AbortSignal): Promise<JsonReply> => {
		const res = await session.submit(path, body, signal);
		return { status: res.status, ok: res.status >= 200 && res.status < 300, body: res.body };
	};
}

function createOpenStream(session: TessSessionTransport) {
	return (path: string, signal?: AbortSignal) => session.openSseStream(path, signal);
}

async function consumeSseBody(
	body: ReadableStream<Uint8Array>,
	maxEventBytes: number,
	onEvent: (ev: TessStreamEvent) => void
): Promise<void> {
	const reader = body.getReader();
	const decoder = new TextDecoder();
	let buffer = '';
	let sawTrailingCr = false;
	const appendChunk = (chunk: string) => {
		if (!chunk) return;
		const trimmed = sawTrailingCr && chunk.startsWith('\n') ? chunk.slice(1) : chunk;
		sawTrailingCr = trimmed.endsWith('\r');
		buffer += trimmed.replace(/\r\n/g, '\n').replace(/\r/g, '\n');
	};
	try {
		while (true) {
			const { value, done } = await reader.read();
			if (done) break;
			appendChunk(decoder.decode(value, { stream: true }));
			if (buffer.length > maxEventBytes) {
				throw new Error('Tess stream exceeded maximum event size.');
			}
			let splitAt = buffer.indexOf('\n\n');
			while (splitAt !== -1) {
				dispatchSseBlock(buffer.slice(0, splitAt), onEvent);
				buffer = buffer.slice(splitAt + 2);
				splitAt = buffer.indexOf('\n\n');
			}
		}
		appendChunk(decoder.decode());
		if (buffer.trim()) dispatchSseBlock(buffer, onEvent);
	} finally {
		await reader.cancel().catch(() => {});
		reader.releaseLock();
	}
}

export function createHttpTessApi(options: HttpTessApiOptions = {}): TessApi {
	const session = options.session ?? defaultSessionTransport();
	const postJson = createPostJson(session);
	const openStream = createOpenStream(session);
	const maxEventBytes = options.maxEventBytes ?? DEFAULT_MAX_SSE_BYTES;

	return {
		async ask(
			organizationId: string,
			request: TessAskRequest,
			signal?: AbortSignal
		): Promise<TessAskResult> {
			const path = `/api/v1/organizations/${encodeURIComponent(organizationId)}/tess/ask`;
			try {
				const res = await postJson(path, request, signal);
				if (res.status === 404 || res.status === 501 || res.status === 503) {
					return {
						kind: 'unavailable',
						reason: 'Live Tess runtime endpoint is not available yet.'
					};
				}
				if (!res.ok) {
					const err = asRec(asRec(res.body)?.error);
					return {
						kind: 'unavailable',
						reason: asStr(err?.message) ?? `Tess request failed (${res.status}).`
					};
				}
				const data = unwrapBodyResult(res.body);
				if (data?.status === 'streaming' && asStr(data.turn_id)) {
					const turnId = asStr(data.turn_id)!;
					return {
						kind: 'streaming',
						turnId,
						streamUrl: canonicalStreamPath(organizationId, turnId)
					};
				}
				return {
					kind: 'fallback',
					turnId: asStr(data?.turn_id),
					fallbackAnswer: asStr(data?.fallback_answer),
					reason: asStr(data?.unavailable_reason) ?? 'No runtime configured for Tess.'
				};
			} catch {
				return { kind: 'unavailable', reason: 'Could not reach HQ for a live Tess turn.' };
			}
		},

		async streamTurn(
			organizationId: string,
			turnId: string,
			_streamUrl: string,
			handlers: TessStreamHandlers,
			signal?: AbortSignal
		): Promise<void> {
			const path = canonicalStreamPath(organizationId, turnId);
			try {
				const res = await openStream(path, signal);
				if (!res.ok || !res.body) {
					handlers.onError?.(`Stream unavailable (${res.status}).`);
					return;
				}
				await consumeSseBody(res.body, maxEventBytes, handlers.onEvent);
				handlers.onDone?.();
			} catch (err) {
				if (signal?.aborted) {
					handlers.onDone?.();
					return;
				}
				handlers.onError?.(err instanceof Error ? err.message : 'Lost live connection to Tess.');
			}
		},

		async cancelTurn(organizationId: string, turnId: string): Promise<boolean> {
			const path = `/api/v1/organizations/${encodeURIComponent(organizationId)}/tess/turns/${encodeURIComponent(turnId)}/cancel`;
			try {
				const res = await postJson(path, {});
				return res.ok;
			} catch {
				return false;
			}
		},

		async applyProposal(
			organizationId: string,
			proposalId: string,
			ifMatch?: string
		): Promise<TessProposalApplyResult> {
			const path = `/api/v1/organizations/${encodeURIComponent(organizationId)}/tess/proposals/${encodeURIComponent(proposalId)}/apply`;
			try {
				const res = await postJson(path, ifMatch ? { if_match: ifMatch } : {});
				if (!res.ok) {
					const err = asRec(asRec(res.body)?.error);
					return {
						ok: false,
						code: mapApplyErrorCode(res.status, asStr(err?.code)),
						message: asStr(err?.message) ?? `Could not apply proposal (${res.status}).`
					};
				}
				const data = unwrapBodyResult(res.body);
				const rec = asRec(data?.receipt) ?? data;
				return {
					ok: true,
					receipt: {
						summary: asStr(rec?.summary) ?? 'Change applied.',
						entityHref: safeAppHref(rec?.entity_href ?? rec?.href),
						entityLabel: asStr(rec?.entity_label ?? rec?.label),
						appliedAt: asStr(rec?.applied_at)
					}
				};
			} catch {
				return {
					ok: false,
					code: 'unavailable',
					message: 'Could not reach HQ to apply this change.'
				};
			}
		},

		async dismissProposal(organizationId: string, proposalId: string): Promise<boolean> {
			const path = `/api/v1/organizations/${encodeURIComponent(organizationId)}/tess/proposals/${encodeURIComponent(proposalId)}/dismiss`;
			try {
				const res = await postJson(path, {});
				return res.ok;
			} catch {
				return false;
			}
		},

		async resetSession(organizationId: string): Promise<boolean> {
			const path = `/api/v1/organizations/${encodeURIComponent(organizationId)}/tess/session/reset`;
			try {
				const res = await postJson(path, {});
				return res.ok;
			} catch {
				return false;
			}
		}
	};
}

export const defaultTessApi: TessApi = createHttpTessApi();
