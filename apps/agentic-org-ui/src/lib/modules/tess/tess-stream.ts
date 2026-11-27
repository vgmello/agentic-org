import { answerExecutiveQuestion } from '$lib/modules/home/model';
import type { HomeDomainData, OrgAiAnswer } from '$lib/modules/home/model';
import { isBuiltInHandle } from '$lib/proto';
import type { ShellAiAskOptions, ShellAiDecisionContext } from '$lib/platform/workspace/shell-ai';
import type {
	TessActionProposal,
	TessContextSnapshot,
	TessStreamEvent,
	TessToolCall,
	TessTurn,
	TessUiBlock
} from './types';

const WRITE_OR_HOWTO_RE =
	/\b(create|assign|pause|resume|hire|move|delete|archive|approve|deny|how do i|how to|configure|set up|setup)\b/i;

export function buildLocalFallbackAnswer(
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

const PROTECTED_PROPOSAL_KINDS =
	/(?:^|[._:-])(member|agent|pause|resume|offboard|remove|delete|rename|move)(?:$|[._:-])/i;

export function isProtectedBuiltInProposal(
	proposal: TessActionProposal,
	protectedMemberIds?: readonly string[]
): boolean {
	if (!PROTECTED_PROPOSAL_KINDS.test(proposal.kind)) return false;
	for (const param of proposal.parameters) {
		const val = param.value.trim();
		if (protectedMemberIds?.includes(val)) return true;
		const norm = val.toLowerCase().replace(/[\s_-]+/g, ' ');
		if (isBuiltInHandle('agent', val) || norm === 'chief of staff' || norm === 'hr lead') {
			return true;
		}
	}
	return false;
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

export function applyStreamEventToTurn(turn: TessTurn, event: TessStreamEvent): TessTurn {
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

export function updateProposalInBlocks(
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
