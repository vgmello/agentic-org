import { createContext } from 'svelte';
import { answerExecutiveQuestion, type HomeDomainData, type OrgAiAnswer } from '$lib/home/model';
import type { OfficeInfo, OrgSnapshot, WorkItemSummary } from '$lib/proto';
import type { ShellAiAskOptions, ShellAiDecisionContext } from '$lib/platform/shell/shell-ai';
import { defaultTessApi } from './api';
import type {
	TessActionProposal,
	TessApi,
	TessContextRef,
	TessRuntimeStatus,
	TessStreamEvent,
	TessToolCall,
	TessTurnMode,
	TessUiBlock
} from './types';

export type { TessTurnMode } from './types';

export interface TessTurn {
	id: string;
	organizationId?: string;
	serverTurnId?: string;
	query: string;
	createdAt: number;
	mode: TessTurnMode;
	streaming?: boolean;
	streamText?: string;
	toolCalls?: TessToolCall[];
	blocks?: TessUiBlock[];
	followUps?: string[];
	errorMessage?: string;
	answer: OrgAiAnswer;
	contextLabel?: string;
}

export interface TessContextSnapshot {
	organizationId?: string;
	snapshot?: OrgSnapshot;
	items: readonly WorkItemSummary[];
	offices: readonly OfficeInfo[];
	ownerId: string;
	domain?: HomeDomainData;
	routeLabel?: string;
	routePath?: string;
	offline?: boolean;
}

export const TESS_STARTERS: readonly string[] = [
	'What needs me first?',
	'Which projects need attention?',
	'Budget, offices & active agents',
	'Where is task time going?'
];

export type TessCompanionMode = 'hidden' | 'pill' | 'open' | 'expanded';

const WRITE_OR_HOWTO_RE =
	/\b(create|assign|pause|resume|hire|move|delete|archive|approve|deny|how do i|how to|configure|set up|setup)\b/i;

function buildLocalFallbackAnswer(
	prompt: string,
	ctx: TessContextSnapshot,
	mergedDomain: HomeDomainData | undefined,
	activeDecision: ShellAiDecisionContext | undefined,
	opts?: ShellAiAskOptions
): OrgAiAnswer {
	if (!activeDecision && WRITE_OR_HOWTO_RE.test(prompt)) {
		return {
			query: prompt,
			headline: 'Live Tess runtime is not connected yet.',
			bullets: [
				{
					text: 'Local quick summary can answer live read-only questions about decisions, projects, budgets, offices, and task flow, but creating or editing items requires a live Tess runtime.',
					href: '/settings/agents',
					linkLabel: 'Configure agent runtimes'
				},
				{
					text: 'You can run this action directly using the workspace menu or Cmd+K commands.',
					href: '/work',
					linkLabel: 'Open Work'
				}
			]
		};
	}

	return answerExecutiveQuestion(prompt, {
		snapshot: ctx.snapshot,
		items: ctx.items,
		offices: ctx.offices,
		ownerId: ctx.ownerId,
		domain: mergedDomain,
		decisionId: opts?.decisionId ?? activeDecision?.id,
		decision: opts?.decision
	});
}

function upsertToolCall(
	calls: readonly TessToolCall[] | undefined,
	next: TessToolCall
): TessToolCall[] {
	const list = calls ? [...calls] : [];
	const idx = list.findIndex((c) => c.id === next.id);
	if (idx >= 0) {
		list[idx] = { ...list[idx], ...next };
		return list;
	}
	return [...list, next];
}

function applyStreamEventToTurn(turn: TessTurn, event: TessStreamEvent): TessTurn {
	switch (event.type) {
		case 'turn.started':
			return { ...turn, serverTurnId: event.turnId || turn.serverTurnId, streaming: true };
		case 'tool.progress':
			return { ...turn, toolCalls: upsertToolCall(turn.toolCalls, event.call) };
		case 'text.delta':
			return { ...turn, streamText: `${turn.streamText ?? ''}${event.delta}` };
		case 'ui.block':
			if (event.block.kind === 'follow_ups') {
				return { ...turn, followUps: event.block.prompts.slice(0, 3) };
			}
			return { ...turn, blocks: [...(turn.blocks ?? []), event.block] };
		case 'turn.completed':
			return {
				...turn,
				streaming: false,
				followUps: event.followUps?.length ? event.followUps.slice(0, 3) : turn.followUps
			};
		case 'turn.error':
			return { ...turn, streaming: false, errorMessage: event.message };
	}
}

