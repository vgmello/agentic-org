/**
 * Embedded Tess (Chief of Staff) wire & UI contracts (doc-97 §5.C & §6, TASK-80.5).
 * Mirrors the HTTP/1.1 + SSE endpoints specified for TASK-80.4 so the UI transitions
 * seamlessly from badged local summary fallback to live streaming LLM turns.
 */

export type TessRuntimeStatus = 'ready' | 'no_runtime' | 'offline';

export type TessTurnMode = 'offline_summary' | 'live';

export interface TessContextRef {
	route?: string;
	routeLabel?: string;
	entityRef?: string;
	decisionId?: string;
	decisionTitle?: string;
}

export interface TessAskRequest {
	prompt: string;
	context_ref?: TessContextRef;
}

export interface TessAskStreamingResponse {
	turn_id: string;
	status: 'streaming';
	stream_url?: string;
}

export interface TessAskFallbackResponse {
	turn_id?: string;
	status: 'fallback';
	fallback_answer?: string;
	unavailable_reason?: string;
}

export type TessAskResponse = TessAskStreamingResponse | TessAskFallbackResponse;

export type TessAskResult =
	| { kind: 'streaming'; turnId: string; streamUrl: string }
	| { kind: 'fallback'; turnId?: string; fallbackAnswer?: string; reason?: string }
	| { kind: 'unavailable'; reason: string };

export interface TessToolCall {
	id: string;
	label: string;
	detail?: string;
	state: 'running' | 'done' | 'error';
	startedAt: number;
	durationMs?: number;
}

export type TessEntityTone = 'ok' | 'warn' | 'bad' | 'accent' | 'muted';

export interface TessEntityRow {
	id?: string;
	ref?: string;
	title: string;
	subtitle?: string;
	badge?: string;
	tone?: TessEntityTone;
	href?: string;
}

export interface TessMetricCell {
	label: string;
	value: string;
	note?: string;
	tone?: TessEntityTone;
}

export type TessProposalRisk = 'reversible' | 'review' | 'high';

export interface TessProposalParam {
	label: string;
	value: string;
}

export interface TessProposalReceipt {
	summary: string;
	entityHref?: string;
	entityLabel?: string;
	appliedAt?: string;
}

export interface TessActionProposal {
	id: string;
	kind: string;
	title: string;
	summary?: string;
	consequence?: string;
	risk: TessProposalRisk;
	riskLabel?: string;
	ctaLabel: string;
	parameters: TessProposalParam[];
	editHref?: string;
	expectedRevision?: string;
	status: 'pending' | 'applying' | 'applied' | 'dismissed' | 'error';
	errorMessage?: string;
	receipt?: TessProposalReceipt;
}

export type TessUiBlock =
	| { kind: 'action_proposal'; proposal: TessActionProposal }
	| { kind: 'entity_list'; title?: string; items: TessEntityRow[] }
	| { kind: 'metrics'; title?: string; metrics: TessMetricCell[] }
	| { kind: 'navigation'; title?: string; items: TessEntityRow[] }
	| { kind: 'follow_ups'; prompts: string[] };

export type TessStreamEvent =
	| { type: 'turn.started'; turnId: string }
	| { type: 'tool.progress'; call: TessToolCall }
	| { type: 'text.delta'; delta: string }
	| { type: 'ui.block'; block: TessUiBlock }
	| { type: 'turn.completed'; turnId?: string; followUps?: string[] }
	| { type: 'turn.error'; message: string };

export interface TessStreamHandlers {
	onEvent: (event: TessStreamEvent) => void;
	onError?: (message: string) => void;
	onDone?: () => void;
}

export type TessProposalApplyResult =
	| { ok: true; receipt: TessProposalReceipt }
	| {
			ok: false;
			code: 'conflict' | 'forbidden' | 'expired' | 'unavailable' | 'error';
			message: string;
	  };

export interface TessApi {
	ask(organizationId: string, request: TessAskRequest): Promise<TessAskResult>;
	streamTurn(
		organizationId: string,
		turnId: string,
		streamUrl: string,
		handlers: TessStreamHandlers,
		signal?: AbortSignal
	): Promise<void>;
	cancelTurn(organizationId: string, turnId: string): Promise<boolean>;
	applyProposal(
		organizationId: string,
		proposalId: string,
		ifMatch?: string
	): Promise<TessProposalApplyResult>;
	dismissProposal(organizationId: string, proposalId: string): Promise<boolean>;
	resetSession(organizationId: string): Promise<boolean>;
}
