// One agent's setup read, with not found as a state of its own (no Retry): any other failure stays an error.
import { toAppError } from '$lib/office/api';
import type { AgentSetupDetail } from '../../types';

export type AgentLookup =
	{ found: true; detail: AgentSetupDetail } | { found: false; message: string };

export async function lookUpAgent(read: () => Promise<AgentSetupDetail>): Promise<AgentLookup> {
	try {
		return { found: true, detail: await read() };
	} catch (e) {
		const failure = toAppError(e);
		if (failure.code === 'not_found') return { found: false, message: failure.message };
		throw e;
	}
}

/** A refused save that carries the newer record (`conflict`): its `latest`, if any. */
export function latestOf(e: unknown): AgentSetupDetail | undefined {
	if (!e || typeof e !== 'object' || !('latest' in e)) return undefined;
	const latest = (e as { latest?: unknown }).latest;
	return latest && typeof latest === 'object' && 'memberId' in latest
		? (latest as AgentSetupDetail)
		: undefined;
}
