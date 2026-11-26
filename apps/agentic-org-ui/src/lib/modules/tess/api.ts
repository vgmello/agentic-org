import { SESSION_PROOF_HEADER } from '$lib/platform/http/wire';
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

type Rec = Record<string, unknown>;

const asRec = (v: unknown): Rec | undefined =>
	v && typeof v === 'object' && !Array.isArray(v) ? (v as Rec) : undefined;
const asStr = (v: unknown): string | undefined =>
	typeof v === 'string' && v.length > 0 ? v : undefined;
const asNum = (v: unknown): number | undefined =>
	typeof v === 'number' && Number.isFinite(v) ? v : undefined;

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

function decodeToolCall(raw: unknown): TessToolCall | undefined {
	const r = asRec(raw);
	const id = asStr(r?.id) ?? asStr(r?.call_id) ?? `tool-${Date.now()}`;
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

const CANONICAL_REF_RE = /\b(T-\d+|BUG-\d+|REQ-\d+|P-\d+|TEAM-\d+)\b/g;

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

export interface HttpTessApiOptions {
	fetch?: typeof fetch;
	getProof?: () => string | undefined;
}

function buildHeaders(proof: string | undefined, contentType?: string): Record<string, string> {
	const headers: Record<string, string> = {};
	if (contentType) headers['Content-Type'] = contentType;
	if (proof) headers[SESSION_PROOF_HEADER] = proof;
	return headers;
}

function normalizeStreamPath(orgId: string, turnId: string, streamUrl: string): string {
	if (streamUrl.startsWith('/api/v1/')) return streamUrl;
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
		else if (line.startsWith('data:')) dataLines.push(line.slice(5).trimStart());
	}
	const parsed = parseTessSseEvent(eventType, dataLines.join('\n'));
	if (parsed) onEvent(parsed);
}

export function createHttpTessApi(options: HttpTessApiOptions = {}): TessApi {
	const send = options.fetch ?? ((...args: Parameters<typeof fetch>) => globalThis.fetch(...args));
	const proofOf = options.getProof ?? (() => undefined);

	return {
		async ask(organizationId: string, request: TessAskRequest): Promise<TessAskResult> {
			const path = `/api/v1/organizations/${encodeURIComponent(organizationId)}/tess/ask`;
			try {
				const res = await send(path, {
					method: 'POST',
					credentials: 'same-origin',
					headers: buildHeaders(proofOf(), 'application/json'),
					body: JSON.stringify(request)
				});
				if (res.status === 404 || res.status === 501 || res.status === 503) {
					return {
						kind: 'unavailable',
						reason: 'Live Tess runtime endpoint is not available yet.'
					};
				}
				const json: unknown = await res.json().catch(() => undefined);
				if (!res.ok) {
					const err = asRec(asRec(json)?.error);
					return {
						kind: 'unavailable',
						reason: asStr(err?.message) ?? `Tess request failed (${res.status}).`
					};
				}
				const data = unwrapBodyResult(json);
				if (data?.status === 'streaming' && asStr(data.turn_id)) {
					const turnId = asStr(data.turn_id)!;
					return {
						kind: 'streaming',
						turnId,
						streamUrl: normalizeStreamPath(organizationId, turnId, asStr(data.stream_url) ?? '')
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
			streamUrl: string,
			handlers: TessStreamHandlers,
			signal?: AbortSignal
		): Promise<void> {
			const path = normalizeStreamPath(organizationId, turnId, streamUrl);
			try {
				const res = await send(path, {
					method: 'GET',
					credentials: 'same-origin',
					headers: { ...buildHeaders(proofOf()), Accept: 'text/event-stream' },
					signal
				});
				if (!res.ok || !res.body) {
					handlers.onError?.(`Stream unavailable (${res.status}).`);
					return;
				}
				const reader = res.body.getReader();
				const decoder = new TextDecoder();
				let buffer = '';
				while (true) {
					const { value, done } = await reader.read();
					if (done) break;
					buffer += decoder.decode(value, { stream: true }).replace(/\r\n/g, '\n');
					let splitAt = buffer.indexOf('\n\n');
					while (splitAt !== -1) {
						dispatchSseBlock(buffer.slice(0, splitAt), handlers.onEvent);
						buffer = buffer.slice(splitAt + 2);
						splitAt = buffer.indexOf('\n\n');
					}
				}
				if (buffer.trim()) dispatchSseBlock(buffer, handlers.onEvent);
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
				const res = await send(path, {
					method: 'POST',
					credentials: 'same-origin',
					headers: buildHeaders(proofOf(), 'application/json'),
					body: '{}'
				});
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
				const res = await send(path, {
					method: 'POST',
					credentials: 'same-origin',
					headers: buildHeaders(proofOf(), 'application/json'),
					body: JSON.stringify(ifMatch ? { if_match: ifMatch } : {})
				});
				const json: unknown = await res.json().catch(() => undefined);
				if (!res.ok) {
					const err = asRec(asRec(json)?.error);
					return {
						ok: false,
						code: mapApplyErrorCode(res.status, asStr(err?.code)),
						message: asStr(err?.message) ?? `Could not apply proposal (${res.status}).`
					};
				}
				const data = unwrapBodyResult(json);
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
				const res = await send(path, {
					method: 'POST',
					credentials: 'same-origin',
					headers: buildHeaders(proofOf(), 'application/json'),
					body: '{}'
				});
				return res.ok;
			} catch {
				return false;
			}
		},

		async resetSession(organizationId: string): Promise<boolean> {
			const path = `/api/v1/organizations/${encodeURIComponent(organizationId)}/tess/session/reset`;
			try {
				const res = await send(path, {
					method: 'POST',
					credentials: 'same-origin',
					headers: buildHeaders(proofOf(), 'application/json'),
					body: '{}'
				});
				return res.ok;
			} catch {
				return false;
			}
		}
	};
}

export const defaultTessApi: TessApi = createHttpTessApi();
