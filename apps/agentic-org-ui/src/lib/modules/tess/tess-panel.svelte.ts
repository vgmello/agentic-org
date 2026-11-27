import { createContext } from 'svelte';
import type { HomeDomainData } from '$lib/modules/home/model';
import { isBuiltInAgent } from '$lib/proto';
import type { ShellAiAskOptions, ShellAiDecisionContext } from '$lib/platform/workspace/shell-ai';
import { defaultTessApi } from './api';
import {
	applyStreamEventToTurn,
	buildLocalFallbackAnswer,
	isProtectedBuiltInProposal,
	updateProposalInBlocks
} from './tess-stream';
import type {
	TessApi,
	TessCompanionMode,
	TessContextRef,
	TessContextSnapshot,
	TessRuntimeStatus,
	TessTurn,
	TessUiBlock
} from './types';
export {
	TESS_STARTERS,
	type TessCompanionMode,
	type TessContextSnapshot,
	type TessTurn,
	type TessTurnMode
} from './types';

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
	#protectedMemberIds: string[] = [];

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
		this.#protectedMemberIds = [];
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
		const idx = this.turns.findIndex((t) => t.id === turnId);
		if (idx < 0) return;
		this.turns[idx] = updater(this.turns[idx]);
	}

	#appendStreamDelta(turnId: string, delta: string): void {
		const idx = this.turns.findIndex((t) => t.id === turnId);
		if (idx < 0) return;
		this.turns[idx].streamText = `${this.turns[idx].streamText ?? ''}${delta}`;
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
		if (isProtectedBuiltInProposal(block.proposal, this.#protectedMemberIds)) {
			const msg = 'Built-in agents (HR and Tess) cannot be modified, moved, paused, or removed.';
			this.#patchTurn(turnId, (t) => ({
				...t,
				blocks: updateProposalInBlocks(t.blocks, proposalId, (p) => ({
					...p,
					status: 'error',
					errorMessage: msg
				}))
			}));
			this.liveAnnouncement = msg;
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

		if (ctx.snapshot) {
			this.#protectedMemberIds = ctx.snapshot.members.filter(isBuiltInAgent).map((m) => m.id);
		}
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
					if (event.type === 'text.delta') {
						this.#appendStreamDelta(localTurnId, event.delta);
					} else {
						this.#patchTurn(localTurnId, (t) => applyStreamEventToTurn(t, event));
					}
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
