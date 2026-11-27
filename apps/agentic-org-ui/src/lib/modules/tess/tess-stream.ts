import { isBuiltInHandle } from '$lib/proto';
import type { TessActionProposal, TessStreamEvent, TessToolCall, TessUiBlock } from './types';
import type { TessTurn } from './tess-panel.svelte';

const PROTECTED_PROPOSAL_KINDS =
	/(?:^|[._:-])(member|agent|pause|resume|offboard|remove|delete|rename|move)(?:$|[._:-])/i;

export function isProtectedBuiltInProposal(proposal: TessActionProposal): boolean {
	if (!PROTECTED_PROPOSAL_KINDS.test(proposal.kind)) return false;
	for (const param of proposal.parameters) {
		const val = param.value.trim();
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