function updateProposalInBlocks(
	blocks: readonly TessUiBlock[] | undefined,
	proposalId: string,
	updater: (proposal: TessActionProposal) => TessActionProposal
): TessUiBlock[] | undefined {
	if (!blocks) return blocks;
	return blocks.map((block) =>
		block.kind === 'action_proposal' && block.proposal.id === proposalId
			? { ...block, proposal: updater(block.proposal) }
			: block
	);
}

/**
 * Embedded Tess Companion controller (TASK-80.5 / doc-97).
 * Connects to doc-97 §5.C live HTTP/SSE endpoints when an organizationId is present and the
 * backend serves TASK-80.4, while falling back cleanly to explicitly badged local summaries.
 */
export class TessPanelState {
	mode = $state<TessCompanionMode>('pill');
	turns = $state<TessTurn[]>([]);
	draft = $state('');
	contextDismissed = $state(false);
	decisionContext = $state<ShellAiDecisionContext | undefined>(undefined);
	runtimeStatus = $state<TessRuntimeStatus>('offline');
	unavailableReason = $state<string | undefined>(undefined);
	liveAnnouncement = $state('');

	#domainContext: HomeDomainData | undefined = undefined;
	#turnSeq = 0;
	#sessionGen = 0;
	#askGen = 0;
	#api: TessApi;
	#abortController: AbortController | undefined = undefined;
	#activeOrgId: string | undefined = undefined;
	#activeLocalTurnId: string | undefined = undefined;

	constructor(api: TessApi = defaultTessApi) {
		this.#api = api;
	}

	get open(): boolean {
		return this.mode === 'open' || this.mode === 'expanded';
	}

	set open(value: boolean) {
		this.mode = value ? (this.mode === 'expanded' ? 'expanded' : 'open') : 'pill';
	}

	get expanded(): boolean {
		return this.mode === 'expanded';
	}

	get isStreaming(): boolean {
		return this.turns.some((t) => t.streaming);
	}

	setDomainContext(domain: HomeDomainData | undefined): void {
		this.#domainContext = domain;
	}

	syncRestingVisibility(showPill: boolean): void {
		if (!showPill && this.mode === 'pill') this.mode = 'hidden';
		else if (showPill && this.mode === 'hidden') this.mode = 'pill';
	}

	openPanel(): void {
		this.mode = this.mode === 'expanded' ? 'expanded' : 'open';
	}

	close(showPill = true): void {
		this.mode = showPill ? 'pill' : 'hidden';
	}

	toggle(showPill = true): void {
		if (this.open) this.close(showPill);
		else this.openPanel();
	}

	toggleExpand(): void {
		this.mode = this.mode === 'expanded' ? 'open' : 'expanded';
	}

	setOrganization(organizationId: string | undefined): void {
		if (organizationId === this.#activeOrgId) return;
		this.#activeOrgId = organizationId;
		this.#sessionGen++;
		this.#askGen++;
		this.#abortStream();
		this.turns = [];
		this.decisionContext = undefined;
		this.contextDismissed = false;
		this.runtimeStatus = 'offline';
		this.unavailableReason = undefined;
	}

	dispose(): void {
		this.#sessionGen++;
		this.#askGen++;
		this.#abortStream();
	}

	clear(): void {
		this.#sessionGen++;
		this.#askGen++;
		this.#abortStream();
		this.turns = [];
	}

	async resetSession(organizationId?: string): Promise<void> {
		const orgId = organizationId ?? this.#activeOrgId;
		const wasReady = this.runtimeStatus === 'ready';
		this.clear();
		if (orgId && wasReady) {
			await this.#api.resetSession(orgId);
		}
		this.liveAnnouncement = 'Started a fresh Tess conversation.';
	}

	dismissContext(): void {
		this.contextDismissed = true;
		this.decisionContext = undefined;
	}

	noteRouteChange(): void {
		this.decisionContext = undefined;
		this.contextDismissed = false;
	}

	#abortStream(): void {
		const prevTurnId = this.#activeLocalTurnId;
		this.#abortController?.abort();
		this.#abortController = undefined;
		this.#activeLocalTurnId = undefined;
		if (prevTurnId) {
			this.#patchTurn(prevTurnId, (t) => (t.streaming ? { ...t, streaming: false } : t));
		}
	}

