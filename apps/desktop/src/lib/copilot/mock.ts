import type { AppError, Severity } from '$lib/proto';
import type { CopilotApi } from './api';
import { checkTextRequest } from './check';
import {
	COPILOT_LIMITS as LIMITS,
	type BugDraft,
	type BugFromSentenceInput,
	type ContextItem,
	type CopilotErrorCode,
	type CopilotFeature,
	type DraftResponse,
	type FeatureId,
	type FeatureShapes,
	type TextDraft,
	type TextImproveInput
} from './types';

/**
 * A scripted copilot for running the UI in a plain browser: rules, not a model, so every answer is
 * predictable. `?copilot=<scenario>` in the URL picks a scenario. It follows the office's rules: a
 * feature list that is empty without a runtime, the input caps, one run per feature at a time.
 */
export type CopilotScenario = 'on' | 'off' | 'failed' | 'timeout';

const SCENARIOS: CopilotScenario[] = ['off', 'failed', 'timeout'];

const FEATURES: CopilotFeature[] = [
	{ id: 'text.improve', version: 1, title: 'Draft or improve' },
	{ id: 'bug.from_sentence', version: 2, title: 'Report a bug from a sentence' }
];

function scenarioFromUrl(): CopilotScenario {
	if (typeof location === 'undefined') return 'on';
	const wanted = new URLSearchParams(location.search).get('copilot');
	return SCENARIOS.find((s) => s === wanted) ?? 'on';
}

const wait = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

const fail = (code: CopilotErrorCode, message: string): never => {
	const error: AppError = { source: 'app', code, message };
	throw error;
};

const chars = (text: string) => [...text].length;

const clip = (text: string, max: number) => (chars(text) > max ? `${[...text].slice(0, max - 3).join('').trimEnd()}...` : text);

/** Fix spacing, capitalise the first letter and end with a full stop: a stand-in for "improve". */
export function tidy(text: string): string {
	let out = text
		.replace(/[ \t]+/g, ' ')
		.replace(/ *\n */g, '\n')
		.replace(/\n{3,}/g, '\n\n')
		.trim();
	out = out.replace(/^\p{Ll}/u, (c) => c.toUpperCase());
	if (/[\p{L}\p{N}]$/u.test(out) && chars(out) < LIMITS.textMaxChars) out += '.';
	return out;
}

function improveText(input: TextImproveInput): DraftResponse<TextDraft> {
	const text = input.text.trim();
	const instruction = input.instruction?.trim() ?? '';
	const problem = checkTextRequest(input);
	if (problem) fail('invalid_request', problem);
	if (input.mode === 'improve') {
		const better = tidy(text);
		return {
			draft: { text: better },
			notes: [better === text ? 'Nothing to fix: it already reads well.' : 'Kept your meaning. Fixed spacing, capital letters and the ending.']
		};
	}
	const basis = tidy(instruction || text);
	const notes = ['Sample data: a rule-based stand-in, not a model’s answer.', 'Unknowns are marked [?].'];
	return { draft: { text: `${basis} [?] Add the details the copilot can't know.` }, notes };
}

/** A guess at how bad it is, from a few words (the bug severities of the office). */
export function guessSeverity(sentence: string): Exclude<Severity, 'unknown'> {
	const s = sentence.toLowerCase();
	if (/\b(crash(es|ed)?|data loss|lost|corrupt\w*|deleted)\b/.test(s)) return 'critical';
	if (/(can't|cannot|won't|doesn't|does not|fails?|failed|broken|error)/.test(s)) return 'major';
	if (/\b(typo|looks?|misaligned|cosmetic|slightly|colou?r)\b/.test(s)) return 'trivial';
	return 'minor';
}

function draftBug(input: BugFromSentenceInput, context: ContextItem[]): DraftResponse<BugDraft> {
	const sentence = input.sentence.trim().replace(/\s+/g, ' ');
	const first = sentence.split(/(?<=[.!?])\s/)[0];
	const title = chars(first) > 80 ? `${[...first].slice(0, 77).join('').trimEnd()}...` : first.replace(/[.!?]$/, '');
	// An area is named only when the sentence names it, and only one from the context.
	const named = context.filter((c) => c.kind === 'area' && c.label.length >= 3 && sentence.toLowerCase().includes(c.label.toLowerCase()));
	const notes = ['Sample data: a rule-based stand-in, not a model\u2019s answer.', "You didn't say how to make it happen, so no steps are listed."];
	return {
		draft: {
			title: title.replace(/^\p{Ll}/u, (c) => c.toUpperCase()),
			what_happened: clip(tidy(sentence), 2000),
			severity: guessSeverity(sentence),
			...(named.length === 1 ? { area_id: named[0].text } : {})
		},
		notes
	};
}

function check(feature: FeatureId, input: FeatureShapes[FeatureId]['input']) {
	if (feature === 'text.improve') {
		const i = input as TextImproveInput;
		if (!['draft', 'improve'].includes(i.mode)) fail('invalid_request', 'Unknown mode.');
		if (!i.field) fail('invalid_request', 'The field needs a name.');
		if (chars(i.field) > LIMITS.fieldMaxChars) fail('too_large', 'The field name is too long.');
		if (chars(i.text) > LIMITS.textMaxChars) fail('too_large', `The text is over ${LIMITS.textMaxChars} characters.`);
		if (i.instruction && chars(i.instruction) > LIMITS.instructionMaxChars)
			fail('too_large', `The instruction is over ${LIMITS.instructionMaxChars} characters.`);
	} else {
		const sentence = (input as BugFromSentenceInput).sentence;
		if (typeof sentence !== 'string' || !sentence.trim()) fail('invalid_request', 'Describe the problem in a sentence first.');
		if (chars(sentence) > LIMITS.sentenceMaxChars) fail('too_large', `Keep it under ${LIMITS.sentenceMaxChars} characters.`);
	}
}

export function createMockCopilot({
	delayMs = 700,
	scenario = scenarioFromUrl()
}: { delayMs?: number; scenario?: CopilotScenario } = {}): CopilotApi {
	// One run per feature at a time, like the office's one per person per feature.
	const running = new Set<FeatureId>();
	return {
		sample: true,
		async features() {
			return scenario === 'off' ? [] : FEATURES.map((f) => ({ ...f }));
		},
		async draft<F extends FeatureId>(feature: F, input: FeatureShapes[F]['input'], context: ContextItem[] = []) {
			if (scenario === 'off') fail('copilot_unavailable', 'No agent runtime is set up on this Mac.');
			if (!FEATURES.some((f) => f.id === feature)) fail('copilot_unavailable', 'The copilot does not offer that.');
			check(feature, input);
			if (running.has(feature)) fail('copilot_busy', 'A run of this feature is still going.');
			running.add(feature);
			try {
				await wait(delayMs);
				if (scenario === 'timeout') fail('timeout', 'The copilot took too long.');
				if (scenario === 'failed') fail('copilot_failed', 'The runtime failed.');
				const answer = feature === 'text.improve' ? improveText(input as TextImproveInput) : draftBug(input as BugFromSentenceInput, context);
				return answer as DraftResponse<FeatureShapes[F]['draft']>;
			} finally {
				running.delete(feature);
			}
		}
	};
}

export const mockCopilot = createMockCopilot();
