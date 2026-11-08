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

/** `text` cut to at most `max` UTF-16 units (what a field's `maxlength` counts), never splitting a character. */
export function fitTo(text: string, max: number | undefined): { text: string; clipped: boolean } {
	if (max === undefined || text.length <= max) return { text, clipped: false };
	let cut = text.slice(0, max);
	const last = cut.charCodeAt(cut.length - 1);
	if (last >= 0xd800 && last <= 0xdbff) cut = cut.slice(0, -1); // half of a surrogate pair
	return { text: cut, clipped: true };
}

/**
 * What Undo should bring back when a draft is used. `previous` is what Undo would restore now,
 * `value` the field's text, `lastApplied` the draft last put in the field. A field that still holds
 * an unedited draft keeps its earlier `previous` (the person's own text), so Undo never restores an
 * earlier draft. A field the person has edited since, or never drafted, is its own text.
 */
export function undoTarget(previous: string | undefined, value: string, lastApplied: string | undefined): string {
	return previous !== undefined && lastApplied !== undefined && value === lastApplied ? previous : value;
}
