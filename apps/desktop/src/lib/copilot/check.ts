import type { TextImproveInput } from './types';

/**
 * What is wrong with a "Draft or improve" request before it is sent, in the person's words, or
 * `undefined` when it can go. An empty field with no instruction gives the copilot nothing to
 * write from, so it is not worth a run.
 */
export function checkTextRequest(input: Pick<TextImproveInput, 'mode' | 'text' | 'instruction'>): string | undefined {
	const text = input.text.trim();
	const instruction = input.instruction?.trim() ?? '';
	if (input.mode === 'improve' && !text) return 'There is nothing to improve yet. Write something, or ask for a draft.';
	if (input.mode === 'draft' && !text && !instruction) return 'Say what it should be about, then ask for a draft.';
	return undefined;
}
