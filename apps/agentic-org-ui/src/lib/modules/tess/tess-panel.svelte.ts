import { createContext } from 'svelte';
import { answerExecutiveQuestion, type HomeDomainData, type OrgAiAnswer } from '$lib/home/model';
import type { OfficeInfo, OrgSnapshot, WorkItemSummary } from '$lib/proto';
import type { ShellAiAskOptions, ShellAiDecisionContext } from '$lib/platform/shell/shell-ai';

export type TessTurnMode = 'offline_summary' | 'live';

export interface TessTurn {
	id: string;
	query: string;
	createdAt: number;
	mode: TessTurnMode;
	answer: OrgAiAnswer;
	contextLabel?: string;
}

export interface TessContextSnapshot {
	snapshot?: OrgSnapshot;
	items: readonly WorkItemSummary[];
	offices: readonly OfficeInfo[];
	ownerId: string;
	domain?: HomeDomainData;
	routeLabel?: string;
}

export const TESS_STARTERS: readonly string[] = [
	'What needs me first?',
	'Which projects need attention?',
	'Budget, offices & active agents',
	'Where is task time going?'
];

/**
 * Persistent right-hand Tess (Chief of Staff) side panel controller (`TASK-80.5` / `doc-97`).
 *
 * Uses local workspace derivation (`answerExecutiveQuestion`) with explicit `offline_summary`
 * badging until the live Tess streaming endpoint (`TASK-80.4`) is connected.
 */
export class TessPanelState {
	open = $state(false);
	turns = $state<TessTurn[]>([]);
	draft = $state('');
	contextDismissed = $state(false);
	decisionContext = $state<ShellAiDecisionContext | undefined>(undefined);
	#domainContext: HomeDomainData | undefined = undefined;
	#turnSeq = 0;

	/** Future seam for live SSE streaming (`TASK-80.4`). Always `'offline'` until wired. */
	readonly runtimeStatus: 'offline' | 'online' = 'offline';

	setDomainContext(domain: HomeDomainData | undefined): void {
		this.#domainContext = domain;
	}

	openPanel(): void {
		this.open = true;
	}

	close(): void {
		this.open = false;
	}

	toggle(): void {
		this.open = !this.open;
	}

	clear(): void {
		this.turns = [];
	}

	dismissContext(): void {
		this.contextDismissed = true;
		this.decisionContext = undefined;
	}

	noteRouteChange(): void {
		this.decisionContext = undefined;
		this.contextDismissed = false;
	}

	ask(prompt: string, ctx: TessContextSnapshot, opts?: ShellAiAskOptions): TessTurn | undefined {
		const trimmed = prompt.trim();
		if (!trimmed) return undefined;

		if (opts?.decision) {
			this.decisionContext = opts.decision;
			this.contextDismissed = false;
		}
		this.open = true;

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

		const answer = answerExecutiveQuestion(trimmed, {
			snapshot: ctx.snapshot,
			items: ctx.items,
			offices: ctx.offices,
			ownerId: ctx.ownerId,
			domain: mergedDomain,
			decisionId: opts?.decisionId ?? activeDecision?.id,
			decision: opts?.decision
		});

		const turn: TessTurn = {
			id: `tess-turn-${++this.#turnSeq}`,
			query: trimmed,
			createdAt: Date.now(),
			mode: 'offline_summary',
			answer,
			contextLabel
		};
		this.turns = [...this.turns, turn];
		this.draft = '';
		return turn;
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