	#patchTurn(turnId: string, updater: (turn: TessTurn) => TessTurn): void {
		this.turns = this.turns.map((t) => (t.id === turnId ? updater(t) : t));
	}

	async stopActiveTurn(): Promise<void> {
		const active = this.turns.find((t) => t.streaming);
		const orgId = active?.organizationId ?? this.#activeOrgId;
		this.#askGen++;
		this.#abortStream();
		if (!active) return;
		this.#patchTurn(active.id, (t) => ({ ...t, streaming: false }));
		this.liveAnnouncement = 'Stopped Tess response.';
		if (orgId && active.serverTurnId) {
			await this.#api.cancelTurn(orgId, active.serverTurnId);
		}
	}

	async applyProposal(turnId: string, proposalId: string): Promise<void> {
		const turn = this.turns.find((t) => t.id === turnId);
		const orgId = turn?.organizationId ?? this.#activeOrgId;
		const block = turn?.blocks?.find(
			(b): b is Extract<TessUiBlock, { kind: 'action_proposal' }> =>
				b.kind === 'action_proposal' && b.proposal.id === proposalId
		);
		if (
			!orgId ||
			!block ||
			block.proposal.status === 'applying' ||
			block.proposal.status === 'applied' ||
			block.proposal.status === 'dismissed'
		) {
			return;
		}
		const sessionGen = this.#sessionGen;

		this.#patchTurn(turnId, (t) => ({
			...t,
			blocks: updateProposalInBlocks(t.blocks, proposalId, (p) => ({
				...p,
				status: 'applying',
				errorMessage: undefined
			}))
		}));

		const res = await this.#api.applyProposal(orgId, proposalId, block.proposal.expectedRevision);
		if (sessionGen !== this.#sessionGen) return;
		if (res.ok) {
			this.#patchTurn(turnId, (t) => ({
				...t,
				blocks: updateProposalInBlocks(t.blocks, proposalId, (p) => ({
					...p,
					status: 'applied',
					receipt: res.receipt,
					errorMessage: undefined
				}))
			}));
			this.liveAnnouncement = res.receipt.summary;
			return;
		}

		this.#patchTurn(turnId, (t) => ({
			...t,
			blocks: updateProposalInBlocks(t.blocks, proposalId, (p) => ({
				...p,
				status: 'error',
				errorMessage: res.message
			}))
		}));
	}

	async dismissProposal(turnId: string, proposalId: string): Promise<void> {
		const turn = this.turns.find((t) => t.id === turnId);
		const orgId = turn?.organizationId ?? this.#activeOrgId;
		const block = turn?.blocks?.find(
			(b): b is Extract<TessUiBlock, { kind: 'action_proposal' }> =>
				b.kind === 'action_proposal' && b.proposal.id === proposalId
		);
		if (
			!block ||
			block.proposal.status === 'applying' ||
			block.proposal.status === 'applied' ||
			block.proposal.status === 'dismissed'
		) {
			return;
		}
		const prevStatus = block.proposal.status;
		const prevError = block.proposal.errorMessage;
		const sessionGen = this.#sessionGen;

		this.#patchTurn(turnId, (t) => ({
			...t,
			blocks: updateProposalInBlocks(t.blocks, proposalId, (p) => ({
				...p,
				status: 'dismissed',
				errorMessage: undefined
			}))
		}));
		if (!orgId) return;
		const ok = await this.#api.dismissProposal(orgId, proposalId);
		if (sessionGen !== this.#sessionGen || ok) return;
		this.#patchTurn(turnId, (t) => ({
			...t,
			blocks: updateProposalInBlocks(t.blocks, proposalId, (p) => ({
				...p,
				status: prevStatus === 'error' ? 'error' : 'pending',
				errorMessage: prevError ?? 'Could not dismiss proposal. Try again.'
			}))
		}));
	}

	ask(prompt: string, ctx: TessContextSnapshot, opts?: ShellAiAskOptions): TessTurn | undefined {
		const trimmed = prompt.trim();
		if (!trimmed) return undefined;

		if (opts?.decision) {
			this.decisionContext = opts.decision;
			this.contextDismissed = false;
		}
		this.openPanel();
		this.#askGen++;
		this.#abortStream();

		const activeDecision =
			opts?.decision ?? (this.contextDismissed ? undefined : this.decisionContext);
		const contextLabel = this.contextDismissed
			? undefined
			: activeDecision
				? activeDecision.title
				: ctx.routeLabel;

		const mergedDomain: HomeDomainData | undefined =
			this.#domainContext || ctx.domain
				? {
						...this.#domainContext,
						...ctx.domain,
						inboxItems: ctx.domain?.inboxItems ?? this.#domainContext?.inboxItems
					}
				: undefined;

		const fallbackAnswer = buildLocalFallbackAnswer(
			trimmed,
			ctx,
			mergedDomain,
			activeDecision,
			opts
		);
		const localId = `tess-turn-${++this.#turnSeq}`;
		const turn: TessTurn = {
			id: localId,
			organizationId: ctx.organizationId,
			query: trimmed,
			createdAt: Date.now(),
			mode: 'offline_summary',
			answer: fallbackAnswer,
			contextLabel
		};
		this.turns = [...this.turns, turn];
		this.draft = '';

		if (ctx.organizationId && !ctx.offline) {
			this.#activeOrgId = ctx.organizationId;
			const contextRef: TessContextRef | undefined = this.contextDismissed
				? undefined
				: {
						route: ctx.routePath,
						routeLabel: ctx.routeLabel,
						decisionId: opts?.decisionId ?? activeDecision?.id,
						decisionTitle: activeDecision?.title
					};
			void this.#runLiveTurn(
				ctx.organizationId,
				localId,
				trimmed,
				this.#sessionGen,
				this.#askGen,
				contextRef
			);
		}
		return turn;
	}

	async #runLiveTurn(
		organizationId: string,
		localTurnId: string,
		prompt: string,
		sessionGen: number,
		askGen: number,
		contextRef?: TessContextRef
	): Promise<void> {
		const controller = new AbortController();
		this.#abortController = controller;
		this.#activeLocalTurnId = localTurnId;

		const isStale = () =>
			sessionGen !== this.#sessionGen || askGen !== this.#askGen || controller.signal.aborted;

		const res = await this.#api.ask(
			organizationId,
			{ prompt, context_ref: contextRef },
			controller.signal
		);
		if (isStale()) return;

		if (res.kind === 'unavailable') {
			this.runtimeStatus = 'offline';
			this.unavailableReason = res.reason;
			if (this.#activeLocalTurnId === localTurnId) {
				this.#abortController = undefined;
				this.#activeLocalTurnId = undefined;
			}
			return;
		}
		if (res.kind === 'fallback') {
			this.runtimeStatus = 'no_runtime';
			this.unavailableReason = res.reason;
			if (res.fallbackAnswer) {
				this.#patchTurn(localTurnId, (t) => ({
					...t,
					answer: {
						query: t.answer.query,
						headline: res.fallbackAnswer!,
						bullets: t.answer.bullets
					}
				}));
			}
			if (this.#activeLocalTurnId === localTurnId) {
				this.#abortController = undefined;
				this.#activeLocalTurnId = undefined;
			}
			return;
		}

		this.runtimeStatus = 'ready';
		this.unavailableReason = undefined;

		this.#patchTurn(localTurnId, (t) => ({
			...t,
			mode: 'live',
			serverTurnId: res.turnId,
			streaming: true,
			streamText: '',
			toolCalls: [],
			blocks: []
		}));

		await this.#api.streamTurn(
			organizationId,
			res.turnId,
			res.streamUrl,
			{
				onEvent: (event) => {
					if (isStale()) return;
					this.#patchTurn(localTurnId, (t) => applyStreamEventToTurn(t, event));
					if (event.type === 'turn.completed') {
						this.liveAnnouncement = 'Tess finished responding.';
					}
				},
				onError: (message) => {
					if (isStale()) return;
					this.#patchTurn(localTurnId, (t) => ({
						...t,
						streaming: false,
						errorMessage: message
					}));
				},
				onDone: () => {
					if (sessionGen !== this.#sessionGen) return;
					this.#patchTurn(localTurnId, (t) => ({ ...t, streaming: false }));
					if (this.#activeLocalTurnId === localTurnId) {
						this.#abortController = undefined;
						this.#activeLocalTurnId = undefined;
					}
				}
			},
			controller.signal
		);
	}
}

const [getTessPanelContext, setTessPanelContext] = createContext<TessPanelState>();

export function provideTessPanel(panel = new TessPanelState()): TessPanelState {
	return setTessPanelContext(panel);
}

export function useTessPanel(): TessPanelState | undefined {
	try {
		return getTessPanelContext();
	} catch {
		return undefined;
	}
}
